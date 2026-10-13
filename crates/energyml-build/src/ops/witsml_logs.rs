//! `witsml-logs` — the WITSML 2.1 side of a wellbore's continuous properties:
//! one `Log`, its `ChannelSet` indexed by measured depth, a `Channel` per
//! property — for the doublet's temperature and hydrostatic pressure that
//! `well-logs` already laid on the wellbore frames (`temperature-doublet`,
//! `pression-doublet` of the recipe).
//!
//! No operation wrote a WITSML log before this one; `witsml-wells` (step 10 of the
//! reference model) wrote the `Well`/`Wellbore`/`Trajectory`
//! side and named, among what it does not write, the logs and channel sets.
//! This is that side, for whatever a frame already carries
//! as a RESQML continuous property — logs that pick their own index kind
//! and content and are not this operation's concern.
//!
//! # What is read, and from where
//!
//! **Wellbore frames**, resolved at the boundary into [`crate::pieces::Known`]
//! exactly as `well-logs` and `witsml-wells` read them: `node_mds` (the index),
//! `interpretation` (which RESQML wellbore this is), `datum` (what the index
//! counts from), `reference` (what a property's `SupportingRepresentation`
//! must name to hang on this frame). **RESQML continuous properties**, each
//! resolved into [`crate::pieces::KnownProperty`] — kind, uom, indexable
//! element, values, facets — read only on a frame
//! (`crates/energyml-build/ARCHITECTURE.md`: a property on a frame is a
//! positional array): the values here are exactly the frame's node count,
//! or the property is refused by name.
//!
//! # The values go inline by default
//!
//! Decision of the project owner: the logs go in the XML, since WITSML
//! cannot actually be written into the h5. WITSML 2.1
//! agrees with the reason, not just the conclusion: `ChannelData.Data` is
//! *"The data blob in JSON form […] to avoid the issues that arise when
//! splitting data across multiple files"*, and `FileUri` *"is ignored"* once
//! `Data` is set (generated doc comment, `crates/generated/energyml-witsml-2-1/src/log.rs`,
//! `ChannelData`; WITSML TRG v2.1 §7.2 p. 146). **By default** this build
//! never sets `FileUri` and never opens the HDF5 for a channel's values.
//!
//! # …unless `channelDataFile` asks for the extension
//!
//! The need: the model does not provide for it, but could the
//! "Data" of the "ChannelData" carry the path of a
//! dataset in the h5? Given a file, each `ChannelSet`'s rows go to **one
//! dataset** of it — `[rows, 1 + channels]`, the inline rows row for row, a
//! value the source states as not-a-number written `NaN` where inline writes
//! `null` — `FileUri` naming the file and `Data` the dataset's path. **An
//! extension beyond WITSML 2.1**, decided and argued;
//! its reader is
//! `energyml_domain::series::channel_set::read_with`. Left out, nothing
//! changes — not a byte of the inline output, not a uuid: the extension's
//! seeds and titles carry the file, so a package can hold both forms of the
//! same channels side by side.
//!
//! # The row layout, and what is and is not cited
//!
//! Neither the XSD nor its generated comment says how the numbers inside that
//! JSON string are arranged — `Data` is typed a bare `string`. The Energistics
//! JSON Style Guide (p. 8) names where
//! the layout **is** specified: *"When reading WITSML v2.1, the JSON on-disk
//! representation for channel sets (a format that is specified in Section 3
//! of the WITSML v2.1 Release Notes), the arrays may have NULLs."* That
//! release notes document is not among the specifications at hand (only
//! technical reference guides and the style guide are), so its exact grammar
//! cannot be cited here page and paragraph — flagged, not guessed past.
//!
//! What **is** applied is the one row shape the object model leaves no other
//! way to read: a `ChannelSet` carries one `Index` (here, one — see below) and
//! `n` `Channel`s, and one `Data` shared by all of them, so a row can only be
//! *the index value(s), then one value per channel, in the order channels are
//! listed* — the ETP `ChannelDataRecord`/`DataItem` convention this format is
//! a file-embedded cousin of (`Channel`'s own doc comment: *"directly
//! corresponds to the ChannelMetadataRecord structure in ETP"*). A value the
//! source states as not-a-number is written `null`, which the style guide's
//! own note allows and which is the only spelling JSON has for *no value
//! here* (JSON has no `NaN`).
//!
//! # One index, never two
//!
//! WITSML TRG 2.1 §7.7 p. 152: `ChannelSet.Index[0].IndexKind` — an MD index
//! **or** a time index, never both.
//! This operation writes exactly one: `MeasuredDepth`, in metres — the
//! convention every frame of this crate already writes its `NodeMd` in
//! (`witsml-wells`'s `measured()`, hardcoded `m`) — increasing, its datum the
//! frame's own (`MdInterval.Datum`, the same reference `witsml-wells` puts on
//! its `Trajectory`'s index).
//!
//! # One Log per frame, nested — not three parts
//!
//! A `Log`, a `ChannelSet` and a `Channel` are each declared `top_level` in
//! the schema (any of the three **can** stand alone), but `ChannelSet.Channel`
//! and `Log.ChannelSet` are inline fields (`Vec<AnyChannelSet>`,
//! `Vec<AnyChannel>`) — *"Primarily a container for one or more channel sets…
//! most of the log information is now at the channel set level"* (`Log`'s own
//! doc comment). Writing three parts per frame would scatter one channel
//! set's data across three files for no reader to reassemble; this build
//! writes **one `NewObject`, a `Log`**, nesting its `ChannelSet` and every
//! `Channel`.
//!
//! # A Channel names a logging company, a Log does not (found, not assumed)
//!
//! `Channel/LoggingCompany` has no `[0..1]` in the schema — required — where
//! `ChannelSet/LoggingCompany` and `Log/LoggingCompany` both do (WITSML TRG
//! v2.1 p. 138, cross-checked against the generated field:
//! `Channel::logging_company: DataObjectReference`, not `Option<…>`, the only
//! one of the three that is not). A property laid by a law has no company
//! that logged it, and nothing here invents a `BusinessAssociate` nobody
//! named — `loggingCompany` is therefore a required setting, a reference the
//! caller gives (`eml23.BusinessAssociate`), not an object this build creates.
//! Lutetia has none yet (the BRGM's is real, and not built), so
//! its recipe step is `attente`, exactly like `temperature-forages` waits on
//! `extend-boreholes`.
//!
//! # The Wellbore reference is the one `witsml-wells` writes, recomputed
//!
//! `witsml-wells` derives a `Wellbore`'s uuid from the RESQML wellbore
//! interpretation it represents: `Uuid::new_v5(NAMESPACE, "witsml-wells:wellbore:{interpretation.uuid}")`
//! (its `plan`, the `bore_uuid[b] = support::next_uuid(cx.identity,
//! &format!("witsml-wells:wellbore:{}", …), &mut rank)` line — `Identity::uuid`
//! ignores `rank` for the derived policy this build uses everywhere, so the
//! seed alone decides it). This operation names the **same** wellbore its
//! frame represents, so its `Log`, `ChannelSet` and every `Channel` reference
//! that uuid — recomputed from the same seed, **not** duplicated as a second
//! formula living in two files by coincidence, but the same string, because
//! `witsml_wells.rs` does not expose it. It should:
//! `pub(crate) fn wellbore_uuid(identity: &Identity, interpretation_uuid: &str) -> Result<String, Error>`
//! in `ops/witsml_wells.rs`, replacing its own inline `format!(…)` with a call
//! to it — see the report handed back with this file.
//!
//! **This equality holds under `Identity::derived()`, the policy this whole
//! crate uses for a recipe** — two operations naming the same content name
//! the same uuid without agreeing on anything but the seed. Under
//! `Identity::Given`, a caller-supplied list, it does not: a given uuid was
//! handed to `witsml-wells`'s own plan, at its own rank, and no rank this
//! plan could ask for would be that one. That is not particular to this
//! operation — any cross-plan reference recomputed this way shares it — and
//! it is said here rather than solved, because solving it is a change to
//! `Identity` itself, not to a wellbore reference.
//!
//! # What a channel takes, and what sets it aside
//!
//! Only a RESQML 2.2 `ContinuousProperty`, indexed on `nodes`, one value per
//! node, no facet (a `Channel` has no field for one), naming a `PropertyKind`
//! by reference and a unit. Anything else — discrete, boolean, a vector, a
//! property indexed on intervals or in time, one with no kind or no unit, one
//! this build cannot map to a WITSML unit code — is **dropped by name**, never
//! silently absorbed or silently ignored (`Plan::dropped`). A frame that ends
//! up with no channel gets no `Log`, and is dropped too; a run that writes no
//! `Log` at all is refused, the case that would otherwise look most like
//! success.

use serde_json::{Value, json};

use energyml_domain::provenance::Parameter;
use energyml_domain::values::Values;
use energyml_domain::wellbore_table::stem;
use energyml_eml_2_3 as eml;
use energyml_witsml_2_1 as witsml;

use crate::declare::{Inputs, ObjectIn, ObjectRef, Params, Shape};
use crate::operation::{
    About, ActivityDraft, Category, Context, Dataset, Dropped, Need, NewObject, Note, Numbers, Operation, Plan, Refusal,
    Slot,
};
use crate::pieces::{KnownProperty, Ref};

use super::support;

/// The operation.
pub struct WitsmlLogs;

/// What `Citation/Description` holds (eml 2.3 `Citation`, `max_length`).
const DESCRIPTION_LIMIT: usize = 2000;

/// What `Mnemonic`/`GlobalMnemonic`/`AxisPropertyKind` hold (WITSML
/// `String64`).
const SHORT_TEXT: usize = 64;

/// What `witsml-logs` is told.
#[derive(Debug, Params)]
pub struct WitsmlLogsParams {
    /// the BusinessAssociate a Channel must name — WITSML 2.1 requires it (unlike a ChannelSet's or a Log's, which do not); a law-derived value has no company that logged it, so name the one responsible for the law, or the author's own
    #[param(need = Need::Object { types: &["eml23.BusinessAssociate"] })]
    pub logging_company: ObjectRef,
    /// the properties are invented (laid by a law, not measured): every object made carries synthetic = true and says so
    ///
    /// The same mark `well-logs` put on
    /// the RESQML property this operation only repackages.
    #[param(default = false, optional)]
    pub synthetic: bool,
    /// the HDF5 file the channel values go into instead of the XML — each ChannelSet's rows become one dataset of it, FileUri naming the file and Data the dataset: an extension beyond WITSML 2.1, which a reader that does not know it refuses; left out, the values are written inline in Data
    ///
    /// **No default, and not `dataFile`**: a recipe gives every operation
    /// declaring `dataFile` its own file, and this one must stay inline unless
    /// asked.
    #[param(group = "naming")]
    pub channel_data_file: Option<String>,
}

/// What `witsml-logs` takes.
#[derive(Debug, Inputs)]
pub struct WitsmlLogsInputs {
    /// the wellbore frames — each becomes one Log, indexed by its own measured depths
    ///
    /// **Left empty, every frame of the package.**
    #[input(least = 1, types = "resqml22.WellboreFrameRepresentation", every)]
    pub frames: Vec<Shape>,
    /// the RESQML continuous properties laid on those frames — each becomes a Channel of its frame's Log
    ///
    /// **Left empty, every continuous property of the package.** One that
    /// hangs on none of `frames`, or that this operation cannot turn into a
    /// channel, is set aside and counted, never silently skipped.
    #[input(types = "resqml22.ContinuousProperty", every)]
    pub properties: Vec<ObjectIn>,
}

/// A frame, as this operation must find it — or why it cannot. Owned
/// (cloned once out of `Known`), the same choice `witsml-wells`'s own `Read`
/// makes for the same reason: a plan reads a package once, at the boundary,
/// and everything after that is values, not borrows to keep straight.
struct FrameInfo {
    /// What a refusal and a title name it by.
    name: String,
    reference: Ref,
    interpretation: Ref,
    datum: Ref,
    mds: Vec<f64>,
}

/// A frame, read — or the refusal that names what it lacks.
fn frame_of(shape: &Shape) -> Result<FrameInfo, String> {
    let known = shape.known.as_ref().ok_or_else(|| {
        format!("`{}` comes with nothing known of it: a Log is written from what the package says of a RESQML wellbore frame", shape.name)
    })?;
    let reference = known
        .reference
        .clone()
        .ok_or_else(|| format!("`{}` names no object of the package for a Log to name", shape.name))?;
    let name = if reference.title.is_empty() { shape.name.clone() } else { reference.title.clone() };
    let interpretation = known.interpretation.clone().ok_or_else(|| {
        format!(
            "`{name}` represents no interpretation: the WITSML Wellbore a Log names is the one `witsml-wells` writes for the RESQML wellbore a frame represents"
        )
    })?;
    let datum = known.datum.clone().ok_or_else(|| {
        format!("`{name}` names no datum: a Channel's measured-depth index needs the reference point its depths are counted from")
    })?;
    let mds = known.node_mds.clone().ok_or_else(|| {
        format!("`{name}` states no measured depth for its nodes — it is no wellbore frame, or this build cannot read its arrays")
    })?;
    if mds.is_empty() {
        return Err(format!("`{name}` has no node: there is nothing to index a channel by"));
    }
    if let Some(at) = mds.iter().position(|md| !md.is_finite()) {
        return Err(format!("node {} of `{name}` has a measured depth that is not a number", at + 1));
    }
    if let Some(at) = mds.windows(2).position(|pair| pair[1] <= pair[0]) {
        return Err(format!("the measured depths of `{name}` do not increase at node {}: {} then {}", at + 2, mds[at], mds[at + 1]));
    }
    Ok(FrameInfo { name, reference, interpretation, datum, mds })
}

/// A continuous property, read as one channel's worth of values — or why it
/// cannot be one. Owns what it reads rather than borrowing it, so it outlives
/// the loop that gathers a frame's channels without tying this module to a
/// lifetime shared with the properties given.
struct Judged {
    /// The RESQML property itself — what a channel's citation and a refusal
    /// name.
    reference: Ref,
    kind: Ref,
    uom: eml::UnitOfMeasureExt,
    values: Vec<f64>,
}

/// The class of a qualified type — `ContinuousProperty` of
/// `resqml22.ContinuousProperty`.
fn class_of(qualified_type: &str) -> &str {
    qualified_type.rsplit('.').next().unwrap_or(qualified_type)
}

/// The WITSML unit code of a uom this operation knows — `well-logs` writes
/// exactly these two (`degC`, `bar`); anything else is refused **by name**
/// rather than guessed at, since no generic uom-string-to-enum reader lives
/// at this layer (`energyml-domain::units` only converts numbers, and this
/// crate stays free of `energyml-schemas`, which is the only place a general
/// one could live without duplicating the schema's own enumeration).
fn uom_of(uom: &str) -> Option<eml::UnitOfMeasureExt> {
    match uom {
        "degC" => Some(eml::UnitOfMeasureExt::DegC),
        "bar" => Some(eml::UnitOfMeasureExt::Bar),
        _ => None,
    }
}

/// A property that can become a channel, or the reason it cannot — every
/// reason a reason **not to read**, never a reading made anyway.
fn judge_property(reference: &Ref, facts: &KnownProperty, nodes: usize) -> Result<Judged, String> {
    if class_of(&reference.qualified_type) != "ContinuousProperty" || !reference.qualified_type.starts_with("resqml22.") {
        return Err(format!(
            "it is a `{}`, and this operation turns RESQML 2.2 continuous properties into channels",
            reference.qualified_type
        ));
    }
    let raw = facts.values.as_ref().map_err(|why| format!("its values did not read — {why}"))?;
    if facts.timed {
        return Err("it is indexed in time — a channel here is indexed by measured depth, not by a time step".to_string());
    }
    if facts.realizations {
        return Err("it holds several realizations, and which one a channel takes is stated nowhere".to_string());
    }
    let per_element: u64 = facts.value_count.iter().product();
    if per_element > 1 {
        return Err(format!("it holds {per_element} value(s) per node — a vector or a tensor — and a channel holds one value per node"));
    }
    if facts.indexable != "nodes" {
        return Err(format!("its values hang on `{}`, and a channel here is indexed by the frame's own nodes", facts.indexable));
    }
    if !facts.facets.is_empty() {
        return Err("it carries a facet, and a Channel has no field for one".to_string());
    }
    let Some(kind) = facts.kind.as_ref() else {
        return Err("it names no property kind by reference, and a Channel's ChannelPropertyKind must point to one".to_string());
    };
    let Some(uom) = facts.uom.as_deref().filter(|u| !u.is_empty()) else {
        return Err("it states no unit, and a Channel's Uom is required".to_string());
    };
    let Some(uom_ext) = uom_of(uom) else {
        return Err(format!("is in `{uom}`, and this build does not know its WITSML unit code"));
    };
    if raw.len() != nodes {
        return Err(format!("it holds {} value(s) where the frame has {nodes} node(s) — a channel takes one value per node", raw.len()));
    }
    let values: Vec<f64> = match raw {
        Values::Floating(reals) => reals.clone(),
        Values::Integer(whole) => whole.iter().map(|v| *v as f64).collect(),
        _ => return Err("it is continuous and its values are not numbers".to_string()),
    };
    Ok(Judged { reference: reference.clone(), kind: kind.clone(), uom: uom_ext, values })
}

/// A reference read from a settled `ObjectRef` — a bare uuid, or a
/// `DataObjectReference` given whole.
pub(crate) fn reference_from(slot: &str, value: &Value, fallback_type: &str) -> Result<eml::DataObjectReference, Refusal> {
    match value {
        Value::String(uuid) => Ok(crate::crs::reference_to(uuid, fallback_type, "")),
        Value::Object(_) => {
            let uuid = value["Uuid"].as_str().unwrap_or_default();
            if uuid.is_empty() {
                return Err(Refusal::on(slot, format!("`{value}` names no `Uuid`")));
            }
            let qualified_type = value["QualifiedType"].as_str().unwrap_or(fallback_type);
            let title = value["Title"].as_str().unwrap_or_default();
            Ok(crate::crs::reference_to(uuid, qualified_type, title))
        }
        other => Err(Refusal::on(slot, format!("expected an object reference, got {other}"))),
    }
}

fn dor(one: &Ref) -> eml::DataObjectReference {
    crate::crs::reference_of(one)
}

/// A value as the row of a `ChannelData.Data` writes it: `null` for what the
/// source states as not-a-number, since JSON has no `NaN` and the Energistics
/// JSON Style Guide (p. 8) names the
/// arrays of this very format as the ones that "may have NULLs".
pub(crate) fn json_number(value: f64) -> Value {
    if value.is_finite() { json!(value) } else { Value::Null }
}

/// A value as a dataset of the extension writes it: `NaN` for whatever inline
/// writes `null` ([`json_number`]), so the two forms read back the same.
fn nan_unless_finite(value: f64) -> f64 {
    if value.is_finite() { value } else { f64::NAN }
}

/// Values as a seed carries them — exactly, NaN included (`well-logs`'s own
/// `digest`, duplicated: a small, pure formatting helper, not the kind of
/// formula this module's doc asks a caller to share rather than copy).
fn digest(values: &[f64]) -> String {
    values.iter().map(|value| format!("{value:?}")).collect::<Vec<_>>().join(",")
}

/// A few names and how many more.
fn listed(names: &[String]) -> String {
    let shown = names.iter().take(3).map(|one| format!("`{one}`")).collect::<Vec<_>>().join(", ");
    if names.len() > 3 { format!("{shown} and {} more", names.len() - 3) } else { shown }
}

impl Operation for WitsmlLogs {
    type Params = WitsmlLogsParams;
    type Inputs = WitsmlLogsInputs;

    fn about(&self) -> About {
        About {
            name: "witsml-logs",
            describe: "the WITSML side of a wellbore's continuous properties: a Log per frame, its ChannelSet indexed by measured depth, a Channel per property — the data blob written inline, or to an HDF5 dataset when channelDataFile asks (an extension beyond WITSML 2.1)",
            category: Category::Wells,
            makes: &["witsml21.Log"],
            recipe: "witsml-logs",
            represents: None,
        }
    }

    fn also(&self) -> &'static [Category] {
        &[Category::Properties]
    }

    fn reports(&self) -> Vec<Slot> {
        let number = || Need::Number { uom: None, least: None, most: None };
        vec![
            Slot::required("logCount", number(), "Log objects written — one per frame that ends up with a channel"),
            Slot::required("channelCount", number(), "Channel objects written, over every Log"),
        ]
    }

    fn plan(&self, inputs: WitsmlLogsInputs, params: WitsmlLogsParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
        if inputs.frames.is_empty() {
            return Err(Refusal::on("frames", "it takes wellbore frames; the input holds none, and the package gives none either"));
        }

        let logging_company = reference_from("loggingCompany", &params.logging_company.0, "eml23.BusinessAssociate")?;

        // -- inline, or the extension (module doc) ---------------------------
        let in_file: Option<String> = match params.channel_data_file.as_deref() {
            None => None,
            Some(file) if file.trim().is_empty() => {
                return Err(Refusal::on(
                    "channelDataFile",
                    "is blank: name the HDF5 file the channel values go into, or leave it out for the values inline",
                ));
            }
            Some(file) => Some(file.to_string()),
        };
        // The extension's objects are not the inline ones: their seeds and
        // titles say the file, so both forms of one set can sit side by side
        // and the inline uuids and titles do not move.
        let marked = |seed: String| match &in_file {
            Some(file) => format!("{seed}:hdf5:{file}"),
            None => seed,
        };
        let titled = |title: String| match &in_file {
            Some(_) => format!("{title} (HDF5)"),
            None => title,
        };
        let mut datasets: Vec<Dataset> = Vec::new();

        let frames: Vec<FrameInfo> =
            inputs.frames.iter().map(frame_of).collect::<Result<_, _>>().map_err(|why| Refusal::on("frames", why))?;
        let frame_uuids: Vec<String> = frames.iter().map(|frame| frame.reference.uuid.to_ascii_lowercase()).collect();

        // -- what every object says of itself ----------------------------------
        let citation = |title: &str, description: Option<String>| eml::Citation {
            title: title.to_string(),
            originator: cx.when.originator.clone(),
            creation: cx.when.creation.clone(),
            format: energyml_core::CITATION_FORMAT.to_string(),
            description,
            ..Default::default()
        };
        let mark = || -> Vec<eml::ExtensionNameValue> {
            if !params.synthetic {
                return Vec::new();
            }
            vec![eml::ExtensionNameValue {
                name: "synthetic".to_string(),
                value: eml::StringMeasure { value: "true".to_string(), uom: None },
                description: Some("written by `witsml-logs` from a RESQML property laid by a law, not measured".to_string()),
                ..Default::default()
            }]
        };

        let mut objects: Vec<NewObject> = Vec::new();
        let mut dropped: Vec<Dropped> = Vec::new();
        let mut activity_inputs: Vec<Parameter> = Vec::new();
        let mut rank = 0usize;
        let mut log_count = 0usize;
        let mut channel_count = 0usize;

        for frame in &frames {
            cx.stop_if_cancelled()?;

            // -- the channels this frame gives -------------------------------
            let mut channels: Vec<Judged> = Vec::new();
            for property in &inputs.properties {
                let Some(reference) = property.known.reference.as_ref() else { continue };
                let Some(facts) = property.known.property.as_ref() else { continue };
                let Some(supporting) = facts.supporting.as_ref() else { continue };
                if !supporting.uuid.eq_ignore_ascii_case(&frame.reference.uuid) {
                    continue;
                }
                match judge_property(reference, facts, frame.mds.len()) {
                    Ok(judged) => channels.push(judged),
                    Err(why) => dropped.push(Dropped { count: 1, why: format!("`{}`: not turned into a channel — {why}", reference.title) }),
                }
            }
            if channels.is_empty() {
                dropped.push(Dropped {
                    count: 1,
                    why: format!("`{}`: no continuous property hangs on it, or none of what does can become a channel", frame.name),
                });
                continue;
            }

            // -- the Wellbore this Log and every Channel reference -----------
            // See the module doc: the same seed `witsml-wells` derives its
            // Wellbore's uuid from, recomputed rather than duplicated as a
            // second formula that happens to agree.
            let wellbore_uuid = cx.identity.uuid(&super::witsml_wells::wellbore_seed(&frame.interpretation.uuid), 0)?;
            let wellbore_title = match stem(&frame.interpretation.title) {
                "" => stem(&frame.name).to_string(),
                found => found.to_string(),
            };
            let wellbore_ref = crate::crs::reference_to(&wellbore_uuid, "witsml21.Wellbore", &wellbore_title);

            // -- the index: measured depth, one, never two ------------
            let channel_index = witsml::ChannelIndex {
                index_kind: eml::DataIndexKind::MeasuredDepth,
                index_property_kind: None,
                uom: eml::UnitOfMeasureExt::M,
                direction: eml::IndexDirection::Increasing,
                mnemonic: "MD".to_string(),
                datum: Some(dor(&frame.datum)),
                index_interval: None,
            };

            // -- the channels -------------------------------------------------
            let mut any_channels: Vec<witsml::AnyChannel> = Vec::new();
            let mut channel_uuids: Vec<String> = Vec::new();
            let mut channel_titles: Vec<String> = Vec::new();
            for judged in &channels {
                activity_inputs.push(Parameter::Object { title: "input".to_string(), reference: judged.reference.dor() });
                let mnemonic = judged.kind.title.clone();
                if mnemonic.chars().count() > SHORT_TEXT {
                    return Err(Refusal::on(
                        "properties",
                        format!(
                            "`{}`'s property kind is titled `{mnemonic}`, {} characters — a Channel's Mnemonic holds {SHORT_TEXT}",
                            judged.reference.title,
                            mnemonic.chars().count()
                        ),
                    ));
                }
                // **Same content, same uuid; different content, a second
                // one.** The property's own uuid is not enough: `well-logs`
                // derives it from the law and its coefficients, but a caller
                // that hands this operation the same property uuid with
                // corrected values (a fixture, or a future writer of
                // `resqml22.ContinuousProperty` that does not) must still get
                // a second Channel, never the first rewritten under its old
                // uuid with new numbers inside.
                let channel_uuid = support::next_uuid(
                    cx.identity,
                    &marked(format!("witsml-logs:channel:{}:{}", judged.reference.uuid.to_ascii_lowercase(), digest(&judged.values))),
                    &mut rank,
                )?;
                let description = format!(
                    "Same values as `{}` (RESQML {}), laid by a law — see that object's citation for the law and its source.",
                    judged.reference.title, judged.reference.qualified_type
                );
                let channel = witsml::Channel {
                    citation: citation(&mnemonic, Some(description)),
                    extension_name_value: mark(),
                    uuid: channel_uuid.clone(),
                    schema_version: "2.1".to_string(),
                    active_status: eml::ActiveStatusKind::Inactive,
                    mnemonic: mnemonic.clone(),
                    global_mnemonic: mnemonic,
                    data_kind: witsml::ChannelDataKind::Double,
                    uom: judged.uom.clone(),
                    channel_property_kind: dor(&judged.kind),
                    logging_company: logging_company.clone(),
                    is_continuous: Some(true),
                    index: vec![channel_index.clone()],
                    wellbore: Some(wellbore_ref.clone()),
                    ..Default::default()
                };
                channel_titles.push(judged.reference.title.clone());
                channel_uuids.push(channel_uuid);
                any_channels.push(witsml::AnyChannel::Channel(channel));
                channel_count += 1;
            }

            // -- the ChannelSet's uuid: the dataset's path names it ------------
            let channelset_seed =
                marked(format!("witsml-logs:channelset:{}:{}", frame.reference.uuid.to_ascii_lowercase(), channel_uuids.join(",")));
            let channelset_uuid = support::next_uuid(cx.identity, &channelset_seed, &mut rank)?;

            // -- the data: inline, or one dataset (the extension) -------
            let channel_data = match &in_file {
                None => {
                    let rows: Vec<Value> = (0..frame.mds.len())
                        .map(|at| {
                            let mut row = vec![json_number(frame.mds[at])];
                            row.extend(channels.iter().map(|judged| json_number(judged.values[at])));
                            Value::Array(row)
                        })
                        .collect();
                    let data_text = serde_json::to_string(&Value::Array(rows)).map_err(|why| {
                        Refusal::new("refused", format!("the channel data of `{}` did not encode: {why}", frame.name))
                    })?;
                    witsml::ChannelData { data: Some(data_text), file_uri: None }
                }
                Some(file) => {
                    // The inline rows, row for row: `[rows, 1 + channels]`,
                    // and `NaN` where inline writes `null`.
                    let width = 1 + channels.len();
                    let mut values = Vec::with_capacity(frame.mds.len() * width);
                    for at in 0..frame.mds.len() {
                        values.push(nan_unless_finite(frame.mds[at]));
                        values.extend(channels.iter().map(|judged| nan_unless_finite(judged.values[at])));
                    }
                    let path = format!("/witsml21/{channelset_uuid}/ChannelData");
                    datasets.push(Dataset {
                        file: file.clone(),
                        path: path.clone(),
                        dimensions: vec![frame.mds.len() as i64, width as i64],
                        values: Numbers::F64(values),
                    });
                    witsml::ChannelData { data: Some(path), file_uri: Some(file.clone()) }
                }
            };

            // -- the ChannelSet, nested (not a second part) -------------------
            let channel_set = witsml::ChannelSet {
                citation: citation(
                    &titled(format!("{wellbore_title} — channels")),
                    Some(format!("The channels of `{}`: {}.", frame.name, listed(&channel_titles))),
                ),
                extension_name_value: mark(),
                uuid: channelset_uuid.clone(),
                schema_version: "2.1".to_string(),
                active_status: eml::ActiveStatusKind::Inactive,
                index: vec![channel_index],
                channel: any_channels,
                data: Some(channel_data),
                wellbore: Some(wellbore_ref.clone()),
                ..Default::default()
            };

            // -- the Log — the one part this operation writes -----------------
            let log_title = titled(format!("{wellbore_title} — Log"));
            let log_description = format!(
                "The WITSML side of {} continuous propert{} on `{}`: {}.",
                channels.len(),
                if channels.len() == 1 { "y" } else { "ies" },
                frame.name,
                listed(&channel_titles)
            );
            if log_description.chars().count() > DESCRIPTION_LIMIT {
                return Err(Refusal::on(
                    "properties",
                    format!("`{}`'s Log would carry a description of {} characters, and Citation/Description holds {DESCRIPTION_LIMIT}", frame.name, log_description.chars().count()),
                ));
            }
            let log_seed = marked(format!("witsml-logs:log:{}:{channelset_uuid}", frame.reference.uuid.to_ascii_lowercase()));
            let log_uuid = support::next_uuid(cx.identity, &log_seed, &mut rank)?;
            let log = witsml::Log {
                citation: citation(&log_title, Some(log_description)),
                extension_name_value: mark(),
                uuid: log_uuid.clone(),
                schema_version: "2.1".to_string(),
                active_status: eml::ActiveStatusKind::Inactive,
                channel_set: vec![witsml::AnyChannelSet::ChannelSet(channel_set)],
                wellbore: Some(wellbore_ref),
                ..Default::default()
            };

            activity_inputs.push(Parameter::Object { title: "input".to_string(), reference: frame.reference.dor() });
            objects.push(NewObject { uuid: log_uuid, qualified_type: "witsml21.Log".to_string(), role: "log".to_string(), title: log_title, json: energyml_core::json::write_value(&log) });
            log_count += 1;
        }

        if log_count == 0 {
            return Err(Refusal::on(
                "frames",
                format!(
                    "no frame ends up with a channel: {}",
                    dropped.iter().map(|one| one.why.as_str()).collect::<Vec<_>>().join("; ")
                ),
            ));
        }

        // -- properties that hang on none of `frames`, said once, not per-frame
        let elsewhere = inputs
            .properties
            .iter()
            .filter(|property| {
                let Some(facts) = property.known.property.as_ref() else { return false };
                let Some(supporting) = facts.supporting.as_ref() else { return true };
                !frame_uuids.contains(&supporting.uuid.to_ascii_lowercase())
            })
            .count();

        // -- what is said -------------------------------------------------------
        let mut notes = vec![match &in_file {
            None => Note {
                key: "written",
                detail: format!("{log_count} Log(s), {channel_count} Channel(s), the values inline — never a FileUri, never the HDF5"),
            },
            Some(file) => Note {
                key: "written",
                detail: format!(
                    "{log_count} Log(s), {channel_count} Channel(s), the values in `{file}`, one dataset per ChannelSet — FileUri names the file and Data the dataset, an extension beyond WITSML 2.1: a reader that does not know it refuses the rows"
                ),
            },
        }];
        if elsewhere > 0 {
            notes.push(Note {
                key: "properties-elsewhere",
                detail: format!("{elsewhere} propert(ies) hang on a frame not given here, or on none: not turned into a channel"),
            });
        }
        for one in &dropped {
            notes.push(Note { key: "dropped", detail: one.why.clone() });
        }
        if params.synthetic {
            notes.push(Note {
                key: "synthetic",
                detail: "every object made carries `synthetic = true` and a description pointing back to the RESQML property it repeats".to_string(),
            });
        }

        let count = |title: &str, value: usize| Parameter::Double { title: title.to_string(), value: value as f64, uom: None };
        let mut parameters = activity_inputs;
        parameters.push(count("logCount", log_count));
        parameters.push(count("channelCount", channel_count));
        let seed = format!("witsml-logs:{}", objects.iter().map(|o| o.uuid.as_str()).collect::<Vec<_>>().join(","));

        Ok(Plan {
            media: Vec::new(),
            blocked: None,
            objects,
            datasets,
            // **No shape**: a Log makes no geometry, it lays values
            // on that of a frame already written (the same
            // rule as `property`).
            shapes: Vec::new(),
            activity: ActivityDraft {
                recipe: "witsml-logs",
                title: format!("witsml-logs → {log_count} Log(s)"),
                seed,
                parameters,
            },
            says: notes,
            dropped,
        })
    }
}
