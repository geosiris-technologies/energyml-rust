//! `prodml-doublet` — the PRODML 2.3 side of a geothermal doublet: the facility
//! its loop runs through, a reporting entity per wellbore, the producer's and
//! the injector's rates period by period, their head series at a fixed step
//! with a daily statistic, and the fluid they carry.
//!
//! Step 11 of the synthetic model Lutetia: the
//! doublet GTH-1 (producer) and GTH-2 (injector), GTH-3 observing. What
//! the reference-model description lists is written **where it can be written honestly**, the objects it
//! calls essential first — `ReportingEntity` and `Facility`,
//! `FluidSystem`, the volumes (SPVR), `TimeSeriesData` (the *Must* items):
//!
//! | object | what it says |
//! |---|---|
//! | `Facility` | the heat exchanger, `processing facility` |
//! | `ReportingEntity` ×2–3 | one per wellbore, kind `wellbore`, `AssociatedObject` → the wellbore |
//! | `WellProductionParameters` ×2 | producer and injector, a `ProductionWellPeriod` per period, `ReportingEntity` 1..1 |
//! | `TimeSeriesData` ×3 | the producer's head pressure and temperature, the injector's rate |
//! | `TimeSeriesStatistic` | the injector's rate over the series' first day |
//! | `FluidSystem` | `unknown`, water, a gas-oil ratio stated negligible |
//!
//! # A wellbore, never a well
//!
//! A reporting entity of a doublet is a **wellbore**: its
//! `Kind` is `wellbore` and its `AssociatedObject` references *“a subsurface
//! entity such as well, wellbore […] which can be described with a specific
//! Energistics object”* (ReportingEntity.xsd, the documentation of the field).
//! The slots take a WITSML `Wellbore` — the reference model's choice — or the RESQML
//! `WellboreFeature` of the same hole; a `Well` is refused by its name (the
//! flow of GTH-3 and of its sidetrack is not one number), and so is an
//! interpretation, which is an opinion of a hole and not the hole. The
//! *“BUSINESS RULE: The kind of Reporting Entity is expected to be a well, a
//! wellbore, a well completion or wellbore completion”* of the PRODML
//! Technical Usage Guide (v2.2, §5.4 p. 28) is met by construction.
//!
//! # n instants make n − 1 periods
//!
//! A `ProductionWellPeriod` runs from one instant to the next: its `StartTime`
//! is the first, its `Duration` the hours to the second — the arithmetic that
//! gives n − 1 periods for n instants (13 dates give 12 values: the only
//! arithmetic that makes sense; not yet confirmed), and the plan says it. The monthly
//! series has 13 instants, so each rate report holds 12 periods.
//!
//! **No PRODML field references a `TimeSeries`**: `ProductionWellPeriod` has a
//! start and a duration, `TimeSeriesData` its own `dTim` (the
//! monthly series serves both, by the dates). The series is therefore
//! an input for the provenance — the activity names it — and its instants
//! are **stated** to the run (`times`): a plan does not read the package, and
//! nothing at the boundary resolves the instants of a series. The plan says so
//! (`instants-stated`); a caller that states other dates than the series has
//! is not caught here.
//!
//! # A period's rate is the law integrated over it
//!
//! The law is `energyml_analysis::flow_law`: a rate in m3/h at the first
//! instant and an exponential decline per year (0: constant). Each period
//! states its **mean** rate — the law integrated over the period, divided by
//! its hours — so that rate × duration gives back the volume, and a declining
//! well is not overstated by the rate at the start of the month. The volumes
//! themselves are reported in the activity (`producedVolume`,
//! `injectedVolume`, m3): the reference model writes the monthly volumes as the rates of
//! `WellProductionParameters`, not as an `AssetProductionVolumes`.
//!
//! The injector returns `reinjected` of what the producer pumps — 1 by
//! default: the doublet is **a loop**, producer → exchanger →
//! reinjector. Its periods say `active --
//! injecting`, the producer's `active -- producing` (`WellStatus`, the one
//! field of a period that tells the two apart).
//!
//! # The head series and the key convention
//!
//! `TimeSeriesData` names its well **only by its keys** (TimeSeriesData.xsd:
//! `Key` is text, no reference). The convention is the reference model's, sourced:
//! an `asset identifier` that is an Energistics URI —
//! `eml://lutetia.geosiris.com/well(GTH-1)/wellbore(01)/wellhead`, Identifier
//! Specification v4.0 §6.3 p. 14, *“path-part = component-type [ "("
//! identifier ")" ]”*. The caller writes the pattern with `{wellbore}` where
//! the wellbore's title goes; the title is percent-encoded
//! (*“identifier = 1*( unreserved / pct-encoded )”*) and the result is
//! checked against that grammar, `unknown` refused (*“The type unknown is
//! explicitly forbidden in an Energistics URI”*, §6.3.2 p. 15). A URI built
//! from the title of the wellbore given **names an object of the package**:
//! the anomaly of a `TimeSeriesData` whose asset identifier resolves to
//! nothing cannot come from here.
//!
//! Two departures from the example it follows, each for a stated reason:
//!
//! - `qualifier` is **`simulated`**, not `measured`: the values are invented
//!   (an invented value says so), and `simulated` is the other half
//!   of the PRODML example itself — *“measured versus simulated”* (Product
//!   Volume, Network Model & Time Series Usage Guide, §2.3 p. 9);
//! - `product` is written for the **rate** only: *“It is generally
//!   meaningless for things like temperature or pressure”* (same guide, p.
//!   10).
//!
//! The pressure and the temperature are the values stated, constant over the
//! window — no variation is invented. A `frozen` gap, when asked, repeats the
//! last reading before it with `status="frozen"`: *“a status of frozen, which
//! indicates the sensor has been reading the same value for a specific period
//! of time”* (same guide, §8.2 p. 32).
//!
//! # The daily statistic
//!
//! The statistic is one `TimeSeriesStatistic`, a daily one: the injector's rate
//! over the first day of the window, `[start, start + 1 day)`, frozen samples
//! set aside and counted — a frozen reading is not a measurement of that
//! minute. The schema types `Minimum`, `Maximum` and `Mean` as
//! `DimensionlessMeasure` and puts the unit in the statistic's `Uom`
//! (*“Element "unit" defines the unit of measure of this value”*); they are
//! written with the dimensionless `Euc` their type requires, and the plan says
//! so.
//!
//! # Every object is synthetic, and sourced
//!
//! The rules on invented objects (ADR-0019): `ExtensionNameValue` `synthetic = true` and a sentence
//! in `Citation/Description` on every object invented, and no constant
//! without a source — `source` is required, and a blank one is refused.
//!
//! # Identity
//!
//! The uuid of each object comes from what it is: the facility from its
//! title, a reporting entity from its wellbore (the same hole reports as the
//! same entity from one run to the next), a rate report from its wellbore,
//! its instants and its law, a head series from its asset, its window and its
//! values. Another law is a second report, never a rewrite of the first.
//!
//! # Not written here, by name
//!
//! - the sample chain, the water analysis and the fluid
//!   characterisation: `fluid-sample` (`fluid_sample.rs`);
//! - DTS: `dts-installation`; `ProductFlowModel`:
//!   `product-flow-model` — each its own operation, sharing this file's
//!   helpers through `prodml_writer.rs`;
//! - the injectivity test and its PTA: an `InjectionFlowTest`
//!   has no global element in FlowTestActivity.xsd v2.3 (only
//!   `FlowTestActivity` has one), so a conformant part is a
//!   `FlowTestActivity` root with an `xsi:type`, which no part writer here
//!   makes; and the PTA cites grid properties no step
//!   writes yet.
//!
//! # References
//!
//! - The flow law is a synthetic law stated in `energyml-analysis`, which cites no published source for it.
//!   TODO(reference): verify — no publication is cited; if the law follows one, add it (authors, year, title, venue, DOI) once checked.

use std::ops::Range;

use energyml_analysis::flow_law::{self, Law, LawError, Period};
use energyml_domain::provenance::Parameter;
use energyml_eml_2_3 as eml;
use energyml_prodml_2_3 as prodml;
use energyml_storage::epc::core_props::{unix_seconds, w3cdtf};

use crate::declare::{Choice, Inputs, ObjectIn, Params};
use crate::operation::{
    About, ActivityDraft, Category, Context, Need, NewObject, Note, Operation, Plan, Refusal, Slot,
};
use crate::pieces::Ref;

/// The operation.
pub struct ProdmlDoublet;

/// A day, in seconds.
const DAY: i64 = 86_400;

/// Minutes in a day — what a step must divide.
const MINUTES_PER_DAY: usize = 1_440;

/// A head series nobody meant to ask for. Its samples are written **inline**,
/// in the XML of the object — `TimeSeriesData` has no array to point at — and
/// a hundred thousand of them is already a few megabytes per series.
const MOST_SAMPLES: usize = 100_000;

/// Where the wellbore's title goes in the asset identifier.
const HOLE: &str = "{wellbore}";

/// Why the fluid system says `unknown` and a gas-oil ratio of zero.
const FLUID_REMARK: &str = "ReservoirFluidKind is `unknown`: its list holds six hydrocarbon fluids and `unknown`, and fresh aquifer water is none of them. SolutionGOR is written 0: negligible for this water, declared as such rather than left NaN — not a measurement.";

// ---------------------------------------------------------------- the declaration

/// The reference conditions eml 2.3 names (`ReferenceCondition`) — less
/// `ambient`, which states neither a temperature nor a pressure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Choice)]
pub enum Standard {
    /// 0 °C and one standard atmosphere.
    #[choice(name = "0 degC 1 atm")]
    ZeroCelsiusAtm,
    /// 0 °C and one bar.
    #[choice(name = "0 degC 1 bar")]
    ZeroCelsiusBar,
    /// 15 °C and one standard atmosphere.
    #[choice(name = "15 degC 1 atm")]
    FifteenCelsiusAtm,
    /// 15 °C and one bar.
    #[choice(name = "15 degC 1 bar")]
    FifteenCelsiusBar,
    /// 20 °C and one standard atmosphere.
    #[choice(name = "20 degC 1 atm")]
    TwentyCelsiusAtm,
    /// 20 °C and one bar.
    #[choice(name = "20 degC 1 bar")]
    TwentyCelsiusBar,
    /// 25 °C and one bar.
    #[choice(name = "25 degC 1 bar")]
    TwentyFiveCelsiusBar,
    /// 60 °F and one standard atmosphere.
    #[choice(name = "60 degF 1 atm")]
    SixtyFahrenheitAtm,
    /// 60 °F and 30 inches of mercury.
    #[choice(name = "60 degF 30 in Hg")]
    SixtyFahrenheitInHg,
}

impl Standard {
    /// The same conditions, as eml 2.3 writes them.
    pub(crate) fn written(self) -> eml::ReferenceConditionExt {
        match self {
            Standard::ZeroCelsiusAtm => eml::ReferenceConditionExt::V0DegC1Atm,
            Standard::ZeroCelsiusBar => eml::ReferenceConditionExt::V0DegC1Bar,
            Standard::FifteenCelsiusAtm => eml::ReferenceConditionExt::V15DegC1Atm,
            Standard::FifteenCelsiusBar => eml::ReferenceConditionExt::V15DegC1Bar,
            Standard::TwentyCelsiusAtm => eml::ReferenceConditionExt::V20DegC1Atm,
            Standard::TwentyCelsiusBar => eml::ReferenceConditionExt::V20DegC1Bar,
            Standard::TwentyFiveCelsiusBar => eml::ReferenceConditionExt::V25DegC1Bar,
            Standard::SixtyFahrenheitAtm => eml::ReferenceConditionExt::V60DegF1Atm,
            Standard::SixtyFahrenheitInHg => eml::ReferenceConditionExt::V60DegF30InHg,
        }
    }
}

/// What `prodml-doublet` is told.
#[derive(Debug, Params)]
pub struct ProdmlDoubletParams {
    /// the title of the surface facility the doublet's loop runs through — its heat exchanger
    pub facility: String,
    /// the title of the fluid system — the water the doublet pumps
    pub fluid: String,
    /// where the law and the head values come from — a document with its page or paragraph; a value stated as plausible says so (synthetic, and why)
    pub source: String,
    /// the instants that bound the periods, as the series given lists them: increasing UTC date-times as 2025-01-01T00:00:00Z — n instants make n − 1 periods
    #[param(least = 2)]
    pub times: Vec<String>,
    /// the producer's flow rate at the first instant, in m3/h
    #[param(uom = "m3/h", min = 0.0)]
    pub rate: f64,
    /// the rate's exponential decline, a fraction per year of 365.25 days — 0 for a constant rate
    #[param(default = 0.0, optional, min = 0.0)]
    pub decline: f64,
    /// the share of the pumped water the injector returns to the aquifer — 1 for a doublet, whose loop returns what it pumps
    #[param(default = 1.0, optional, min = 0.0, max = 1.0)]
    pub reinjected: f64,
    /// the reference conditions the rates and the fluid system are stated at
    pub standard_conditions: Standard,
    /// the water temperature at the producer's head, in degC
    #[param(uom = "degC")]
    pub producer_head_temperature: f64,
    /// the water temperature at the injector's head, in degC — left out, none is written
    #[param(uom = "degC")]
    pub injector_head_temperature: Option<f64>,
    /// the pressure at the producer's head, in bar
    #[param(uom = "bar", min = 0.0)]
    pub producer_head_pressure: f64,
    /// the asset identifier of a head series: an Energistics URI with {wellbore} where the wellbore's title goes — eml://authority/well({wellbore})/wellbore(01)/wellhead
    pub asset_identifier: String,
    /// the first sample of the head series, a UTC date-time as 2025-06-02T00:00:00Z
    pub samples_from: String,
    /// how many whole days the head series cover
    #[param(min = 1)]
    pub sample_days: usize,
    /// the time between two samples of a head series, in minutes — a divisor of a day
    #[param(min = 1)]
    pub sample_step: usize,
    /// A head series whose sensor froze for a while.
    #[param(flatten)]
    pub frozen: Frozen,
}

/// which head series had its sensor frozen for a while — none, or the one named
#[derive(Debug, Params)]
#[params(tag = "frozen")]
pub enum Frozen {
    /// No sensor froze.
    #[case(default, name = "none")]
    Never,
    /// The producer's head pressure.
    Pressure(FrozenWindow),
    /// The producer's head temperature.
    Temperature(FrozenWindow),
    /// The injector's rate.
    Rate(FrozenWindow),
}

/// When a sensor froze, and for how long.
#[derive(Debug, Params)]
pub struct FrozenWindow {
    /// with a frozen sensor: its first frozen sample, a UTC date-time
    pub frozen_from: String,
    /// with a frozen sensor: how many minutes it stayed frozen
    #[param(min = 1)]
    pub frozen_minutes: usize,
}

/// What `prodml-doublet` takes.
#[derive(Debug, Inputs)]
pub struct ProdmlDoubletInputs {
    /// the producing wellbore of the doublet — a WITSML `Wellbore`, or the RESQML `WellboreFeature` of the hole
    #[input(types = "witsml21.Wellbore, resqml22.WellboreFeature")]
    pub producer: ObjectIn,
    /// the injecting wellbore of the doublet — a WITSML `Wellbore`, or the RESQML `WellboreFeature` of the hole
    #[input(types = "witsml21.Wellbore, resqml22.WellboreFeature")]
    pub injector: ObjectIn,
    /// the time series whose instants bound the periods — named in the activity, its instants stated in `times`
    #[input(types = "eml23.TimeSeries")]
    pub series: ObjectIn,
    /// an observing wellbore — a reporting entity, which neither produces nor injects
    #[input(types = "witsml21.Wellbore, resqml22.WellboreFeature")]
    pub observation: Option<ObjectIn>,
    /// the rock-fluid organisation the fluid system belongs to
    #[input(types = "resqml22.RockFluidOrganizationInterpretation")]
    pub fluid_organization: Option<ObjectIn>,
}

// ---------------------------------------------------------------- the inputs

/// The class of a qualified type — `Wellbore` of `witsml21.Wellbore`.
pub(crate) fn class_of(qualified_type: &str) -> &str {
    qualified_type.rsplit('.').next().unwrap_or(qualified_type)
}

/// What an object input stands for, or a refusal naming its slot.
pub(crate) fn reference<'a>(slot: &str, input: &'a ObjectIn) -> Result<&'a Ref, Refusal> {
    input.known.reference.as_ref().ok_or_else(|| {
        Refusal::on(
            slot,
            format!("`{}` was given without a reference to write", input.name),
        )
    })
}

/// A wellbore, or why the object given is none.
fn wellbore<'a>(slot: &str, input: &'a ObjectIn) -> Result<&'a Ref, Refusal> {
    wellbore_because(
        slot,
        input,
        "a doublet reports per wellbore: a well and its sidetrack do not flow as one — give the wellbore",
        "a reporting entity of a doublet is a wellbore: a WITSML `Wellbore`, or the RESQML `WellboreFeature` of the hole",
    )
}

/// A wellbore — a WITSML `Wellbore` or the RESQML `WellboreFeature` of the
/// hole — or why the object given is none: `not_a_well` follows *“is a
/// `Well`, and”*, `other` follows *“is a `…`, and”*. Shared by the PRODML
/// operations, each saying why it wants a hole.
pub(crate) fn wellbore_because<'a>(
    slot: &str,
    input: &'a ObjectIn,
    not_a_well: &str,
    other: &str,
) -> Result<&'a Ref, Refusal> {
    let known = reference(slot, input)?;
    match class_of(&known.qualified_type) {
        "Wellbore" | "WellboreFeature" => Ok(known),
        "Well" => Err(Refusal::on(
            slot,
            format!("`{}` is a `Well`, and {not_a_well}", known.title),
        )),
        "WellboreInterpretation" => Err(Refusal::on(
            slot,
            format!(
                "`{}` is a `WellboreInterpretation` — an opinion of a hole, not the hole: give the WITSML `Wellbore`, or the `WellboreFeature` it interprets",
                known.title
            ),
        )),
        found => Err(Refusal::on(
            slot,
            format!("`{}` is a `{found}`, and {other}", known.title),
        )),
    }
}

/// An object of one of these classes, or a refusal naming what it is.
pub(crate) fn of_class<'a>(
    slot: &str,
    input: &'a ObjectIn,
    classes: &[&str],
    why: &str,
) -> Result<&'a Ref, Refusal> {
    let known = reference(slot, input)?;
    let class = class_of(&known.qualified_type);
    if classes.contains(&class) {
        Ok(known)
    } else {
        Err(Refusal::on(
            slot,
            format!("`{}` is a `{class}`, and {why}", known.title),
        ))
    }
}

/// Two references to one object.
pub(crate) fn same(one: &Ref, other: &Ref) -> bool {
    one.uuid.eq_ignore_ascii_case(&other.uuid)
}

// ---------------------------------------------------------------- the asset identifier

/// A title as the identifier of an Energistics URI: RFC 3986 unreserved
/// characters as they are, every other byte `%XX` — *“identifier = 1*(
/// unreserved / pct-encoded )”* (Identifier Specification v4.0 §6.3, p. 14).
fn identifier(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Whether `text` is an identifier of that grammar — one character at least.
fn is_identifier(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.is_empty() {
        return false;
    }
    let mut at = 0;
    while at < bytes.len() {
        let byte = bytes[at];
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            at += 1;
        } else if byte == b'%'
            && at + 2 < bytes.len()
            && bytes[at + 1].is_ascii_hexdigit()
            && bytes[at + 2].is_ascii_hexdigit()
        {
            at += 3;
        } else {
            return false;
        }
    }
    true
}

/// What is wrong with `uri` as an Energistics URI — `eml:`, an optional
/// `//authority`, then `/`-separated path parts `component-type [ "("
/// identifier ")" ]` (Identifier Specification v4.0 §6.1–§6.3, p. 13–15).
fn uri_fault(uri: &str) -> Option<String> {
    if !uri
        .get(..4)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("eml:"))
    {
        return Some("it does not start with the scheme `eml:`".to_string());
    }
    let mut rest = &uri[4..];
    if let Some(after) = rest.strip_prefix("//") {
        match after.find('/') {
            None => return Some("an authority is followed by a path, which starts with `/`".to_string()),
            Some(0) => return Some("`//` opens an authority and names none".to_string()),
            Some(end) => rest = &after[end..],
        }
    }
    let Some(path) = rest.strip_prefix('/') else {
        return Some("the path starts with `/`".to_string());
    };
    for part in path.split('/') {
        if part.is_empty() {
            return Some("a path part is empty — two `/` in a row, or one at the end".to_string());
        }
        let (kind, inner) = match part.find('(') {
            None => (part, None),
            Some(open) => match part[open + 1..].strip_suffix(')') {
                Some(inner) => (&part[..open], Some(inner)),
                None => return Some(format!("`{part}` opens `(` and does not end with `)`")),
            },
        };
        if kind.is_empty() || kind.contains(')') {
            return Some(format!("`{part}` names no component type"));
        }
        if kind.eq_ignore_ascii_case("unknown") {
            return Some("the component type `unknown` is explicitly forbidden (§6.3.2)".to_string());
        }
        if let Some(inner) = inner
            && !is_identifier(inner)
        {
            return Some(format!(
                "`{inner}` in `{part}` is no identifier: unreserved characters and `%XX` escapes, one at least"
            ));
        }
    }
    None
}

/// The asset identifier of a wellbore's head, from the caller's pattern.
fn asset(pattern: &str, title: &str) -> Result<String, Refusal> {
    let pattern = pattern.trim();
    if !pattern.contains(HOLE) {
        return Err(Refusal::on(
            "assetIdentifier",
            format!(
                "`{pattern}` names no wellbore: write `{HOLE}` where the wellbore's title goes — without it, every head series would name one asset"
            ),
        ));
    }
    let uri = pattern.replace(HOLE, &identifier(title));
    match uri_fault(&uri) {
        None => Ok(uri),
        Some(why) => Err(Refusal::on(
            "assetIdentifier",
            format!("`{uri}` is no Energistics URI: {why} (Identifier Specification v4.0 §6)"),
        )),
    }
}

// ---------------------------------------------------------------- the pieces written

/// Which way a wellbore flows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    Producing,
    Injecting,
}

impl Direction {
    /// `production` or `injection` — the words of PRODML's `ReportingFlow`.
    fn word(self) -> &'static str {
        match self {
            Direction::Producing => prodml::ReportingFlow::Production.as_xml_str(),
            Direction::Injecting => prodml::ReportingFlow::Injection.as_xml_str(),
        }
    }

    fn status(self) -> eml::WellStatus {
        match self {
            Direction::Producing => eml::WellStatus::ActiveMinusMinusProducing,
            Direction::Injecting => eml::WellStatus::ActiveMinusMinusInjecting,
        }
    }
}

/// Which head series.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Head {
    Pressure,
    Temperature,
    Rate,
}

/// `synthetic = true`: the mark of an invented value.
fn synthetic(what: &str) -> eml::ExtensionNameValue {
    synthetic_by("prodml-doublet", what)
}

/// `synthetic = true`, naming the operation that invented the object.
pub(crate) fn synthetic_by(operation: &str, what: &str) -> eml::ExtensionNameValue {
    eml::ExtensionNameValue {
        name: "synthetic".to_string(),
        value: eml::StringMeasure {
            value: "true".to_string(),
            uom: None,
        },
        description: Some(format!("{what} — written by `{operation}`, not measured")),
        ..Default::default()
    }
}

/// A keyword and its value.
fn key(keyword: prodml::TimeSeriesKeyword, value: &str) -> prodml::KeywordValueStruct {
    prodml::KeywordValueStruct {
        value: value.to_string(),
        keyword,
    }
}

/// The samples of a head series, the frozen ones said.
fn samples(at: &[i64], values: &[f64], frozen: Option<&Range<usize>>) -> Vec<prodml::AbstractValue> {
    at.iter()
        .zip(values)
        .enumerate()
        .map(|(rank, (at, value))| {
            prodml::AbstractValue::DoubleValue(prodml::DoubleValue {
                double_value: prodml::TimeSeriesDoubleSample {
                    value: *value,
                    d_tim: Some(w3cdtf(*at)),
                    status: frozen
                        .is_some_and(|range| range.contains(&rank))
                        .then_some(prodml::ValueStatus::Frozen),
                },
            })
        })
        .collect()
}

/// The samples a frozen window covers — or why it covers none that can be
/// frozen.
fn frozen_range(window: &FrozenWindow, from: i64, end: i64, at: &[i64]) -> Result<Range<usize>, Refusal> {
    let text = window.frozen_from.trim();
    let Some(start) = unix_seconds(text) else {
        return Err(Refusal::on(
            "frozenFrom",
            format!(
                "`{text}` is not a UTC date-time as `2025-06-02T12:00:00Z`: it is refused rather than read some other way"
            ),
        ));
    };
    let stop = start + window.frozen_minutes as i64 * 60;
    if start < from || stop > end {
        return Err(Refusal::on(
            "frozenFrom",
            format!(
                "`{text}` for {} minute(s) is not within the head series, from `{}` to `{}`",
                window.frozen_minutes,
                w3cdtf(from),
                w3cdtf(end)
            ),
        ));
    }
    let first = at.iter().position(|one| *one >= start).unwrap_or(at.len());
    let last = at.iter().position(|one| *one >= stop).unwrap_or(at.len());
    if first == 0 {
        return Err(Refusal::on(
            "frozenFrom",
            "the gap starts at the first sample: a frozen sensor repeats its last reading, and there is none before it",
        ));
    }
    if first >= last {
        return Err(Refusal::on(
            "frozenMinutes",
            format!(
                "the gap from `{text}` covers no sample: {} minute(s) fall between two samples",
                window.frozen_minutes
            ),
        ));
    }
    Ok(first..last)
}

/// A refusal of the instants, naming the one at fault.
fn refused_instants(stated: &[(String, i64)], why: LawError) -> Refusal {
    match why {
        LawError::NotIncreasing { rank, .. } if rank >= 2 && rank <= stated.len() => Refusal::on(
            "times",
            format!(
                "instant {rank} `{}` does not come after `{}`: a period runs from one instant to the next, so the instants increase and none repeats",
                stated[rank - 1].0,
                stated[rank - 2].0
            ),
        ),
        other => Refusal::on("times", other),
    }
}

/// A title the caller gave, or a refusal: it names objects, so it is not
/// blank, and a `Citation/Title` holds 256 characters.
pub(crate) fn titled(slot: &str, given: &str) -> Result<String, Refusal> {
    let title = given.trim();
    if title.is_empty() {
        return Err(Refusal::on(
            slot,
            "is empty: it is the title of an object this operation writes",
        ));
    }
    if title.chars().count() > 256 {
        return Err(Refusal::on(
            slot,
            format!(
                "has {} characters, and a Citation/Title holds 256",
                title.chars().count()
            ),
        ));
    }
    Ok(title.to_string())
}

/// The law, in the words a description and a note use.
fn law_words(law: &Law, anchor: &str) -> String {
    if law.decline == 0.0 {
        format!("a constant {} m3/h", law.rate)
    } else {
        format!(
            "{} m3/h at {anchor}, declining exponentially by {} a year (a year of 365.25 days)",
            law.rate, law.decline
        )
    }
}

/// One head series before it is written.
struct Series<'a> {
    head: Head,
    title: String,
    what: &'static str,
    hole: &'a Ref,
    keys: Vec<prodml::KeywordValueStruct>,
    uom: eml::UnitOfMeasureExt,
    class: eml::MeasureClass,
    values: Vec<f64>,
}

// ---------------------------------------------------------------- the operation

/// One well's side of the doublet, as the rates report it: the wellbore, its
/// reporting entity, its periods, its law, which way it flows, its head
/// temperature.
type Report<'a> = (&'a Ref, &'a Ref, &'a [Period], &'a Law, Direction, Option<f64>);

impl Operation for ProdmlDoublet {
    type Params = ProdmlDoubletParams;
    type Inputs = ProdmlDoubletInputs;

    fn about(&self) -> About {
        About {
            name: "prodml-doublet",
            describe: "the PRODML side of a geothermal doublet — its facility, a reporting entity per wellbore, the rates period by period, the head series with a daily statistic, the fluid it pumps — from a stated, sourced law",
            category: Category::Synthetic,
            makes: &[
                "prodml23.Facility",
                "prodml23.ReportingEntity",
                "prodml23.WellProductionParameters",
                "prodml23.TimeSeriesData",
                "prodml23.TimeSeriesStatistic",
                "prodml23.FluidSystem",
            ],
            recipe: "prodml-doublet",
            represents: None,
        }
    }

    /// The kind of each object given — a wellbore, a series, an
    /// organisation — which no declaration of a slot's kind can say. That the
    /// producer and the injector are two holes is the plan's to refuse: an
    /// offer is judged on what each input *is*.
    fn check(&self, inputs: &ProdmlDoubletInputs) -> Result<(), Refusal> {
        wellbore("producer", &inputs.producer)?;
        wellbore("injector", &inputs.injector)?;
        if let Some(observation) = &inputs.observation {
            wellbore("observation", observation)?;
        }
        of_class(
            "series",
            &inputs.series,
            &["TimeSeries"],
            "the periods count on the instants of a time series",
        )?;
        if let Some(organisation) = &inputs.fluid_organization {
            of_class(
                "fluidOrganization",
                organisation,
                &["RockFluidOrganizationInterpretation"],
                "a fluid system belongs to a rock-fluid organisation",
            )?;
        }
        Ok(())
    }

    fn plan(
        &self,
        inputs: ProdmlDoubletInputs,
        params: ProdmlDoubletParams,
        cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        // -- what the run is told, before anything is computed -----------------
        let facility_title = titled("facility", &params.facility)?;
        let fluid_title = titled("fluid", &params.fluid)?;
        let source = params.source.trim().to_string();
        if source.is_empty() {
            return Err(Refusal::on(
                "source",
                "is empty: every value this operation writes is invented, and an invented value states where it comes from",
            ));
        }

        // -- the wellbores, the series, the organisation ------------------------
        let producer = wellbore("producer", &inputs.producer)?.clone();
        let injector = wellbore("injector", &inputs.injector)?.clone();
        let observation = inputs
            .observation
            .as_ref()
            .map(|one| wellbore("observation", one).cloned())
            .transpose()?;
        if same(&producer, &injector) {
            return Err(Refusal::on(
                "injector",
                format!(
                    "`{}` is also the producer: a doublet is two holes, one pumping and one returning",
                    injector.title
                ),
            ));
        }
        if let Some(observing) = &observation
            && (same(observing, &producer) || same(observing, &injector))
        {
            return Err(Refusal::on(
                "observation",
                format!(
                    "`{}` is already a hole of the doublet: an observing wellbore neither produces nor injects",
                    observing.title
                ),
            ));
        }
        let series = of_class(
            "series",
            &inputs.series,
            &["TimeSeries"],
            "the periods count on the instants of a time series",
        )?
        .clone();
        let organisation = inputs
            .fluid_organization
            .as_ref()
            .map(|one| {
                of_class(
                    "fluidOrganization",
                    one,
                    &["RockFluidOrganizationInterpretation"],
                    "a fluid system belongs to a rock-fluid organisation",
                )
                .cloned()
            })
            .transpose()?;

        // -- the periods: n instants, n − 1 periods -----------------------
        let mut stated: Vec<(String, i64)> = Vec::with_capacity(params.times.len());
        for (at, text) in params.times.iter().enumerate() {
            let text = text.trim();
            let Some(seconds) = unix_seconds(text) else {
                return Err(Refusal::on(
                    "times",
                    format!(
                        "instant {} `{text}` is not a UTC date-time as `2025-01-01T00:00:00Z`: it is refused rather than read some other way",
                        at + 1
                    ),
                ));
            };
            stated.push((text.to_string(), seconds));
        }
        let Some((first_text, anchor)) = stated.first().cloned() else {
            return Err(Refusal::on(
                "times",
                "no instant: a period runs from one instant to the next",
            ));
        };
        let last_text = stated.last().map(|(text, _)| text.clone()).unwrap_or_default();
        let hours: Vec<f64> = stated
            .iter()
            .map(|(_, seconds)| (seconds - anchor) as f64 / 3600.0)
            .collect();

        let producer_law = Law::new(params.rate, params.decline).map_err(|why| {
            let slot = if matches!(why, LawError::BadDecline { .. }) {
                "decline"
            } else {
                "rate"
            };
            Refusal::on(slot, why)
        })?;
        if !(params.reinjected > 0.0 && params.reinjected <= 1.0) {
            return Err(Refusal::on(
                "reinjected",
                format!(
                    "{} is not a share an injector returns: in (0, 1], and 1 for a doublet's loop",
                    params.reinjected
                ),
            ));
        }
        let injector_law = producer_law.scaled(params.reinjected);
        let produced =
            flow_law::periods(&producer_law, &hours).map_err(|why| refused_instants(&stated, why))?;
        let injected =
            flow_law::periods(&injector_law, &hours).map_err(|why| refused_instants(&stated, why))?;
        let produced_volume: f64 = produced.iter().map(|one| one.volume).sum();
        let injected_volume: f64 = injected.iter().map(|one| one.volume).sum();

        // -- the head series: a window, a step, a frozen gap ---------------------
        let from_text = params.samples_from.trim().to_string();
        let Some(from) = unix_seconds(&from_text) else {
            return Err(Refusal::on(
                "samplesFrom",
                format!(
                    "`{from_text}` is not a UTC date-time as `2025-06-02T00:00:00Z`: it is refused rather than read some other way"
                ),
            ));
        };
        let step = params.sample_step;
        if step == 0 || MINUTES_PER_DAY % step != 0 {
            return Err(Refusal::on(
                "sampleStep",
                format!("{step} minute(s) do not divide a day: the daily statistic would straddle two days"),
            ));
        }
        let per_day = MINUTES_PER_DAY / step;
        let count = match per_day.checked_mul(params.sample_days) {
            Some(count) if count > 0 && count <= MOST_SAMPLES => count,
            _ => {
                return Err(Refusal::on(
                    "sampleDays",
                    format!(
                        "{} day(s) at {step} minute(s) are not a head series this writes: one sample at least, {MOST_SAMPLES} at most per series, each written inline",
                        params.sample_days
                    ),
                ));
            }
        };
        let step_seconds = (step * 60) as i64;
        let end = from + params.sample_days as i64 * DAY;
        let instants: Vec<i64> = (0..count).map(|rank| from + rank as i64 * step_seconds).collect();
        let (frozen_head, frozen) = match &params.frozen {
            Frozen::Never => (None, None),
            Frozen::Pressure(window) => (
                Some(Head::Pressure),
                Some(frozen_range(window, from, end, &instants)?),
            ),
            Frozen::Temperature(window) => (
                Some(Head::Temperature),
                Some(frozen_range(window, from, end, &instants)?),
            ),
            Frozen::Rate(window) => (
                Some(Head::Rate),
                Some(frozen_range(window, from, end, &instants)?),
            ),
        };
        let frozen_count = frozen.as_ref().map(ExactSizeIterator::len).unwrap_or(0);

        let producer_asset = asset(&params.asset_identifier, &producer.title)?;
        let injector_asset = asset(&params.asset_identifier, &injector.title)?;
        let simulated = prodml::FlowQualifier::Simulated.as_xml_str();
        let head_keys = |uri: &str, direction: Direction| {
            vec![
                key(prodml::TimeSeriesKeyword::AssetIdentifier, uri),
                key(prodml::TimeSeriesKeyword::Flow, direction.word()),
                key(prodml::TimeSeriesKeyword::Qualifier, simulated),
            ]
        };
        let rate_keys = vec![
            key(prodml::TimeSeriesKeyword::AssetIdentifier, &injector_asset),
            key(
                prodml::TimeSeriesKeyword::Product,
                prodml::ReportingProduct::Water.as_xml_str(),
            ),
            key(prodml::TimeSeriesKeyword::Flow, Direction::Injecting.word()),
            key(prodml::TimeSeriesKeyword::Qualifier, simulated),
        ];
        let mut heads = vec![
            Series {
                head: Head::Pressure,
                title: format!("{} — wellhead pressure", producer.title),
                what: "wellhead pressure",
                hole: &producer,
                keys: head_keys(&producer_asset, Direction::Producing),
                uom: eml::UnitOfMeasureExt::Bar,
                class: eml::MeasureClass::Pressure,
                values: vec![params.producer_head_pressure; count],
            },
            Series {
                head: Head::Temperature,
                title: format!("{} — wellhead temperature", producer.title),
                what: "wellhead temperature",
                hole: &producer,
                keys: head_keys(&producer_asset, Direction::Producing),
                uom: eml::UnitOfMeasureExt::DegC,
                class: eml::MeasureClass::ThermodynamicTemperature,
                values: vec![params.producer_head_temperature; count],
            },
            Series {
                head: Head::Rate,
                title: format!("{} — injection rate", injector.title),
                what: "injection rate",
                hole: &injector,
                keys: rate_keys.clone(),
                uom: eml::UnitOfMeasureExt::M3PerH,
                class: eml::MeasureClass::VolumePerTime,
                values: instants
                    .iter()
                    .map(|at| injector_law.rate_at((at - anchor) as f64 / 3600.0))
                    .collect(),
            },
        ];
        // A frozen sensor repeats its last reading: `frozen_range` refuses a
        // gap at the first sample, so there is always one before it.
        if let (Some(head), Some(range)) = (frozen_head, &frozen)
            && let Some(series) = heads.iter_mut().find(|one| one.head == head)
        {
            let held = series.values[range.start - 1];
            for value in &mut series.values[range.clone()] {
                *value = held;
            }
        }

        // The daily statistic: the injector's rate over the first day, frozen
        // readings set aside.
        let frozen_rate = |rank: usize| {
            frozen_head == Some(Head::Rate) && frozen.as_ref().is_some_and(|range| range.contains(&rank))
        };
        let rate_values = heads
            .iter()
            .find(|one| one.head == Head::Rate)
            .map(|one| one.values.clone())
            .unwrap_or_default();
        let day: Vec<f64> = (0..per_day.min(rate_values.len()))
            .filter(|rank| !frozen_rate(*rank))
            .map(|rank| rate_values[rank])
            .collect();
        let set_aside = per_day - day.len();
        let Some(daily) = flow_law::summary(&day) else {
            return Err(Refusal::on(
                "frozenMinutes",
                "the injector's rate is frozen over the whole first day: its daily statistic would be of nothing",
            ));
        };

        // -- the objects ------------------------------------------------------------
        let mut rank = 0usize;
        let mut uuid_of = |seed: &str| -> Result<String, Refusal> {
            let uuid = cx.identity.uuid(seed, rank)?;
            rank += 1;
            Ok(uuid)
        };
        let described = |what: &str| -> Result<String, Refusal> {
            let text = format!("Synthetic — {what}. Source: {source}");
            let length = text.chars().count();
            if length > 2000 {
                return Err(Refusal::on(
                    "source",
                    format!(
                        "makes a description of {length} characters, and a Citation/Description holds 2000 — cite the document shorter"
                    ),
                ));
            }
            Ok(text)
        };
        let citation = |title: &str, description: String| eml::Citation {
            title: title.to_string(),
            originator: cx.when.originator.clone(),
            creation: cx.when.creation.clone(),
            format: energyml_core::CITATION_FORMAT.to_string(),
            description: Some(description),
            ..Default::default()
        };
        let conditions = || {
            eml::AbstractTemperaturePressure::ReferenceTemperaturePressure(
                eml::ReferenceTemperaturePressure {
                    reference_temp_pres: params.standard_conditions.written(),
                },
            )
        };
        let mut objects: Vec<NewObject> = Vec::new();

        // The facility.
        let facility_uuid = uuid_of(&format!("prodml-doublet:facility:{facility_title}"))?;
        let facility = prodml::Facility {
            uuid: facility_uuid.clone(),
            schema_version: "2.3".to_string(),
            citation: citation(
                &facility_title,
                described(
                    "the surface facility an invented doublet's loop runs through, a processing facility",
                )?,
            ),
            extension_name_value: vec![synthetic("the facility of an invented doublet")],
            kind: prodml::ReportingFacilityExt::ProcessingFacility,
            ..Default::default()
        };
        objects.push(NewObject {
            uuid: facility_uuid.clone(),
            qualified_type: "prodml23.Facility".to_string(),
            role: "facility".to_string(),
            title: facility_title.clone(),
            json: energyml_core::json::write_value(&facility),
        });

        // A reporting entity per wellbore — producer, injector, and the
        // observing one when given.
        let mut entities: Vec<Ref> = Vec::new();
        for hole in [&producer, &injector].into_iter().chain(observation.iter()) {
            let uuid = uuid_of(&format!(
                "prodml-doublet:reporting-entity:{}",
                hole.uuid.to_ascii_lowercase()
            ))?;
            let entity = prodml::ReportingEntity {
                uuid: uuid.clone(),
                schema_version: "2.3".to_string(),
                citation: citation(
                    &hole.title,
                    described(&format!(
                        "`{}` as a reporting entity of an invented doublet",
                        hole.title
                    ))?,
                ),
                extension_name_value: vec![synthetic("a reporting entity of an invented doublet")],
                kind: prodml::ReportingEntityKind::Wellbore,
                associated_object: Some(crate::crs::reference_of(hole)),
                ..Default::default()
            };
            objects.push(NewObject {
                uuid: uuid.clone(),
                qualified_type: "prodml23.ReportingEntity".to_string(),
                role: "reporting-entity".to_string(),
                title: hole.title.clone(),
                json: energyml_core::json::write_value(&entity),
            });
            entities.push(Ref::new(uuid, "prodml23.ReportingEntity", hole.title.clone()));
        }

        // The rates period by period — producer, then injector.
        let joined: Vec<&str> = stated.iter().map(|(text, _)| text.as_str()).collect();
        let joined = joined.join(",");
        let reports: [Report<'_>; 2] = [
            (
                &producer,
                &entities[0],
                &produced,
                &producer_law,
                Direction::Producing,
                Some(params.producer_head_temperature),
            ),
            (
                &injector,
                &entities[1],
                &injected,
                &injector_law,
                Direction::Injecting,
                params.injector_head_temperature,
            ),
        ];
        for (hole, entity, periods, law, direction, head) in reports {
            let word = direction.word();
            let seed = format!(
                "prodml-doublet:{word}:{}:{joined}:{}:{}:{}:{}",
                hole.uuid.to_ascii_lowercase(),
                law.rate,
                law.decline,
                params.standard_conditions.word(),
                head.map(|temperature| temperature.to_string())
                    .unwrap_or_default()
            );
            let uuid = uuid_of(&seed)?;
            let title = format!("{} — {word}", hole.title);
            let what = format!(
                "the {word} rates of `{}` period by period, from {} — each the law integrated over its period and divided by its hours",
                hole.title,
                law_words(law, &first_text)
            );
            let report = prodml::WellProductionParameters {
                uuid: uuid.clone(),
                schema_version: "2.3".to_string(),
                citation: citation(&title, described(&what)?),
                extension_name_value: vec![synthetic("rates of an invented doublet, from a stated law")],
                standard_conditions: Some(conditions()),
                // `WellProductionParameters.StartDate` and `.EndDate` are `xs:date`
                // (prodml 2.3 SimpleProductVolume.xsd, `WellProductionParameters`),
                // not `eml:TimeStamp`: the instant's date, not the instant. Found
                // 04/10/2026 by the validation of what `apply` writes — a
                // `2025-01-01T00:00:00Z` there is not a date.
                start_date: Some(date_of(&first_text)),
                end_date: Some(date_of(&last_text)),
                production_period: periods
                    .iter()
                    .zip(&stated)
                    .map(|(period, (start, _))| prodml::ProductionWellPeriod {
                        start_time: start.clone(),
                        duration: eml::TimeMeasure {
                            value: period.hours(),
                            uom: eml::TimeUom::H,
                        },
                        well_status: Some(direction.status()),
                        well_flowing_condition: head.map(|temperature| prodml::WellFlowingCondition {
                            tubing_head_stabilized_temperature: Some(eml::ThermodynamicTemperatureMeasure {
                                value: temperature,
                                uom: eml::ThermodynamicTemperatureUom::DegC,
                            }),
                            ..Default::default()
                        }),
                        reporting_entity: crate::crs::reference_of(entity),
                        product_rate: vec![prodml::ProductRate {
                            // flow rate in `water - processed`.
                            product_fluid_kind: prodml::ProductFluidKindExt::WaterMinusProcessed,
                            volume_flow_rate: Some(eml::FlowRateValue {
                                flow_rate: eml::VolumePerTimeMeasure {
                                    value: period.mean_rate,
                                    uom: eml::VolumePerTimeUomWithLegacy::M3PerH,
                                },
                                ..Default::default()
                            }),
                            ..Default::default()
                        }],
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            };
            objects.push(NewObject {
                uuid,
                qualified_type: "prodml23.WellProductionParameters".to_string(),
                role: word.to_string(),
                title,
                json: energyml_core::json::write_value(&report),
            });
        }

        // The head series.
        let window_words = format!(
            "every {step} min over {} day(s) from {from_text}",
            params.sample_days
        );
        let mut rate_series: Option<(String, String)> = None;
        for series_of in &heads {
            let frozen_here = if frozen_head == Some(series_of.head) {
                frozen.as_ref()
            } else {
                None
            };
            let values_seed: String = match series_of.head {
                Head::Pressure => format!("{} bar", params.producer_head_pressure),
                Head::Temperature => format!("{} degC", params.producer_head_temperature),
                Head::Rate => format!("{}:{}", injector_law.rate, injector_law.decline),
            };
            let seed = format!(
                "prodml-doublet:series:{}:{}:{from_text}:{}:{step}:{values_seed}:{}:{}",
                series_of
                    .keys
                    .first()
                    .map(|one| one.value.as_str())
                    .unwrap_or_default(),
                series_of.what,
                params.sample_days,
                if series_of.head == Head::Rate {
                    first_text.as_str()
                } else {
                    ""
                },
                frozen_here
                    .map(|range| format!("{}..{}", range.start, range.end))
                    .unwrap_or_default()
            );
            let uuid = uuid_of(&seed)?;
            let value_words = match series_of.head {
                Head::Pressure => format!("a constant {} bar", params.producer_head_pressure),
                Head::Temperature => format!("a constant {} degC", params.producer_head_temperature),
                Head::Rate => law_words(&injector_law, &first_text),
            };
            let frozen_words = frozen_here
                .map(|range| {
                    format!(
                        ", frozen for {} sample(s) from {}",
                        range.len(),
                        w3cdtf(instants[range.start])
                    )
                })
                .unwrap_or_default();
            let what = format!(
                "the {} of `{}`, {value_words}, {window_words}{frozen_words}",
                series_of.what, series_of.hole.title
            );
            let data = prodml::TimeSeriesData {
                uuid: uuid.clone(),
                schema_version: "2.3".to_string(),
                citation: citation(&series_of.title, described(&what)?),
                extension_name_value: vec![synthetic("a head series of an invented doublet")],
                key: series_of.keys.clone(),
                uom: Some(series_of.uom.clone()),
                measure_class: Some(series_of.class.clone()),
                comment: Some(format!(
                    "synthetic, qualified `simulated`: {value_words}{frozen_words}"
                )),
                data_value: samples(&instants, &series_of.values, frozen_here),
                ..Default::default()
            };
            if series_of.head == Head::Rate {
                rate_series = Some((uuid.clone(), series_of.title.clone()));
            }
            objects.push(NewObject {
                uuid,
                qualified_type: "prodml23.TimeSeriesData".to_string(),
                role: "series".to_string(),
                title: series_of.title.clone(),
                json: energyml_core::json::write_value(&data),
            });
        }

        // The daily statistic of the injector's rate.
        let (rate_uuid, rate_title) = rate_series.unwrap_or_default();
        let day_end = from + DAY;
        let statistic_title = format!("{rate_title} — daily statistic");
        let statistic_uuid = uuid_of(&format!("prodml-doublet:statistic:{rate_uuid}:{}", w3cdtf(from)))?;
        let statistic = prodml::TimeSeriesStatistic {
            uuid: statistic_uuid.clone(),
            schema_version: "2.3".to_string(),
            citation: citation(
                &statistic_title,
                described(&format!(
                    "the least, greatest and mean of `{rate_title}` over [{}, {}), frozen readings set aside",
                    w3cdtf(from),
                    w3cdtf(day_end)
                ))?,
            ),
            extension_name_value: vec![synthetic("a statistic of an invented head series")],
            key: rate_keys,
            uom: Some(eml::UnitOfMeasureExt::M3PerH),
            measure_class: Some(eml::MeasureClass::VolumePerTime),
            comment: Some(format!(
                "{} sample(s) of the first day, {set_aside} frozen set aside; the values are in m3/h (the statistic's Uom), written as the DimensionlessMeasure the schema types them with",
                daily.count
            )),
            minimum: Some(eml::DimensionlessMeasure {
                value: daily.minimum,
                uom: eml::DimensionlessUom::Euc,
            }),
            maximum: Some(eml::DimensionlessMeasure {
                value: daily.maximum,
                uom: eml::DimensionlessUom::Euc,
            }),
            mean: Some(eml::DimensionlessMeasure {
                value: daily.mean,
                uom: eml::DimensionlessUom::Euc,
            }),
            d_tim_min: prodml::EndpointDateTime {
                value: w3cdtf(from),
                endpoint: prodml::EndpointQualifierInterval::Inclusive,
            },
            d_tim_max: prodml::EndpointDateTime {
                value: w3cdtf(day_end),
                endpoint: prodml::EndpointQualifierInterval::Exclusive,
            },
            ..Default::default()
        };
        objects.push(NewObject {
            uuid: statistic_uuid,
            qualified_type: "prodml23.TimeSeriesStatistic".to_string(),
            role: "statistic".to_string(),
            title: statistic_title,
            json: energyml_core::json::write_value(&statistic),
        });

        // The fluid system.
        let fluid_uuid = uuid_of(&format!(
            "prodml-doublet:fluid:{fluid_title}:{}:{}",
            params.standard_conditions.word(),
            organisation
                .as_ref()
                .map(|one| one.uuid.to_ascii_lowercase())
                .unwrap_or_default()
        ))?;
        let fluid = prodml::FluidSystem {
            uuid: fluid_uuid.clone(),
            schema_version: "2.3".to_string(),
            citation: citation(
                &fluid_title,
                described(
                    "the water an invented doublet pumps, classified as the reference model decides — its gas-oil ratio stated negligible, not measured",
                )?,
            ),
            extension_name_value: vec![synthetic("the fluid system of an invented doublet")],
            standard_conditions: Some(conditions()),
            reservoir_fluid_kind: prodml::ReservoirFluidKind::Unknown,
            phases_present: Some(prodml::PhasePresent::Water),
            rock_fluid_organization_interpretation: organisation.as_ref().map(crate::crs::reference_of),
            solution_gor: eml::VolumePerVolumeMeasure {
                value: 0.0,
                uom: eml::VolumePerVolumeUomWithLegacy::M3PerM3,
            },
            remark: Some(FLUID_REMARK.to_string()),
            ..Default::default()
        };
        objects.push(NewObject {
            uuid: fluid_uuid,
            qualified_type: "prodml23.FluidSystem".to_string(),
            role: "fluid-system".to_string(),
            title: fluid_title,
            json: energyml_core::json::write_value(&fluid),
        });

        // -- what the plan says -------------------------------------------------------
        let mut says = vec![
            Note {
                key: "periods",
                detail: format!(
                    "{} instant(s) make {} period(s), each from one instant to the next",
                    stated.len(),
                    produced.len()
                ),
            },
            Note {
                key: "instants-stated",
                detail: format!(
                    "the periods count on the instants stated to this run; `{}` is named in the activity and its own instants are not read — a plan does not read the package, and no PRODML field references a time series: the link is by dates",
                    series.title
                ),
            },
            Note {
                key: "volumes",
                detail: format!(
                    "`{}` pumps {produced_volume} m3 and `{}` returns {injected_volume} m3 over the periods; each period states its mean rate in m3/h — the law integrated over it, divided by its hours — so rate × duration gives the volume back",
                    producer.title, injector.title
                ),
            },
            Note {
                key: "reinjected",
                detail: if params.reinjected == 1.0 {
                    "the injector returns what the producer pumps: the loop of a doublet".to_string()
                } else {
                    format!("the injector returns {} of what the producer pumps", params.reinjected)
                },
            },
            Note {
                key: "asset-identifier",
                detail: format!(
                    "the head series name their wellbores by `{producer_asset}` and `{injector_asset}` — the titles of the wellbores given, so each names an object of the package"
                ),
            },
            Note {
                key: "qualifier",
                detail: "the head series are qualified `simulated`, not `measured` as the example it follows writes: their values are invented".to_string(),
            },
            Note {
                key: "statistic",
                detail: format!(
                    "the daily statistic of `{rate_title}` covers [{}, {}): {} sample(s), {set_aside} frozen set aside; its minimum, maximum and mean are in m3/h, written as the DimensionlessMeasure (`Euc`) the schema types them with",
                    w3cdtf(from),
                    w3cdtf(day_end),
                    daily.count
                ),
            },
            Note {
                key: "synthetic",
                detail: "every object written is invented: `synthetic = true` and the source in Citation/Description".to_string(),
            },
        ];
        if let (Some(head), Some(range)) = (frozen_head, &frozen) {
            let named = match head {
                Head::Pressure => "the producer's head pressure",
                Head::Temperature => "the producer's head temperature",
                Head::Rate => "the injector's rate",
            };
            says.push(Note {
                key: "frozen",
                detail: format!(
                    "{} sample(s) of {named} are frozen from {}: each repeats the last reading before the gap",
                    range.len(),
                    w3cdtf(instants[range.start])
                ),
            });
        }
        if params.injector_head_temperature.is_none() {
            says.push(Note {
                key: "injector-head",
                detail: "no temperature is written at the injector's head: none was given".to_string(),
            });
        }
        if organisation.is_none() {
            says.push(Note {
                key: "fluid-organization",
                detail: "the fluid system names no rock-fluid organisation: one is awaited".to_string(),
            });
        }

        // -- the activity -------------------------------------------------------------
        let seed = format!(
            "prodml-doublet:{}",
            objects
                .iter()
                .map(|one| one.uuid.as_str())
                .collect::<Vec<_>>()
                .join(",")
        );
        let mut parameters = vec![
            Parameter::Double {
                title: "periodCount".to_string(),
                value: produced.len() as f64,
                uom: None,
            },
            Parameter::Double {
                title: "producedVolume".to_string(),
                value: produced_volume,
                uom: Some("m3".to_string()),
            },
            Parameter::Double {
                title: "injectedVolume".to_string(),
                value: injected_volume,
                uom: Some("m3".to_string()),
            },
            Parameter::Double {
                title: "sampleCount".to_string(),
                value: count as f64,
                uom: None,
            },
            Parameter::Double {
                title: "frozenCount".to_string(),
                value: frozen_count as f64,
                uom: None,
            },
        ];
        // The objects taken, as inputs — the series among them: no PRODML
        // field can reference it, and this is where the run says which one
        // its periods follow.
        for input in [&producer, &injector]
            .into_iter()
            .chain(observation.iter())
            .chain(std::iter::once(&series))
            .chain(organisation.iter())
        {
            parameters.push(Parameter::Object {
                title: "input".to_string(),
                reference: input.dor(),
            });
        }

        Ok(Plan {
            media: Vec::new(),
            blocked: None,
            objects,
            datasets: Vec::new(),
            activity: ActivityDraft {
                recipe: "prodml-doublet",
                title: format!("prodml-doublet → {facility_title}"),
                seed,
                parameters,
            },
            says,
            dropped: Vec::new(),
            // No geometry: rates, series and a fluid have no shape.
            shapes: Vec::new(),
        })
    }

    /// What the run measured: the periods, the volumes, the samples.
    fn reports(&self) -> Vec<Slot> {
        let number = |uom: Option<&'static str>| Need::Number {
            uom,
            least: None,
            most: None,
        };
        vec![
            Slot::required(
                "periodCount",
                number(None),
                "periods written for the producer and for the injector — one fewer than the instants",
            ),
            Slot::required(
                "producedVolume",
                number(Some("m3")),
                "what the producer pumps over the periods — its rate integrated",
            ),
            Slot::required(
                "injectedVolume",
                number(Some("m3")),
                "what the injector returns over the periods",
            ),
            Slot::required("sampleCount", number(None), "samples in each head series"),
            Slot::required(
                "frozenCount",
                number(None),
                "samples written with the status `frozen`",
            ),
        ]
    }
}

/// The date of an instant written `YYYY-MM-DDThh:mm:ss…`.
fn date_of(instant: &str) -> String {
    instant.split('T').next().unwrap_or(instant).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_title_becomes_an_identifier_by_percent_encoding() {
        assert_eq!(identifier("GTH-1"), "GTH-1");
        assert_eq!(identifier("GTH-3 ST1"), "GTH-3%20ST1");
        assert_eq!(identifier("é"), "%C3%A9");
        assert!(is_identifier("GTH-3%20ST1"));
        assert!(!is_identifier("GTH 1"));
        assert!(!is_identifier("%2"));
        assert!(!is_identifier(""));
    }

    #[test]
    fn the_reference_model_convention_is_a_uri_and_its_faults_are_named() {
        // The convention of the synthetic model, verbatim.
        assert_eq!(
            uri_fault("eml://lutetia.geosiris.com/well(GTH-1)/wellbore(01)/wellhead"),
            None
        );
        // Identifier Specification v4.0 §6.3.1 p. 14, verbatim — a component
        // type may hold a space.
        assert_eq!(
            uri_fault("eml://prodml.mycompany.com/well(123456)/wellbore(01)/bottomhole/pressure meter"),
            None
        );
        // §6.2.1 p. 14: a wellbore identified alone under a well-known authority.
        assert_eq!(uri_fault("eml://api/wellbore(123456789012)"), None);
        assert!(uri_fault("http://lutetia/well(GTH-1)").is_some());
        assert!(uri_fault("eml://lutetia/well(GTH 1)/wellhead").is_some());
        assert!(uri_fault("eml://lutetia/unknown(GTH-1)").is_some());
        assert!(uri_fault("eml://lutetia/well(GTH-1").is_some());
        assert!(uri_fault("eml://lutetia//wellhead").is_some());
        assert!(uri_fault("eml://lutetia").is_some());
    }
}
