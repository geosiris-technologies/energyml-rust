//! A package's `WellboreFrameRepresentation` turned into a [`layout::Board`]
//! — the third step of the sheet design.
//!
//! # What this module reuses, and what it does not
//!
//! This is deliberately **not** a second reader of a frame's `NodeMd`, of its
//! stratigraphy, or of a table's rows: those already exist, and are called
//! here rather than rewritten.
//!
//! - **Stratigraphy** ([`energyml_domain::well_strata::of_frame`]) answers
//!   for a rank of `IntervalStratigraphicUnits` *and* for a categorical
//!   property with a `CategoryLookup` — both come back as a
//!   [`well_strata::Strata`] (sections, units, colours, the FIRP colour
//!   cascade already resolved), and [`track_from_strata`] is the only new
//!   code: turning that shape into an [`IntervalTrack`].
//! - **A table row's label** is [`energyml_domain::table::Lookup`], reached
//!   through `well_strata::of_frame`'s own `Wanted::Category` — this module
//!   never opens a `ColumnBasedTable` itself.
//! - **A property kind's title** is
//!   [`energyml_domain::property_kind::Resolver`] — package, then
//!   dictionary, then the standard name, exactly as every other reader in
//!   this workspace resolves one.
//!
//! What genuinely is new here, because nothing above needed it: aligning a
//! **discrete or continuous** interval property (no `CategoryLookup`, so
//! `well_strata` has nothing to read) against the frame's own `[md_top,
//! md_base]` windows, and reading `NodeMd` once up front so a board can be
//! built even for a frame that carries no stratigraphy at all.
//!
//! `energyml_geometry::scene::attributes_for` was read before writing this
//! (per the crate's rules) for the same interval ↔ node correspondence
//! rule — *"each node takes the interval it opens, the deepest one the
//! interval it closes"* — but it is not called from here: it moves interval
//! values **onto nodes**, for painting a 3D tube that has one point per node
//! and no notion of an interval at all. A sheet's [`IntervalTrack`] already
//! has `[md_top, md_base]` cells, so there is nothing to project — the rule
//! is the same, the shape it is applied to is not. Depending on
//! `energyml-geometry` for this increment would pull in the whole point/
//! topology evaluation stack for a correspondence that is four lines without
//! it; TVD and the "real" view, when they arrive, are the reason that
//! crate is named as a future dependency of this crate.
//!
//! # What is deliberately left for a later increment
//!
//! - **Per-node properties create no track** (curves come in step 4). They are
//!   counted and named in [`Read::notes`], never silently dropped.
//! - **Only one line of `NodeMd`** is read — a frame with several patches, or
//!   several `IntervalStratigraphicUnits` disagreeing across more than one
//!   rank, is handled ("never merged"), but nothing here reads a
//!   second frame to align against (`energyml_domain::strata_by_md`, for a
//!   well's other representations, is a further increment).

use std::collections::HashMap;

use energyml_core::identity;
use energyml_core::registry::DynObject;
use energyml_core::value::{FieldValue, Reflect};
use energyml_domain::color::GraphicalSet;
use energyml_domain::property::{self, IndexableElement};
use energyml_domain::property_kind::Resolver;
use energyml_domain::source::{ArraySource, ObjectSource};
use energyml_domain::values::{self, Values};
use energyml_domain::well_strata::{self, Known, Wanted};
use energyml_storage::ObjectSummary;

use crate::layout::{Board, Interval, IntervalTrack, MdTrack, Track};

/// The frame's own MD array — spelled the same in every release. Read here,
/// directly, with [`energyml_domain::values::read`] — the shared low-level
/// array reader every version-agnostic reader in this workspace goes
/// through — rather than through `well_strata`, so a board can still be built
/// for a frame with properties and no stratigraphy at all.
const NODE_MD: &str = "NodeMd";

/// The frame's own stratigraphic mappings — the two spellings
/// `energyml_domain::well_strata`'s own (private) `MAPPINGS` constant reads;
/// duplicated as literal strings rather than imported, because listing
/// *which ranks a frame names* is not reading what they say — the indices,
/// the units and the colours stay `well_strata`'s alone (see the module doc).
const MAPPINGS: &[&str] = &["IntervalStratigraphicUnits", "IntervalStratigraphiUnits"];
/// And what a mapping names as the rank its indices count in —
/// `well_strata`'s own (private) `ORGANISATION`.
const ORGANISATION: &[&str] = &["StratigraphicOrganizationInterpretation", "StratigraphicOrganization"];

// ---------------------------------------------------------------------------
// What is asked
// ---------------------------------------------------------------------------

/// How a property column's header is chosen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PropertyLabel {
    /// The title of the property's resolved `PropertyKind` — the sheet design's own
    /// proposal, and the default: a property's own title usually repeats the
    /// well and the context ("GTH-1 — pressure, yearly — …"), and a sheet
    /// that is already GTH-1's says less in more room.
    #[default]
    Kind,
    /// The property's own `Citation.Title`.
    Title,
}

/// Which of the frame's interval properties become tracks, and in which
/// order — "the checkboxes of the request become the list of
/// tracks, orderable".
#[derive(Clone, Debug, Default)]
pub enum TrackSelection {
    /// Every interval property the frame carries, in the package's own order.
    #[default]
    All,
    /// Only these property uuids, in this order — compared without case.
    Only(Vec<String>),
}

/// What [`board_of_frame`] needs beyond the frame and the store.
///
/// Unlike [`crate::layout::Params`], this **does** carry defaults for its
/// numeric fields, marked `// Provisional` below: a first sheet is wanted to look at before choosing a scale, a text size or
/// `max_chars`/`min_chars` for real (the scales and the rest may be revised
/// once a result has been seen). Every one
/// of them is a plain field a caller overrides directly, or through the
/// `with_*` builders below.
pub struct Options {
    /// What a person reads above the MD column — no default: the sheet design forbids a
    /// hardcoded label in either language, so a caller supplies one (its own
    /// translation, or an English default it owns — never invented here).
    pub md_header: String,
    /// How a property column's header is chosen.
    pub label: PropertyLabel,
    /// Which property tracks to draw, and in which order.
    pub tracks: TrackSelection,
    /// A text column's width, in characters.
    ///
    /// Provisional: [`IntervalTrack::DEFAULT_MAX_CHARS`] (32).
    pub max_chars: usize,
    /// The floor under `max_chars`.
    ///
    /// Provisional: [`IntervalTrack::DEFAULT_MIN_CHARS`] (8).
    pub min_chars: usize,
}

impl Options {
    /// `md_header` is mandatory (see the field's own doc); every other field
    /// takes this increment's provisional default.
    pub fn new(md_header: impl Into<String>) -> Self {
        Options {
            md_header: md_header.into(),
            label: PropertyLabel::default(),
            tracks: TrackSelection::default(),
            max_chars: IntervalTrack::DEFAULT_MAX_CHARS,
            min_chars: IntervalTrack::DEFAULT_MIN_CHARS,
        }
    }

    /// Override [`Options::label`].
    pub fn with_label(mut self, label: PropertyLabel) -> Self {
        self.label = label;
        self
    }

    /// Override [`Options::tracks`].
    pub fn with_tracks(mut self, tracks: TrackSelection) -> Self {
        self.tracks = tracks;
        self
    }

    /// Override [`Options::max_chars`].
    pub fn with_max_chars(mut self, max_chars: usize) -> Self {
        self.max_chars = max_chars;
        self
    }

    /// Override [`Options::min_chars`].
    pub fn with_min_chars(mut self, min_chars: usize) -> Self {
        self.min_chars = min_chars;
        self
    }
}

// ---------------------------------------------------------------------------
// What comes back
// ---------------------------------------------------------------------------

/// [`board_of_frame`]'s answer: the board [`crate::layout::compute`] takes,
/// plus what this reading had to say rather than silently do or silently
/// drop — a per-node property counted, a property whose count fits neither
/// the intervals nor the nodes, two ranks that disagreed and were drawn as
/// separate tracks rather than merged.
///
/// Not folded into [`Board`] itself: this crate's rule is that
/// `layout::Board` is plain data nobody but its own module writes, and
/// [`crate::layout::compute`] will add its own notes to what it lays out —
/// [`Read::notes`] are the ones that exist **before** a layout is even asked
/// for.
pub struct Read {
    /// The board, ready for [`crate::layout::compute`].
    pub board: Board,
    /// What could not be drawn, or had to be said rather than silently done.
    pub notes: Vec<String>,
}

/// Why a frame could not become a [`Board`] at all — named, never a guess.
#[derive(Debug, thiserror::Error)]
pub enum Refusal {
    /// The object is not a wellbore frame, or states no measured depths.
    #[error("{title}: {why}")]
    NoNodeMd {
        /// The object's title, or its type when it has none.
        title: String,
        /// What is missing.
        why: String,
    },
    /// Reading an array failed.
    #[error(transparent)]
    Read(#[from] energyml_domain::Error),
}

// ---------------------------------------------------------------------------
// `&dyn ArraySource` as an `ObjectSource`
// ---------------------------------------------------------------------------

/// The same workaround `energyml_domain::well_strata`'s own (private)
/// `Objects` type exists for: trait upcasting needs Rust 1.86, and this
/// workspace's `rust-version` is 1.85.
struct AsObjects<'a>(&'a dyn ArraySource);

impl ObjectSource for AsObjects<'_> {
    fn object(&self, uuid: &str) -> Option<Box<dyn DynObject>> {
        self.0.object(uuid)
    }

    fn read_object(&self, uuid: &str) -> Result<Box<dyn DynObject>, String> {
        self.0.read_object(uuid)
    }

    // The uri doors are forwarded too: left to the defaults they would ask the
    // wrapped source by uuid, and a reference followed through this wrapper
    // would lose the version it names.
    fn object_at(&self, uri: &str) -> Option<Box<dyn DynObject>> {
        self.0.object_at(uri)
    }

    fn read_object_at(&self, uri: &str) -> Result<Box<dyn DynObject>, String> {
        self.0.read_object_at(uri)
    }
}

// ---------------------------------------------------------------------------
// Reading the frame
// ---------------------------------------------------------------------------

/// Read `frame` (a `WellboreFrameRepresentation`, 2.0.1 or 2.2) into a
/// [`Board`]: the MD column, then one track per stratigraphic rank the frame
/// carries, then one track per interval property attached to it.
///
/// `resources` is the package's own listing — nothing in a representation
/// lists the properties or the colour sets that decorate it (the same reason
/// `energyml_geometry::scene::of_package` takes one), so the caller supplies
/// it, the way it already supplies one to build a scene.
pub fn board_of_frame(
    frame: &dyn Reflect,
    resources: &[ObjectSummary],
    source: &dyn ArraySource,
    options: &Options,
) -> Result<Read, Refusal> {
    let raw_title = identity::title_of_reflect(frame).filter(|title| !title.is_empty());
    let display_title = raw_title.unwrap_or_else(|| frame.type_info().type_key).to_string();

    let Some(FieldValue::Object(node_md)) = frame.field(NODE_MD) else {
        return Err(Refusal::NoNodeMd {
            title: display_title,
            why: "states no NodeMd, so it is not a wellbore frame and has no measured-depth interval to draw"
                .to_string(),
        });
    };
    let mds: Vec<f64> = match values::read(node_md, source, NODE_MD)? {
        Values::Floating(values) => values,
        Values::Integer(values) => values.into_iter().map(|value| value as f64).collect(),
        _ => {
            return Err(Refusal::NoNodeMd {
                title: display_title,
                why: "its NodeMd holds no numbers".to_string(),
            });
        }
    };
    if mds.len() < 2 {
        return Err(Refusal::NoNodeMd {
            title: display_title,
            why: format!("has {} node(s), hence no interval to draw", mds.len()),
        });
    }

    let frame_key = identity::key_of_reflect(frame);
    let mut notes: Vec<String> = Vec::new();
    let mut tracks: Vec<Track> = vec![Track::Md(MdTrack { header: options.md_header.clone() })];

    // Every `GraphicalInformationSet` and every `StratigraphicUnitInterpretation`
    // of the package, read once — `well_strata::Known`'s own contract: several
    // sets may exist (the GeolTs import writes the chart's colours in a set of
    // its own beside the package's), and a category row is matched to a unit by
    // title only among the units the package actually holds.
    let uri_of: HashMap<String, String> =
        resources.iter().map(|summary| (summary.uuid.to_ascii_lowercase(), summary.uri.clone())).collect();
    let graphics: Vec<(String, GraphicalSet)> = resources
        .iter()
        .filter(|summary| summary.qualified_type.ends_with("GraphicalInformationSet"))
        .filter_map(|summary| source.object_at(&summary.uri).map(|object| (summary.uuid.clone(), GraphicalSet::read(object.as_reflect()))))
        .collect();
    let unit_titles: Vec<(String, String)> = resources
        .iter()
        .filter(|summary| summary.qualified_type.ends_with("StratigraphicUnitInterpretation"))
        .filter_map(|summary| {
            source
                .object_at(&summary.uri)
                .map(|object| (summary.uuid.clone(), identity::title_of_reflect(object.as_reflect()).unwrap_or_default().to_string()))
        })
        .collect();
    let known = Known { graphics: &graphics, units: &unit_titles };

    // -- one track per stratigraphic rank the frame carries -----
    let ranks = stratigraphy_ranks(frame);
    if !ranks.is_empty() {
        match well_strata::of_frame(frame, &Wanted::IntervalUnits { rank: None }, source, &known) {
            Ok(strata) => {
                let header = if ranks.len() == 1 { rank_header(&ranks[0].1, source) } else { "Stratigraphy".to_string() };
                notes.extend(strata.notes.clone());
                tracks.push(Track::Intervals(track_from_strata(&strata, header, options, &uri_of)));
            }
            Err(well_strata::Refusal::Ambiguous { why, .. }) => {
                // "Two contradictory sources of strata... two
                // tracks, never a merge" — so every rank is drawn on its
                // own rather than the merged read that just refused.
                notes.push(format!(
                    "`{display_title}`'s stratigraphic ranks disagree on at least one interval ({why}); drawn \
                     as separate tracks, one per rank, never merged"
                ));
                for (rank_uuid, rank_uri) in &ranks {
                    match well_strata::of_frame(frame, &Wanted::IntervalUnits { rank: Some(rank_uuid) }, source, &known) {
                        Ok(strata) => {
                            notes.extend(strata.notes.clone());
                            tracks.push(Track::Intervals(track_from_strata(&strata, rank_header(rank_uri, source), options, &uri_of)));
                        }
                        Err(why) => notes.push(format!("stratigraphic rank {rank_uuid}: {why}")),
                    }
                }
            }
            // No mapping through any rank names an interval — an ordinary
            // absence (a frame may name a rank in its schema without a real
            // mapping), not a defect.
            Err(well_strata::Refusal::Absent { .. }) => {}
            Err(why) => notes.push(format!("`{display_title}`'s stratigraphy: {why}")),
        }
    }

    // -- one track per interval property, none for a per-node one -----
    let mut candidates: Vec<&ObjectSummary> = Vec::new();
    let mut node_property_count = 0usize;
    for summary in resources {
        if !summary.qualified_type.ends_with("Property") {
            continue;
        }
        let object = match source.read_object_at(&summary.uri) {
            Ok(object) => object,
            Err(why) => {
                notes.push(format!("property {} ({}): {why}", summary.uuid, summary.qualified_type));
                continue;
            }
        };
        let read = match property::read(object.as_reflect()) {
            Ok(read) => read,
            Err(why) => {
                notes.push(format!("property {}: {why}", summary.uuid));
                continue;
            }
        };
        // The frame this property hangs on is the one its **reference** names:
        // a package holding the frame in two versions has properties on each,
        // and the uuid alone would give this sheet both sets.
        let matches_frame = match (object.as_reflect().field("SupportingRepresentation"), &frame_key) {
            (Some(FieldValue::Object(supporting)), Some(frame_key)) => match identity::key_of_reflect(supporting) {
                Some(supporting) => supporting.same_object(frame_key),
                // A reference that names no type has no full identity: its uuid is all it says.
                None => identity::uuid_of_reflect(supporting).is_some_and(|uuid| uuid.eq_ignore_ascii_case(frame_key.uuid)),
            },
            _ => false,
        };
        if !matches_frame {
            continue;
        }
        match read.indexable {
            IndexableElement::Nodes => node_property_count += 1,
            IndexableElement::Intervals | IndexableElement::IntervalsFromDatum => candidates.push(summary),
            // Neither this frame's nodes nor its intervals — out of scope
            // for a per-frame sheet whatever it is (e.g. a property hanging
            // on `patches` or `representation`); not drawn, not counted as a
            // curve either, since it names no MD axis at all.
            _ => {}
        }
    }

    let ordered: Vec<&ObjectSummary> = match &options.tracks {
        TrackSelection::All => candidates,
        TrackSelection::Only(wanted) => wanted
            .iter()
            .filter_map(|uuid| candidates.iter().find(|summary| summary.uuid.eq_ignore_ascii_case(uuid)).copied())
            .collect(),
    };

    let mut objects: Vec<(&ObjectSummary, Box<dyn DynObject>)> = Vec::with_capacity(ordered.len());
    for summary in ordered {
        match source.read_object_at(&summary.uri) {
            Ok(object) => objects.push((summary, object)),
            Err(why) => notes.push(format!("property {}: {why}", summary.uuid)),
        }
    }
    let mut labelled: Vec<(property::Property<'_>, &dyn Reflect, String)> = Vec::with_capacity(objects.len());
    for (summary, object) in &objects {
        match property::read(object.as_reflect()) {
            Ok(read) => {
                let label = column_label(&read, source, options);
                labelled.push((read, object.as_reflect(), label));
            }
            Err(why) => notes.push(format!("property {}: {why}", summary.uuid)),
        }
    }
    disambiguate(&mut labelled, raw_title.unwrap_or_default(), &mut notes);

    for (read, property_object, label) in &labelled {
        let header = with_unit(label, read.uom.as_deref());
        if let Some(track) =
            build_property_track(read, *property_object, frame, &mds, source, &known, options, &header, &uri_of, &mut notes)
        {
            tracks.push(track);
        }
    }

    if node_property_count > 0 {
        notes.push(format!(
            "{node_property_count} propert{} of `{display_title}` hang on its nodes, not its intervals; a \
             per-node curve is not drawn this increment (step 4 of the sheet design) and is only \
             counted here",
            if node_property_count == 1 { "y" } else { "ies" }
        ));
    }

    Ok(Read { board: Board { title: None, tracks }, notes })
}

/// Distinct stratigraphic ranks the frame's mapping names — read on the
/// document alone, the same two field-name pairs `well_strata` reads under
/// its own private `MAPPINGS`/`ORGANISATION` (see the module doc): *which*
/// ranks are named, never their indices, their units or their colours.
///
/// Each rank as `(uuid, uri)`: the uuid is what `well_strata::Wanted` asks by
/// and what tells two ranks apart, the uri is what the header is read by.
fn stratigraphy_ranks(frame: &dyn Reflect) -> Vec<(String, String)> {
    let maps: Vec<&dyn Reflect> = match MAPPINGS.iter().find_map(|name| frame.field(name)) {
        Some(FieldValue::List(items)) => items.into_iter().filter_map(|item| item.as_object()).collect(),
        Some(FieldValue::Object(one)) => vec![one],
        _ => Vec::new(),
    };
    let mut ranks: Vec<(String, String)> = Vec::new();
    for map in maps {
        let Some(reference) = ORGANISATION.iter().find_map(|name| match map.field(name) {
            Some(FieldValue::Object(reference)) => Some(reference),
            _ => None,
        }) else {
            continue;
        };
        let Some(uuid) = identity::uuid_of_reflect(reference) else {
            continue;
        };
        // A reference that names no type has no uri: its uuid is then its address.
        let uri = identity::uri_of_reflect(reference).unwrap_or_else(|| uuid.to_string());
        let uuid = uuid.to_ascii_lowercase();
        if !uuid.is_empty() && !ranks.iter().any(|(known, _)| *known == uuid) {
            ranks.push((uuid, uri));
        }
    }
    ranks
}

/// A rank's own title, for a track header — "Stratigraphy" (the sheet design's own
/// fallback wording, not a French or English phrase invented here) when the
/// package does not hold the rank or it has no title.
fn rank_header(rank_uri: &str, source: &dyn ArraySource) -> String {
    source
        .object_at(rank_uri)
        .and_then(|object| identity::title_of_reflect(object.as_reflect()).map(str::to_string))
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| "Stratigraphy".to_string())
}

/// A [`well_strata::Strata`] turned into an [`IntervalTrack`] — the only new
/// code a stratigraphy or a categorical-property track needs, since every
/// section, unit, colour and title already came from `well_strata`.
fn track_from_strata(strata: &well_strata::Strata, header: String, options: &Options, uri_of: &HashMap<String, String>) -> IntervalTrack {
    let intervals: Vec<Interval> = strata
        .sections
        .iter()
        .map(|section| {
            let unit = section.unit.and_then(|at| strata.units.get(at));
            Interval {
                md_top: section.top,
                md_base: section.base,
                text: unit.map(|unit| unit.title.clone()),
                colour: strata.colour_of(section),
                uri: unit.and_then(|unit| unit.uuid.as_deref()).and_then(|uuid| uri_of.get(&uuid.to_ascii_lowercase())).cloned(),
                row: unit.map(|unit| format_number(unit.code)),
            }
        })
        .collect();
    IntervalTrack::new(header, intervals, vec!["fill".to_string(), "text".to_string()])
        .with_max_chars(options.max_chars)
        .with_min_chars(options.min_chars)
}

/// A property column's label: the chosen [`PropertyLabel`],
/// before its unit — [`with_unit`] appends that, once [`disambiguate`] has
/// had its say.
fn column_label(read: &property::Property<'_>, source: &dyn ArraySource, options: &Options) -> String {
    match options.label {
        PropertyLabel::Title => read.title.clone().unwrap_or_else(|| read.uuid.clone()),
        PropertyLabel::Kind => read
            .kind
            .as_ref()
            .and_then(|kind_ref| Resolver::new(&AsObjects(source)).resolve(kind_ref).ok())
            .map(|resolution| resolution.kind().title.clone())
            .filter(|title| !title.is_empty())
            .or_else(|| read.title.clone())
            .unwrap_or_else(|| read.uuid.clone()),
    }
}

/// The unit appended when the property has one — the sheet design's own example,
/// *"pressure (bar)"*.
fn with_unit(label: &str, uom: Option<&str>) -> String {
    match uom {
        Some(unit) if !unit.is_empty() => format!("{label} ({unit})"),
        _ => label.to_string(),
    }
}

/// **Two columns never share a header.** A kind names *what is measured*, and
/// a frame may carry several properties of one kind — Lutetia's
/// `BSS000NSYT` has two `source`, two `name`, two `identifier`: under
/// identical headers the columns cannot be told apart. Every column whose
/// label is shared takes its property's own title instead, shortened by
/// [`beyond_shared`] of what it repeats of the frame's title — the well's
/// name, which the sheet already is. Said in a
/// note, since the header is then not the one asked for.
fn disambiguate(labelled: &mut [(property::Property<'_>, &dyn Reflect, String)], frame_title: &str, notes: &mut Vec<String>) {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for (_, _, label) in labelled.iter() {
        *counts.entry(label.clone()).or_default() += 1;
    }
    let shared: Vec<String> = {
        let mut shared: Vec<String> = counts.into_iter().filter(|(_, count)| *count > 1).map(|(label, _)| label).collect();
        shared.sort();
        shared
    };
    if shared.is_empty() {
        return;
    }
    for (read, _, label) in labelled.iter_mut() {
        if shared.contains(label) {
            if let Some(title) = read.title.as_deref().filter(|title| !title.is_empty()) {
                *label = beyond_shared(title, frame_title).to_string();
            }
        }
    }
    notes.push(format!(
        "several columns are of one kind ({}); each is headed by its property's title instead",
        shared.join(", ")
    ));
}

/// `title` without the words it shares, at its start, with `frame_title` —
/// `"BSS000NSYT — source_etage"` beside `"BSS000NSYT — frame"` is
/// `"source_etage"`. The cut falls on a word boundary, never inside a word
/// (`"… — formation"` beside `"… — frame"` keeps `formation`), and the
/// punctuation left in front is dropped. A title that would be left empty, or
/// that shares nothing, is returned whole: shortening is a courtesy, never a
/// reason to lose the name.
pub fn beyond_shared<'a>(title: &'a str, frame_title: &str) -> &'a str {
    let common = title.char_indices().zip(frame_title.chars()).take_while(|((_, a), b)| a == b).last();
    let Some(((at, last), _)) = common else { return title };
    let end = at + last.len_utf8();
    // Back to the last boundary inside the shared part, unless the shared
    // part ends exactly on one in both titles.
    let whole_words = end == title.len()
        || title[end..].starts_with(char::is_whitespace)
        || title[..end].ends_with(char::is_whitespace);
    let cut = if whole_words { end } else { title[..end].rfind(char::is_whitespace).map_or(0, |at| at + 1) };
    let rest = title[cut..].trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '—' | '–' | '-' | ':' | ',' | '/' | '|'));
    if rest.is_empty() || cut == 0 { title } else { rest }
}

/// One interval property's track — categorical (through its `CategoryLookup`,
/// by `well_strata::of_frame`'s own `Wanted::Category`) or, lacking one,
/// discrete/continuous, read and aligned against `mds` directly.
///
/// `None` when the property could not be attached at all — always with a
/// note in `notes` naming why, never a silent drop (refuse
/// rather than draw).
#[allow(clippy::too_many_arguments)]
fn build_property_track(
    read: &property::Property<'_>,
    property_object: &dyn Reflect,
    frame: &dyn Reflect,
    mds: &[f64],
    source: &dyn ArraySource,
    known: &Known<'_>,
    options: &Options,
    header: &str,
    uri_of: &HashMap<String, String>,
    notes: &mut Vec<String>,
) -> Option<Track> {
    if read.indexable == IndexableElement::IntervalsFromDatum {
        // The design's own wording: reported like `attributes_for` reports it, even
        // though nothing is moved here — a sheet's cells are already
        // MD-indexed, so there is no node to move the value onto (module doc).
        notes.push(format!(
            "`{header}` hangs on intervals from datum; drawn as an ordinary interval track, since this \
             track's cells are already `[md_top, md_base]` and need no node to move a value onto"
        ));
    }

    if read.lookup.is_some() {
        return match well_strata::of_frame(frame, &Wanted::Category { property: property_object }, source, known) {
            Ok(strata) => {
                notes.extend(strata.notes.clone());
                Some(Track::Intervals(track_from_strata(&strata, header.to_string(), options, uri_of)))
            }
            Err(why) => {
                notes.push(format!("`{header}`: {why}"));
                None
            }
        };
    }

    let Some(patch) = read.patches.first() else {
        notes.push(format!("`{header}` states no values"));
        return None;
    };
    if read.patches.len() > 1 {
        notes.push(format!(
            "`{header}` has {} patches; a frame has one, and the first is read",
            read.patches.len()
        ));
    }
    let values = match patch.read(source) {
        Ok(values) => values,
        Err(why) => {
            notes.push(format!("`{header}`: {why}"));
            return None;
        }
    };
    let numbers: Vec<Option<f64>> = match values {
        Values::Floating(values) => values.into_iter().map(|value| (!value.is_nan()).then_some(value)).collect(),
        Values::Integer(values) => values.into_iter().map(|value| Some(value as f64)).collect(),
        _ => {
            notes.push(format!("`{header}` holds no numbers"));
            return None;
        }
    };

    let interval_count = mds.len() - 1;
    let node_count = mds.len();
    if numbers.len() != interval_count {
        if numbers.len() == node_count {
            notes.push(format!(
                "`{header}` holds {node_count} value(s), one per node of this frame — a per-node value is not \
                 drawn as an interval track (step 4 of the sheet design)"
            ));
        } else {
            notes.push(format!(
                "`{header}` holds {} value(s), and this frame has {interval_count} interval(s) and {node_count} \
                 node(s) — its count fits neither",
                numbers.len()
            ));
        }
        return None;
    }

    let uom = read.uom.as_deref();
    let intervals: Vec<Interval> = mds
        .windows(2)
        .zip(numbers.iter())
        .map(|(pair, value)| Interval {
            md_top: pair[0],
            md_base: pair[1],
            text: value.map(|value| format_value(value, uom)),
            colour: None,
            uri: None,
            row: None,
        })
        .collect();
    Some(Track::Intervals(
        IntervalTrack::new(header.to_string(), intervals, vec!["text".to_string()])
            .with_max_chars(options.max_chars)
            .with_min_chars(options.min_chars),
    ))
}

/// A number without useless zeros — `10`, not `10.0` — the same convention
/// [`crate::layout`]'s own (private) `format_md` uses, kept separate because
/// a property value is not a measured depth and does not need to share code
/// with it to agree on what "no useless zeros" means.
fn format_number(value: f64) -> String {
    if (value - value.round()).abs() < 1e-9 {
        format!("{}", value.round() as i64)
    } else {
        format!("{value}")
    }
}

/// A discrete or continuous value, in text, with its unit when it has one
/// (*"discreteProperties are to be interpreted by the CBT if they
/// have one linked"* — no lookup here means there is none).
fn format_value(value: f64, uom: Option<&str>) -> String {
    match uom {
        Some(unit) if !unit.is_empty() => format!("{} {unit}", format_number(value)),
        _ => format_number(value),
    }
}
