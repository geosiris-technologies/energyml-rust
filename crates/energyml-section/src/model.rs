//! The data of the section view - what goes in, what one package gives, what
//! the calculation answers, and how it is looked at.
//!
//! ADR-0015: **this file is the contract
//! between the modules** (`read`, `assemble`, `crosses`, `layout`, `svg`): they
//! are written against these types, never against each other's internals.
//!
//! Everything here is `serde`, in camelCase, because it travels: to the front end
//! (the catalogue and the view), into the SVG's metadata (the inputs and the
//! view), and back from it (`energyml profile --from-svg`).

use serde::{Deserialize, Serialize};

/// The version of the catalogue's format. A reader meeting another one refuses
/// by name rather than guessing what a field meant.
pub const FORMAT: u32 = 1;

// ---------------------------------------------------------------------------
// Identity

/// A horizontal coordinate system: an EPSG code and the unit its coordinates
/// are written in - `energyml_domain::reproject::System`, serialisable.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct System {
    /// The EPSG code.
    pub epsg: u64,
    /// The unit, as `energyml_domain::units` spells it (`"m"`, `"ft"`, …).
    pub unit: String,
}

/// An object of a package, as the section view names it everywhere.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectId {
    /// The package it is in - the front end's store id (`s1`, `s2`, …).
    pub store: String,
    /// Its uri in that package.
    pub uri: String,
    /// Its uuid, lower case.
    pub uuid: String,
    /// Its citation title, as the package writes it (possibly empty).
    pub title: String,
    /// Its qualified type (`resqml22.TriangulatedSetRepresentation`).
    pub qualified_type: String,
}

impl ObjectId {
    /// The key of an object across stores: store and uri joined by a NUL - one
    /// spelling, so a key read off the SVG (`data-key`) is a key any consumer
    /// of this model can build from the same two parts.
    pub fn key(&self) -> String {
        object_key(&self.store, &self.uri)
    }
}

/// The key of an object, from its store and uri: the two joined by a NUL.
pub fn object_key(store: &str, uri: &str) -> String {
    format!("{store}\u{0}{uri}")
}

// ---------------------------------------------------------------------------
// Inputs - what, if it changes, makes a new catalogue

/// What the calculation is made of. Two catalogues built from equal inputs are
/// the same catalogue: this is the cache key, and the SVG's metadata.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Inputs {
    /// The leading trace, `[x, y]` in the scene's system - where it was pointed.
    pub trace: Vec<[f64; 2]>,
    /// That system; `None` for a scene with none (every object then has to be
    /// in its own frame, and an object that names a system is refused).
    pub system: Option<System>,
    /// The long section's corridor, left and right of the trace as it was drawn.
    pub left: f64,
    /// See [`Inputs::left`].
    pub right: f64,
    /// The cross sections, if any.
    pub crosses: Crosses,
    /// The objects asked for, package by package.
    pub objects: Vec<Asked>,
    /// The scene's time scale - metres of the vertical axis per unit of time -
    /// when an object in time is asked for. `None`: an object in
    /// time is refused by name.
    pub time_factor: Option<f64>,
}

/// What one package is asked for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Asked {
    /// The package.
    pub store: String,
    /// Its revision when asked - part of the cache key: an edit makes a new
    /// contribution from this package, and only from it.
    pub revision: u64,
    /// Wellbore trajectories and frames, by uri.
    pub wells: Vec<String>,
    /// Anything with triangles - surfaces, and IJK grids (their envelope), by uri.
    pub surfaces: Vec<String>,
}

/// The cross sections of a long section.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Crosses {
    /// No cross section.
    None,
    /// One every `spacing` along the trace's arc length.
    #[serde(rename_all = "camelCase")]
    Every {
        /// The step, in the trace's unit.
        spacing: f64,
        /// What every cross section takes, unless it has its own.
        common: CrossWidths,
        /// The cross sections set apart, by index.
        own: Vec<CrossOwn>,
    },
    /// `count` of them, spread evenly along the trace.
    #[serde(rename_all = "camelCase")]
    Count {
        /// How many.
        count: u32,
        /// See [`Crosses::Every`].
        common: CrossWidths,
        /// See [`Crosses::Every`].
        own: Vec<CrossOwn>,
    },
}

/// A cross section's two pairs of widths: its **length** either side of
/// the long section, and its **corridor** for projecting wells.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrossWidths {
    /// Length on the left of the long section (seen along the trace).
    pub before: f64,
    /// Length on the right of the long section.
    pub after: f64,
    /// Its corridor, left of the cross section as it runs (left to right of the trace).
    pub left: f64,
    /// See [`CrossWidths::left`].
    pub right: f64,
}

/// One cross section set apart from the common widths.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrossOwn {
    /// Its index along the trace, from 0.
    pub index: u32,
    /// Its widths.
    pub widths: CrossWidths,
}

// ---------------------------------------------------------------------------
// Sections

/// Which section: the long one, or a cross section by index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SectionKey {
    /// The long section, along the leading trace.
    Long,
    /// A cross section.
    Cross {
        /// Its index along the trace.
        index: u32,
    },
}

/// A section's trace in plan, as `crosses::traces` computes it from the inputs -
/// what `read` cuts along and `assemble` projects on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionTrace {
    /// Which section.
    pub key: SectionKey,
    /// Its polyline in plan, `[x, y]` of the scene. A cross section's runs from
    /// its `before` end to its `after` end.
    pub trace: Vec<[f64; 2]>,
    /// Its corridor.
    pub left: f64,
    /// See [`SectionTrace::left`].
    pub right: f64,
    /// For a cross section, where it crosses the long section: the abscissa on
    /// the long section, and on itself. `None` for the long section.
    pub crossing: Option<[f64; 2]>,
}

// ---------------------------------------------------------------------------
// What one package gives - `read`'s answer, before any projection

/// What one package gives the calculation (`read::contribution`). Pure data in
/// the scene's system: `assemble` needs no source to project it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Contribution {
    /// The package.
    pub store: String,
    /// Its wells, read.
    pub wells: Vec<WellRead>,
    /// Its surfaces and grids, cut along every section trace.
    pub surfaces: Vec<SurfaceRead>,
    /// What was asked and could not be read, with the reason.
    pub refused: Vec<Refused>,
    /// What was read but not all of it kept - a well's nodes or intervals that
    /// are not numbers (`read::finite`). The object is drawn, so this is not a
    /// refusal; `assemble` passes these on to the catalogue's notes.
    #[serde(default)]
    pub notes: Vec<Note>,
}

/// An object refused, and why - in English, naming what was refused.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Refused {
    /// The object, as far as it could be named.
    pub object: ObjectId,
    /// Why.
    pub reason: String,
}

/// Whether a well object is a frame or a trajectory.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WellKind {
    /// A `WellboreFrameRepresentation` (carries strata).
    Frame,
    /// A `WellboreTrajectoryRepresentation`.
    Trajectory,
}

/// One well, read: its head and its path in the scene's system, its strata.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WellRead {
    /// The object.
    pub object: ObjectId,
    /// Frame or trajectory.
    pub kind: WellKind,
    /// The wellbore it describes - what makes a frame and its trajectory one
    /// line: the uuid of the wellbore feature, or of the trajectory a
    /// frame hangs on when no feature is reachable. `None`: it stands alone.
    pub wellbore: Option<String>,
    /// The head, `[x, y, z]` - the datum, never the first node.
    pub head: [f64; 3],
    /// The nodes drawn, `[x, y, z]`.
    pub path: Vec<[f64; 3]>,
    /// The MD of each node, when known (same length as `path`).
    pub md: Option<Vec<f64>>,
    /// The strata intervals along the MD.
    pub intervals: Vec<Interval>,
    /// Why there are no intervals, when there are none and some were wanted.
    pub strata: Option<String>,
    /// The intervals are **borrowed** from other frames of the wellbore, looked
    /// up at this one's MDs - not stated by this object. What makes the frame
    /// that states them keep the line (`assemble`).
    #[serde(default)]
    pub borrowed: bool,
    /// Told by its head alone - **not read**: no node, no interval. A well far
    /// from every section (`read::contribution_bounded`); `assemble` never folds
    /// it into a wellbore's line (nothing to rank it by) and refuses it on its
    /// own row, from its head.
    #[serde(default)]
    pub head_only: bool,
}

/// A stratigraphic interval along a well, in MD.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Interval {
    /// Where it starts, MD.
    pub from: f64,
    /// Where it ends, MD.
    pub to: f64,
    /// Its colour, `#rrggbb` or `#rrggbbaa`, resolved by the package's cascade -
    /// `None` when the package gives none (drawn in the well's colour, never
    /// left transparent).
    pub colour: Option<String>,
    /// The unit's title.
    pub unit: String,
    /// The unit's **full uri** (uuid and version, as the package holds it) -
    /// what lets a view say the colour of the unit after the catalogue was made
    /// ([`View::unit_colours`]). `None` when the unit cannot be named without a
    /// guess (two units of one title and colour in the frame, or one the package
    /// does not hold): its baked `colour` is then all there is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit_uri: Option<String>,
}

/// What a surface is, as its interpretation says - never its title.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TraceRole {
    /// A horizon.
    Horizon,
    /// A fault.
    Fault,
    /// Anything else, or nothing said.
    Other,
}

/// One surface or grid, read and cut along every section.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SurfaceRead {
    /// The object.
    pub object: ObjectId,
    /// What it is.
    pub role: TraceRole,
    /// Read in time and carried onto the depth axis by `time_factor`.
    pub time: bool,
    /// The colour the package gives it - its `GraphicalInformationSet`, by the
    /// library's cascade (representation, interpretation, feature) - or `None`
    /// when it gives none. A colour of the **file**, never of the scene.
    #[serde(default)]
    pub colour: Option<String>,
    /// Its age, `[old, young]` in millions of years before present, when its
    /// interpretation states one as numbers (`read::surfaces::age_of`); `None`
    /// when the file does not - an age is never inferred.
    #[serde(default)]
    pub age: Option<[f64; 2]>,
    /// The uuid of the feature its interpretation interprets (lowercase), when
    /// the package can be followed that far. Two surfaces of the same feature
    /// are two versions of one horizon, and a fill never goes from one to the
    /// other. `None` when unreadable: nothing is grouped by a title or by
    /// geometry.
    #[serde(default)]
    pub feature: Option<String>,
    /// Its cut along each section; a section it does not cross has no entry.
    pub cuts: Vec<SurfaceCut>,
}

/// A surface or grid cut along one section.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SurfaceCut {
    /// Which section.
    pub section: SectionKey,
    /// What is drawn.
    pub shape: CutShape,
    /// What the cut could not say - a folded span dropped, and why.
    pub notes: Vec<String>,
    /// Where each point of a `Trace` cut comes from on the surface, parallel
    /// to `spans` (one list per span, one source per point) - what lets a
    /// colour given **per node** of the surface (`View::paints`) be laid along
    /// the cut. Empty for an envelope, and when the reader cannot say: the cut
    /// is then drawn in the object's flat colour, as before.
    #[serde(default)]
    pub sources: Vec<Vec<CutSource>>,
}

/// A point of a cut, as a place on an edge of the surface: `node a + t·(node b
/// − node a)`, the nodes numbered **as the representation numbers them** - the
/// running count of its patches' nodes, holes included, which is what a
/// per-node colour buffer of the representation indexes (`colour_buffer`). The
/// reader drops non-finite nodes before it cuts; `read::surfaces` translates
/// its indices back (`Prepared::nodes`), cross-audited on a real model by a
/// downstream test. An end clipped to the trace's window
/// lies inside a triangle, not on an edge: it takes the nearer crossing's
/// edge, a fraction of a triangle at the two ends of a span.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CutSource {
    /// The first node of the edge.
    pub a: u32,
    /// The second node of the edge.
    pub b: u32,
    /// Where on the edge, from `a` (0) to `b` (1).
    pub t: f64,
}

/// The cut of a surface, or of a grid's envelope.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CutShape {
    /// A triangulated surface: continuous spans, `(s, z)`, s increasing
    /// (`fence::curve`).
    Trace {
        /// The spans.
        spans: Vec<Vec<[f64; 2]>>,
    },
    /// An IJK grid's envelope.
    Envelope {
        /// The top, in spans.
        top: Vec<Vec<[f64; 2]>>,
        /// The bottom, in spans.
        bottom: Vec<Vec<[f64; 2]>>,
        /// Closed polygons between them, broken where the grid has holes.
        fills: Vec<Vec<[f64; 2]>>,
        /// The faces of split coordinate lines, cut.
        faults: Vec<Vec<[f64; 2]>>,
    },
}

// ---------------------------------------------------------------------------
// The catalogue - `assemble`'s answer

/// What the calculation answers: everything that is drawn, in data.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalogue {
    /// [`FORMAT`].
    pub format: u32,
    /// What it was made from.
    pub inputs: Inputs,
    /// The long section.
    pub long: Section,
    /// The cross sections, by increasing abscissa on the trace.
    pub crosses: Vec<Section>,
    /// The view from above.
    pub plan: Plan,
    /// Every object asked for, and what became of it.
    pub objects: Vec<ObjectRow>,
    /// What the calculation noticed without it being a refusal.
    pub notes: Vec<Note>,
}

/// One section, drawn in `(s, z)`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Section {
    /// Which section.
    pub key: SectionKey,
    /// Its trace in plan, scene `[x, y]`.
    pub trace: Vec<[f64; 2]>,
    /// Its length.
    pub length: f64,
    /// Its marks along `s`: vertices, crossings.
    pub marks: Vec<Mark>,
    /// The band its corridor keeps, scene `[x, y]`, counter-clockwise, not closed.
    pub band: Vec<[f64; 2]>,
    /// What is drawn on it.
    pub items: Vec<Item>,
    /// The vertical range of what is drawn; `None` when nothing is.
    pub z_range: Option<[f64; 2]>,
}

/// A mark along a section's abscissa.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Mark {
    /// A vertex of the section's trace, by its index.
    Vertex {
        /// Where.
        s: f64,
        /// Which vertex.
        index: u32,
    },
    /// Where another section crosses this one.
    Crossing {
        /// Where.
        s: f64,
        /// Which section crosses here.
        other: SectionKey,
    },
}

/// What is drawn on a section.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Item {
    /// A well projected.
    Well(WellItem),
    /// A surface cut.
    Trace(TraceItem),
    /// A grid's envelope cut.
    Envelope(EnvelopeItem),
}

/// Which side of a section a well's head is on, read along the section.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Side {
    /// On the left.
    Left,
    /// On the right.
    Right,
}

/// A well, projected on a section.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WellItem {
    /// The object drawn (the frame when its wellbore has one).
    pub object: ObjectId,
    /// Abscissa of the head's projection.
    pub s: f64,
    /// Plan distance from the head to the section.
    pub d: f64,
    /// Which side.
    pub side: Side,
    /// The head, `(s, z)`.
    pub head: [f64; 2],
    /// Each node, `(s, z)`.
    pub path: Vec<[f64; 2]>,
    /// The MD of each node, when known.
    pub md: Option<Vec<f64>>,
    /// The strata intervals.
    pub intervals: Vec<Interval>,
    /// Why there are none, when there are none.
    pub strata: Option<String>,
    /// The other objects of the same wellbore, folded into this line.
    pub merged: Vec<ObjectId>,
}

/// A surface, cut along a section.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceItem {
    /// The object.
    pub object: ObjectId,
    /// What it is.
    pub role: TraceRole,
    /// The spans, `(s, z)`.
    pub spans: Vec<Vec<[f64; 2]>>,
    /// Read in time, carried by the time factor.
    pub time: bool,
    /// What the cut could not say.
    pub notes: Vec<String>,
    /// The colour the package gives it ([`SurfaceRead::colour`]).
    #[serde(default)]
    pub colour: Option<String>,
    /// Its age ([`SurfaceRead::age`]); absent when the file states none.
    #[serde(default)]
    pub age: Option<[f64; 2]>,
    /// The feature it is a version of ([`SurfaceRead::feature`]).
    #[serde(default)]
    pub feature: Option<String>,
    /// Where each point of each span comes from on the surface
    /// ([`SurfaceCut::sources`]), parallel to `spans` - what a paint of the
    /// front end (`View::paints`) is laid along. Empty when the reader cannot say.
    #[serde(default)]
    pub sources: Vec<Vec<CutSource>>,
}

/// A grid's envelope, cut along a section.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvelopeItem {
    /// The object.
    pub object: ObjectId,
    /// The top, in spans.
    pub top: Vec<Vec<[f64; 2]>>,
    /// The bottom, in spans.
    pub bottom: Vec<Vec<[f64; 2]>>,
    /// Filled polygons between them.
    pub fills: Vec<Vec<[f64; 2]>>,
    /// Fault faces cut.
    pub faults: Vec<Vec<[f64; 2]>>,
    /// Read in time.
    pub time: bool,
    /// The colour the package gives it ([`SurfaceRead::colour`]).
    #[serde(default)]
    pub colour: Option<String>,
}

/// The view from above, in local coordinates: the first point of the trace is
/// `(0, 0)` - no six-figure coordinates in the SVG.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    /// The first point of the trace, in the scene's system: what `(0, 0)` is.
    pub origin: [f64; 2],
    /// That system.
    pub system: Option<System>,
    /// Every section's trace, local.
    pub sections: Vec<(SectionKey, Vec<[f64; 2]>)>,
    /// Every section's band, local.
    pub bands: Vec<(SectionKey, Vec<[f64; 2]>)>,
    /// Every well asked for, with where it went.
    pub wells: Vec<PlanWell>,
}

/// A well in the plan.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanWell {
    /// The object.
    pub object: ObjectId,
    /// Its head, local `[x, y]`.
    pub head: [f64; 2],
    /// Where it was projected, on which section, with its foot (local) - empty
    /// for a well projected nowhere.
    pub feet: Vec<(SectionKey, [f64; 2])>,
    /// Why it was projected nowhere, when it was not (beyond an end, too far…).
    pub rejected: Option<String>,
}

/// An object asked for, and what became of it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectRow {
    /// The object.
    pub object: ObjectId,
    /// Well, surface or grid.
    pub kind: RowKind,
    /// What became of it.
    pub outcome: Outcome,
    /// Whether the front end may offer a fill up or down from this object: a
    /// surface that is a horizon or anything but a fault - triangulated or a 2D
    /// grid alike. Never a fault (a sub-vertical surface bounds no unit), an
    /// IJK grid (its envelope has its own fills) or a well. Decided here, once;
    /// the front end only reads it.
    #[serde(default)]
    pub fillable: bool,
}

/// What kind of object a row is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RowKind {
    /// A well.
    Well,
    /// A surface cut in a line: a triangulated surface or a 2D grid.
    Surface,
    /// An IJK grid.
    Grid,
}

/// What became of an object asked for.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Outcome {
    /// Drawn on these sections.
    Drawn {
        /// Where.
        sections: Vec<SectionKey>,
    },
    /// Folded into another line of the same wellbore.
    Merged {
        /// The line it went into.
        into: ObjectId,
    },
    /// Read, but it crosses no section - not a refusal.
    Missed,
    /// Refused, and why.
    Refused {
        /// Why, naming what.
        reason: String,
    },
}

/// Something the calculation noticed: cross sections running through each
/// other, a disagreement at a crossing…
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    /// A stable word for what it is (`crossesCross`, `crossingMismatch`, …),
    /// which the front end turns into a sentence.
    pub code: String,
    /// The sections concerned.
    pub sections: Vec<SectionKey>,
    /// The objects concerned.
    pub objects: Vec<ObjectId>,
    /// The sentence, in English, for the CLI and the console.
    pub said: String,
}

// ---------------------------------------------------------------------------
// The view - how a catalogue is looked at

/// What to draw and how - nothing here makes a new catalogue.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    /// A section, or the plan.
    pub target: Target,
    /// The window drawn; `None`: everything, with a margin.
    pub window: Option<Window>,
    /// A portion of the trace: `[s start, s end]`, a truncation - the
    /// abscissas keep their origin at the trace's first point.
    pub portion: Option<[f64; 2]>,
    /// The drawing's size in pixels.
    pub size_px: [f64; 2],
    /// The background colour.
    pub background: String,
    /// Objects hidden in this drawing, by key.
    pub hidden: Vec<String>,
    /// Colours the scene gives objects, by key - the front end's colour stack,
    /// resolved by the front end (never a second resolution here).
    pub colours: Vec<(String, String)>,
    /// The colour of each stratigraphic unit **now**, by the key of its well's
    /// package and the unit's full uri ([`object_key`] of `store` and
    /// [`Interval::unit_uri`]), resolved by the library's colour walks. Read
    /// before the colour baked into an interval, by the wells **and** by the
    /// strata legend, so that the two cannot disagree - and a colour edited
    /// after the catalogue was made shows without making a new one. Empty: the
    /// baked colours, as drawings always were (and then **not written** into
    /// the metadata).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unit_colours: Vec<(String, String)>,
    /// Line widths in pixels, by object key: each overrides, for that object
    /// only, the default width (`well_width_px` for a well). A width that is
    /// not a finite number in `(0, MAX_WIDTH_PX]` is refused by name in the
    /// drawing's warnings and the default is drawn.
    #[serde(default)]
    pub widths: Vec<(String, f64)>,
    /// How wells are coloured.
    pub well_colouring: WellColouring,
    /// How wide a well is drawn, in pixels.
    pub well_width_px: f64,
    /// For each surface, which way its colour fills.
    pub fills: Vec<FillChoice>,
    /// What is shown.
    pub show: Show,
    /// What the horizontal axis says.
    pub axis: AxisMode,
    /// Objects put forward on purpose - exported.
    pub highlight: Vec<String>,
    /// How objects are named on the drawing: their whole title, or a short code
    /// (`H0`, `W3`, `F1`…) and a legend beside it.
    #[serde(default)]
    pub labels: LabelMode,
    /// What the front end's scene paints along an object, by key: one colour per
    /// node, resolved by the front end from what its 3D view already holds (a
    /// property, the strata) - never a second resolution here (the resolver is supplied by the front end). An object without
    /// one is drawn as before: that is the default resolver.
    #[serde(default)]
    pub paints: Vec<(String, Paint)>,
    /// Marks along a painted well - at every node, or where its colour
    /// changes - the 3D view's joints.
    #[serde(default)]
    pub joints: Joint,
    /// Write, into the SVG, the script that lets it zoom itself when opened
    /// alone in a browser: lines keep their width in pixels, objects part
    /// (on by default).
    #[serde(default = "zoom_on")]
    pub zoom_script: bool,
    /// A legend of the stratigraphic units the drawn wells show, each with the
    /// colour its interval is drawn in, on the left of the drawing and in a
    /// group of its own (`<g id="strata-legend">`, an Inkscape layer) that is
    /// easy to move or edit. Off by default, and then **not
    /// written** into the metadata: a drawing made before the option existed
    /// is the same bytes.
    #[serde(default, skip_serializing_if = "is_off")]
    pub strata_legend: bool,
    /// A short black tick across the well at every boundary between two
    /// intervals of a strata-coloured well, in the well's own group.
    /// Off by default, and then **not written** into the metadata.
    #[serde(default, skip_serializing_if = "is_off")]
    pub strata_ticks: bool,
    /// Under strata colouring, draw the portions of a well that carry no strata
    /// information (below the last interval, between two) in the well's own
    /// colour - as drawings always did. Off: a well is drawn only where a unit
    /// is known (its head and label stay), and a note counts the wells that lost
    /// portions. On by default, and then **not written** into
    /// the metadata, so earlier drawings keep their bytes.
    ///
    /// **A legacy alias of [`UnstratifiedStyle::Hidden`]**, kept so that every
    /// document written before the styles still reads: `false` here is the same
    /// as `unstratified: hidden`, and wins over it ([`View::unstratified_style`]
    /// is the one place that decides). New code sets `unstratified`.
    #[serde(default = "shown", skip_serializing_if = "is_on")]
    pub draw_unstratified: bool,
    /// How the portions of a well with no strata information are drawn, under
    /// strata colouring (several styles are offered: fixed colour, texture, dashes…). The default is the solid neutral grey drawings always had,
    /// and then it is **not written** into the metadata, so earlier drawings
    /// keep their bytes. [`crate::style::declared`] lists the styles and their
    /// parameters; the front end builds its form from that list.
    #[serde(default, skip_serializing_if = "UnstratifiedStyle::is_default")]
    pub unstratified: UnstratifiedStyle,
    /// What each package is called, by the front end's store id (`s1` → the file's
    /// name): the strata legend writes it beside a unit shown more than once -
    /// the same unit, two colours, because two packages do not agree on it
    /// (the legend says which package the entries sharing a name come from). A store with no name
    /// here is named by its id. Empty, and then **not written** into the
    /// metadata, so earlier drawings keep their bytes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub store_names: Vec<(String, String)>,
    /// Named markers on a set of objects - a symbol or a contour at the head,
    /// the bottom or a depth of each - listed in a legend of their own
    /// (ADR-0015). Empty,
    /// and then **not written** into the metadata, so earlier drawings keep
    /// their bytes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub markers: Vec<Marker>,
}

/// A named marker put on a set of objects: what the legend of markers lists.
/// Which shapes and places exist is **declared** by
/// [`crate::markers::declared`]; the front end builds its form from that list.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Marker {
    /// The title in the legend. Never empty.
    pub name: String,
    /// The id of a declared shape ([`crate::markers::DeclaredShape::id`]).
    pub shape: String,
    /// `#rrggbb` or `#rrggbbaa`.
    pub colour: String,
    /// Where on each object.
    pub at: MarkerPlace,
    /// The objects, by the front end's key ([`object_key`]).
    pub keys: Vec<String>,
    /// The symbol's size, pixels (the width of a contour is the object's, plus a halo).
    pub size_px: f64,
    /// Kept in the view and not drawn: no symbol, no legend entry, no note
    /// (a marker can be hidden without being deleted). Off by
    /// default, and then **not written**.
    #[serde(default, skip_serializing_if = "is_off")]
    pub hidden: bool,
}

/// Where a marker sits on an object. Serialised as `{ "kind": "md", "value": 1500 }`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum MarkerPlace {
    /// The head of a well, the first point of a trace.
    Head,
    /// The last point of the drawn path.
    Bottom,
    /// The point of a well at this measured depth, in the unit of the well's MDs.
    Md {
        /// The measured depth.
        value: f64,
    },
}

/// The neutral grey a well's own line is drawn in when nothing gives it a colour.
pub const NEUTRAL_COLOUR: &str = "#555555";

/// How the portions of a well that no interval covers are drawn. Serialised as
/// `{ "kind": "dashed", "colour": "#999999", "dash": 6, "gap": 4 }`; the kinds
/// and their parameters are declared by [`crate::style::declared`], which a test
/// holds against this enum.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum UnstratifiedStyle {
    /// Not drawn at all.
    Hidden,
    /// A plain line in one colour.
    Solid {
        /// Any SVG colour.
        colour: String,
    },
    /// A dashed line.
    Dashed {
        /// Any SVG colour.
        colour: String,
        /// The length of a dash, pixels.
        dash: f64,
        /// The gap between two dashes, pixels.
        gap: f64,
    },
    /// A texture: parallel stripes, an SVG `<pattern>` the line is stroked with.
    Hatched {
        /// Any SVG colour.
        colour: String,
        /// The distance between two stripes, pixels.
        spacing: f64,
        /// The stripes' angle, degrees.
        angle: f64,
    },
}

impl Default for UnstratifiedStyle {
    /// Solid neutral grey: what a drawing always showed there.
    fn default() -> Self {
        UnstratifiedStyle::Solid {
            colour: NEUTRAL_COLOUR.to_string(),
        }
    }
}

impl UnstratifiedStyle {
    /// Whether this is the default - what is not written.
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// The reason a style cannot be drawn, if any: a colour that is empty, a
    /// length that is not a finite number in its range. Said by name in the
    /// drawing's notes, and the default is drawn.
    pub fn refusal(&self) -> Option<String> {
        let colour = |c: &str| (c.trim().is_empty()).then(|| "its colour is empty".to_string());
        let range = |name: &str, v: f64, lo: f64, hi: f64| {
            (!(v.is_finite() && v >= lo && v <= hi))
                .then(|| format!("its {name} ({v}) is not a number in [{lo}, {hi}]"))
        };
        match self {
            Self::Hidden => None,
            Self::Solid { colour: c } => colour(c),
            Self::Dashed { colour: c, dash, gap } => colour(c)
                .or_else(|| range("dash", *dash, style_limits::LENGTH.0, style_limits::LENGTH.1))
                .or_else(|| range("gap", *gap, style_limits::LENGTH.0, style_limits::LENGTH.1)),
            Self::Hatched {
                colour: c,
                spacing,
                angle,
            } => colour(c)
                .or_else(|| {
                    range(
                        "spacing",
                        *spacing,
                        style_limits::SPACING.0,
                        style_limits::SPACING.1,
                    )
                })
                .or_else(|| range("angle", *angle, style_limits::ANGLE.0, style_limits::ANGLE.1)),
        }
    }
}

/// The ranges the style parameters are held to - **the same numbers**
/// [`crate::style::declared`] gives the front end as `min` and `max`.
pub mod style_limits {
    /// A dash or a gap, pixels.
    pub const LENGTH: (f64, f64) = (1.0, 100.0);
    /// The distance between stripes, pixels.
    pub const SPACING: (f64, f64) = (2.0, 50.0);
    /// A stripe's angle, degrees.
    pub const ANGLE: (f64, f64) = (0.0, 180.0);
}

fn is_on(on: &bool) -> bool {
    *on
}

fn is_off(on: &bool) -> bool {
    !*on
}

fn zoom_on() -> bool {
    true
}

/// A colour per node of an object, as the scene shows it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Paint {
    /// `#rrggbbaa`, one per node of the object in the order the module reads
    /// its nodes (`colourBuffer`) - alpha 0: no value there, a gap, never a
    /// colour.
    pub nodes: Vec<String>,
    /// How the colour runs from one node to the next.
    #[serde(default)]
    pub blend: Blend,
}

/// How a paint is read between two nodes - a front end's `Blend`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Blend {
    /// Node `k`'s colour over `[k, k+1)`: a discrete property, the strata.
    #[default]
    Net,
    /// From node `k`'s colour to node `k+1`'s: a continuous property.
    Smooth,
}

/// Where a well is marked - a front end's `JointKind`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JointKind {
    /// Nowhere.
    #[default]
    None,
    /// At every node.
    Nodes,
    /// Where the colour changes.
    Changes,
}

/// A mark along a painted well: a stretch of `px` pixels centred on the node,
/// in `colour` at `alpha` - alpha 0 is a gap.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Joint {
    /// Where.
    pub kind: JointKind,
    /// Its length along the well, pixels of the drawing.
    pub px: f64,
    /// `#rrggbb`.
    pub colour: String,
    /// 0 (a gap) to 1.
    pub alpha: f64,
}

impl Default for Joint {
    fn default() -> Self {
        Self {
            kind: JointKind::None,
            px: 3.0,
            colour: "#000000".to_string(),
            alpha: 1.0,
        }
    }
}

/// The widest line a view may ask for, pixels.
pub const MAX_WIDTH_PX: f64 = 40.0;

/// How objects are named on a drawing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LabelMode {
    /// The title, in full (cut with "…" past a width).
    #[default]
    Full,
    /// A letter and a number - `W` wells, `H` horizons, `F` faults, `S` other
    /// surfaces, `G` grids - numbered in an order the whole catalogue shares, so
    /// an object has the same code on every section and on the plan; the titles
    /// are in a legend beside the drawing.
    Codes,
}

impl View {
    /// The style the unstratified portions are drawn in: `hidden` when the
    /// legacy `draw_unstratified` is off, whatever `unstratified` says - the one
    /// place the two are read together.
    pub fn unstratified_style(&self) -> UnstratifiedStyle {
        if self.draw_unstratified {
            self.unstratified.clone()
        } else {
            UnstratifiedStyle::Hidden
        }
    }

    /// A view of `target` at `size_px`, everything else at its default: the
    /// whole section, a white background, wells coloured by their strata,
    /// names, distances, vertices and crossings shown, the projected distance
    /// on the axis.
    pub fn new(target: Target, size_px: [f64; 2]) -> Self {
        Self {
            target,
            window: None,
            portion: None,
            size_px,
            background: "#ffffff".to_string(),
            hidden: Vec::new(),
            colours: Vec::new(),
            unit_colours: Vec::new(),
            widths: Vec::new(),
            well_colouring: WellColouring::Strata,
            well_width_px: 3.0,
            fills: Vec::new(),
            show: Show {
                crossings: true,
                vertices: true,
                distances: true,
                names: true,
                rejected: false,
                bands: true,
                surface_names: true,
                notes: true,
                origin: true,
                direction: true,
            },
            axis: AxisMode::Projected,
            highlight: Vec::new(),
            labels: LabelMode::Full,
            paints: Vec::new(),
            joints: Joint::default(),
            zoom_script: true,
            strata_legend: false,
            strata_ticks: false,
            draw_unstratified: true,
            unstratified: UnstratifiedStyle::default(),
            store_names: Vec::new(),
            markers: Vec::new(),
        }
    }
}

/// What a view draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Target {
    /// A section, in `(s, z)`.
    Section {
        /// Which.
        key: SectionKey,
    },
    /// The plan, local `(x, y)`.
    Plan,
}

/// A window: `[s or x min, s or x max] × [z or y min, z or y max]`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Window {
    /// Horizontal range.
    pub x: [f64; 2],
    /// Vertical range.
    pub y: [f64; 2],
}

/// How wells are coloured.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WellColouring {
    /// By side of the section.
    Side,
    /// The colour the scene gives them (`View::colours`).
    Scene,
    /// By strata intervals.
    Strata,
}

/// Which way a surface's colour fills.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FillChoice {
    /// The surface, by key.
    pub key: String,
    /// Up to the next line above.
    pub up: bool,
    /// Down to the next line below.
    pub down: bool,
}

/// What a drawing shows, beyond the items.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Show {
    /// Marks where other sections cross.
    pub crossings: bool,
    /// Marks the trace's vertices on the axis.
    pub vertices: bool,
    /// Each well's distance and side under its name.
    pub distances: bool,
    /// Well names.
    pub names: bool,
    /// In the plan: wells projected nowhere.
    pub rejected: bool,
    /// In the plan: the bands.
    pub bands: bool,
    /// Each surface's name, at the end of its trace.
    #[serde(default = "shown")]
    pub surface_names: bool,
    /// What the drawing says about itself under its plot - refused objects,
    /// wells set aside, labels folded. Off where another drawing beside it
    /// says the same (the front end's plan band, above its section).
    #[serde(default = "shown")]
    pub notes: bool,
    /// Where the trace's first point (`s = 0`) is, with its EPSG code, at the
    /// centre of the title band.
    #[serde(default = "shown")]
    pub origin: bool,
    /// In the plan: a chevron along each section's line, pointing the way the
    /// abscissa grows from the line's point 0.
    #[serde(default = "shown")]
    pub direction: bool,
}

/// A setting that is on unless a document written before it says otherwise.
fn shown() -> bool {
    true
}

/// What the horizontal axis of a section says.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AxisMode {
    /// Distance along the trace, in the projected system.
    Projected,
    /// Distance along the trace on the ellipsoid.
    Geodesic,
    /// The true coordinates of the points of the axis.
    Coordinates,
}
