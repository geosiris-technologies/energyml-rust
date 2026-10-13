//! A wellbore's stratigraphy along its measured depth: which unit each interval
//! of a frame belongs to, and the colour the file gives that unit.
//!
//! The need: when the stratigraphy is on measured depths, it should be shown as a
//! well-section colour, as is done for properties. Perhaps another section in
//! the "apply" frame, when on a well or when applying to wells (RESQML or
//! WITSML).
//!
//! # Two places a frame says it
//!
//! | source | what one interval holds | what it designates |
//! |---|---|---|
//! | `IntervalStratigraphicUnits` (RESQML TRG 2.2 §5.34, p. 130) | an index | a unit of the rank the mapping names: its **position** in `StratigraphicUnits` in 2.2, its **declared `Index`** in 2.0.1 |
//! | a category property on the intervals | a code | the row of its `CategoryLookup` whose **key column** holds that code (common TRG 2.3 §9.1: *"Defines a key column which make the table a lookup table"*) |
//!
//! Which index is which unit is **not re-read here**:
//! [`crate::stratigraphy::columns`] is the reader the column view already uses,
//! and a second reading of a rank's order would be a second answer to one
//! question. A table is read by [`crate::table::Lookup`], and a code is matched
//! against the key the way the front end's `rowLabel` matches it - the value parsed
//! as a number, the first row that holds it - so that a legend and a section
//! cannot name two different rows for one code.
//!
//! RESQML 2.0.1 spells the frame's field `IntervalStratigraphiUnits` (the
//! missing `c` is in its XSD) and names the rank `StratigraphicOrganization`;
//! 2.2 writes `IntervalStratigraphicUnits` and
//! `StratigraphicOrganizationInterpretation`. Both spellings sit in the lists
//! below, the house rule for a respelling.
//!
//! # A unit's colour, and what is not one
//!
//! The same cascade the column view asks for (a front end's colour lookup on a unit):
//! the unit's own `DefaultGraphicalInformation`, then the feature it interprets -
//! so a unit is the same colour in both views. Only a **constant** colour is a
//! unit's colour: a colour map decorating a unit says how to paint a property of
//! it, and a band has no value to look up in it.
//!
//! A category property may carry a map of its own (a `ColorInformation` on the
//! property or its kind); that is the author's word about those codes and it is
//! sampled first. Otherwise a table row is linked to a unit of the package **by
//! its title**, when exactly one unit carries it - and `Unit::by_title` says
//! so, because a title is not an identity (the rule `property_kind` keeps).
//!
//! A unit the file does not colour has **no** colour here, never one made up:
//! the caller paints it with its colour for absent values, or with a palette
//! **it** hands in (`Strata::drawn`) - and either way the answer says the
//! colour is not the file's (`Drawn::Fallback`), so a legend can too.
//!
//! # Several `GraphicalInformationSet`s
//!
//! A package may hold several, and the GeolTs import writes the chart's colours
//! in **a set of its own** (`energyml-build`, `ops/stratigraphy.rs`), next to
//! whatever set the package's own colours live in. Reading only the first set
//! found would lose every unit colour whenever the other one lists first - a
//! well drawn transparent with nothing to say why. Nothing in the standard
//! ranks one set above another (ADR-0009), so the sets are **read as one** - `GraphicalSet::merged`,
//! then one cascade, the reading every other face uses - and the rule is the
//! one ADR-0009 proposes: the **most specific** level answers (the unit before its
//! feature), and at one level the later statement. A colour is decided by one
//! rule (ADR-0009). Previously each set was asked apart and two sets giving two colours left
//! the unit without one, which turned a recoloured unit black. Two sets that
//! disagree are **said** in a note and the later statement is taken; a unit
//! coloured in one set only is found, which is what that reading was for.
//!
//! # What is refused, and as what
//!
//! An absence and a fault are not the same news, so `Refusal` keeps them
//! apart: a frame that maps no interval is an ordinary frame
//! (`Refusal::Absent`); an index past its rank is a file that contradicts
//! itself (`Refusal::Defect`); two ranks naming two units for one interval is
//! a choice the file leaves open (`Refusal::Ambiguous`) - nothing in the TRG
//! ranks one rank above another, so the caller names the rank it wants.
//!
//! # WITSML
//!
//! A WITSML 2.1 `WellboreGeology` (its `InterpretedGeologyInterval`s) or a
//! `WellboreMarkerSet` states the same kind of fact, but this build draws no
//! WITSML wellbore: there is no tube for its intervals to colour. It is refused
//! **by name** rather than read into sections nobody can show.

use std::collections::{BTreeSet, HashMap};

use energyml_core::identity;
use energyml_core::registry::DynObject;
use energyml_core::value::{FieldValue, Reflect};

use crate::color::{Cascade, ColorMap, GraphicalSet, Level, Paint, Request, Rgba, Scaling, Scope};
use crate::property::{self, IndexableElement};
use crate::property_kind::Resolver;
use crate::source::{ArraySource, ObjectSource};
use crate::stratigraphy;
use crate::table::{Cell, Lookup};
use crate::values::{self, Values};

/// The spellings of the frame's mapping - RESQML 2.0.1 drops a `c`.
const MAPPINGS: &[&str] = &["IntervalStratigraphicUnits", "IntervalStratigraphiUnits"];
/// The spellings of what a mapping indexes into.
const ORGANISATION: &[&str] = &[
    "StratigraphicOrganizationInterpretation",
    "StratigraphicOrganization",
];
/// The indices: a jagged array in 2.2, one integer per interval in 2.0.1.
const UNIT_INDICES: &str = "UnitIndices";
/// The measured depth of each node - one spelling in every release.
const NODE_MD: &str = "NodeMd";
/// What joins the cells of a table row into a unit's name: the separator the
/// front end's `rowLabel` writes, so a row reads the same in a legend and a column.
const JOIN: &str = " - ";

// ---------------------------------------------------------------------------
// What comes back
// ---------------------------------------------------------------------------

/// Why a frame's stratigraphy cannot be read - and **which kind of news** it is.
///
/// Each variant is repaired somewhere else: an absence by choosing another
/// frame or another source, a defect in the file, an ambiguity by asking for
/// one rank, an unsupported shape by a newer build.
#[derive(Debug, thiserror::Error)]
pub enum Refusal {
    /// The object states no stratigraphy along its depth, or names an object
    /// the package does not hold.
    #[error("{what}: {why}")]
    Absent {
        /// What was asked about.
        what: String,
        /// What is not there.
        why: String,
    },
    /// The file says something that cannot be true.
    #[error("{what}: {why}")]
    Defect {
        /// Where the contradiction is.
        what: String,
        /// What contradicts what.
        why: String,
    },
    /// The file says two things and nothing in it chooses between them.
    #[error("{what}: {why}")]
    Ambiguous {
        /// Where the file says two things.
        what: String,
        /// The two things, and how to choose.
        why: String,
    },
    /// Two measured depths that are not counted the same way - another datum,
    /// another MD domain, a unit that does not convert - so looking one up
    /// against the other would be a guess (`crate::strata_by_md`).
    #[error("{what}: {why}")]
    Incomparable {
        /// What was asked about.
        what: String,
        /// Why its MDs and the frames' cannot be compared.
        why: String,
    },
    /// A shape this build does not read.
    #[error("{what}: {why}")]
    Unsupported {
        /// What was met.
        what: String,
        /// Why it is not read.
        why: String,
    },
    /// Reading a value array, a property or a table failed - in the reader's
    /// own words.
    #[error(transparent)]
    Read(#[from] crate::Error),
}

impl Refusal {
    fn absent(what: impl Into<String>, why: impl Into<String>) -> Self {
        Refusal::Absent {
            what: what.into(),
            why: why.into(),
        }
    }

    fn defect(what: impl Into<String>, why: impl Into<String>) -> Self {
        Refusal::Defect {
            what: what.into(),
            why: why.into(),
        }
    }

    fn ambiguous(what: impl Into<String>, why: impl Into<String>) -> Self {
        Refusal::Ambiguous {
            what: what.into(),
            why: why.into(),
        }
    }

    fn unsupported(what: impl Into<String>, why: impl Into<String>) -> Self {
        Refusal::Unsupported {
            what: what.into(),
            why: why.into(),
        }
    }

    /// Nothing to colour here, as opposed to something to repair.
    pub fn is_absence(&self) -> bool {
        matches!(self, Refusal::Absent { .. })
    }
}

/// Where a frame's stratigraphy was read from.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// `IntervalStratigraphicUnits`: the ranks its indices index into,
    /// lowercased, in the order the frame lists its mappings.
    IntervalUnits {
        /// The uuids of the ranks read.
        ranks: Vec<String>,
    },
    /// A category property on the intervals, and the table that names its
    /// codes.
    Category {
        /// The property's uuid.
        property: String,
        /// Its `CategoryLookup`'s uuid.
        table: String,
    },
}

/// A unit a section can belong to - one entry of a legend.
#[derive(Clone, Debug, PartialEq)]
pub struct Unit {
    /// The number a legend keys this unit on.
    ///
    /// For a category, the value the property stores for it - the key, or the
    /// row where the table names no key column - which is what the front end's
    /// category legend keys on too. For interval units, the unit's rank in
    /// [`Strata::units`]: two ranks both count from zero, so their own indices
    /// would collide in one legend.
    pub code: f64,
    /// The `StratigraphicUnitInterpretation`, lowercased - always for interval
    /// units, only through a title match for a category.
    pub uuid: Option<String>,
    /// **Its uri**, the identity: set for an interval unit (the uri the rank's
    /// reference names, [`stratigraphy::Named::uri`] - the version included),
    /// `None` for a category, which is only ever matched by title. What a
    /// legend keys a unit by when the package holds two versions of it.
    pub uri: Option<String>,
    /// What a person reads: the unit's title, or the row's non-key cells.
    pub title: String,
    /// The rank it was read from, for interval units.
    pub rank: Option<String>,
    /// The colour the file gives it. `None` is *nothing said*, never a
    /// default.
    pub colour: Option<Rgba>,
    /// The object that carried that colour: the unit, its feature, or the
    /// property whose map was sampled.
    pub colour_from: Option<String>,
    /// The unit object was found by its **title**. Said, because a title is
    /// not an identity.
    pub by_title: bool,
}

/// One interval of the frame, from a node to the next.
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    /// The MD of the node that opens it - the frame's order, which is top
    /// down in every file this was written against, but is not re-sorted.
    pub top: f64,
    /// The MD of the node that closes it.
    pub base: f64,
    /// Its unit, as a position in [`Strata::units`]. `None` when the file
    /// names none here (a null value, a negative index, a `NaN`) - or several
    /// at once, which [`Section::several`] lists.
    pub unit: Option<usize>,
    /// The units named **together** for this interval, which the jagged
    /// `UnitIndices` of 2.2 allows (*"a single interval may corresponds to
    /// several units"*). Not arbitrated: [`Section::unit`] stays `None`.
    pub several: Vec<usize>,
}

/// Where the colour a unit is drawn with comes from - [`Strata::drawn`].
///
/// Three answers kept apart because they are three different pieces of news: a
/// legend that showed a palette colour as the file's would be the front end
/// inventing what the package says.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Drawn {
    /// The file gives it: [`Unit::colour`].
    File(Rgba),
    /// The file gives none, and the caller's palette does.
    Fallback(Rgba),
    /// Nothing gives one: the caller's colour for absent values applies.
    Nothing,
}

impl Drawn {
    /// The colour drawn, file or fallback.
    pub fn colour(&self) -> Option<Rgba> {
        match self {
            Drawn::File(colour) | Drawn::Fallback(colour) => Some(*colour),
            Drawn::Nothing => None,
        }
    }

    /// `file`, `fallback` - or `None` - as a face spells it.
    pub fn from(&self) -> Option<&'static str> {
        match self {
            Drawn::File(_) => Some("file"),
            Drawn::Fallback(_) => Some("fallback"),
            Drawn::Nothing => None,
        }
    }
}

/// A wellbore frame's stratigraphy, section by section.
#[derive(Clone, Debug, PartialEq)]
pub struct Strata {
    /// The frame's uuid, lowercased.
    pub frame: String,
    /// Its title.
    pub title: String,
    /// Where the units were read from.
    pub source: Source,
    /// Every unit a section could name, in the source's order - the rank's
    /// order for interval units (every unit of the rank, so that one legend
    /// holds for every frame mapped through it), the table's for a category.
    pub units: Vec<Unit>,
    /// One per interval, in the frame's order.
    pub sections: Vec<Section>,
    /// What could not be read or had to be said.
    pub notes: Vec<String>,
}

impl Strata {
    /// The colour a section is drawn with, or `None` when the file gives none.
    pub fn colour_of(&self, section: &Section) -> Option<Rgba> {
        section
            .unit
            .and_then(|unit| self.units.get(unit))
            .and_then(|unit| unit.colour)
    }

    /// One colour per **node** of the frame, as RGBA bytes - what a tube or a
    /// line is painted with, in the order of the frame's `NodeMd`.
    ///
    /// # The rule, and it is ours
    ///
    /// A frame has one node more than it has sections. Each node takes the
    /// section it **opens**, and the deepest the section it **closes** - the
    /// correspondence `energyml_geometry::scene` states for a property on
    /// `intervals from datum`, kept identical so that a well coloured by its
    /// units and one coloured by a property meet a node the same way. Nothing
    /// is averaged: a unit between two others is a third rock, not a shade of
    /// them.
    ///
    /// # What it costs, stated
    ///
    /// A tube is coloured at its rings and blends between them, so a contact
    /// shows as a blend across the section above it: the colour is exact at
    /// the frame's nodes and nowhere else. Crisp contacts need a ring on each
    /// side of a contact - a different sweep, not a different colouring.
    ///
    /// `missing` is what a section with no unit takes, and a unit the file
    /// gives no colour: the caller's colour for absent values.
    pub fn node_colours(&self, missing: Rgba) -> Vec<u8> {
        self.node_colours_with(missing, &[])
    }

    /// [`Strata::node_colours`], a unit the file does not colour taking its
    /// colour from the caller's `fallback` palette ([`Strata::drawn`]).
    ///
    /// `missing` is then only what a section with **no unit** takes - a null
    /// index, or several units at once - and a unit when `fallback` is empty.
    pub fn node_colours_with(&self, missing: Rgba, fallback: &[Rgba]) -> Vec<u8> {
        let Some(last) = self.sections.len().checked_sub(1) else {
            return Vec::new();
        };
        let drawn = self.drawn(fallback);
        (0..=self.sections.len())
            .flat_map(|node| {
                let section = &self.sections[node.min(last)];
                section
                    .unit
                    .and_then(|unit| drawn.get(unit))
                    .and_then(Drawn::colour)
                    .unwrap_or(missing)
                    .to_bytes()
            })
            .collect()
    }

    /// How each unit of [`Strata::units`] is drawn, in their order: the file's
    /// colour, else an entry of the caller's `fallback` palette, else nothing.
    ///
    /// # The rule, and it is ours
    ///
    /// The *k*-th unit the file does not colour - counted in
    /// [`Strata::units`]' order, which is the rank's - takes
    /// `fallback[k % fallback.len()]`. Counted among the **uncoloured** units
    /// rather than all of them, so a palette of twelve covers twelve units the
    /// file left out however many it did colour; and in the rank's order rather
    /// than the sections', so every frame mapped through one rank gives a unit
    /// the same colour - one legend holds for all of them. More uncoloured
    /// units than colours cycles the palette: [`Strata::fallback_cycled`] says
    /// so, since two units then share a colour.
    ///
    /// Nothing here is a default: `fallback` is the caller's, and the answer
    /// keeps it apart from the file's ([`Drawn::Fallback`]).
    pub fn drawn(&self, fallback: &[Rgba]) -> Vec<Drawn> {
        let mut uncoloured = 0usize;
        self.units
            .iter()
            .map(|unit| match unit.colour {
                Some(colour) => Drawn::File(colour),
                None if fallback.is_empty() => Drawn::Nothing,
                None => {
                    let colour = fallback[uncoloured % fallback.len()];
                    uncoloured += 1;
                    Drawn::Fallback(colour)
                }
            })
            .collect()
    }

    /// Whether [`Strata::drawn`] had to reuse a colour of `fallback` - more
    /// units the file does not colour than the palette has colours.
    pub fn fallback_cycled(&self, fallback: &[Rgba]) -> bool {
        !fallback.is_empty()
            && self.units.iter().filter(|unit| unit.colour.is_none()).count() > fallback.len()
    }

    /// The units a section names and the file gives no colour - what a legend
    /// must say is not the file's.
    pub fn uncoloured(&self) -> Vec<&Unit> {
        let named: BTreeSet<usize> = self.sections.iter().filter_map(|section| section.unit).collect();
        named
            .into_iter()
            .filter_map(|at| self.units.get(at))
            .filter(|unit| unit.colour.is_none())
            .collect()
    }
}

// ---------------------------------------------------------------------------
// What is asked
// ---------------------------------------------------------------------------

/// Where to read a frame's stratigraphy from.
pub enum Wanted<'a> {
    /// The frame's own `IntervalStratigraphicUnits` - every mapping merged, or
    /// only the one through this rank (a uuid, any case).
    IntervalUnits {
        /// The rank to keep, when the caller chose one.
        rank: Option<&'a str>,
    },
    /// A category property of the frame, on its intervals.
    Category {
        /// The property object.
        property: &'a dyn Reflect,
    },
}

/// What the package says that is not in the frame: its colours, and the units
/// a table row may name.
///
/// Passed in rather than found here, because both are **about the package**:
/// finding the `GraphicalInformationSet` means scanning a listing, and a front end
/// colouring a thousand wells must be able to pay that once.
#[derive(Default)]
pub struct Known<'a> {
    /// Every `GraphicalInformationSet` of the package, read, with its uuid -
    /// all of them, in the package's order (see the module's *Several sets*).
    /// Empty: the package states no colour, which is an answer.
    pub graphics: &'a [(String, GraphicalSet)],
    /// The package's `StratigraphicUnitInterpretation`s as `(uuid, title)` -
    /// what a category row is matched against. Unused for interval units.
    pub units: &'a [(String, String)],
}

/// `&dyn ArraySource` seen as an [`ObjectSource`].
///
/// Spelled out rather than upcast: trait upcasting is stable from Rust 1.86,
/// and the workspace declares 1.85 - the reason `DynObject::into_reflect`
/// exists too.
struct Objects<'a>(&'a dyn ArraySource);

impl ObjectSource for Objects<'_> {
    fn object(&self, uuid: &str) -> Option<Box<dyn DynObject>> {
        self.0.object(uuid)
    }

    fn read_object(&self, uuid: &str) -> Result<Box<dyn DynObject>, String> {
        self.0.read_object(uuid)
    }

    // Forwarded: the defaults would turn a uri back into its uuid and lose the
    // version it names.
    fn object_at(&self, uri: &str) -> Option<Box<dyn DynObject>> {
        self.0.object_at(uri)
    }

    fn read_object_at(&self, uri: &str) -> Result<Box<dyn DynObject>, String> {
        self.0.read_object_at(uri)
    }
}

/// What one source gives: its units, and the ones each interval names.
struct Read {
    source: Source,
    units: Vec<Unit>,
    /// Per interval, the distinct units it names, as positions in `units`.
    named: Vec<Vec<usize>>,
}

// ---------------------------------------------------------------------------
// The reading
// ---------------------------------------------------------------------------

/// A wellbore frame's stratigraphy.
///
/// `source` answers for the frame's own arrays, the rank, the units and the
/// table; `known` for what is true of the whole package. Refuses by name
/// rather than returning an empty list: a frame with no section to show looks
/// exactly like a frame whose sections were lost.
pub fn of_frame(
    frame: &dyn Reflect,
    wanted: &Wanted<'_>,
    source: &dyn ArraySource,
    known: &Known<'_>,
) -> Result<Strata, Refusal> {
    let info = frame.type_info();
    let title = identity::title_of_reflect(frame)
        .filter(|title| !title.is_empty())
        .unwrap_or(info.type_key)
        .to_string();
    if info.package == "witsml" {
        return Err(Refusal::unsupported(
            format!("WITSML `{}` `{title}`", info.type_key),
            "this build draws no WITSML wellbore, so its geology intervals and markers would have \
             no tube to colour; a RESQML WellboreFrameRepresentation carries the same units through \
             IntervalStratigraphicUnits",
        ));
    }
    let Some(FieldValue::Object(node_md)) = frame.field(NODE_MD) else {
        return Err(Refusal::unsupported(
            format!("`{}` `{title}`", info.type_key),
            "states no NodeMd, so it is not a wellbore frame and has no measured-depth interval to colour",
        ));
    };
    let mds: Vec<f64> = match values::read(node_md, source, NODE_MD)? {
        Values::Floating(values) => values,
        Values::Integer(values) => values.into_iter().map(|value| value as f64).collect(),
        _ => return Err(Refusal::defect(format!("`{title}`.NodeMd"), "holds no numbers")),
    };
    if mds.len() < 2 {
        return Err(Refusal::absent(
            title,
            format!(
                "has {} node(s), hence no interval - a section runs from one node to the next",
                mds.len()
            ),
        ));
    }
    let uuid = identity::uuid_of_reflect(frame)
        .unwrap_or_default()
        .to_ascii_lowercase();

    let mut notes = Vec::new();
    let read = match wanted {
        Wanted::IntervalUnits { rank } => {
            interval_units(frame, &title, &mds, *rank, source, known, &mut notes)?
        }
        Wanted::Category { property } => category(*property, &uuid, &title, &mds, source, known, &mut notes)?,
    };

    let sections: Vec<Section> = read
        .named
        .into_iter()
        .enumerate()
        .map(|(at, named)| {
            let (unit, several) = match named.as_slice() {
                [] => (None, Vec::new()),
                [one] => (Some(*one), Vec::new()),
                many => (None, many.to_vec()),
            };
            Section {
                top: mds[at],
                base: mds[at + 1],
                unit,
                several,
            }
        })
        .collect();

    let several = sections
        .iter()
        .filter(|section| !section.several.is_empty())
        .count();
    if several > 0 {
        notes.push(format!(
            "{several} interval(s) name several units at once, which the jagged UnitIndices allows \
             (TRG 2.2 §5.34); they are drawn as having no single unit rather than one of them chosen"
        ));
    }
    let mut strata = Strata {
        frame: uuid,
        title,
        source: read.source,
        units: read.units,
        sections,
        notes,
    };
    let uncoloured: Vec<String> = strata
        .uncoloured()
        .iter()
        .map(|unit| unit.title.clone())
        .collect();
    if !uncoloured.is_empty() {
        strata.notes.push(format!(
            "{} unit(s) the file gives no colour - drawn with a colour the caller chooses (its palette, or \
             its colour for absent values), never one the file states: {}",
            uncoloured.len(),
            uncoloured.join(", ")
        ));
    }
    Ok(strata)
}

/// Whether a frame carries any `IntervalStratigraphicUnits` - read on the
/// document alone, no array opened: what tells a frame that states units from
/// one that does not before its `NodeMd` is read (`crate::strata_by_md`, which
/// must not be stopped by an unreadable array of a frame that says nothing).
pub fn states_units(frame: &dyn Reflect) -> bool {
    MAPPINGS
        .iter()
        .any(|name| frame.field(name).is_some_and(|value| !value.is_empty()))
}

/// Where an interval is, for a message: its rank and its two MDs.
fn interval(title: &str, mds: &[f64], at: usize) -> String {
    format!(
        "`{title}`, interval {at} (MD {} to {})",
        number(mds[at]),
        number(mds[at + 1])
    )
}

/// The frame's `IntervalStratigraphicUnits`, merged - or only the rank asked.
fn interval_units(
    frame: &dyn Reflect,
    title: &str,
    mds: &[f64],
    rank: Option<&str>,
    source: &dyn ArraySource,
    known: &Known<'_>,
    notes: &mut Vec<String>,
) -> Result<Read, Refusal> {
    let intervals = mds.len() - 1;
    let maps: Vec<&dyn Reflect> = match MAPPINGS.iter().find_map(|name| frame.field(name)) {
        Some(FieldValue::List(items)) => items.into_iter().filter_map(|item| item.as_object()).collect(),
        Some(FieldValue::Object(one)) => vec![one],
        _ => Vec::new(),
    };
    if maps.is_empty() {
        return Err(Refusal::absent(
            format!("`{title}`"),
            "states no IntervalStratigraphicUnits, so none of its intervals is mapped to a stratigraphic unit",
        ));
    }

    // The mappings, read and counted before any rank is opened: a count off by
    // one shifts every section, and that is the file's fault whatever the rank
    // says.
    let wanted = rank.map(str::to_ascii_lowercase);
    let mut named_ranks: Vec<String> = Vec::new();
    // The rank's uuid, the uri its mapping names it by (version included), the runs.
    let mut kept: Vec<(String, Option<String>, Vec<Vec<i64>>)> = Vec::new();
    for (at, map) in maps.iter().enumerate() {
        let what = format!("`{title}`, IntervalStratigraphicUnits {at}");
        let reference = ORGANISATION.iter().find_map(|name| match map.field(name) {
            Some(FieldValue::Object(reference)) => Some(reference),
            _ => None,
        });
        let Some(rank_uuid) = reference
            .and_then(identity::uuid_of_reflect)
            .filter(|uuid| !uuid.is_empty())
            .map(str::to_ascii_lowercase)
        else {
            return Err(Refusal::defect(
                what,
                "names no stratigraphic organisation, so its indices index nothing",
            ));
        };
        let rank_uri = reference.and_then(identity::uri_of_reflect);
        named_ranks.push(rank_uuid.clone());
        if wanted.as_deref().is_some_and(|wanted| wanted != rank_uuid) {
            continue;
        }
        let Some(FieldValue::Object(indices)) = map.field(UNIT_INDICES) else {
            return Err(Refusal::defect(what, "states no UnitIndices"));
        };
        let runs = unit_runs(indices, source, &what)?;
        if runs.len() != intervals {
            return Err(Refusal::defect(
                what,
                format!(
                    "gives {} run(s) of unit indices for the frame's {intervals} interval(s) - TRG 2.2 §5.34: \
                     \"BUSINESS RULE: Array length must equal the number of intervals\"",
                    runs.len()
                ),
            ));
        }
        kept.push((rank_uuid, rank_uri, runs));
    }
    if kept.is_empty() {
        return Err(Refusal::absent(
            format!("`{title}`"),
            format!(
                "maps no interval through rank {}; its IntervalStratigraphicUnits name {}",
                rank.unwrap_or_default(),
                named_ranks.join(", ")
            ),
        ));
    }

    let objects = Objects(source);
    let merged = merged_graphics(known);
    let cascade = Cascade::new(&merged);
    let mut units: Vec<Unit> = Vec::new();
    let mut position_of: HashMap<String, usize> = HashMap::new();
    // Per interval, each unit it is given and the mapping that gave it.
    let mut given: Vec<Vec<(usize, usize)>> = vec![Vec::new(); intervals];
    let mut ranks: Vec<String> = Vec::new();

    for (which, (rank_uuid, rank_uri, runs)) in kept.iter().enumerate() {
        let ranked = read_rank(rank_uuid, rank_uri.as_deref(), title, source)?;
        notes.extend(ranked.notes.iter().cloned());
        ranks.push(rank_uuid.clone());

        // The index an interval holds → the unit's place in `units`. First
        // wins on a repeated index: `read_rank` keeps the document order on a
        // tie and says so, and a second rule here would disagree with it.
        let mut by_index: HashMap<i64, usize> = HashMap::new();
        for member in &ranked.units {
            // Keyed by the uri: two versions of one unit are two units.
            let position = match position_of.get(&member.unit.uri) {
                Some(position) => *position,
                None => {
                    let (colour, colour_from) =
                        unit_colour(&member.unit.uri, &cascade, known, &objects, notes);
                    units.push(Unit {
                        code: 0.0,
                        uuid: Some(member.unit.uuid.clone()),
                        uri: Some(member.unit.uri.clone()),
                        title: member.unit.title.clone(),
                        rank: Some(rank_uuid.clone()),
                        colour,
                        colour_from,
                        by_title: false,
                    });
                    position_of.insert(member.unit.uri.clone(), units.len() - 1);
                    units.len() - 1
                }
            };
            by_index.entry(member.index).or_insert(position);
        }

        for (at, run) in runs.iter().enumerate() {
            for index in run {
                let Some(position) = by_index.get(index) else {
                    let mut held: Vec<i64> = ranked.units.iter().map(|member| member.index).collect();
                    held.dedup();
                    return Err(Refusal::defect(
                        interval(title, mds, at),
                        format!(
                            "indexes unit {index} of rank `{}`, which holds {} unit(s) indexed {}",
                            ranked.rank.title,
                            ranked.units.len(),
                            held.iter().map(i64::to_string).collect::<Vec<_>>().join(", ")
                        ),
                    ));
                };
                if !given[at].iter().any(|(already, _)| already == position) {
                    given[at].push((*position, which));
                }
            }
        }
    }

    // Two ranks naming two units for one interval: nothing in the TRG ranks
    // one rank above another, so which to draw is not the file's to say here.
    let mut named = Vec::with_capacity(intervals);
    for (at, found) in given.into_iter().enumerate() {
        let mappings: BTreeSet<usize> = found.iter().map(|(_, mapping)| *mapping).collect();
        if mappings.len() > 1 {
            let names: Vec<&str> = found
                .iter()
                .map(|(unit, _)| units[*unit].title.as_str())
                .collect();
            return Err(Refusal::ambiguous(
                interval(title, mds, at),
                format!(
                    "is given {} by {} different ranks, and nothing in the file says which to draw - \
                     ask for one rank",
                    names.join(" and "),
                    mappings.len()
                ),
            ));
        }
        named.push(found.into_iter().map(|(unit, _)| unit).collect());
    }

    for (code, unit) in units.iter_mut().enumerate() {
        unit.code = code as f64;
    }
    Ok(Read {
        source: Source::IntervalUnits { ranks },
        units,
        named,
    })
}

/// A rank, through the column view's own reader.
///
/// Read twice, and on purpose: once for the uuids of its units, then with those
/// units held, so that each title is the object's own and a unit reported
/// missing really is missing - `stratigraphy` prefers what the target says
/// about itself over what a reference cached, and says which units it could
/// not find.
fn read_rank(
    rank_uuid: &str,
    rank_uri: Option<&str>,
    title: &str,
    source: &dyn ArraySource,
) -> Result<stratigraphy::ColumnRank, Refusal> {
    // By the uri the mapping names: the version it names is the rank read.
    let found = match rank_uri {
        Some(uri) => source.object_at(uri),
        None => source.object(rank_uuid),
    };
    let Some(rank_object) = found else {
        return Err(Refusal::absent(
            format!("rank {rank_uuid}"),
            format!(
                "is what `{title}`'s IntervalStratigraphicUnits index into, and the package does not hold it - \
                 what each index means is unknown"
            ),
        ));
    };
    let type_key = rank_object.as_reflect().type_info().type_key;
    // The mapping may name any `AbstractGeologicUnitOrganizationInterpretation`
    // (TRG 2.2 §5.34). An occurrence (§6.31) lists units of its own **and**
    // names the rank it is an occurrence of, and nothing says which of the two
    // lists an index counts in: refused as the choice it is, not guessed.
    if type_key.contains("Occurrence") {
        return Err(Refusal::ambiguous(
            format!("`{type_key}` {rank_uuid}"),
            "is what the frame's IntervalStratigraphicUnits index into; an occurrence lists its own units \
             and names the rank it is an occurrence of (IsOccurrenceOf), and the TRG (2.2 §5.34, §6.31) does \
             not say which of the two lists the indices count in",
        ));
    }
    let unsupported = || {
        Refusal::unsupported(
            format!("`{type_key}` {rank_uuid}"),
            "is what the frame's IntervalStratigraphicUnits index into, and this reader knows the units \
             of a StratigraphicColumnRankInterpretation only (TRG 2.2 §6.45)",
        )
    };
    let first = stratigraphy::columns(std::iter::once(rank_object.as_reflect()))
        .loose_ranks
        .into_iter()
        .next()
        .ok_or_else(unsupported)?;
    let held: Vec<Box<dyn DynObject>> = first
        .units
        .iter()
        .filter_map(|member| source.object(&member.unit.uuid))
        .collect();
    stratigraphy::columns(
        std::iter::once(rank_object.as_reflect()).chain(held.iter().map(|object| object.as_reflect())),
    )
    .loose_ranks
    .into_iter()
    .next()
    .ok_or_else(unsupported)
}

/// The unit indices of each interval, nulls left out.
///
/// 2.2 writes them as a jagged array - *"Since a single interval may
/// corresponds to several units, the mapping is done using a jagged array"*
/// (TRG 2.2 §5.34) - and 2.0.1 as one integer per interval. Told apart by the
/// jagged array's own two fields, never by the version.
///
/// A negative index names nothing: the 2.0.1 annotation writes *"use null
/// (-1)"*, and no position is negative.
fn unit_runs(indices: &dyn Reflect, source: &dyn ArraySource, what: &str) -> Result<Vec<Vec<i64>>, Refusal> {
    if let (Some(FieldValue::Object(elements)), Some(FieldValue::Object(lengths))) =
        (indices.field("Elements"), indices.field("CumulativeLength"))
    {
        let elements = integers(elements, source, &format!("{what}.UnitIndices.Elements"))?;
        let ends = integers(lengths, source, &format!("{what}.UnitIndices.CumulativeLength"))?;
        let mut runs = Vec::with_capacity(ends.len());
        let mut start = 0usize;
        for (at, end) in ends.iter().enumerate() {
            let Some(end) = end.filter(|end| *end >= 0).map(|end| end as usize) else {
                return Err(Refusal::defect(
                    what,
                    format!(
                        "CumulativeLength {at} is null or negative, so interval {at} has no run of indices"
                    ),
                ));
            };
            if end < start || end > elements.len() {
                return Err(Refusal::defect(
                    what,
                    format!(
                        "CumulativeLength {at} is {end}, outside {start}..={} - a cumulative length never \
                         decreases and never passes the elements it counts",
                        elements.len()
                    ),
                ));
            }
            runs.push(
                elements[start..end]
                    .iter()
                    .filter_map(|index| index.filter(|value| *value >= 0))
                    .collect(),
            );
            start = end;
        }
        return Ok(runs);
    }
    Ok(integers(indices, source, &format!("{what}.UnitIndices"))?
        .into_iter()
        .map(|index| index.filter(|value| *value >= 0).into_iter().collect())
        .collect())
}

/// The declared `NullValue` of an integer array, when it has one.
fn null_value(array: &dyn Reflect) -> Option<i64> {
    match array.field("NullValue") {
        Some(FieldValue::Scalar(scalar)) => scalar.as_f64().map(|value| value as i64),
        _ => None,
    }
}

/// An integer array as the file states it: `None` where it holds its declared
/// `NullValue` - or a `NaN`, for a producer that stored indices as floats.
fn integers(array: &dyn Reflect, source: &dyn ArraySource, at: &str) -> Result<Vec<Option<i64>>, Refusal> {
    let null = null_value(array);
    match values::read(array, source, at)? {
        Values::Integer(values) => Ok(values
            .into_iter()
            .map(|value| (null != Some(value)).then_some(value))
            .collect()),
        Values::Floating(values) => values
            .into_iter()
            .map(|value| {
                if value.is_nan() {
                    Ok(None)
                } else if value.fract() == 0.0 && value.is_finite() {
                    Ok(Some(value as i64))
                } else {
                    Err(Refusal::defect(
                        at,
                        format!("holds {value}, which is not an index"),
                    ))
                }
            })
            .collect(),
        _ => Err(Refusal::defect(at, "is not an array of numbers")),
    }
}

/// A numeric array as the file states it: `None` for its declared
/// `NullValue` and for a `NaN` - how the corpus writes *no value here*.
fn numbers(array: &dyn Reflect, source: &dyn ArraySource, at: &str) -> Result<Vec<Option<f64>>, Refusal> {
    let null = null_value(array);
    match values::read(array, source, at)? {
        Values::Integer(values) => Ok(values
            .into_iter()
            .map(|value| (null != Some(value)).then_some(value as f64))
            .collect()),
        Values::Floating(values) => Ok(values
            .into_iter()
            .map(|value| (!value.is_nan()).then_some(value))
            .collect()),
        _ => Err(Refusal::defect(at, "is not an array of numbers")),
    }
}

/// A category property on the frame's intervals, and the rows its codes name.
fn category(
    property: &dyn Reflect,
    frame: &str,
    title: &str,
    mds: &[f64],
    source: &dyn ArraySource,
    known: &Known<'_>,
    notes: &mut Vec<String>,
) -> Result<Read, Refusal> {
    let intervals = mds.len() - 1;
    let read = property::read(property)?;
    let name = read.title.clone().unwrap_or_else(|| read.uuid.clone());
    let what = format!("property `{name}`");
    if !read
        .supporting_representation
        .as_deref()
        .is_some_and(|supporting| supporting.eq_ignore_ascii_case(frame))
    {
        return Err(Refusal::absent(
            what,
            format!(
                "hangs on {}, not on `{title}`, so it says nothing about this frame's intervals",
                read.supporting_representation
                    .as_deref()
                    .unwrap_or("no representation")
            ),
        ));
    }
    if !matches!(
        read.indexable,
        IndexableElement::Intervals | IndexableElement::IntervalsFromDatum
    ) {
        return Err(Refusal::unsupported(
            what,
            format!(
                "hangs on {}, and a section of a well is an interval",
                read.indexable.as_str()
            ),
        ));
    }
    let Some(table_uuid) = read.lookup.clone() else {
        return Err(Refusal::absent(
            what,
            "names no CategoryLookup, so its values are codes with no unit to name",
        ));
    };
    let found = match read.lookup_uri.as_deref() {
        Some(uri) => source.object_at(uri),
        None => source.object(&table_uuid),
    };
    let Some(table_object) = found else {
        return Err(Refusal::absent(
            format!("table {table_uuid}"),
            format!(
                "is the CategoryLookup of `{name}`, and the package does not hold it - what each code means is unknown"
            ),
        ));
    };
    let lookup = Lookup::read(table_object.as_reflect(), source)?;
    notes.extend(lookup.notes.iter().cloned());
    if lookup.key_columns.len() > 1 {
        return Err(Refusal::unsupported(
            format!("table {table_uuid}"),
            format!(
                "is keyed on {} columns, and `{name}` stores one value per interval - which key that value \
                 is cannot be read",
                lookup.key_columns.len()
            ),
        ));
    }

    // **The key, not the position** - common TRG 2.3 §9.1. A table keyed
    // `1..n`, which is what real files write, gives every row a key one above
    // its position; reading the position named every facies of `h+g_PaleoGTM`
    // with its neighbour's name. The key is parsed as a number, the way the
    // front end's `categories` parse it, so a text key that reads as a number
    // matches the same rows in both.
    let key_column = lookup.key_columns.first().copied();
    let key_of = |row: usize| -> Option<f64> {
        let column = key_column?;
        lookup
            .rows
            .cell(row, column)?
            .to_string()
            .trim()
            .parse::<f64>()
            .ok()
    };
    if !lookup.duplicate_keys().is_empty() {
        notes.push(format!(
            "table {table_uuid} repeats a key; the first row holding it is the one a code names"
        ));
    }

    let mut units = Vec::with_capacity(lookup.rows.rows);
    for row in 0..lookup.rows.rows {
        let cells: Vec<String> = (0..lookup.titles.len())
            .filter(|column| !lookup.key_columns.contains(column))
            .filter_map(|column| lookup.rows.cell(row, column))
            .map(|cell| cell_text(&cell))
            .filter(|text| !text.is_empty())
            .collect();
        // The row's number is what the front end's legend keys it on: the key,
        // the position where the key does not read as a number.
        let code = key_of(row).unwrap_or(row as f64);
        let label = if cells.is_empty() {
            number(code)
        } else {
            cells.join(JOIN)
        };
        let (uuid, by_title) = unit_titled(&cells, &label, known, notes);
        units.push(Unit {
            code,
            uuid,
            uri: None,
            title: label,
            rank: None,
            colour: None,
            colour_from: None,
            by_title,
        });
    }

    let Some(patch) = read.patches.first() else {
        return Err(Refusal::absent(what, "states no values"));
    };
    if read.patches.len() > 1 {
        notes.push(format!(
            "`{name}` has {} patches; a frame has one, and the first is read",
            read.patches.len()
        ));
    }
    let values = numbers(patch.array, source, &what)?;
    if values.len() != intervals {
        return Err(Refusal::defect(
            what,
            format!(
                "holds {} value(s) where `{title}` has {intervals} interval(s) - reading it here would shift \
                 every section",
                values.len()
            ),
        ));
    }

    let mut named = Vec::with_capacity(intervals);
    for (at, value) in values.iter().enumerate() {
        let Some(value) = value else {
            named.push(Vec::new());
            continue;
        };
        let row = match key_column {
            Some(_) => (0..lookup.rows.rows).find(|row| key_of(*row) == Some(*value)),
            // No key column: the position is all the document offers.
            None => (value.fract() == 0.0 && *value >= 0.0 && (*value as usize) < lookup.rows.rows)
                .then_some(*value as usize),
        };
        let Some(row) = row else {
            return Err(Refusal::defect(
                interval(title, mds, at),
                format!(
                    "`{name}` stores {}, which no row of table {table_uuid} holds{}",
                    number(*value),
                    if key_column.is_some() {
                        " in its key column"
                    } else {
                        ""
                    }
                ),
            ));
        };
        named.push(vec![row]);
    }

    // The colours: the property's own map first - the author's word about
    // these codes - then the unit a row was matched to.
    let objects = Objects(source);
    let merged = merged_graphics(known);
    let cascade = Cascade::new(&merged);
    let map = property_map(&read, frame, &objects, known, &cascade, notes);
    for unit in &mut units {
        if let Some((map, scaling, from)) = &map
            && let Some(rgba) = map.sample(Some(unit.code), scaling)
        {
            unit.colour = Some(rgba);
            unit.colour_from = Some(from.clone());
            continue;
        }
        if let Some(uuid) = unit.uuid.clone() {
            let (colour, colour_from) = unit_colour(&uuid, &cascade, known, &objects, notes);
            unit.colour = colour;
            unit.colour_from = colour_from;
        }
    }

    Ok(Read {
        source: Source::Category {
            property: read.uuid.clone(),
            table: table_uuid,
        },
        units,
        named,
    })
}

/// The unit of the package whose title a table row carries - when exactly one
/// does.
///
/// Matched on each non-key cell and on the joined label, trimmed, case kept: a
/// row `e6 - Bartonian` names the unit titled `Bartonian` as well as one
/// titled `e6 - Bartonian`. Two units with one title are **not** chosen
/// between: the row keeps its label and the ambiguity is noted.
fn unit_titled(
    cells: &[String],
    label: &str,
    known: &Known<'_>,
    notes: &mut Vec<String>,
) -> (Option<String>, bool) {
    let wanted: Vec<&str> = cells
        .iter()
        .map(|cell| cell.trim())
        .chain(std::iter::once(label.trim()))
        .collect();
    let found: BTreeSet<String> = known
        .units
        .iter()
        .filter(|(_, title)| !title.trim().is_empty() && wanted.contains(&title.trim()))
        .map(|(uuid, _)| uuid.to_ascii_lowercase())
        .collect();
    match found.len() {
        0 => (None, false),
        1 => (found.into_iter().next(), true),
        many => {
            notes.push(format!(
                "the row `{label}` matches {many} units of the package by title; none is chosen"
            ));
            (None, false)
        }
    }
}

/// The package's sets read as one - **the one reading of several sets**
/// ([`GraphicalSet::merged`]), made once per read and asked of one [`Cascade`].
fn merged_graphics(known: &Known<'_>) -> GraphicalSet {
    GraphicalSet::merged(known.graphics.iter().map(|(_, set)| set.clone()))
}

/// The colour a unit is drawn with: its own `DefaultGraphicalInformation`,
/// then its feature's - the column view's cascade, so a unit is the same colour
/// in both - asked of the package's sets **read as one**.
///
/// One cascade over [`GraphicalSet::merged`], the reading every other face uses
/// (the front end's colour column and path, the export, the command line): the most
/// specific level answers, and at one level the **last** entry written, a later
/// set being a later statement. The rule (ADR-0009) is
/// that a colour is decided by one rule; this function used
/// to ask each set apart and leave the unit **without** a colour when two sets
/// disagreed at one level. Recolouring one entry of Lutetia's two sets made them
/// disagree, and the unit went black in the legend and the painted wells while
/// the colour column showed the new blue (the legend showed black instead of
/// blue, and the colour in the section was not updated). A disagreement is now **information**, a note naming the set and
/// the colour taken, never a reason to drop the colour. A unit coloured in one
/// set only is found all the same, which is what asking every set was for.
fn unit_colour(
    address: &str,
    cascade: &Cascade<'_>,
    known: &Known<'_>,
    objects: &dyn ObjectSource,
    notes: &mut Vec<String>,
) -> (Option<Rgba>, Option<String>) {
    // The cascade is asked by the unit's uri; the messages say its uuid.
    let uuid = crate::inherit::uuid_of_identity(address);
    let request = Request {
        representation: address,
        property: None,
        kind_chain: &[],
        element: Scope::WholeObject,
    };
    let decision = match cascade.resolve(&request, objects) {
        Ok(Some(decision)) => decision,
        Ok(None) => return (None, None),
        Err(why) => {
            notes.push(format!("the colour of unit {uuid} could not be read: {why}"));
            return (None, None);
        }
    };
    let Paint::Constant(rgba) = decision.paint else {
        notes.push(format!(
            "unit {uuid} is decorated with a colour map, which paints a property of it and not the unit; \
             it has no colour"
        ));
        return (None, None);
    };
    // Said, not decided: the sets that answer at the winning level with another
    // colour. Asked set by set for the note only.
    if known.graphics.len() > 1 {
        let taken = rgba.to_hex();
        for (set_uuid, set) in known.graphics {
            if let Ok(Some(own)) = Cascade::new(set).resolve(&request, objects)
                && own.level == decision.level
                && let Paint::Constant(own_rgba) = own.paint
                && own_rgba.to_hex() != taken
            {
                notes.push(format!(
                    "unit {uuid} is given {} by set {set_uuid} and {taken} by a later statement, at the same level \
                     of the cascade - the later statement is taken: {taken}",
                    own_rgba.to_hex()
                ));
                break;
            }
        }
    }
    (Some(rgba), Some(decision.found_on))
}

/// The colour map a category property carries - on itself or on its kind -
/// with its scaling and the object that carried it.
///
/// Only those two levels: the cascade goes on to the frame, its interpretation
/// and its feature, and a well's own colour is not a colour for its units.
/// Asked of the sets read as one, like [`unit_colour`]: a disagreement between
/// two sets is a note, the later statement is the map.
fn property_map(
    read: &property::Property<'_>,
    frame: &str,
    objects: &Objects<'_>,
    known: &Known<'_>,
    cascade: &Cascade<'_>,
    notes: &mut Vec<String>,
) -> Option<(Box<ColorMap>, Scaling, String)> {
    if known.graphics.is_empty() {
        return None;
    }
    let chain: Vec<String> = read
        .kind
        .as_ref()
        .and_then(|kind| Resolver::new(objects).resolve(kind).ok())
        .map(|resolution| {
            resolution
                .chain
                .into_iter()
                .map(|kind| kind.uuid)
                .filter(|uuid| !uuid.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let request = Request {
        representation: frame,
        property: Some(read.uuid.as_str()),
        kind_chain: &chain,
        element: Scope::WholeObject,
    };
    let decision = match cascade.resolve(&request, objects) {
        Ok(Some(decision)) if matches!(decision.level, Level::Property | Level::PropertyKind) => decision,
        Ok(_) => return None,
        Err(why) => {
            notes.push(format!(
                "the colour map of property {} could not be read: {why}",
                read.uuid
            ));
            return None;
        }
    };
    match decision.paint {
        Paint::Mapped { map, scaling, .. } => {
            if known.graphics.len() > 1 {
                for (set_uuid, set) in known.graphics {
                    if let Ok(Some(own)) = Cascade::new(set).resolve(&request, objects)
                        && matches!(own.level, Level::Property | Level::PropertyKind)
                        && let Paint::Mapped { map: other, .. } = own.paint
                        && other.uuid != map.uuid
                    {
                        notes.push(format!(
                            "property {} is drawn through map {} by set {set_uuid} and map {} by a later statement - \
                             the later statement is taken: {}",
                            read.uuid, other.uuid, map.uuid, map.uuid
                        ));
                        break;
                    }
                }
            }
            Some((map, scaling, decision.found_on))
        }
        Paint::Constant(_) => {
            notes.push(format!(
                "property {} is given one constant colour, which names no unit; its units are coloured by what \
                 they are",
                read.uuid
            ));
            None
        }
    }
}

/// A table cell as a person reads it.
fn cell_text(cell: &Cell) -> String {
    match cell {
        Cell::Text(text) => text.trim().to_string(),
        Cell::Number(value) => number(*value),
        Cell::Boolean(flag) => flag.to_string(),
    }
}

/// A number without useless zeros - `10`, not `10.0`.
fn number(value: f64) -> String {
    if value == value.trunc() && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}
