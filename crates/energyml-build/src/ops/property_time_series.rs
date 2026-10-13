//! `property-time-series` — a property **through time**, laid by a sourced law
//! of time on a snapshot property of the package, in either of the two forms
//! RESQML 2.2 has for it.
//!
//! The fifth of the reference model's steps, `property-time-series` (both forms), and the four objects it
//! produces:
//! produces:
//!
//! > a `ContinuousProperty` for pressure, a series — `TimeOrIntervalSeries`
//! > {false, the daily series}; values \[11]\[10]\[20]\[24]
//! >
//! > a temperature series, likewise
//! >
//! > pressure, **one per year** — 11 properties with a `Time`
//! >
//! > mean monthly pressure — `TimeOrIntervalSeries` {**true**,
//! > the monthly series}
//!
//! # The two forms, and why there are two and not three
//!
//! `AbstractProperty` states a `Time` (`GeologicTime`, 0..1) and a
//! `TimeOrIntervalSeries` (0..1) — RESQML Technical Reference Guide v2.2 §7.1
//! p. 222 — and nothing else about time. The reference-model description settles which way each is
//! used:
//!
//! - **`series`**: one property holding **every** step, the time its slowest
//!   dimension — TRG v2.2 §7, the note of the diagram p. 221: *“\[TimeCount] :
//!   don't provide this dimension if not needed”*, above
//!   *“\[IndexableElementCount]”*. The array is `[steps, elements]`. With
//!   `useInterval`, a step is the interval from an instant to the next: `n`
//!   instants make `n − 1` values;
//! - **`steps`**: one property per instant, each dated by its `Time` — and
//!   **not** pointing at the series. `TimeOrIntervalSeries` has no index
//!   (CommonTypes.xsd l. 423–428), so N properties naming the series would each
//!   claim to hold all N steps; the `TimeIndex` a draft of the usage guide gave
//!   a property (2.2RC, fig. 6-4) is absent from the final XSD. The instant is
//!   the series' own, taken from it by place: property `k` holds at instant
//!   `k`, and says so in its description.
//!
//! # The values: a snapshot, and a law of time
//!
//! What is laid is `v(e, t) = v₀(e) + change · (t − anchor) / year`
//! (`energyml_analysis::time_law`): `v₀` the values of a property **of the
//! package** — `base`, a continuous property of one value per element, its
//! representation, element, kind and unit taken as they are — holding at
//! `anchor`, the series' first instant unless stated. The base is where the
//! physics is (a hydrostatic pressure by `well-logs`), and its law is
//! already sourced there; this operation lays the time on it, and a second
//! writer of the depth law would be a second place for its errors. An element
//! the base says nothing about (`NaN`) says nothing at any step, counted.
//!
//! No constant without a source: `source` is
//! required, and every property written carries `synthetic = true` and its law
//! in `Citation/Description` (rule 4) — a drift laid by a law is not a
//! measurement.
//!
//! # The instants are the series', read at the boundary
//!
//! `Known::instants`, resolved by `read::known_of_object` — never restated in
//! a setting beside the series, which would be a second list of the same
//! instants, free to disagree with the first. A series that does not increase
//! is refused by its rank: a time index names an instant by its place.
//!
//! # What is refused, by name
//!
//! A base that is no continuous property, already indexed in time, with
//! realizations, a vector, qualifiers (`Facet`s this writer would drop), or
//! values this build did not read — the boundary reads a property's values
//! **only on a wellbore frame** (`read::property_of`), so the pressure and temperature series, on
//! the IJK grid, wait for that as much as for the grid.
//!
//! # The writing is `property`'s
//!
//! [`super::property::write_timed`], the one writer of a `ContinuousProperty`,
//! timed or not. Each uuid comes from what the property is — the base, the
//! series, the form, the step, **and the values**
//! (ADR 0017): a corrected change writes a second property, never one rewritten
//! under the same uuid. The source is not in it: rewording a citation rewrites
//! the same property.
//!
//! # References
//!
//! - The time law is a synthetic law stated in `energyml-analysis`, which cites no published source for it.
//!   TODO(reference): verify — no publication is cited; if the law follows one, add it (authors, year, title, venue, DOI) once checked.

use serde_json::json;

use energyml_analysis::time_law::{self, Drift, Steps, TimeLawError};
use energyml_domain::provenance::Parameter;
use energyml_storage::epc::core_props::unix_seconds;

use crate::declare::{Field, Inputs, ObjectIn, ObjectRef, Params, PropertyKindRef, Title};
use crate::operation::{
    About, ActivityDraft, Category, Context, Identity, Need, Note, Operation, Plan, Refusal, Slot,
};
use crate::pieces::{Known, KnownProperty, Ref};

use super::property::{PropertyParams, Timing};

/// The operation.
pub struct PropertyTimeSeries;

/// What `property-time-series` is told.
#[derive(Debug, Params)]
pub struct PropertyTimeSeriesParams {
    /// The form the time takes.
    #[param(flatten)]
    pub form: FormParams,
    /// the change of every value per Julian year (365.25 days), in the base's unit — negative for a decline
    pub change: f64,
    /// the instant the base's values hold at, a UTC date-time as 2016-01-01T00:00:00Z; left out, the series' first instant
    pub anchor: Option<String>,
    /// where the law and its change come from — a document with its page or paragraph; a value stated as plausible says so (synthetic, and why)
    pub source: String,
    /// the title of what is written — each step's property adds its instant; left out, the base's, and a series adds its series' title
    pub title: Option<String>,
    /// the HDF5 file the value arrays go into
    #[param(default = String::from("data.h5"), group = "naming")]
    pub data_file: String,
}

/// how the property goes through time — one property per instant dated by its Time, or one property holding every step of the series
///
/// **No default**: the two forms are two different sets of objects, and a
/// default would choose for whoever did not.
#[derive(Debug, Params)]
#[params(tag = "form")]
pub enum FormParams {
    /// One property per instant of the series, each dated by its `Time`.
    Steps,
    /// One property holding every step, `TimeOrIntervalSeries`.
    Series(SeriesParams),
}

/// What the series form is told.
#[derive(Debug, Params)]
pub struct SeriesParams {
    /// series: each value holds over the interval from an instant to the next rather than at an instant — n instants give n − 1 values
    #[param(default = false, optional)]
    pub use_interval: bool,
}

/// What `property-time-series` takes.
#[derive(Debug, Inputs)]
pub struct PropertyTimeSeriesInputs {
    /// the property whose values hold at the anchor — a continuous property of the package, one value per element; its representation, element, kind and unit are those written
    #[input(types = "resqml22.ContinuousProperty")]
    pub base: ObjectIn,
    /// the time series the property goes through — its instants are read from it
    #[input(types = "eml23.TimeSeries")]
    pub series: ObjectIn,
}

/// The class of a qualified type — `ContinuousProperty` of
/// `resqml22.ContinuousProperty`.
fn class_of(qualified_type: &str) -> &str {
    qualified_type
        .rsplit('.')
        .next()
        .unwrap_or(qualified_type)
        .trim_start_matches("obj_")
}

/// The reference an object input names, of the class wanted — or a refusal
/// naming its slot.
fn of_class<'a>(slot: &str, input: &'a ObjectIn, class: &str, why: &str) -> Result<&'a Ref, Refusal> {
    let reference = input
        .known
        .reference
        .as_ref()
        .ok_or_else(|| Refusal::on(slot, format!("`{}` names no object of the package", input.name)))?;
    if class_of(&reference.qualified_type) != class {
        return Err(Refusal::on(
            slot,
            format!(
                "`{}` is a `{}`, where a `{class}` is wanted: {why}",
                reference.title, reference.qualified_type
            ),
        ));
    }
    Ok(reference)
}

/// The facts of the base, or why they cannot be laid through time.
fn facts_of<'a>(base: &'a Ref, known: &'a Known) -> Result<(&'a KnownProperty, Vec<f64>), Refusal> {
    let title = &base.title;
    let facts = known.property.as_ref().ok_or_else(|| {
        Refusal::on(
            "base",
            format!("nothing is known of `{title}` as a property: give a property of the package"),
        )
    })?;
    if facts.timed {
        return Err(Refusal::on(
            "base",
            format!(
                "`{title}` is already indexed in time: a series laid on a series would give each step a history of its own"
            ),
        ));
    }
    if facts.realizations {
        return Err(Refusal::on(
            "base",
            format!("`{title}` holds realizations, a dimension of its own that this writer does not carry"),
        ));
    }
    if facts.value_count.iter().product::<u64>() != 1 {
        return Err(Refusal::on(
            "base",
            format!(
                "`{title}` holds {:?} value(s) per element — a vector or a tensor, which this writer does not carry",
                facts.value_count
            ),
        ));
    }
    if !facts.facets.is_empty() {
        return Err(Refusal::on(
            "base",
            format!(
                "`{title}` is qualified by {} facet(s), which this writer does not carry — dropped, the property would say something else",
                facts.facets.len()
            ),
        ));
    }
    let values = facts
        .values
        .as_ref()
        .map_err(|why| Refusal::on("base", format!("the values of `{title}` were not read: {why}")))?;
    let values = values
        .to_f64()
        .ok_or_else(|| Refusal::on("base", format!("the values of `{title}` are no numbers")))?;
    if values.is_empty() {
        return Err(Refusal::on("base", format!("`{title}` holds no value")));
    }
    Ok((facts, values))
}

/// Values as the seed carries them — exactly, NaN included.
fn digest(values: &[f64]) -> String {
    values
        .iter()
        .map(|value| format!("{value:?}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// A count, as the activity records it.
fn count(title: &str, value: usize) -> Parameter {
    Parameter::Double {
        title: title.to_string(),
        value: value as f64,
        uom: None,
    }
}

/// A refusal of the time law, under the slot it comes from.
fn refusal_of(why: &TimeLawError) -> Refusal {
    match why {
        TimeLawError::BadChange { .. } => Refusal::on("change", why),
        TimeLawError::TooFewInstants { .. } | TimeLawError::NotIncreasing { .. } => {
            Refusal::on("series", why)
        }
    }
}

impl Operation for PropertyTimeSeries {
    type Params = PropertyTimeSeriesParams;
    type Inputs = PropertyTimeSeriesInputs;

    fn about(&self) -> About {
        About {
            name: "property-time-series",
            describe: "a property through a time series, laid by a sourced drift in time on a property of the package — one property per instant dated by its Time, or one holding every step, time its slowest dimension; marked synthetic",
            category: Category::Synthetic,
            makes: &["resqml22.ContinuousProperty"],
            recipe: "property-time-series",
            represents: None,
        }
    }

    /// What each object is — a continuous property, a time series — which no
    /// declaration of a slot's kind can say. What the base holds is the plan's
    /// to refuse: an offer is judged on what each input *is*.
    fn check(&self, inputs: &PropertyTimeSeriesInputs) -> Result<(), Refusal> {
        of_class(
            "base",
            &inputs.base,
            "ContinuousProperty",
            "the values laid through time are a continuous property's",
        )?;
        of_class(
            "series",
            &inputs.series,
            "TimeSeries",
            "the steps are the instants of a time series",
        )?;
        Ok(())
    }

    fn plan(
        &self,
        inputs: PropertyTimeSeriesInputs,
        params: PropertyTimeSeriesParams,
        cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        // -- the source, before anything is laid --------------------------------
        let source = params.source.trim().to_string();
        if source.is_empty() {
            return Err(Refusal::on(
                "source",
                "is empty: a law is sourced — a document with its page or paragraph, or a plausible value said to be one",
            ));
        }

        // -- the base: a snapshot of one value per element ------------------------
        let base = of_class(
            "base",
            &inputs.base,
            "ContinuousProperty",
            "the values laid through time are a continuous property's",
        )?;
        let (facts, snapshot) = facts_of(base, &inputs.base.known)?;
        let supporting = facts.supporting.as_ref().ok_or_else(|| {
            Refusal::on(
                "base",
                format!("`{}` names no representation its values hang on", base.title),
            )
        })?;
        let kind = facts.kind.as_ref().ok_or_else(|| {
            Refusal::on(
                "base",
                format!("`{}` names no property kind by reference", base.title),
            )
        })?;
        let uom = facts.uom.clone().ok_or_else(|| {
            Refusal::on(
                "base",
                format!(
                    "`{}` states no unit — the change per year is in its unit",
                    base.title
                ),
            )
        })?;

        // -- the series: its instants, read at the boundary -----------------------
        let series = of_class(
            "series",
            &inputs.series,
            "TimeSeries",
            "the steps are the instants of a time series",
        )?;
        let texts = inputs.series.known.instants.as_ref().ok_or_else(|| {
            Refusal::on(
                "series",
                format!("the instants of `{}` were not read — a series of the package, each instant with its date-time, is what the steps are counted on", series.title),
            )
        })?;
        let mut instants = Vec::with_capacity(texts.len());
        for (rank, text) in texts.iter().enumerate() {
            let seconds = unix_seconds(text).ok_or_else(|| {
                Refusal::on(
                    "series",
                    format!("instant {} of `{}`, `{text}`, is not a UTC date-time as `2016-01-01T00:00:00Z`: it is refused rather than read some other way", rank + 1, series.title),
                )
            })?;
            instants.push(seconds);
        }

        // -- the law ------------------------------------------------------------------
        let (anchor_text, anchor) = match params.anchor.as_deref().map(str::trim) {
            Some(text) => {
                let seconds = unix_seconds(text).ok_or_else(|| {
                    Refusal::on(
                        "anchor",
                        format!("`{text}` is not a UTC date-time as `2016-01-01T00:00:00Z`"),
                    )
                })?;
                (text.to_string(), seconds)
            }
            None => match (texts.first(), instants.first()) {
                (Some(text), Some(&seconds)) => (text.clone(), seconds),
                _ => {
                    return Err(Refusal::on(
                        "series",
                        format!("`{}` holds no instant", series.title),
                    ));
                }
            },
        };
        let drift = Drift::new(anchor, params.change).map_err(|why| refusal_of(&why))?;
        let (steps, use_interval) = match &params.form {
            FormParams::Steps => (Steps::Instants, false),
            FormParams::Series(series) if series.use_interval => (Steps::Intervals, true),
            FormParams::Series(_) => (Steps::Instants, false),
        };
        let offsets = time_law::offsets(&drift, &instants, steps).map_err(|why| refusal_of(&why))?;
        let law_words = format!(
            "the values of `{}` holding at {anchor_text}, changing by {} {uom} per Julian year (365.25 days)",
            base.title, params.change
        );
        let empty = snapshot.iter().filter(|value| value.is_nan()).count();

        // -- what each property is written with ------------------------------------------
        let given = params
            .title
            .as_deref()
            .map(str::trim)
            .filter(|title| !title.is_empty());
        let stem = given.unwrap_or(&base.title).to_string();
        let property_params = |title: String| PropertyParams {
            representation: ObjectRef(supporting.dor()),
            property_kind: PropertyKindRef(kind.dor()),
            uom: uom.clone(),
            title: Title(title),
            data_file: params.data_file.clone(),
        };
        // The writer checks the values against the elements it is told of: the
        // base's own count, when they are nodes.
        let counted = Known {
            node_count: (facts.indexable == "nodes").then_some(snapshot.len()),
            ..Known::default()
        };
        let field = |values: Vec<f64>| Field {
            name: format!("{} through `{}`", base.title, series.title),
            over: supporting.title.clone(),
            on: facts.indexable.clone(),
            values,
            uom: Some(uom.clone()),
        };
        let marked = |drawn: &mut Plan, description: String, form: &str| -> Result<(), Refusal> {
            // `Citation/Description` holds 2000 characters: refused rather than
            // cut short, since a source cut short cites nothing.
            if description.chars().count() > 2000 {
                return Err(Refusal::on(
                    "source",
                    format!(
                        "makes a description of {} characters, and a Citation/Description holds 2000 — cite the document shorter",
                        description.chars().count()
                    ),
                ));
            }
            for object in drawn.objects.iter_mut() {
                object.json["Citation"]["Description"] = json!(description);
                // The mark of an invented value.
                object.json["ExtensionNameValue"] = json!([{
                    "$type": "eml23.ExtensionNameValue",
                    "Name": "synthetic",
                    "Value": { "$type": "eml23.StringMeasure", "_": "true" },
                    "Description": format!("laid by `property-time-series` ({form}) by a drift in time, not measured"),
                }]);
            }
            Ok(())
        };

        let mut objects = Vec::new();
        let mut datasets = Vec::new();
        let mut says: Vec<Note> = Vec::new();
        let (written, form_word) = match &params.form {
            FormParams::Steps => {
                for (index, (text, offset)) in texts.iter().zip(&offsets).enumerate() {
                    cx.stop_if_cancelled()?;
                    let values = time_law::lay(&snapshot, &[*offset]);
                    let seed = format!(
                        "property-time-series:steps:{}:{}:{index}:{text}:{}",
                        base.uuid,
                        series.uuid,
                        digest(&values)
                    );
                    // **One uuid each, in the plan's order.** The writer takes the
                    // first of the identity it is given; a list given by the caller
                    // is handed out one uuid per property.
                    let identity = match cx.identity {
                        Identity::Given(_) => Identity::Given(vec![cx.identity.uuid("", index)?]),
                        derived => derived.clone(),
                    };
                    let one = Context {
                        identity: &identity,
                        when: cx.when,
                        purpose: cx.purpose,
                        feedback: cx.feedback,
                        bag: cx.bag,
                    };
                    let mut drawn = super::property::write_timed(
                        &field(values),
                        Some(&counted),
                        &property_params(format!("{stem} — {text}")),
                        &seed,
                        &one,
                        &Timing::At(text.clone()),
                    )?;
                    marked(
                        &mut drawn,
                        format!(
                            "Laid by a law, not measured: {law_words}. It holds at {text}, instant {index} of the time series `{}` (uuid {}), which it does not reference — a property naming a series holds every step of it. Source: {source}",
                            series.title, series.uuid
                        ),
                        "steps",
                    )?;
                    objects.append(&mut drawn.objects);
                    datasets.append(&mut drawn.datasets);
                    says.append(&mut drawn.says);
                }
                (texts.len(), "steps")
            }
            FormParams::Series(_) => {
                cx.stop_if_cancelled()?;
                let values = time_law::lay(&snapshot, &offsets);
                let seed = format!(
                    "property-time-series:series:{}:{}:{use_interval}:{}",
                    base.uuid,
                    series.uuid,
                    digest(&values)
                );
                let mut drawn = super::property::write_timed(
                    &field(values),
                    Some(&counted),
                    &property_params(match given {
                        Some(title) => title.to_string(),
                        None => format!("{stem} — {}", series.title),
                    }),
                    &seed,
                    cx,
                    &Timing::Series {
                        series: series.dor(),
                        use_interval,
                        steps: offsets.len(),
                    },
                )?;
                let over = if use_interval {
                    format!(
                        "Each of its {} steps is the mean over an interval from an instant of the time series `{}` to the next — step k from instant k to instant k + 1",
                        offsets.len(),
                        series.title
                    )
                } else {
                    format!(
                        "Each of its {} steps holds at an instant of the time series `{}` — step k at instant k",
                        offsets.len(),
                        series.title
                    )
                };
                marked(
                    &mut drawn,
                    format!(
                        "Laid by a law, not measured: {law_words}. {over}, time the slowest dimension. Source: {source}"
                    ),
                    "series",
                )?;
                objects.append(&mut drawn.objects);
                datasets.append(&mut drawn.datasets);
                says.append(&mut drawn.says);
                (1, "series")
            }
        };

        // -- what is said ----------------------------------------------------------------
        let mut notes = vec![
            Note { key: "law", detail: format!("{law_words} — source: {source}") },
            match &params.form {
                FormParams::Steps => Note {
                    key: "steps",
                    detail: format!(
                        "{written} propert{}, one per instant of `{}`, each dated by its `Time` and none referencing the series: RESQML 2.2 gives a property no time index, and a property naming a series holds all its steps",
                        if written == 1 { "y" } else { "ies" },
                        series.title
                    ),
                },
                FormParams::Series(_) => Note {
                    key: "series",
                    detail: format!(
                        "one property of {} step(s) × {} element(s), time the slowest dimension (RESQML TRG v2.2 §7, p. 221){}",
                        offsets.len(),
                        snapshot.len(),
                        if use_interval {
                            format!(" — over intervals: {} instants make {} values", instants.len(), offsets.len())
                        } else {
                            String::new()
                        }
                    ),
                },
            },
            Note {
                key: "synthetic",
                detail: "each property is laid, not measured: it carries `synthetic = true` and a description of its law and its source".to_string(),
            },
        ];
        if empty > 0 {
            notes.push(Note {
                key: "empty",
                detail: format!(
                    "{empty} element(s) of `{}` hold no value (NaN) — they hold none at any step",
                    base.title
                ),
            });
        }
        notes.append(&mut says);

        // -- the activity -----------------------------------------------------------------
        let parameters = vec![
            Parameter::Object {
                title: "input".to_string(),
                reference: base.dor(),
            },
            Parameter::Object {
                title: "input".to_string(),
                reference: series.dor(),
            },
            count("propertyCount", written),
            count("stepCount", offsets.len()),
            count("emptyCount", empty),
        ];

        Ok(Plan {
            media: Vec::new(),
            blocked: None,
            objects,
            datasets,
            // **No shape**: a property is laid on a geometry, it
            // makes none.
            shapes: Vec::new(),
            activity: ActivityDraft {
                recipe: "property-time-series",
                title: format!(
                    "property-time-series → {stem} through {} ({form_word})",
                    series.title
                ),
                seed: format!(
                    "property-time-series:{form_word}:{use_interval}:{}:{}:{:?}:{anchor}",
                    base.uuid, series.uuid, params.change
                ),
                parameters,
            },
            says: notes,
            dropped: Vec::new(),
        })
    }

    /// The law's settings are inputs, recorded under the parameters the
    /// projection already declares — a title cannot be both.
    fn reports(&self) -> Vec<Slot> {
        let number = || Need::Number {
            uom: None,
            least: None,
            most: None,
        };
        vec![
            Slot::required(
                "propertyCount",
                number(),
                "properties written — one per instant, or one holding the series",
            ),
            Slot::required(
                "stepCount",
                number(),
                "steps the values go through — the series' instants, one fewer over intervals",
            ),
            Slot::required(
                "emptyCount",
                number(),
                "elements the base holds no value for — none at any step",
            ),
        ]
    }
}
