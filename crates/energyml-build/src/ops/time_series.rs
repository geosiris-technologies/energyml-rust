//! `time-series` - an `eml23.TimeSeries` from a list of instants, continuing
//! another series when asked: the time a property, a geometry or an activity
//! counts in.
//!
//! No operation wrote one before 24/09, and the synthetic model Lutetia needs
//! three: the operation of the
//! doublet, year by year; the drilling and commissioning dates it continues;
//! and a month by month series that continues the first. The need was
//! to try the front end on exactly this: the time indexes.
//!
//! # What is written, and what is refused
//!
//! - **the instants as given**, each a `GeologicTime` with its `DateTime` -
//!   a W3CDTF date-time in UTC, `2016-01-01T00:00:00Z`. One that does not read
//!   is refused by name, never widened or guessed: a series in which one date
//!   was reinterpreted looks exactly like a series that was right;
//! - **in increasing order**, or refused. `Time` is *“the list ordering used
//!   by the time index”* (the documentation of `TimeSeries.Time`,
//!   CommonTypes.xsd), so a property that says *index 3* means the fourth date
//!   as listed; a series out of order would give every index a date nobody
//!   meant. Two equal instants are refused too - an index that two dates share
//!   is an index that names neither;
//! - **`TimeStep`, on demand**: 0…n−1 inline. The documentation of
//!   `TimeSeries` states the one rule it has - *“If present TimeStep count
//!   must match Time count”* - and this writes it so;
//! - **a parent, given as an input**: `TimeSeriesParentage` with
//!   `ParentTimeIndex` {`Index`, `TimeSeries` → the parent} and `HasOverlap` -
//!   *“the end state of one calculation is the initial state of the
//!   next”* (the documentation of `HasOverlap`). The index is checked against
//!   nothing it cannot see: the parent's dates are not read here (a plan is
//!   pure), so an index is only refused when it is negative, and the activity
//!   records it.

use energyml_domain::provenance::Parameter;
use energyml_eml_2_3 as eml;

use crate::declare::{Inputs, ObjectIn, Params, Title};
use crate::operation::{About, ActivityDraft, Category, Context, NewObject, Note, Operation, Plan, Refusal};

/// The operation.
pub struct TimeSeries;

/// What `time-series` is told.
#[derive(Debug, Params)]
pub struct TimeSeriesParams {
    /// the series' title
    pub title: Title,
    /// the instants, in increasing order, each a UTC date-time as 2016-01-01T00:00:00Z
    #[param(least = 1)]
    pub times: Vec<String>,
    /// also write TimeStep, 0 to n−1 - its count must match the instants'
    #[param(default = false, optional)]
    pub time_steps: bool,
    /// with a parent: the index, in the parent series, of the instant this one continues from
    pub parent_index: Option<i64>,
    /// with a parent: whether this series' first instant is the parent's instant it continues from
    #[param(default = true, optional)]
    pub overlap: bool,
}

/// What `time-series` takes.
#[derive(Debug, Inputs)]
pub struct TimeSeriesInputs {
    /// the series this one continues, when it continues one
    #[input(types = "eml23.TimeSeries")]
    pub parent: Option<ObjectIn>,
}

impl Operation for TimeSeries {
    type Params = TimeSeriesParams;
    type Inputs = TimeSeriesInputs;

    fn about(&self) -> About {
        About {
            name: "time-series",
            describe: "a time series from a list of instants, continuing another one when asked - the time a property, a geometry or an activity counts in",
            category: Category::Frame,
            makes: &["eml23.TimeSeries"],
            recipe: "time-series",
            represents: None,
        }
    }

    fn plan(
        &self,
        inputs: TimeSeriesInputs,
        params: TimeSeriesParams,
        cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        let mut instants: Vec<(String, i64)> = Vec::with_capacity(params.times.len());
        for (at, text) in params.times.iter().enumerate() {
            let text = text.trim();
            let Some(seconds) = energyml_storage::epc::core_props::unix_seconds(text) else {
                return Err(Refusal::on(
                    "times",
                    format!(
                        "instant {} `{text}` is not a UTC date-time as `2016-01-01T00:00:00Z`: it is refused rather than read some other way",
                        at + 1
                    ),
                ));
            };
            if let Some((before, previous)) = instants.last()
                && seconds <= *previous
            {
                return Err(Refusal::on(
                    "times",
                    format!(
                        "instant {} `{text}` does not come after `{before}`: a time index names a date by its place in the list, so the list is in increasing order and no two dates are equal",
                        at + 1
                    ),
                ));
            }
            instants.push((text.to_string(), seconds));
        }

        let parentage = match (&inputs.parent, params.parent_index) {
            (None, None) => None,
            (None, Some(_)) => {
                return Err(Refusal::on(
                    "parentIndex",
                    "an index in a parent series, and no parent series: give the series this one continues",
                ));
            }
            (Some(parent), index) => {
                let Some(reference) = parent.known.reference.as_ref() else {
                    return Err(Refusal::on(
                        "parent",
                        "the parent series was given without a reference to write",
                    ));
                };
                let index = index.ok_or_else(|| {
                    Refusal::on(
                        "parentIndex",
                        format!(
                            "`{}` is continued, and from which of its instants is not said: give its index",
                            reference.title
                        ),
                    )
                })?;
                if index < 0 {
                    return Err(Refusal::on(
                        "parentIndex",
                        format!("{index} is not an index: the first instant of a series is 0"),
                    ));
                }
                Some((reference.clone(), index))
            }
        };

        let title = params.title.0.trim().to_string();
        let first = instants.first().map(|(text, _)| text.clone()).unwrap_or_default();
        let last = instants.last().map(|(text, _)| text.clone()).unwrap_or_default();
        // The same series is the same uuid: its title and its instants say
        // what it is, and a parent it continues is part of that.
        let seed = format!(
            "time-series:{title}:{}:{}",
            instants
                .iter()
                .map(|(text, _)| text.as_str())
                .collect::<Vec<_>>()
                .join(","),
            parentage
                .as_ref()
                .map(|(parent, index)| format!("{}#{index}:{}", parent.uuid, params.overlap))
                .unwrap_or_default()
        );
        let uuid = cx.identity.uuid(&seed, 0)?;

        let series = eml::TimeSeries {
            uuid: uuid.clone(),
            schema_version: "2.3".to_string(),
            citation: eml::Citation {
                title: title.clone(),
                originator: cx.when.originator.clone(),
                creation: cx.when.creation.clone(),
                format: energyml_core::CITATION_FORMAT.to_string(),
                ..Default::default()
            },
            time: instants
                .iter()
                .map(|(text, _)| eml::GeologicTime {
                    date_time: text.clone(),
                    ..Default::default()
                })
                .collect(),
            time_step: params.time_steps.then(|| {
                eml::AbstractIntegerArray::IntegerXmlArray(eml::IntegerXmlArray {
                    count_per_value: 1,
                    null_value: -1,
                    values: (0..instants.len() as i64).collect(),
                    ..Default::default()
                })
            }),
            time_series_parentage: parentage
                .as_ref()
                .map(|(parent, index)| eml::TimeSeriesParentage {
                    has_overlap: params.overlap,
                    parent_time_index: eml::TimeIndex {
                        index: *index,
                        time_series: crate::crs::reference_of(parent),
                    },
                }),
            ..Default::default()
        };

        let mut says = vec![Note {
            key: "instants",
            detail: format!("{} instant(s), from {first} to {last}", instants.len()),
        }];
        if let Some((parent, index)) = &parentage {
            says.push(Note {
                key: "continues",
                detail: format!(
                    "continues `{}` from its instant {index}{} - the index is not checked against the parent's dates, which a plan does not read",
                    parent.title,
                    if params.overlap { ", which is also this series' first instant" } else { "" }
                ),
            });
        }

        let mut parameters = vec![
            Parameter::Text {
                title: "title".to_string(),
                value: title.clone(),
            },
            Parameter::Double {
                title: "instantCount".to_string(),
                value: instants.len() as f64,
                uom: None,
            },
        ];
        if let Some((_, index)) = &parentage {
            parameters.push(Parameter::Double {
                title: "parentIndex".to_string(),
                value: *index as f64,
                uom: None,
            });
        }

        Ok(Plan {
            media: Vec::new(),
            blocked: None,
            objects: vec![NewObject {
                uuid,
                qualified_type: "eml23.TimeSeries".to_string(),
                role: "time-series".to_string(),
                title: title.clone(),
                json: energyml_core::json::write_value(&series),
            }],
            datasets: Vec::new(),
            activity: ActivityDraft {
                recipe: "time-series",
                title: format!("time-series → {title}"),
                seed,
                parameters,
            },
            says,
            dropped: Vec::new(),
            // No geometry: a series of instants has no shape.
            shapes: Vec::new(),
        })
    }
}
