//! `section-view` — the trace a section was drawn along, as a polyline set the
//! package can point at, and the drawing attached to it.
//!
//! The need: to put it in the reference model, create a
//! polyline set (with suitable interpretations and features) to represent
//! the profile line, an activity instance and template for the computation of the
//! profiles, and link the SVG and PNG section images to the activity as a
//! result.
//!
//! # The image is the input, and the trace is read from it
//!
//! A section is computed by `energyml-section` from a whole package —
//! every well, every surface — which a plan cannot read: `plan` is pure. So the
//! drawing is made first (`energyml profile --svg --png`, the front end's export),
//! and this operation takes **the drawing**. Its `<metadata>` holds the inputs
//! it was computed from — the trace, the corridors, the cross sections — and
//! `energyml_section::svg::read_metadata` is how they come back. The lines
//! written are therefore the lines drawn, **one fact**: a trace given beside
//! the image would be a second statement of it, free to disagree.
//!
//! # What is written
//!
//! - a `CulturalFeature` of kind **`energyml-rust:section line`** and its
//!   `GenericFeatureInterpretation` — `CulturalFeatureKindExt` is open
//!   (`resqml22`), and none of the listed kinds says *the line a section
//!   follows*; or the interpretation named in `interpretation`;
//! - the `PolylineSetRepresentation`, by [`super::polylines::write`]: patch 0
//!   the long section's trace, patch 1 its cross sections (when there are any)
//!   — the traces `energyml_section::crosses::traces` computes, the same
//!   function the drawing used;
//! - an `eml23.Attachment` per image, in the **part-address** form, by
//!   [`super::attach::Attach`] — the part in the zip, related to its
//!   `Attachment`, which names the polyline set. The activity lists every
//!   object written as its output, so the images are results of the run.
//!
//! # Further drawings
//!
//! The need: an attachment as a folder that holds all the exports.
//! `drawings` takes the other files of an export — the cross
//! sections, the view from above, as SVG or PNG — and attaches each to the lines
//! like the first, in the same folder. What they are is read from their bytes
//! (a PNG by its signature, an SVG by opening as one), never their name; an SVG
//! that states a trace must state **this** one; two files on one part are refused
//! by their name. A further drawing's `Attachment` uuid is derived from the lines
//! **and its file's name**, so storing the section again replaces it, and the
//! activity names each part (`drawingPart`, one parameter per file). Without
//! `drawings` nothing here changes.
//!
//! # The activity is the computation's
//!
//! Every setting the section was computed and drawn with is **a parameter of
//! the template** ([`DrawingParams`]): suggested from the drawing, so the form
//! shows it and the registry records it, and refused when an answer disagrees
//! with it. A list is a parameter per entry. The wells and surfaces the drawing
//! was computed from are the activity's **`input`s**, by reference, taken from
//! the package (`objects`, filled by default with every well and surface); one
//! the package does not hold is refused by its uri. One JSON string of it all
//! was the first attempt, and a `StringParameter` holds 2000 characters —
//! Lutetia's held 10 155 (03/10).
//!
//! # What is checked, because the two can disagree
//!
//! - **the system**: the image says the EPSG code its trace is in; a
//!   `sourceEpsg` that says another is refused with both, and so is a named
//!   frame whose projected half names another;
//! - **the PNG is a PNG** (its signature, never its name), and **of the same
//!   drawing**: its proportions are the SVG's, to the pixel a rounding moves.
//!   Its scale is recorded. That it was rendered from *this* SVG is not
//!   something bytes can prove, and nothing here claims it.
//!
//! # A height nobody drew
//!
//! The trace is a line on a map. A polyline set holds three coordinates, and
//! `elevation` says the third — asked, never assumed (zero is a datum).

use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::{Geometry, Patch, Topology};
use energyml_section::model::SectionKey;

use super::attach::{Attach, AttachInputs, AttachParams, Form};
use super::media::Document;
use super::polylines::PolylinesInputs;
use super::support;
use super::written::{NewFeatureParams, Written, WrittenImposed};
use energyml_section::layout::Metadata;
use energyml_section::model::{Crosses, Target};
use serde_json::{Value, json};

use crate::declare::{Choice, InputParam, Inputs, ObjectIn, Params, Shape, Suggestions, Title, TypeName};
use crate::operation::{About, Category, Context, Identity, Need, Note, Operation, Plan, Refusal, Slot};
use crate::pieces::{Known, Pieces, Ref, Source};

/// The operation.
pub struct SectionView;

/// The feature this writes, and its kind.
const FEATURE: &str = "resqml22.CulturalFeature";
const INTERPRETATION: &str = "resqml22.GenericFeatureInterpretation";
/// **An extension value, not a bare word.** `CulturalFeatureKindExt` is the union
/// of `CulturalFeatureKind` and `eml:EnumExtensionPattern` (`.*:.*`, resqml 2.2
/// Features.xsd l. 38-40): a value outside the listed ones must carry an
/// authority and a colon. `section line` did not, and was invalid per the
/// schema (found 04/10/2026); packages written before carry the old spelling,
/// which readers still accept (web `isSectionLineFeature`).
const KIND: &str = "energyml-rust:section line";

/// What `section-view` is told.
#[derive(Debug, Params)]
pub struct SectionViewParams {
    /// The frame, what is represented, the naming.
    #[param(flatten)]
    pub written: WrittenImposed,
    /// the elevation the lines are written at, in the vertical system — under a frame of the package counting down, written as its depth; a section is drawn along a line on a map, and the drawing holds no height for it
    pub elevation: f64,
    /// the SVG's part name in the package — left out, `media/sections/`, a folder named after the section, and the file's name
    pub svg_path: Option<String>,
    /// the PNG's part name in the package — left out, `media/sections/`, a folder named after the section, and the file's name
    pub png_path: Option<String>,
    /// what the images are, in a word (64 characters at most)
    #[param(default = String::from("SectionView"), optional)]
    pub category: String,
    /// the id this package was asked under in the drawing (`s1`, `s2`… in the front end) — its objects are the run's inputs, another package's are not
    #[param(default = String::from("s1"), optional)]
    pub store: String,
    /// What the section was computed with, and how it was drawn.
    #[param(flatten)]
    pub drawing: DrawingParams,
}

/// The cross sections of a long section, as `energyml_section::model::Crosses` has them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Choice)]
pub enum CrossKind {
    /// No cross section.
    None,
    /// One every `crossSpacing` along the trace.
    Every,
    /// `crossCount` of them, spread evenly, ends included.
    Count,
}

/// The section drawn: the long one, the plan, or a cross section by index.
///
/// Text and not a choice, because a cross section carries its index — the
/// words of `energyml profile --draw`: `long`, `plan`, `cross:N`.
fn target_word(target: &Target) -> String {
    match target {
        Target::Plan => "plan".to_string(),
        Target::Section { key: SectionKey::Long } => "long".to_string(),
        Target::Section { key: SectionKey::Cross { index } } => format!("cross:{index}"),
    }
}

/// **What the section was computed with, and how it was drawn — each a
/// parameter of the template** (an activity template defines all the
/// parameters, and the activity adapts them).
///
/// Every one is read from the drawing — suggested, so the form shows them and
/// the registry records them — and **refused if it says otherwise**: the
/// drawing is what was computed, and a value that disagrees with it would be
/// an activity describing a run that did not happen. The lists keyed by object
/// (`hidden`, `colours`, …) are one parameter per entry, each short: a single
/// JSON string held 10 155 characters on Lutetia, and a `StringParameter`
/// holds 2000.
#[derive(Debug, Params)]
pub struct DrawingParams {
    /// the long section's corridor on the left of the trace, as it was drawn, in the trace's unit
    pub left: Option<f64>,
    /// the long section's corridor on the right of the trace
    pub right: Option<f64>,
    /// the cross sections: none, one every `crossSpacing`, or `crossCount` of them
    pub crosses: Option<CrossKind>,
    /// the step between two cross sections, along the trace
    pub cross_spacing: Option<f64>,
    /// how many cross sections, spread evenly, ends included
    pub cross_count: Option<i64>,
    /// a cross section's length on the left of the long section
    pub cross_before: Option<f64>,
    /// a cross section's length on the right of the long section
    pub cross_after: Option<f64>,
    /// a cross section's corridor on its left
    pub cross_left: Option<f64>,
    /// a cross section's corridor on its right
    pub cross_right: Option<f64>,
    /// a cross section with its own widths, one per entry: `index: before, after, left, right`
    pub cross_own: Vec<String>,
    /// the metres of the vertical axis a unit of time is worth, when an object in time was asked for
    pub time_factor: Option<f64>,
    /// the section drawn: `long`, `plan` or `cross:N`
    pub draw: Option<String>,
    /// the drawing's width, in pixels
    pub width: Option<f64>,
    /// the drawing's height, in pixels
    pub height: Option<f64>,
    /// how objects are named on the drawing: their title (`full`) or a code and a legend (`codes`)
    pub labels: Option<String>,
    /// what the horizontal axis says: `projected`, `geodesic` or `coordinates`
    pub axis: Option<String>,
    /// how wells are coloured: `side`, `scene` or `strata`
    pub well_colouring: Option<String>,
    /// how wide a well is drawn, in pixels
    pub well_width: Option<f64>,
    /// the background colour
    pub background: Option<String>,
    /// the start of the portion of the trace drawn — the abscissas keep their origin
    pub portion_start: Option<f64>,
    /// the end of the portion of the trace drawn
    pub portion_end: Option<f64>,
    /// the window drawn, `x0, x1, y0, y1` — left out, everything with a margin
    pub window: Vec<f64>,
    /// what is shown, one word per entry: crossings, vertices, distances, names, rejected, bands, surfaceNames
    pub shown: Vec<String>,
    /// an object hidden in this drawing, by its page key (`store␀uri`)
    pub hidden: Vec<String>,
    /// an object put forward on purpose, by its page key
    pub highlight: Vec<String>,
    /// a colour the scene gave an object: `key=#rrggbb`
    pub colours: Vec<String>,
    /// which way a surface's colour fills: `key=up`, `key=down`, `key=both` or `key=none`
    pub fills: Vec<String>,
}

/// The front end's key as the SVG spells it: the NUL becomes U+2400 (`svg.rs`).
fn spelled(key: &str) -> String {
    key.replace('\0', "\u{2400}")
}

/// What the drawing states, slot by slot — the **one** reading of the metadata
/// into parameters, used to suggest and to confront. A list is a JSON array; a
/// value the drawing does not have (no time factor, no window) is left out.
fn drawn(metadata: &Metadata) -> Vec<(&'static str, Value)> {
    let inputs = &metadata.inputs;
    let view = &metadata.view;
    let mut out: Vec<(&'static str, Value)> = vec![("left", json!(inputs.left)), ("right", json!(inputs.right))];
    let (kind, common, own) = match &inputs.crosses {
        Crosses::None => ("none", None, &[][..]),
        Crosses::Every { spacing, common, own } => {
            out.push(("crossSpacing", json!(spacing)));
            ("every", Some(common), own.as_slice())
        }
        Crosses::Count { count, common, own } => {
            out.push(("crossCount", json!(count)));
            ("count", Some(common), own.as_slice())
        }
    };
    out.push(("crosses", json!(kind)));
    if let Some(widths) = common {
        out.extend([
            ("crossBefore", json!(widths.before)),
            ("crossAfter", json!(widths.after)),
            ("crossLeft", json!(widths.left)),
            ("crossRight", json!(widths.right)),
        ]);
    }
    let own: Vec<String> = own
        .iter()
        .map(|o| format!("{}: {}, {}, {}, {}", o.index, o.widths.before, o.widths.after, o.widths.left, o.widths.right))
        .collect();
    if !own.is_empty() {
        out.push(("crossOwn", json!(own)));
    }
    if let Some(factor) = inputs.time_factor {
        out.push(("timeFactor", json!(factor)));
    }
    out.extend([
        ("draw", json!(target_word(&view.target))),
        ("width", json!(view.size_px[0])),
        ("height", json!(view.size_px[1])),
        ("labels", json!(word(&view.labels))),
        ("axis", json!(word(&view.axis))),
        ("wellColouring", json!(word(&view.well_colouring))),
        ("wellWidth", json!(view.well_width_px)),
        ("background", json!(view.background)),
    ]);
    if let Some([start, end]) = view.portion {
        out.extend([("portionStart", json!(start)), ("portionEnd", json!(end))]);
    }
    if let Some(window) = &view.window {
        out.push(("window", json!([window.x[0], window.x[1], window.y[0], window.y[1]])));
    }
    let show = &view.show;
    let shown: Vec<&str> = [
        ("crossings", show.crossings),
        ("vertices", show.vertices),
        ("distances", show.distances),
        ("names", show.names),
        ("rejected", show.rejected),
        ("bands", show.bands),
        ("surfaceNames", show.surface_names),
        ("notes", show.notes),
    ]
    .into_iter()
    .filter_map(|(name, on)| on.then_some(name))
    .collect();
    let lists: [(&'static str, Vec<String>); 5] = [
        ("shown", shown.into_iter().map(str::to_string).collect()),
        ("hidden", view.hidden.iter().map(|k| spelled(k)).collect()),
        ("highlight", view.highlight.iter().map(|k| spelled(k)).collect()),
        ("colours", view.colours.iter().map(|(k, c)| format!("{}={c}", spelled(k))).collect()),
        (
            "fills",
            view.fills
                .iter()
                .map(|f| {
                    let way = match (f.up, f.down) {
                        (true, true) => "both",
                        (true, false) => "up",
                        (false, true) => "down",
                        (false, false) => "none",
                    };
                    format!("{}={way}", spelled(&f.key))
                })
                .collect(),
        ),
    ];
    for (name, list) in lists {
        if !list.is_empty() {
            out.push((name, json!(list)));
        }
    }
    out
}

/// The word serde gives one of the view's enumerations — the same as in the
/// SVG's metadata, so that a parameter reads as the drawing does.
fn word<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

/// What the parameters say, slot by slot, in the shape [`drawn`] gives.
fn given(p: &DrawingParams) -> Vec<(&'static str, Option<Value>)> {
    let list = |values: &[String]| (!values.is_empty()).then(|| json!(values));
    vec![
        ("left", p.left.map(|v| json!(v))),
        ("right", p.right.map(|v| json!(v))),
        ("crosses", p.crosses.map(|v| json!(v.word()))),
        ("crossSpacing", p.cross_spacing.map(|v| json!(v))),
        ("crossCount", p.cross_count.map(|v| json!(v))),
        ("crossBefore", p.cross_before.map(|v| json!(v))),
        ("crossAfter", p.cross_after.map(|v| json!(v))),
        ("crossLeft", p.cross_left.map(|v| json!(v))),
        ("crossRight", p.cross_right.map(|v| json!(v))),
        ("crossOwn", list(&p.cross_own)),
        ("timeFactor", p.time_factor.map(|v| json!(v))),
        ("draw", p.draw.clone().map(|v| json!(v))),
        ("width", p.width.map(|v| json!(v))),
        ("height", p.height.map(|v| json!(v))),
        ("labels", p.labels.clone().map(|v| json!(v))),
        ("axis", p.axis.clone().map(|v| json!(v))),
        ("wellColouring", p.well_colouring.clone().map(|v| json!(v))),
        ("wellWidth", p.well_width.map(|v| json!(v))),
        ("background", p.background.clone().map(|v| json!(v))),
        ("portionStart", p.portion_start.map(|v| json!(v))),
        ("portionEnd", p.portion_end.map(|v| json!(v))),
        ("window", (!p.window.is_empty()).then(|| json!(p.window))),
        ("shown", list(&p.shown)),
        ("hidden", list(&p.hidden)),
        ("highlight", list(&p.highlight)),
        ("colours", list(&p.colours)),
        ("fills", list(&p.fills)),
    ]
}

/// Two JSON numbers are one value whatever their spelling (`3` and `3.0`).
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(a, b)| same(a, b)),
        _ => a == b,
    }
}

/// The drawing's metadata, from its bytes.
fn metadata_of(svg: &Document) -> Result<Metadata, Refusal> {
    let text = std::str::from_utf8(&svg.bytes).map_err(|_| Refusal::on("svg", format!("`{}` is not UTF-8 text, and an SVG is", svg.file)))?;
    energyml_section::svg::read_metadata(text).map_err(|why| Refusal::on("svg", format!("`{}`: {why}", svg.file)))
}

/// What a uri read from a drawing's metadata names among the objects held.
enum Named<'a> {
    /// Exactly one.
    One(&'a Ref),
    /// None of them.
    None,
    /// Several: the uri does not say which (its uuid is held in several
    /// versions and it names none) — their uris.
    Several(Vec<String>),
}

/// The held object an ETP uri names — read by the one parser of uris
/// (`EtpUri`), never by hand: a hand-made reading took `uuid=…,version='2'` for
/// a uuid. **The uuid, then the version the uri names**; a short uri
/// names the entry that declares none when there are several, as the store's own
/// resolution does (`EpcStore::resolve`), and is otherwise ambiguous.
fn named_in<'a>(held: &[&'a Ref], uri: &str) -> Named<'a> {
    let Ok(asked) = energyml_core::identifiers::EtpUri::parse(uri) else { return Named::None };
    let Some(uuid) = asked.uuid.as_deref() else { return Named::None };
    let mut same: Vec<&'a Ref> = held.iter().copied().filter(|one| one.uuid.eq_ignore_ascii_case(uuid)).collect();
    match asked.object_version.as_deref() {
        Some(version) => same.retain(|one| one.object_version.as_deref() == Some(version)),
        // Several and no version named: the one that declares none, when exactly
        // one does — and otherwise they all stay, and the uri is ambiguous.
        None if same.len() > 1 => {
            let bare: Vec<&'a Ref> = same.iter().copied().filter(|one| one.object_version.is_none()).collect();
            if bare.len() == 1 {
                same = bare;
            }
        }
        None => {}
    }
    match same.as_slice() {
        [] => Named::None,
        [one] => Named::One(one),
        _ => Named::Several(same.iter().map(|one| one.uri()).collect()),
    }
}

/// What `section-view` takes.
#[derive(Debug, Inputs)]
pub struct SectionViewInputs {
    /// the section, drawn as SVG by the section view — the trace and everything it was computed from are read from its metadata
    pub svg: Document,
    /// the same drawing as a PNG, for a reader that draws no SVG
    pub png: Option<Document>,
    /// the other drawings of the same section — its cross sections, its plan view — as SVG or PNG, each attached to the lines like the first; the type of each is read from its bytes, and a second file of the same part name is refused
    pub drawings: Vec<Document>,
    /// the wells and surfaces of the package — those the drawing was computed from are recorded as the activity's inputs, and one it names that the package does not hold is refused
    #[input(
        types = "resqml22.WellboreFrameRepresentation, resqml22.WellboreTrajectoryRepresentation, resqml22.TriangulatedSetRepresentation, resqml22.Grid2dRepresentation, resqml22.IjkGridRepresentation, resqml20.obj_WellboreFrameRepresentation, resqml20.obj_WellboreTrajectoryRepresentation, resqml20.obj_TriangulatedSetRepresentation, resqml20.obj_Grid2dRepresentation, resqml20.obj_IjkGridRepresentation",
        every
    )]
    pub objects: Vec<ObjectIn>,
}

impl InputParam for Vec<Document> {
    type Item = Document;
    fn least() -> u32 {
        0
    }
    fn most() -> Option<u32> {
        None
    }
    fn from_items(items: Vec<Document>) -> Self {
        items
    }
}

/// What an `eml23.StringParameter` holds (`Activities.xsd`, `String2000`).
const STRING_PARAMETER: usize = 2000;

/// The eight bytes every PNG starts with (PNG §5.2).
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

/// A PNG's width and height, from its `IHDR` — which PNG §5.6 puts first.
fn png_size(bytes: &[u8]) -> Option<[u32; 2]> {
    if bytes.len() < 24 || bytes[..8] != PNG_SIGNATURE || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let word = |at: usize| u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    Some([word(16), word(20)])
}

/// What a further drawing is, from its bytes and never its name: a PNG by its
/// signature, an SVG by being text that opens as an SVG document (an optional
/// byte-order mark, an XML prolog, comments, a doctype, then `<svg`). Anything
/// else is refused, so a file is never attached under a type it is not.
fn drawing_type(file: &Document) -> Result<&'static str, Refusal> {
    let neither = |why: &str| {
        Refusal::on("drawings", format!("`{}` is neither a PNG (no signature) nor an SVG ({why}): only those two are drawings", file.file))
    };
    if png_size(&file.bytes).is_some() {
        return Ok("image/png");
    }
    let Ok(text) = std::str::from_utf8(&file.bytes) else {
        return Err(neither("not UTF-8 text"));
    };
    let mut rest = text.trim_start_matches('\u{feff}').trim_start();
    // The prolog of an XML document: declarations, comments, a doctype.
    while let Some((open, close)) = [("<?", "?>"), ("<!--", "-->"), ("<!", ">")].into_iter().find(|(open, _)| rest.starts_with(open)) {
        match rest.find(close) {
            Some(at) => rest = rest[at + close.len()..].trim_start(),
            None => return Err(neither(&format!("`{open}` is never closed"))),
        }
    }
    match rest.strip_prefix("<svg") {
        Some(after) if after.starts_with(|c: char| c.is_whitespace() || c == '>' || c == '/') => Ok("image/svg+xml"),
        _ => Err(neither("it does not open as an SVG document")),
    }
}

/// A part name for a file: the one stated, or `media/sections/<the section>/`
/// and its name — a folder per section, so that drawing a section again
/// writes over its own files and nobody else's (a
/// folder named like the section).
fn part_of(stated: &Option<String>, title: &str, file: &Document) -> String {
    stated.clone().unwrap_or_else(|| {
        format!("media/sections/{}/{}", folder_of(title), file.file.rsplit(['/', '\\']).next().unwrap_or(&file.file))
    })
}

/// A section's title as a folder name: what a part name may hold (letters,
/// digits, `-`, `_`, `.`), the rest a `_`, never empty.
fn folder_of(title: &str) -> String {
    let named: String = title
        .trim()
        .chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' })
        .collect();
    let named = named.trim_matches(['_', '.']).to_string();
    if named.is_empty() { "section".to_string() } else { named }
}

impl Operation for SectionView {
    type Params = SectionViewParams;
    type Inputs = SectionViewInputs;

    fn about(&self) -> About {
        About {
            name: "section-view",
            describe: "the line a section view was drawn along, as a section-line feature with its polylines, and the drawings — SVG and PNG — attached to it",
            category: Category::Create,
            makes: &[FEATURE, INTERPRETATION, "resqml22.PolylineSetRepresentation", "eml23.Attachment"],
            recipe: "section-view",
            represents: Some((FEATURE, INTERPRETATION)),
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        super::written::suggest_written(bag, self.about().represents, slots);
        // What the drawing was computed with, read off the drawing itself: the
        // form shows it, the registry records it, and `plan` holds an answer
        // that disagrees to it.
        if let Some(Source::Document { name, content_type, bytes }) = bag.get("svg") {
            let svg = Document { name: "svg".to_string(), file: name.clone(), declared: content_type.clone(), bytes: bytes.clone() };
            if let Ok(metadata) = metadata_of(&svg) {
                for (slot, value) in drawn(&metadata) {
                    slots.set(slot, value);
                }
            }
        }
    }

    fn plan(&self, inputs: SectionViewInputs, params: SectionViewParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
        // -- the drawing, read back, and what was said of it held to it
        let metadata = metadata_of(&inputs.svg)?;
        let states = drawn(&metadata);
        for (slot, told) in given(&params.drawing) {
            let Some(told) = told else { continue };
            match states.iter().find(|(name, _)| *name == slot) {
                Some((_, value)) if same(value, &told) => {}
                Some((_, value)) => {
                    return Err(Refusal::on(
                        slot,
                        format!("the drawing was made with {value}, and {told} is given: the activity would describe a run that did not happen — draw it again with {told}, or leave `{slot}` out"),
                    ));
                }
                None => {
                    return Err(Refusal::on(
                        slot,
                        format!("the drawing says nothing of `{slot}`, and {told} is given — draw it again with it, or leave it out"),
                    ));
                }
            }
        }
        let (sections, _) =
            energyml_section::crosses::traces(&metadata.inputs).map_err(|why| Refusal::on("svg", format!("`{}`: {why}", inputs.svg.file)))?;
        if !params.elevation.is_finite() {
            return Err(Refusal::on("elevation", format!("{} is not a height", params.elevation)));
        }

        // -- the system the trace is in, said twice
        let mut says: Vec<Note> = Vec::new();
        match &metadata.inputs.system {
            Some(system) => {
                let drawn = system.epsg as i64;
                if let Some(source) = params.written.frame.source_epsg.as_ref().map(|code| code.0)
                    && source != drawn
                {
                    return Err(Refusal::on(
                        "sourceEpsg",
                        format!("the drawing's trace is in EPSG:{drawn} — its metadata says so — and `sourceEpsg` says EPSG:{source}: the lines would be written where they were not drawn"),
                    ));
                }
                if params.written.frame.crs.is_some()
                    && let Some(named) = cx.known().iter().find_map(|known| known.crs_epsg)
                    && named != drawn
                {
                    return Err(Refusal::on(
                        "crs",
                        format!("the drawing's trace is in EPSG:{drawn} — its metadata says so — and the frame named is over EPSG:{named}"),
                    ));
                }
            }
            None => says.push(Note {
                key: "system",
                detail: "the drawing names no system: its trace is written in the frame given, as its numbers stand".to_string(),
            }),
        }

        // -- the PNG: a PNG, of the same proportions
        let size = metadata.view.size_px;
        let png = match &inputs.png {
            None => None,
            Some(file) => {
                let Some([w, h]) = png_size(&file.bytes) else {
                    return Err(Refusal::on("png", format!("`{}` is not a PNG — it does not start with the PNG signature", file.file)));
                };
                let (drawn, given) = (size[0] / size[1], f64::from(w) / f64::from(h));
                // A pixel of rounding on either side, and no more.
                let slack = 1.0 / f64::from(w.min(h)).max(1.0) + 1.0 / size[0].min(size[1]).max(1.0);
                if (given / drawn - 1.0).abs() > slack {
                    return Err(Refusal::on(
                        "png",
                        format!("`{}` is {w}×{h} px, and the drawing is {}×{} px: not the same proportions, so not the same drawing", file.file, size[0], size[1]),
                    ));
                }
                Some((file.clone(), f64::from(w) / size[0]))
            }
        };

        // -- the further drawings: each a PNG or an SVG by its bytes, an SVG
        // that states its trace stating **this** one, and no two on one part
        let title_of_section = params.written.naming.title();
        let mut further: Vec<(Document, String, &'static str)> = Vec::new();
        let mut without_metadata = 0usize;
        for file in &inputs.drawings {
            let kind = drawing_type(file)?;
            if kind == "image/svg+xml" {
                match metadata_of(file) {
                    Ok(other) => {
                        let (theirs, ours) = (other.inputs.system.as_ref().map(|s| s.epsg as i64), metadata.inputs.system.as_ref().map(|s| s.epsg as i64));
                        if other.inputs.trace != metadata.inputs.trace || theirs != ours {
                            return Err(Refusal::on(
                                "drawings",
                                format!("`{}` states another trace than `{}`: it is the drawing of another section, and would be attached to this one's lines", file.file, inputs.svg.file),
                            ));
                        }
                    }
                    Err(_) => without_metadata += 1,
                }
            }
            further.push((file.clone(), part_of(&None, &title_of_section, file), kind));
        }
        {
            let mut parts: Vec<(String, String)> = vec![(part_of(&params.svg_path, &title_of_section, &inputs.svg).to_lowercase(), inputs.svg.file.clone())];
            if let Some(file) = &inputs.png {
                parts.push((part_of(&params.png_path, &title_of_section, file).to_lowercase(), file.file.clone()));
            }
            for (file, part, _) in &further {
                // Part names are case-insensitive (EPC v1.0 §3.2, OPC).
                if let Some((_, first)) = parts.iter().find(|(held, _)| *held == part.to_lowercase()) {
                    return Err(Refusal::on(
                        "drawings",
                        format!("`{}` would be stored as `{part}`, the part `{first}` already takes: two files of the same name in a section's folder — rename one", file.file),
                    ));
                }
                parts.push((part.to_lowercase(), file.file.clone()));
            }
        }

        // -- the lines: the long section, then its cross sections
        let groups: Vec<Vec<&[[f64; 2]]>> = {
            let long: Vec<&[[f64; 2]]> = sections.iter().filter(|s| s.key == SectionKey::Long).map(|s| s.trace.as_slice()).collect();
            let crosses: Vec<&[[f64; 2]]> = sections.iter().filter(|s| s.key != SectionKey::Long).map(|s| s.trace.as_slice()).collect();
            [long, crosses].into_iter().filter(|g| !g.is_empty()).collect()
        };
        // **A height, written in the frame's own sense.** The trace is in
        // projected numbers and `elevation` is a height; a frame of the package
        // places them by its own origin, rotation and sense — a frame counting
        // down takes the height as its depth (the drawings are
        // stored in a package's existing frame rather than a new one). A frame
        // created here is the projected one, counting up: the numbers stand.
        let named = params.written.frame.crs.as_ref().and_then(|crs| {
            let uuid = crs.0["Uuid"].as_str().or_else(|| crs.0.as_str())?;
            cx.known()
                .into_iter()
                .find(|known| known.crs.as_ref().is_some_and(|r| r.uuid.eq_ignore_ascii_case(uuid)))?
                .placement
                .clone()
        });
        let placed = |x: f64, y: f64| match &named {
            Some(frame) => frame.from_projected([x, y, params.elevation]),
            None => [x, y, params.elevation],
        };
        let patches: Vec<Patch> = groups
            .iter()
            .enumerate()
            .map(|(index, lines)| Patch {
                index,
                node_offset: 0,
                points: lines.iter().flat_map(|line| line.iter().map(|[x, y]| placed(*x, *y))).collect(),
                topology: Topology::Polylines(lines.iter().map(|line| line.len() as u32).collect()),
                closed: vec![false; lines.len()],
            })
            .collect();
        let crosses = groups.get(1).map_or(0, Vec::len);
        let title = params.written.naming.title();
        let written = Written {
            frame: params.written.frame.clone(),
            represented: params.written.represented.clone(),
            naming: params.written.naming.clone(),
            new_feature: NewFeatureParams {
                feature_type: Some(TypeName(FEATURE.to_string())),
                interpretation_type: TypeName(INTERPRETATION.to_string()),
                feature_kind: Some(KIND.to_string()),
            },
        };
        let frame = support::frame_in(&cx.known(), &written.frame, cx.identity, cx.when, &mut 0)?;
        let shape = Shape {
            name: inputs.svg.name.clone(),
            geometry: support::geometry_placed(&inputs.svg.name, &Geometry { kind: "polylines", patches }, None, &frame)?,
            closed: None,
            known: None,
        };
        let seed = cx.seed(&title);
        let lines = PolylinesInputs { shape: vec![shape], features: None };
        let mut plan = super::polylines::write(&lines, &written, Some(false), &seed, cx)?;
        let representation = plan
            .objects
            .iter()
            .find(|o| o.role == "representation")
            .map(|o| Ref::new(o.uuid.clone(), o.qualified_type.clone(), o.title.clone()))
            .ok_or_else(|| Refusal::new("refused", "no representation was planned"))?;

        // -- the images, attached to the lines by the one writer of attachments
        let mut rank = plan.objects.len();
        let mut parameters = std::mem::take(&mut plan.activity.parameters);
        // `what` names the attachment's title; `seed` is what it is, in the uuid
        // derived for it; `parameter` is the title of the activity's record.
        let mut attach = |file: Document, part: String, kind: &str, what: &str, seed: &str, parameter: &str| -> Result<(), Refusal> {
            // Given uuids are handed out in the plan's order: the attachment
            // takes the next one, not the first, which the lines have.
            let identity = match cx.identity {
                Identity::Derived { .. } => cx.identity.clone(),
                Identity::Given(_) => Identity::Given(vec![support::next_uuid(cx.identity, "", &mut rank)?]),
            };
            let sub = Context { identity: &identity, ..*cx };
            let target = ObjectIn { name: "target".to_string(), known: Known { reference: Some(representation.clone()), ..Known::default() } };
            let made = Attach.plan(
                AttachInputs { target, file: Some(file) },
                AttachParams {
                    form: Some(Form::PartAddress),
                    address: None,
                    md: None,
                    md_uom: "m".to_string(),
                    category: Some(params.category.clone()),
                    file_name: None,
                    content_type: Some(kind.to_string()),
                    path: Some(part.clone()),
                    title: Some(Title(format!("{title} — {what}"))),
                },
                &sub,
            )?;
            parameters.push(Parameter::Text { title: parameter.to_string(), value: part });
            // **A drawing drawn again replaces its attachment.** `attach` names
            // an attachment by its bytes too — a second file is a second
            // attachment —, and a redrawn section would then leave the first
            // beside the second, both naming the same part. A section's drawing
            // is named by the section and what it is (`SVG`, `PNG`): the same
            // section stored twice writes the same object over itself. A further
            // drawing is named by its file's name instead — a section has one
            // SVG and one PNG of its own, and as many others as it has files.
            let mut objects = made.objects;
            let mut media = made.media;
            if let (Identity::Derived { .. }, Some(object)) = (cx.identity, objects.first_mut()) {
                let uuid = cx.identity.uuid(&format!("section-view:attachment:{}:{seed}", representation.uuid.to_ascii_lowercase()), 0)?;
                object.json["uuid"] = serde_json::json!(uuid);
                object.uuid = uuid.clone();
                for part in &mut media {
                    part.related_to = vec![uuid.to_ascii_lowercase()];
                }
            }
            plan.objects.extend(objects);
            plan.media.extend(media);
            Ok(())
        };
        // Known to be an SVG — its metadata was just read from it — and a PNG
        // by its signature: the types are stated from the bytes, never the name.
        let svg_part = part_of(&params.svg_path, &title, &inputs.svg);
        attach(inputs.svg.clone(), svg_part, "image/svg+xml", "SVG", "SVG", "svgPart")?;
        let mut png_scale = None;
        if let Some((file, scale)) = png {
            let part = part_of(&params.png_path, &title, &file);
            attach(file, part, "image/png", "PNG", "PNG", "pngPart")?;
            png_scale = Some(scale);
        }
        // The further drawings, in the order given: one attachment each, and
        // one `drawingPart` parameter each in the activity.
        for (file, part, kind) in further {
            let name = part.rsplit('/').next().unwrap_or(&part).to_string();
            let seed = format!("file:{}", name.to_lowercase());
            attach(file, part, kind, &name, &seed, "drawingPart")?;
        }
        // The closure is done with `parameters`: what it left is the scale.
        if let Some(scale) = png_scale {
            parameters.push(Parameter::Double { title: "pngScale".to_string(), value: scale, uom: None });
        }

        // -- what the run was. The scalars are recorded by the registry, as
        // every settled choice; a list is one parameter per entry, written
        // here — each held to what a `StringParameter` holds.
        parameters.push(Parameter::Double { title: "crossSectionCount".to_string(), value: crosses as f64, uom: None });
        for (slot, value) in &states {
            let Value::Array(entries) = value else { continue };
            for entry in entries {
                let parameter = match entry {
                    Value::Number(n) => Parameter::Double { title: slot.to_string(), value: n.as_f64().unwrap_or(f64::NAN), uom: None },
                    other => {
                        let text = other.as_str().map(str::to_string).unwrap_or_else(|| other.to_string());
                        if text.chars().count() > STRING_PARAMETER {
                            return Err(Refusal::on(
                                slot,
                                format!("an entry of {} characters, and a string parameter holds {STRING_PARAMETER} (`Activities.xsd`)", text.chars().count()),
                            ));
                        }
                        Parameter::Text { title: slot.to_string(), value: text }
                    }
                };
                parameters.push(parameter);
            }
        }
        // The objects the section was computed from are the run's inputs, by
        // reference — and one the package does not hold says the drawing was
        // made from another package, or before that object was written.
        let held: Vec<&Ref> = inputs.objects.iter().filter_map(|one| one.known.reference.as_ref()).collect();
        // Only this package's: a drawing over several packages names objects
        // this one does not hold, and that is not a refusal — it is said.
        let elsewhere: usize = metadata
            .inputs
            .objects
            .iter()
            .filter(|asked| asked.store != params.store)
            .map(|asked| asked.wells.len() + asked.surfaces.len())
            .sum();
        if elsewhere > 0 {
            says.push(Note {
                key: "other-packages",
                detail: format!("{elsewhere} object(s) the drawing was computed from are in other packages, and are not recorded as this run's inputs"),
            });
        }
        let mine = metadata.inputs.objects.iter().filter(|asked| asked.store == params.store);
        for uri in mine.flat_map(|asked| asked.wells.iter().chain(&asked.surfaces)) {
            let found = match named_in(&held, uri) {
                Named::One(found) => found,
                Named::None => {
                    return Err(Refusal::on(
                        "objects",
                        format!("the drawing was computed from `{uri}`, which this package does not hold — it was drawn from another package, or before that object was written"),
                    ));
                }
                Named::Several(uris) => {
                    return Err(Refusal::on(
                        "objects",
                        format!("the drawing was computed from `{uri}`, which names an object held in several versions — {} — name one with its version", uris.join(", ")),
                    ));
                }
            };
            // The reference carries the version of the object it names, so that
            // the input of a versioned copy never names its original.
            parameters.push(Parameter::Object { title: "input".to_string(), reference: found.dor() });
        }
        says.insert(
            0,
            Note {
                key: "drawing",
                detail: format!(
                    "the lines are the drawing's: the long section{} as `{}` reads them from `{}`, at an elevation of {}",
                    if crosses > 0 { format!(" and {crosses} cross section(s), patch 1,") } else { String::new() },
                    "svg::read_metadata",
                    inputs.svg.file,
                    params.elevation
                ),
            },
        );
        if !inputs.drawings.is_empty() {
            let more = inputs.drawings.len();
            let bare = if without_metadata > 0 { format!(", {without_metadata} of them an SVG that states no section view and is attached as it is") } else { String::new() };
            says.push(Note {
                key: "drawings",
                detail: format!("{more} further drawing(s) attached to the lines under `media/sections/{}/`{bare}", folder_of(&title)),
            });
        }
        says.extend(std::mem::take(&mut plan.says));
        plan.says = says;
        plan.activity.recipe = "section-view";
        plan.activity.title = format!("section view → {title}");
        plan.activity.parameters = parameters;
        Ok(plan)
    }

    fn reports(&self) -> Vec<Slot> {
        let mut slots = crate::operation::AnyOperation::reports(&crate::operation::Typed(super::polylines::Polylines));
        let number = || Need::Number { uom: None, least: None, most: None };
        slots.extend([
            Slot::required("svgPart", Need::Text, "the SVG's part name in the package"),
            Slot::optional("pngPart", Need::Text, "the PNG's part name in the package"),
            Slot::many("drawingPart", Need::Text, "the part name of each further drawing, in the order given", 0),
            Slot::optional("pngScale", number(), "the PNG's pixels per pixel of the drawing"),
            Slot::required("crossSectionCount", number(), "the cross sections written, patch 1"),
        ]);
        slots
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const X: &str = "7e400000-0000-4000-8000-000000000001";
    const QT: &str = "resqml22.TriangulatedSetRepresentation";

    fn named(held: &[Ref], uri: &str) -> Result<String, usize> {
        let borrowed: Vec<&Ref> = held.iter().collect();
        match named_in(&borrowed, uri) {
            Named::One(one) => Ok(one.uri()),
            Named::None => Err(0),
            Named::Several(uris) => Err(uris.len()),
        }
    }

    #[test]
    fn a_uri_of_a_drawing_names_the_held_object_of_its_version() {
        let original = Ref::new(X, QT, "H3");
        let copy = Ref::new(X, QT, "H3").with_version(Some("2".to_string()));
        let both = [original.clone(), copy.clone()];

        // The version the uri names is the version taken…
        assert_eq!(named(&both, &copy.uri()), Ok(copy.uri()));
        // …a short uri the entry that declares none, as the store resolves it…
        assert_eq!(named(&both, &original.uri()), Ok(original.uri()));
        // …and a version nobody holds, or an uuid nobody holds, nothing.
        let third = Ref::new(X, QT, "H3").with_version(Some("3".to_string()));
        assert_eq!(named(&both, &third.uri()), Err(0));
        assert_eq!(named(&both, "eml:///resqml22.TriangulatedSetRepresentation(7e400000-0000-4000-8000-0000000000ff)"), Err(0));
        assert_eq!(named(&both, "not a uri"), Err(0));

        // Two versions and none without: the short uri does not say which.
        let versioned = [Ref::new(X, QT, "H3").with_version(Some("1".to_string())), copy];
        assert_eq!(named(&versioned, &original.uri()), Err(2));
    }
}
