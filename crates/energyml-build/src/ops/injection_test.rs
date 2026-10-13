//! `injection-test` — a step-rate injection test down a wellbore: the WITSML
//! log that holds its measurements, the PRODML test that points at them, and
//! the collection of well tests that lists it.
//!
//! The reference model's injection-test case:
//!
//! | object | what it is |
//! |---|---|
//! | `witsml21.Log` + nested `ChannelSet` | the test's channels, indexed in date time: `TIME`, the injection rate, the bottom-hole pressure and temperature |
//! | `prodml23.InjectionFlowTest` | the test: where, when, and which channels hold its rate, its pressure and its other data |
//! | `prodml23.ProductionWellTests` | the collection of well tests: `FlowTestActivity` 1..n (SimpleProductVolume.xsd l. 519–522) |
//!
//! # The measurements are in a WITSML channel set
//!
//! The reference model first called for a PRODML `ChannelSet`. The PRODML Technical Usage Guide
//! v2.3 says otherwise, §13.6 p. 118: *« Channel is a WITSML object; Channels
//! are collected into Channel Sets and the Channel Set is where the actual
//! values are stored »* — PRODML's own `ChannelSet` has nothing but a
//! *« data blob in JSON form »* whose form nobody gives. Decision: the
//! guide wins. The log is written as `witsml-logs` writes one — a `Log`, its
//! `ChannelSet` and its `Channel`s in **one** part, values inline — and
//! the test references the nested channel set and channels by uuid; a
//! reference to a nested data object resolves to the part that holds it
//! (`identity::nested_objects`, same decision).
//!
//! # `TimeChannel` is a channel of dates
//!
//! `AbstractFlowTestData.TimeChannel` is *« The Channel containing the Time
//! data »* (FlowTestActivity.xsd l. 100–103). In WITSML 2.1 the time of a
//! channel set is its **index**, not a channel. Decision: a channel
//! `TIME` whose values are the instants — `DataKind` `date time`, kind
//! `absolute time` (*« A time value which is intended to fix an event in
//! time, such as […] a date »*, the standard dictionary), unit `s`: a unit of
//! its kind's quantity class `time`, as WITSML TRG v2.1 §7.1 p. 141 asks of
//! every channel (*« The Uom MUST be compatible with the QuantityClass of the
//! PropertyKind »*). The index is `date time` in `s` for the same reason —
//! *« one of the units allowed for the specified IndexKind »*, §7.5 p. 150.
//! A row of `Data` is then the index, then one value per channel in the order
//! they are listed: `["2025-06-02T00:00:00Z", "2025-06-02T00:00:00Z", q, p, T]`.
//!
//! # Every value is invented, by laws stated in the recipe
//!
//! Decision: the values are *synthetic and sourced*. A step-rate test: `steps` rates,
//! each held `stepHours`, sampled every `sampleMinutes`. The pressure answers
//! each rate through the injectivity index — `p = static + q / II` once
//! settled — approached exponentially with `responseMinutes` from where the
//! previous step left it. The temperature leaves the reservoir's towards the
//! injected water's with `coolingHours`. Every object carries
//! `synthetic = true` and the `source` in `Citation/Description`
//! (the reference-model rules on invented objects, ADR-0019), and a blank source is
//! refused.

use serde_json::{Value, json};

use energyml_core::xml::XmlText;
use energyml_domain::provenance::Parameter;
use energyml_eml_2_3 as eml;
use energyml_prodml_2_3 as prodml;
use energyml_storage::epc::core_props::w3cdtf;
use energyml_witsml_2_1 as witsml;

use super::prodml_doublet::{Standard, class_of, titled};
use super::prodml_writer::{Writer, instant};
use super::witsml_logs::{json_number, reference_from};
use crate::declare::{Inputs, ObjectIn, ObjectRef, Params};
use crate::operation::{About, ActivityDraft, Category, Context, Need, Note, Operation, Plan, Refusal, Slot};
use crate::pieces::Ref;

/// The operation.
pub struct InjectionTest;

/// The uid of the test's one period — the whole step-rate schedule. An
/// analysis names it (`PressureTransientAnalysis.PrincipalTestPeriodRef`,
/// `pressure-transient-analysis`), so it is fixed rather than left out: a
/// `TestPeriod` with no uid cannot be named.
pub const TEST_PERIOD: &str = "step-rate";

/// What `injection-test` is told.
#[derive(Debug, Params)]
pub struct InjectionTestParams {
    /// the title of the test — the log, its channels and the collection are named after it
    pub title: String,
    /// where the laws and their values come from — a document with its page or paragraph; a value stated as plausible says so (synthetic, and why)
    pub source: String,
    /// when the injection starts, a UTC date-time as 2025-06-02T00:00:00Z
    pub start: String,
    /// the injection rate of each step, in m3/h, in the order they are held
    #[param(least = 1, uom = "m3/h")]
    pub steps: Vec<f64>,
    /// how long each step is held, in hours
    #[param(uom = "h", min = 0.0)]
    pub step_hours: f64,
    /// the time between two samples, in minutes — a divisor of a step
    #[param(min = 1)]
    pub sample_minutes: usize,
    /// the bottom-hole pressure before the injection starts, in bar
    #[param(uom = "bar", min = 0.0)]
    pub static_pressure: f64,
    /// the injectivity index, in m3/h per bar: a settled step raises the pressure by its rate over this
    #[param(min = 0.0)]
    pub injectivity: f64,
    /// how fast the pressure settles on a step, in minutes — 0 for at once
    #[param(default = 0.0, optional, min = 0.0)]
    pub response_minutes: f64,
    /// the bottom-hole temperature before the injection starts, in degC
    #[param(uom = "degC")]
    pub reservoir_temperature: f64,
    /// the temperature of the injected water, in degC
    #[param(uom = "degC")]
    pub injection_temperature: f64,
    /// how fast the bottom of the hole cools towards the injected water, in hours — 0 for at once
    #[param(default = 0.0, optional, min = 0.0)]
    pub cooling_hours: f64,
    /// the reference conditions of the collection of well tests — `StandardConditions` is 1..1 on it
    pub standard_conditions: Standard,
    /// the nominal period of the collection of well tests — day, week, month, year, month to date, year to date, life to date, total cumulative
    pub nominal_period: String,
    /// how the test was conducted — its WellTestMethod
    #[param(default = String::from("step rate"), optional)]
    pub method: String,
    /// the BusinessAssociate a Channel must name — WITSML 2.1 requires it; a law-derived value has no company that logged it, so name the one responsible for the law
    #[param(need = Need::Object { types: &["eml23.BusinessAssociate"] })]
    pub logging_company: ObjectRef,
}

/// What `injection-test` takes.
#[derive(Debug, Inputs)]
pub struct InjectionTestInputs {
    /// the wellbore injected into — a WITSML `Wellbore`: its log is WITSML
    #[input(types = "witsml21.Wellbore")]
    pub wellbore: ObjectIn,
}

/// One channel of the log: what it measures and its values.
struct Measured {
    mnemonic: &'static str,
    kind: &'static str,
    uom: &'static str,
    data_kind: witsml::ChannelDataKind,
    values: Vec<Value>,
}

/// The kind the standard dictionary holds under `title`, or a refusal.
fn standard_kind(title: &str) -> Result<eml::DataObjectReference, Refusal> {
    energyml_domain::property_kind::with_default(|dictionary| {
        dictionary
            .by_title(title)
            .first()
            .map(|kind| crate::crs::reference_to(&kind.uuid, "eml23.PropertyKind", &kind.title))
    })
    .ok_or_else(|| {
        Refusal::new(
            "refused",
            format!("the dictionary in use holds no kind called `{title}`"),
        )
    })
}

/// A unit code as the schema spells it.
fn unit(code: &str) -> Result<eml::UnitOfMeasureExt, Refusal> {
    eml::UnitOfMeasureExt::from_xml_text(code)
        .map_err(|why| Refusal::new("refused", format!("`{code}` is no unit this build writes: {why}")))
}

/// A finite number, or a refusal naming the slot.
fn finite(slot: &str, value: f64, what: &str) -> Result<f64, Refusal> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(Refusal::on(slot, format!("{value} is not {what}")))
    }
}

impl Operation for InjectionTest {
    type Params = InjectionTestParams;
    type Inputs = InjectionTestInputs;

    fn about(&self) -> About {
        About {
            name: "injection-test",
            describe: "a step-rate injection test down a wellbore — its WITSML log in date time, the PRODML test pointing at the log's channels, the collection of well tests — every value stated by a law and sourced",
            category: Category::Synthetic,
            makes: &[
                "witsml21.Log",
                "prodml23.InjectionFlowTest",
                "prodml23.ProductionWellTests",
            ],
            recipe: "injection-test",
            represents: None,
        }
    }

    fn check(&self, inputs: &InjectionTestInputs) -> Result<(), Refusal> {
        match inputs
            .wellbore
            .known
            .reference
            .as_ref()
            .map(|one| class_of(&one.qualified_type))
        {
            Some("Wellbore") => Ok(()),
            _ => Err(Refusal::on(
                "wellbore",
                "the object given is no WITSML `Wellbore`: an injection test is logged down one",
            )),
        }
    }

    fn plan(
        &self,
        inputs: InjectionTestInputs,
        params: InjectionTestParams,
        cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        let title = titled("title", &params.title)?;
        let mut writer = Writer::new(cx, "injection-test", &params.source)?;
        let (start_text, start) = instant("start", &params.start)?;
        let hole: Ref = match inputs.wellbore.known.reference.as_ref() {
            Some(one) if class_of(&one.qualified_type) == "Wellbore" => one.clone(),
            Some(one) => {
                return Err(Refusal::on(
                    "wellbore",
                    format!(
                        "`{}` is a `{}`: an injection test is logged down a WITSML `Wellbore`",
                        one.title,
                        class_of(&one.qualified_type)
                    ),
                ));
            }
            None => {
                return Err(Refusal::on(
                    "wellbore",
                    format!(
                        "`{}` was given without a reference to write",
                        inputs.wellbore.name
                    ),
                ));
            }
        };
        let logging_company = reference_from(
            "loggingCompany",
            &params.logging_company.0,
            "eml23.BusinessAssociate",
        )?;
        let nominal = prodml::ReportingDurationKindExt::from_xml_text(params.nominal_period.trim())
            .ok()
            .filter(|kind| !matches!(kind, prodml::ReportingDurationKindExt::Other(_)))
            .ok_or_else(|| {
                Refusal::on(
                    "nominalPeriod",
                    format!("`{}` is none of day, week, month, year, month to date, year to date, life to date, total cumulative", params.nominal_period.trim()),
                )
            })?;

        // -- the schedule ------------------------------------------------------
        for (at, rate) in params.steps.iter().enumerate() {
            if !(rate.is_finite() && *rate >= 0.0) {
                return Err(Refusal::on(
                    "steps",
                    format!("step {} is {rate} m3/h: a rate is a finite number ≥ 0", at + 1),
                ));
            }
        }
        let step_hours = finite("stepHours", params.step_hours, "a duration")?;
        if step_hours <= 0.0 {
            return Err(Refusal::on(
                "stepHours",
                format!("{step_hours} h: a step lasts a while"),
            ));
        }
        let step_minutes = step_hours * 60.0;
        let per_step = step_minutes / params.sample_minutes as f64;
        if per_step.fract() != 0.0 || per_step < 1.0 {
            return Err(Refusal::on(
                "sampleMinutes",
                format!(
                    "{} min does not divide a step of {step_minutes} min: a sample would straddle two rates",
                    params.sample_minutes
                ),
            ));
        }
        let per_step = per_step as usize;
        let injectivity = finite("injectivity", params.injectivity, "an injectivity index")?;
        if injectivity <= 0.0 {
            return Err(Refusal::on(
                "injectivity",
                "an index of 0 would take an infinite pressure to inject anything",
            ));
        }
        let static_pressure = finite("staticPressure", params.static_pressure, "a pressure")?;
        let reservoir = finite(
            "reservoirTemperature",
            params.reservoir_temperature,
            "a temperature",
        )?;
        let injected = finite(
            "injectionTemperature",
            params.injection_temperature,
            "a temperature",
        )?;
        let response = finite("responseMinutes", params.response_minutes, "a time constant")?;
        let cooling = finite("coolingHours", params.cooling_hours, "a time constant")? * 60.0;

        // -- the samples -------------------------------------------------------
        // Sample i is at start + i × step; it belongs to step ⌊i / per_step⌋,
        // the last one ending the test. A step starts from the pressure the
        // previous one left.
        let count = params.steps.len() * per_step;
        let relax = |from: f64, to: f64, elapsed: f64, tau: f64| {
            if tau == 0.0 {
                to
            } else {
                to + (from - to) * (-elapsed / tau).exp()
            }
        };
        let mut times: Vec<String> = Vec::with_capacity(count + 1);
        let (mut rates, mut pressures, mut temperatures) = (Vec::new(), Vec::new(), Vec::new());
        let mut from = static_pressure;
        for i in 0..=count {
            let step = (i / per_step).min(params.steps.len() - 1);
            if i > 0 && i % per_step == 0 && i < count {
                // The pressure the step that ends here leaves the next.
                from = pressures.last().copied().unwrap_or(static_pressure);
            }
            let rate = params.steps[step];
            let into_step = (i - step * per_step) as f64 * params.sample_minutes as f64;
            let elapsed = i as f64 * params.sample_minutes as f64;
            let pressure = if i == 0 {
                static_pressure
            } else {
                relax(from, static_pressure + rate / injectivity, into_step, response)
            };
            let temperature = relax(reservoir, injected, elapsed, cooling);
            times.push(w3cdtf(start + (elapsed * 60.0) as i64));
            rates.push(rate);
            pressures.push(pressure);
            temperatures.push(temperature);
        }
        let end_text = times.last().cloned().unwrap_or_else(|| start_text.clone());

        let measured = [
            Measured {
                mnemonic: "TIME",
                kind: "absolute time",
                uom: "s",
                data_kind: witsml::ChannelDataKind::DateTime,
                values: times.iter().map(|one| json!(one)).collect(),
            },
            Measured {
                mnemonic: "QINJ",
                kind: "water flowrate",
                uom: "m3/h",
                data_kind: witsml::ChannelDataKind::Double,
                values: rates.iter().map(|v| json_number(*v)).collect(),
            },
            Measured {
                mnemonic: "BHP",
                kind: "pressure",
                uom: "bar",
                data_kind: witsml::ChannelDataKind::Double,
                values: pressures.iter().map(|v| json_number(*v)).collect(),
            },
            Measured {
                mnemonic: "BHT",
                kind: "temperature",
                uom: "degC",
                data_kind: witsml::ChannelDataKind::Double,
                values: temperatures.iter().map(|v| json_number(*v)).collect(),
            },
        ];

        // -- the uuids, from what they are -------------------------------------
        let digest = format!(
            "{}:{start_text}:{:?}:{step_hours:?}:{}:{static_pressure:?}:{injectivity:?}:{response:?}:{reservoir:?}:{injected:?}:{cooling:?}",
            hole.uuid.to_ascii_lowercase(),
            params.steps,
            params.sample_minutes
        );
        let log_uuid = writer.uuid(&format!("log:{digest}"))?;
        let set_uuid = writer.uuid(&format!("channel-set:{digest}"))?;
        let channel_uuids: Vec<String> = measured
            .iter()
            .map(|one| writer.uuid(&format!("channel:{}:{digest}", one.mnemonic)))
            .collect::<Result<_, _>>()?;
        let test_uuid = writer.uuid(&format!("test:{digest}"))?;
        let tests_uuid = writer.uuid(&format!("tests:{test_uuid}"))?;

        let hole_dor = crate::crs::reference_of(&hole);
        let index = witsml::ChannelIndex {
            index_kind: eml::DataIndexKind::DateTime,
            index_property_kind: None,
            uom: unit("s")?,
            direction: eml::IndexDirection::Increasing,
            mnemonic: "TIME".to_string(),
            datum: None,
            index_interval: None,
        };

        // -- the log ----------------------------------------------------
        let mut channels = Vec::new();
        let mut channel_refs: Vec<eml::DataObjectReference> = Vec::new();
        for (one, uuid) in measured.iter().zip(&channel_uuids) {
            let channel = witsml::Channel {
                citation: writer.citation(
                    one.mnemonic,
                    &format!(
                        "the `{}` of an invented injection test of `{}`",
                        one.kind, hole.title
                    ),
                )?,
                extension_name_value: vec![writer.synthetic("a channel of an invented injection test")],
                uuid: uuid.clone(),
                schema_version: "2.1".to_string(),
                active_status: eml::ActiveStatusKind::Inactive,
                mnemonic: one.mnemonic.to_string(),
                global_mnemonic: one.mnemonic.to_string(),
                data_kind: one.data_kind.clone(),
                uom: unit(one.uom)?,
                channel_property_kind: standard_kind(one.kind)?,
                logging_company: logging_company.clone(),
                is_continuous: Some(true),
                index: vec![index.clone()],
                wellbore: Some(hole_dor.clone()),
                ..Default::default()
            };
            channel_refs.push(crate::crs::reference_to(uuid, "witsml21.Channel", one.mnemonic));
            channels.push(witsml::AnyChannel::Channel(channel));
        }
        let rows: Vec<Value> = (0..times.len())
            .map(|at| {
                let mut row = vec![json!(times[at])];
                row.extend(measured.iter().map(|one| one.values[at].clone()));
                Value::Array(row)
            })
            .collect();
        let data = serde_json::to_string(&Value::Array(rows))
            .map_err(|why| Refusal::new("refused", format!("the channel data did not encode: {why}")))?;
        let set_title = format!("{title} — channels");
        let channel_set = witsml::ChannelSet {
            citation: writer.citation(
                &set_title,
                &format!(
                    "the channels of an invented injection test of `{}`, in date time",
                    hole.title
                ),
            )?,
            extension_name_value: vec![writer.synthetic("the channels of an invented injection test")],
            uuid: set_uuid.clone(),
            schema_version: "2.1".to_string(),
            active_status: eml::ActiveStatusKind::Inactive,
            index: vec![index],
            channel: channels,
            data: Some(witsml::ChannelData {
                data: Some(data),
                file_uri: None,
            }),
            wellbore: Some(hole_dor.clone()),
            ..Default::default()
        };
        let log_title = format!("{title} — Log");
        let log = witsml::Log {
            citation: writer.citation(
                &log_title,
                &format!(
                    "the log of an invented injection test of `{}`, from {start_text} to {end_text}",
                    hole.title
                ),
            )?,
            extension_name_value: vec![writer.synthetic("the log of an invented injection test")],
            uuid: log_uuid.clone(),
            schema_version: "2.1".to_string(),
            active_status: eml::ActiveStatusKind::Inactive,
            channel_set: vec![witsml::AnyChannelSet::ChannelSet(channel_set)],
            wellbore: Some(hole_dor.clone()),
            ..Default::default()
        };
        writer.push(&log_uuid, "witsml21.Log", "log", &log_title, &log);

        // -- the test ---------------------------------------------------
        let set_ref = crate::crs::reference_to(&set_uuid, "witsml21.ChannelSet", &set_title);
        let [time_ref, rate_ref, pressure_ref, temperature_ref] =
            [0, 1, 2, 3].map(|at| channel_refs[at].clone());
        let test_title = title.clone();
        let test = prodml::InjectionFlowTest {
            citation: writer.citation(
                &test_title,
                &format!(
                    "an invented step-rate injection test of `{}`, {} step(s) of {step_hours} h",
                    hole.title,
                    params.steps.len()
                ),
            )?,
            extension_name_value: vec![writer.synthetic("an invented injection test")],
            uuid: test_uuid.clone(),
            schema_version: "2.3".to_string(),
            validated: Some(false),
            well_test_method: Some(params.method.trim().to_string()).filter(|one| !one.is_empty()),
            effective_date: Some(start_text.clone()),
            interval_measurement_set: prodml::FlowTestMeasurementSet {
                location: prodml::FlowTestLocation {
                    wellbore: Some(hole_dor.clone()),
                    ..Default::default()
                },
                test_period: Some(prodml::TestPeriod {
                    start_time: start_text.clone(),
                    end_time: end_text.clone(),
                    test_period_kind: Some(if params.steps.len() == 1 {
                        prodml::TestPeriodKind::ConstantRateInjection
                    } else {
                        prodml::TestPeriodKind::VariableRateInjection
                    }),
                    uid: Some(TEST_PERIOD.to_string()),
                    ..Default::default()
                }),
                measured_flow_rate: vec![prodml::AbstractPtaFlowData::MeasuredFlowData(
                    prodml::MeasuredFlowData {
                        channel_set: set_ref.clone(),
                        time_channel: time_ref.clone(),
                        time_series_point_representation: Some(
                            prodml::TimeSeriesPointRepresentation::PointByPoint,
                        ),
                        fluid_phase_measured_kind: Some(prodml::FluidPhaseKind::SinglePhaseWater),
                        flow_channel: rate_ref,
                        ..Default::default()
                    },
                )],
                measured_pressure: vec![prodml::MeasuredPressureData {
                    channel_set: set_ref.clone(),
                    time_channel: time_ref.clone(),
                    time_series_point_representation: Some(
                        prodml::TimeSeriesPointRepresentation::PointByPoint,
                    ),
                    pressure_channel: pressure_ref,
                    ..Default::default()
                }],
                other_data: vec![prodml::OtherData {
                    channel_set: set_ref,
                    time_channel: time_ref,
                    time_series_point_representation: Some(
                        prodml::TimeSeriesPointRepresentation::PointByPoint,
                    ),
                    remark: Some("the bottom-hole temperature".to_string()),
                    data_channel: temperature_ref,
                    ..Default::default()
                }],
                ..Default::default()
            },
            ..Default::default()
        };
        writer.push(
            &test_uuid,
            "prodml23.InjectionFlowTest",
            "test",
            &test_title,
            &test,
        );

        // -- the collection ---------------------------------------------
        let tests_title = format!("{title} — well tests");
        let tests = prodml::ProductionWellTests {
            citation: writer.citation(
                &tests_title,
                &format!("the well tests of `{}`: one invented injection test", hole.title),
            )?,
            extension_name_value: vec![writer.synthetic("a collection of invented well tests")],
            uuid: tests_uuid.clone(),
            schema_version: "2.3".to_string(),
            standard_conditions: Some(eml::AbstractTemperaturePressure::ReferenceTemperaturePressure(
                eml::ReferenceTemperaturePressure {
                    reference_temp_pres: params.standard_conditions.written(),
                },
            )),
            nominal_period_kind: nominal,
            start_date: start_text.clone(),
            end_date: end_text.clone(),
            flow_test_activity: vec![crate::crs::reference_to(
                &test_uuid,
                "prodml23.InjectionFlowTest",
                &test_title,
            )],
            ..Default::default()
        };
        writer.push(
            &tests_uuid,
            "prodml23.ProductionWellTests",
            "tests",
            &tests_title,
            &tests,
        );

        let settled: Vec<String> = params
            .steps
            .iter()
            .map(|q| format!("{:.2}", static_pressure + q / injectivity))
            .collect();
        let says = vec![
            Note {
                key: "channels",
                detail: format!(
                    "{} sample(s) from {start_text} to {end_text}, every {} min: TIME, QINJ (m3/h), BHP (bar), BHT (degC) in one WITSML channel set, inline",
                    times.len(),
                    params.sample_minutes
                ),
            },
            Note { key: "pressure", detail: format!("settled bottom-hole pressure per step: {} bar", settled.join(", ")) },
            Note {
                key: "witsml",
                detail: "the measurements are in a WITSML channel set, as the PRODML Technical Usage Guide v2.3 §13.6 p. 118 puts them; the test references its nested channels".to_string(),
            },
            Note {
                key: "synthetic",
                detail: "every object written is invented: `synthetic = true` and the source in Citation/Description".to_string(),
            },
        ];
        let parameters = vec![
            Parameter::Object {
                title: "input".to_string(),
                reference: hole.dor(),
            },
            Parameter::Double {
                title: "sampleCount".to_string(),
                value: times.len() as f64,
                uom: None,
            },
        ];
        Ok(Plan {
            media: Vec::new(),
            blocked: None,
            activity: ActivityDraft {
                recipe: "injection-test",
                title: format!("injection-test → {title}"),
                seed: writer.activity_seed(),
                parameters,
            },
            objects: writer.objects,
            datasets: Vec::new(),
            says,
            dropped: Vec::new(),
            shapes: Vec::new(),
        })
    }

    /// What the run wrote.
    fn reports(&self) -> Vec<Slot> {
        vec![Slot::required(
            "sampleCount",
            Need::Number {
                uom: None,
                least: None,
                most: None,
            },
            "how many instants the log holds",
        )]
    }
}
