//! `ijk-grid-from-horizons` - an `IjkGridRepresentation` between ordered
//! horizon grids: K layers per zone, vertical pillars on the horizons' lattice
//! (or every `coarsening`-th node of it), in the horizons' frame.
//!
//! This is the IJK-grid step of the reference model's build plan: the main
//! grid has zone 1, the Ypresian (layers 1-3, from the third to the fifth
//! horizon, the fourth inside); one K gap, the Meudon marls, an unmeshed cover
//! (from the fifth to the sixth horizon); and zone 2, the Chalk (layers 4-10,
//! from the sixth to the seventh horizon). With the five horizons in that order, the grid is
//! `layers = [2, 1, 0, 7]`: three layers of Ypresian split at the fourth horizon, the gap, seven
//! of Chalk.
//!
//! # The computation is `energyml_analysis::ijk`
//!
//! The house rule: a computation lives in `energyml-analysis`, never in an
//! operation (ADR-0017). That module reads the lattice, stacks the levels, decides which
//! cells have a shape and says what it met; this one reads the bag, checks the
//! frames, writes the object and says each count.
//!
//! # Roles by the slot's name, order by the slot's order
//!
//! `horizons` is **`ordered`**, top to bottom as declared: which surface is over
//! which is what the grid is built on, and sorting by height would decide it
//! exactly where two picks disagree (a role is read from the slot's name, and a
//! crossing is **said, never corrected** - `crossing` refuses by default, and
//! `pinch` is a decision the caller states, recorded in the activity and
//! counted).
//!
//! # One lattice, the horizons'
//!
//! Every horizon is a **2D grid on the same lattice** - the same counts, the
//! same nodes - refused by name otherwise: pillars standing on one horizon's
//! nodes and reading another's heights between its nodes would be a resampling
//! nobody stated. A grid of the package comes in the order its
//! `FastestAxisCount` says; the lattice must be the house one, along +x then
//! +y of its frame (a grid is never reprojected, and its order is the house order), or it is refused as
//! turned.
//!
//! # One frame, the horizons' - and a turned grid
//!
//! The pillars are the horizons' nodes, so the grid **is** in their frame:
//! `crs` is suggested from them, and a `crs` naming another system - or a frame
//! created here with `sourceEpsg` - is refused, as `horizon-from-law` refuses
//! it. Two horizons in two frames, or counting in two units, are refused the
//! way `thickness` refuses them.
//!
//! The reference model's main grid is in a local frame turned 30° (confirmed in
//! review). Nothing here turns a lattice: a grid in that frame is built from
//! horizons **gridded in that frame** - their lattice is straight in that frame's
//! numbers, and turned only once placed in Lambert-93. What this refuses is a
//! lattice turned **in its own frame's numbers**, which the pillars could only
//! follow by re-indexing.
//!
//! # What the file says, and where the TRG says it
//!
//! - **the points are explicit**: a `Point3dExternalArray` of
//!   `levels × (Nj + 1) × (Ni + 1) × 3`, levels first, `i` fastest - the layout
//!   `energyml_geometry::ijk` reads (*« Point Geometry is that of the column
//!   layer coordinate line nodes. Coordinate line nodes for all of the
//!   coordinate lines, with NKL nodes per line »*, TRG 2.2 §5.1 p. 94). The reference model
//!   asks for parametric vertical pillars (`ParametricLineArray` kind 0): the
//!   same nodes, stored another way, left for later;
//! - **`PillarShape` `vertical`** (TRG §5.39 p. 135): x and y are the same at
//!   every level of a pillar;
//! - **`KDirection` `down`** (TRG §5.35 p. 131): the horizons are given top to
//!   bottom, so K increases with depth - pinched cells included;
//! - **`KGaps`** for a zone of no layer (TRG §5.36 p. 132), `GapAfterLayer`
//!   inline - `Nk − 1` flags;
//! - **`GridIsRighthanded`**, from the triple product of the I, J, K tangents
//!   in the frame's numbers (TRG §5.29 `IjkGridGeometry` p. 125): false in elevation, true in
//!   depth. The reader does not trust it and measures each cell - which is
//!   what `crates/energyml-geometry/src/ijk.rs` says it should do;
//! - **a hole is a cell whose geometry is not defined**, not an inactive cell.
//!   `CellGeometryIsDefined` false where a corner has no height - *« If the
//!   indicator shows that the cell geometry is NOT defined, then this attribute
//!   overrides any other node geometry specification »* (TRG §5.1 p. 94) - and
//!   `PillarGeometryIsDefined` false for a pillar no defined cell touches
//!   (mandatory, same page); the missing nodes are written `NaN`. Both are a
//!   `BooleanConstantArray` when every one is true, an external array of 0/1
//!   otherwise (HDF5 has no boolean; `energyml_domain::values` reads a boolean
//!   array from a file as integers compared against zero).
//!
//!   **Activity is left out, and said.** The usage guide keeps the two apart:
//!   *« RESQML does not use "ACTNUM" as such. Instead, RESQML uses: an optional
//!   explicit Boolean array CellGeometryIsDefined, which indicates exactly
//!   that. An optional standard property kind called "active" »* (RESQML
//!   Technical Usage Guide 2.2, draft, §9.19.1 p. 172), and an active cell is one
//!   *« to include for processing »* (§9.17 p. 168). Which cells a simulator
//!   runs is a decision about the model, not about its shape: it is a property
//!   of the grid, for `grid-properties` (step 8) to write, not a guess made here.
//!
//! # What is left out, and said
//!
//! Split coordinate lines (the reference model's 21 split pillars along one fault, TRG §5.17) -
//! those are `faulted-ijk-grid`'s (`faulted_grid.rs`), which cuts the grid this
//! module stacks and writes it through `between`, the one writer of both -, the
//! `IntervalStratigraphicUnits` (`unit-lookup`'s),
//! `NodeIsColocatedInKDirection` for the pinched nodes (optional; its layout,
//! *« #CoordinateLines x (NKL-1) »*, has no reader here to confirm its order,
//! and a guessed layout is worse than none), and the `active` property.
//!
//! # Identity
//!
//! From what the grid **is**: its horizons (their uuid, or their name in the
//! bag), the layers, the coarsening, the crossing rule, the title. A new
//! layering is another grid - with another cell count, a property written on
//! the old one would no longer fit, and enriching does not overwrite
//! (identity comes from the content). The horizons' heights are not in the
//! seed: re-gridding a horizon rewrites the same grid, the way correcting a
//! thickness rewrites the same `horizon-from-law` horizon.

use serde_json::{Value, json};

use energyml_analysis::ijk::{self as analysis, IjkError, Request};
use energyml_analysis::ijk_faults::Faulted;
use energyml_analysis::surface::Up;
use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::{Geometry, Patch, Topology};

use crate::declare::{Choice, Inputs, Params, Shape, Suggestions, known_of};
use crate::operation::{
    About, ActivityDraft, Category, Context, Dataset, Need, NewObject, Note, Numbers, Operation, Plan,
    Refusal, Slot,
};
use crate::pieces::Pieces;

use super::support;
use super::thickness::Vertical;
use super::written::Written;

/// The operation.
pub struct IjkGridFromHorizons;

/// What to do where a horizon lies above the one declared over it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Choice)]
pub enum Crossing {
    /// Refuse the grid, naming the first node where it happens.
    Refuse,
    /// Bring the lower horizon up to the upper one there: a zero-thickness cell.
    Pinch,
}

/// What `ijk-grid-from-horizons` is told.
#[derive(Debug, Params)]
pub struct IjkGridParams {
    /// the layers of each zone, top to bottom - one count per pair of consecutive horizons; 0 leaves the zone unmeshed, a K gap
    ///
    /// **Ordered**: the first count is the zone between the first two
    /// horizons. No default - how finely a zone is layered is the model.
    #[param(least = 1, ordered)]
    pub layers: Vec<usize>,
    /// a pillar on every this-many nodes of the horizons' lattice, along both axes - 1 keeps every node
    #[param(default = 1_usize, min = 1)]
    pub coarsening: usize,
    /// what the third coordinate of the horizons measures - an elevation (larger is higher) or a depth (larger is lower)
    ///
    /// Suggested from the horizons' frame, never guessed: *above* is what a
    /// crossing is, and above is +z in elevation and −z in depth.
    pub axis: Vertical,
    /// where a horizon lies above the one declared over it - refuse the grid, or pinch: the lower brought up to the upper, a zero-thickness cell
    #[param(default = Crossing::Refuse)]
    pub crossing: Crossing,
    /// The frame, what is represented, the naming.
    #[param(flatten)]
    pub written: Written,
}

/// What `ijk-grid-from-horizons` takes.
#[derive(Debug, Inputs)]
pub struct IjkGridInputs {
    /// the horizons the grid lies between, **top to bottom** - 2D grids on one lattice, one per input
    #[input(topology = "grid", least = 2, ordered)]
    pub horizons: Vec<Shape>,
}

/// A count, as the activity records it.
fn double(title: &str, value: f64) -> Parameter {
    Parameter::Double {
        title: title.to_string(),
        value,
        uom: None,
    }
}

/// The title the object behind a shape carries, or the shape's name.
fn title_of(shape: &Shape) -> String {
    shape
        .known
        .as_ref()
        .and_then(|known| known.reference.as_ref())
        .map(|r| r.title.clone())
        .unwrap_or_else(|| shape.name.clone())
}

/// The key a horizon enters the seed by: its uuid, or its name in the bag.
fn key_of(shape: &Shape) -> String {
    shape
        .known
        .as_ref()
        .and_then(|known| known.reference.as_ref())
        .map(|r| r.uuid.clone())
        .unwrap_or_else(|| shape.name.clone())
}

/// The counts of a horizon - `[ni, nj]` - or why it is none a pillar stands on.
fn grid_counts(shape: &Shape) -> Result<[usize; 2], String> {
    let [patch] = shape.geometry.patches.as_slice() else {
        return Err(format!(
            "`{}` holds {} patch(es): a horizon of the grid is one 2D grid",
            title_of(shape),
            shape.geometry.patches.len()
        ));
    };
    let Topology::Grid { fastest, slowest } = &patch.topology else {
        return Err(format!(
            "`{}` is no 2D grid: the pillars stand on the horizons' lattice, so each horizon is a Grid2dRepresentation - make one with `interpolate` first",
            title_of(shape)
        ));
    };
    let counts = [*fastest, *slowest];
    if patch.points.len() != counts[0] * counts[1] {
        return Err(format!(
            "`{}` declares {} × {} nodes and holds {}",
            title_of(shape),
            counts[0],
            counts[1],
            patch.points.len()
        ));
    }
    Ok(counts)
}

/// The lattice of every horizon, the same - or the refusal that names the
/// first one that is not.
fn one_lattice(horizons: &[Shape]) -> Result<analysis::Lattice, Refusal> {
    let mut first: Option<(analysis::Lattice, String)> = None;
    for shape in horizons {
        let counts = grid_counts(shape).map_err(|why| Refusal::on("horizons", why))?;
        let lattice = analysis::lattice_of(&shape.geometry.patches[0].points, counts)
            .map_err(|why| Refusal::on("horizons", format!("`{}` {why}", title_of(shape))))?;
        let Some((reference, name)) = &first else {
            first = Some((lattice, title_of(shape)));
            continue;
        };
        if !reference.same_as(&lattice) {
            return Err(Refusal::on(
                "horizons",
                format!(
                    "`{}` is on a lattice of {} × {} nodes from ({}, {}) every ({}, {}), and `{name}` on one of {} × {} from ({}, {}) every ({}, {}): the pillars stand on one lattice - grid the horizons on the same one",
                    title_of(shape),
                    lattice.counts[0],
                    lattice.counts[1],
                    lattice.origin[0],
                    lattice.origin[1],
                    lattice.spacing[0],
                    lattice.spacing[1],
                    reference.counts[0],
                    reference.counts[1],
                    reference.origin[0],
                    reference.origin[1],
                    reference.spacing[0],
                    reference.spacing[1]
                ),
            ));
        }
    }
    first
        .map(|(lattice, _)| lattice)
        .ok_or_else(|| Refusal::on("horizons", "holds no horizon"))
}

/// A boolean array: a constant when every flag is true, an external array of
/// 0/1 otherwise - and the dataset it needs.
fn flags(flags: &[bool], file: &str, path: String, dimensions: Vec<i64>) -> (Value, Option<Dataset>) {
    if flags.iter().all(|flag| *flag) {
        return (
            json!({ "$type": "eml23.BooleanConstantArray", "Value": true, "Count": flags.len() }),
            None,
        );
    }
    let array = json!({
        "$type": "eml23.BooleanExternalArray",
        "CountPerValue": 1,
        "Values": support::external(file, &path, &dimensions),
    });
    let dataset = Dataset {
        file: file.to_string(),
        path,
        dimensions,
        values: Numbers::I64(flags.iter().map(|flag| i64::from(*flag)).collect()),
    };
    (array, Some(dataset))
}

impl Operation for IjkGridFromHorizons {
    type Params = IjkGridParams;
    type Inputs = IjkGridInputs;

    fn about(&self) -> About {
        About {
            name: "ijk-grid-from-horizons",
            describe: "an IJK grid between horizon grids given top to bottom - layers per zone, a zone of no layer a K gap, vertical pillars on the horizons' lattice, in their frame",
            category: Category::Geometry,
            makes: &["resqml22.IjkGridRepresentation"],
            recipe: "ijk-grid-from-horizons",
            represents: None,
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        suggest_grid(bag, slots);
    }

    fn check(&self, inputs: &IjkGridInputs) -> Result<(), Refusal> {
        one_lattice(&inputs.horizons).map(|_| ())
    }

    fn plan(&self, inputs: IjkGridInputs, params: IjkGridParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
        between(inputs.horizons, params, cx, None)
    }

    /// `layers`, `coarsening`, `axis` and `crossing` are inputs, recorded under
    /// the parameters the projection already declares - a title cannot be both.
    fn reports(&self) -> Vec<Slot> {
        support::frame_reports(grid_reports())
    }
}

/// What a grid between horizons suggests - the frame and the axis from the
/// horizons, and neither their title nor their interpretation.
pub(super) fn suggest_grid(bag: &Pieces, slots: &mut Suggestions<'_>) {
    super::written::suggest_written(bag, None, slots);
    // **Not a horizon's interpretation, nor its title**: the grid is none
    // of its horizons, and H3's name on it is the error a suggestion would
    // make look deliberate.
    for name in ["interpretation", "title"] {
        if let Some(slot) = slots.slot_mut(name) {
            slot.suggested = None;
            slot.from = None;
        }
    }
    let Some(known) = known_of(bag, "horizons") else {
        return;
    };
    if let Some(crs) = &known.crs {
        slots.inferred("crs", crs.dor(), crs.clone());
    }
    // From the frame the reader read, never from the heights.
    if let Some(up) = known.vertical_up {
        slots.set("axis", json!(if up { "elevation" } else { "depth" }));
    }
}

/// The lattice the horizons share, or the refusal naming the first that does
/// not - what `check` asks of any grid between horizons.
pub(super) fn check_horizons(horizons: &[Shape]) -> Result<(), Refusal> {
    one_lattice(horizons).map(|_| ())
}

/// The counts every grid between horizons reports.
pub(super) fn grid_reports() -> Vec<Slot> {
    let count = |name: &'static str, about: &'static str| {
        Slot::required(
            name,
            Need::Number {
                uom: None,
                least: None,
                most: None,
            },
            about,
        )
    };
    vec![
        count("ni", "cells along I, along x"),
        count("nj", "cells along J, along y"),
        count("nk", "layers"),
        count("gapCount", "K gaps - zones of no layer"),
        count(
            "holeCount",
            "horizon nodes on the pillars with no height, written as NaN",
        ),
        count(
            "undefinedCellCount",
            "cells with a corner of no height - CellGeometryIsDefined false",
        ),
        count(
            "crossedCount",
            "pillar nodes where a horizon lay above the one over it - pinched when asked, refused otherwise",
        ),
        count(
            "zeroThicknessCount",
            "pillar nodes where a zone has zero thickness",
        ),
    ]
}

/// What a grid cut by faults adds to it: what the cut made, and what it says.
pub(super) struct Cut {
    /// The lines, the splits, the heights and the flags.
    pub faulted: Faulted,
    /// What the plan says of the cut.
    pub says: Vec<Note>,
    /// What the activity records of it.
    pub parameters: Vec<Parameter>,
}

/// The cut, given the stacked grid, the pillars' `(x, y)` and the unit the
/// heights are counted in (`None` when nothing states it).
pub(super) type Cutter<'a> =
    Box<dyn FnOnce(&analysis::Stacked, &[[f64; 2]], Option<&str>) -> Result<Cut, Refusal> + 'a>;

/// What `faulted-ijk-grid` adds to a grid between horizons: the name its
/// activity goes by, what enters the seed beyond the grid's own, and the cut.
pub(super) struct Faulting<'a> {
    /// The operation's recipe name.
    pub recipe: &'static str,
    /// What distinguishes this cut in the seed: the faults and the law.
    pub seed: String,
    /// The cut.
    pub cut: Cutter<'a>,
}

/// The grid between `horizons`, cut by faults when `faulting` is given - the
/// one writer of an `IjkGridRepresentation` between horizons (an operation
/// that writes the same object as another delegates to it).
pub(super) fn between(
    horizons: Vec<Shape>,
    params: IjkGridParams,
    cx: &Context<'_>,
    faulting: Option<Faulting<'_>>,
) -> Result<Plan, Refusal> {
    let lattice = one_lattice(&horizons)?;

    // -- one frame, the horizons' ------------------------------------------------
    let frame_of = |shape: &Shape| shape.known.as_ref().and_then(|k| k.crs.clone());
    let unit_of = |shape: &Shape| shape.known.as_ref().and_then(|k| k.vertical_uom.clone());
    let (first_frame, first_unit) = (frame_of(&horizons[0]), unit_of(&horizons[0]));
    for shape in &horizons[1..] {
        if let (Some(a), Some(b)) = (&first_frame, frame_of(shape))
            && !a.uuid.eq_ignore_ascii_case(&b.uuid)
        {
            return Err(Refusal::new(
                "frames",
                format!(
                    "`{}` is in `{}` and `{}` in `{}`: a grid between heights read in two frames stacks two origins - bring them into one frame first",
                    title_of(&horizons[0]),
                    a.title,
                    title_of(shape),
                    b.title
                ),
            ));
        }
        if let (Some(a), Some(b)) = (&first_unit, unit_of(shape))
            && *a != b
        {
            return Err(Refusal::new(
                "units",
                format!(
                    "`{}` counts its heights in `{a}` and `{}` in `{b}` - one unit, or no grid",
                    title_of(&horizons[0]),
                    title_of(shape)
                ),
            ));
        }
    }
    if let Some(frame) = &first_frame {
        match (&params.written.frame.crs, params.written.frame.source_epsg) {
            (Some(named), _) => {
                let uuid = named
                    .0
                    .get("Uuid")
                    .and_then(|v| v.as_str())
                    .or_else(|| named.0.as_str())
                    .unwrap_or_default();
                if !uuid.eq_ignore_ascii_case(&frame.uuid) {
                    return Err(Refusal::on(
                        "crs",
                        format!(
                            "names another frame than the horizons' `{}`: the pillars are their nodes, so the grid is in their frame - name it, or leave `crs` out to take it",
                            frame.title
                        ),
                    ));
                }
            }
            (None, Some(_)) => {
                return Err(Refusal::on(
                    "sourceEpsg",
                    format!(
                        "would create a frame, and the horizons' heights are in `{}`: name that one as `crs`, or leave both out to take it",
                        frame.title
                    ),
                ));
            }
            (None, None) => {}
        }
    }

    // -- the grid, stacked -----------------------------------------------------------
    let up = match params.axis {
        Vertical::Elevation => Up::PlusZ,
        Vertical::Depth => Up::MinusZ,
    };
    let rule = match params.crossing {
        Crossing::Refuse => analysis::Crossing::Refuse,
        Crossing::Pinch => analysis::Crossing::Pinch,
    };
    let heights: Vec<Vec<f64>> = horizons
        .iter()
        .map(|shape| shape.geometry.patches[0].points.iter().map(|p| p[2]).collect())
        .collect();
    cx.stop_if_cancelled()?;
    let built = analysis::stack(&Request {
        heights: &heights,
        counts: lattice.counts,
        layers: &params.layers,
        step: params.coarsening,
        up,
        crossing: rule,
    })
    .map_err(|why| {
        let name = |at: usize| horizons.get(at).map(title_of).unwrap_or_else(|| format!("horizon {}", at + 1));
        match &why {
            IjkError::LayerCount { .. } | IjkError::GapAtEdge { .. } | IjkError::GapsTogether { .. } => Refusal::on("layers", &why),
            IjkError::Step { .. } | IjkError::TooMany { .. } => Refusal::on("coarsening", &why),
            IjkError::Crossed { count, first } => Refusal::on(
                "crossing",
                format!(
                    "{count} pillar node(s) have a horizon above the one declared over it - the first at ({}, {}): `{}` at {} over `{}` at {}. Give the horizons top to bottom, correct the picks, or ask for `pinch`",
                    lattice.origin[0] + first.node[0] as f64 * lattice.spacing[0],
                    lattice.origin[1] + first.node[1] as f64 * lattice.spacing[1],
                    name(first.lower),
                    first.lower_z,
                    name(first.upper),
                    first.upper_z
                ),
            ),
            IjkError::NothingDefined { .. } => Refusal::new("refused", why.to_string()),
            IjkError::TooFewHorizons(_) | IjkError::Heights { .. } => Refusal::on("horizons", &why),
        }
    })?;
    cx.stop_if_cancelled()?;

    // -- the frame -----------------------------------------------------------------------
    let written = &params.written;
    let title = written.naming.title();
    let keys: Vec<String> = horizons.iter().map(key_of).collect();
    let layers: Vec<String> = params.layers.iter().map(|n| n.to_string()).collect();
    let grid_seed = format!(
        "{}:{}:{}:{}",
        keys.join("|"),
        layers.join(","),
        params.coarsening,
        params.crossing.word()
    );
    // The seed of the grid between horizons is what it was before any fault:
    // its uuid does not move. A cut grid is another grid, named by its cut.
    let (recipe, seed, cutter) = match faulting {
        None => (
            "ijk-grid-from-horizons",
            format!("ijk-grid-from-horizons:{grid_seed}:{title}"),
            None,
        ),
        Some(Faulting { recipe, seed, cut }) => {
            (recipe, format!("{recipe}:{grid_seed}:{seed}:{title}"), Some(cut))
        }
    };
    let mut rank = 0usize;
    let frame = support::frame_in(&cx.known(), &written.frame, cx.identity, cx.when, &mut rank)?;
    if let Some((target, source)) = &frame.place
        && target.epsg() != *source
    {
        return Err(Refusal::on(
            "project",
            format!(
                "the pillars stand on the horizons' lattice, in the frame it was computed in: placing EPSG:{source} into EPSG:{} would bend its axes - name a `crs` of the package, or create the frame with `sourceEpsg` equal to `project`",
                target.epsg()
            ),
        ));
    }
    // The unit the heights count in: the horizons' own, or the frame created
    // here - `crs::build` writes its systems in metres.
    let unit = first_unit
        .clone()
        .or_else(|| (!frame.objects.is_empty()).then(|| "m".to_string()));

    // -- the cut, when there is one --------------------------------------------------------
    let [ni, nj, nk] = built.cells;
    let [pi, pj] = built.pillars;
    let plain = built.points(&horizons[0].geometry.patches[0].points);
    let cut = match cutter {
        Some(cutter) => {
            let pillars: Vec<[f64; 2]> = plain[..pi * pj].iter().map(|p| [p[0], p[1]]).collect();
            cx.stop_if_cancelled()?;
            Some((cutter(&built, &pillars, unit.as_deref())?, pillars))
        }
        None => None,
    };
    let uuid = support::next_uuid(cx.identity, &format!("ijk-grid:{seed}"), &mut rank)?;
    let file = written.naming.data_file.clone();

    // -- the object ------------------------------------------------------------------------
    let (points, point_dimensions, cell_defined, pillar_defined) = match &cut {
        // *« Otherwise the nodes will be 2D »* (TRG 2.2 §5.1 p. 94): levels,
        // then every coordinate line - the pillars', then the splits.
        Some((cut, pillars)) => (
            cut.faulted.points(pillars),
            vec![cut.faulted.levels as i64, cut.faulted.lines as i64, 3],
            cut.faulted.cell_defined.clone(),
            cut.faulted.pillar_defined.clone(),
        ),
        None => (
            plain,
            vec![built.levels as i64, pj as i64, pi as i64, 3],
            built.cell_defined.clone(),
            built.pillar_defined.clone(),
        ),
    };
    let points_path = format!("/resqml22/{uuid}/points");
    let mut datasets = vec![Dataset {
        file: file.clone(),
        path: points_path.clone(),
        dimensions: point_dimensions.clone(),
        values: Numbers::F64(points.iter().flatten().copied().collect()),
    }];
    let (cell_flags, cell_dataset) = flags(
        &cell_defined,
        &file,
        format!("/resqml22/{uuid}/cellGeometryIsDefined"),
        vec![nk as i64, nj as i64, ni as i64],
    );
    let (pillar_flags, pillar_dataset) = flags(
        &pillar_defined,
        &file,
        format!("/resqml22/{uuid}/pillarGeometryIsDefined"),
        vec![pj as i64, pi as i64],
    );
    datasets.extend(cell_dataset);
    datasets.extend(pillar_dataset);

    let mut object = json!({
        "$type": "resqml22.IjkGridRepresentation",
        "uuid": uuid,
        "schemaVersion": "2.2",
        "Citation": support::citation(cx.when, &title),
        "Ni": ni,
        "Nj": nj,
        "Nk": nk,
        "Geometry": {
            "$type": "resqml22.IjkGridGeometry",
            "LocalCrs": frame.crs,
            "Points": {
                "$type": "resqml22.Point3dExternalArray",
                "Coordinates": support::external(&file, &points_path, &point_dimensions),
            },
            "CellGeometryIsDefined": cell_flags,
            "KDirection": "down",
            "PillarGeometryIsDefined": pillar_flags,
            "PillarShape": "vertical",
            "GridIsRighthanded": built.right_handed(),
        },
    });
    if let Some((cut, _)) = &cut {
        // TRG 2.2 §5.17 `ColumnLayerSplitCoordinateLines` p. 111: the pillar
        // of each split line, and - a jagged array - the columns that use it.
        let faulted = &cut.faulted;
        let count = faulted.split_pillars.len();
        let columns: Vec<i64> = faulted
            .split_columns
            .iter()
            .flatten()
            .map(|c| *c as i64)
            .collect();
        let mut run = 0i64;
        let cumulative: Vec<i64> = faulted
            .split_columns
            .iter()
            .map(|one| {
                run += one.len() as i64;
                run
            })
            .collect();
        let pillar_path = format!("/resqml22/{uuid}/pillarIndices");
        let columns_path = format!("/resqml22/{uuid}/columnsPerSplitCoordinateLine/elements");
        let cumulative_path = format!("/resqml22/{uuid}/columnsPerSplitCoordinateLine/cumulativeLength");
        object["Geometry"]["ColumnLayerSplitCoordinateLines"] = json!({
            "$type": "resqml22.ColumnLayerSplitCoordinateLines",
            "Count": count,
            "PillarIndices": integers(&file, &pillar_path, &[count as i64]),
            "ColumnsPerSplitCoordinateLine": {
                "$type": "eml23.JaggedArray",
                "Elements": integers(&file, &columns_path, &[columns.len() as i64]),
                "CumulativeLength": integers(&file, &cumulative_path, &[count as i64]),
            },
        });
        datasets.push(Dataset {
            file: file.clone(),
            path: pillar_path,
            dimensions: vec![count as i64],
            values: Numbers::I64(faulted.split_pillars.iter().map(|p| *p as i64).collect()),
        });
        datasets.push(Dataset {
            file: file.clone(),
            path: columns_path,
            dimensions: vec![columns.len() as i64],
            values: Numbers::I64(columns),
        });
        datasets.push(Dataset {
            file: file.clone(),
            path: cumulative_path,
            dimensions: vec![count as i64],
            values: Numbers::I64(cumulative),
        });
    }
    let gaps = built.gaps();
    if gaps > 0 {
        object["KGaps"] = json!({
            "$type": "resqml22.KGaps",
            "Count": gaps,
            "GapAfterLayer": { "$type": "eml23.BooleanXmlArray", "CountPerValue": 1, "Values": built.gap_after_layer },
        });
    }
    let represented = support::represented_in(
        &written.represented,
        Some(&written.new_feature),
        None,
        cx.identity,
        cx.when,
        &seed,
        &title,
        &mut rank,
    )?;
    if let Some(reference) = represented.reference {
        object["RepresentedObject"] = reference;
    }

    // **The computed shape**: the nodes and the defined cells,
    // in the order the reader returns - which
    // `tests/ijk_grid.rs::the_geometry_reader_reads_the_nodes_and_the_cells_planned`
    // and `tests/faulted_grid.rs` check against the `energyml-geometry` reader,
    // which shares nothing with this computation.
    let (cells, index) = match &cut {
        Some((cut, _)) => cut.faulted.hexahedra(),
        None => built.hexahedra(),
    };
    let shape = support::made_root(
        &title,
        &uuid,
        Geometry {
            kind: "IjkGridRepresentation",
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points,
                topology: Topology::Hexahedra {
                    cells,
                    dimensions: [ni, nj, nk],
                    index,
                },
                closed: Vec::new(),
            }],
        },
    );

    // -- what is said ------------------------------------------------------------------------
    let total = ni * nj * nk;
    let undefined = built.undefined_cells();
    let mut says = Vec::new();
    let mut say = |key: &'static str, detail: String| says.push(Note { key, detail });
    let order: Vec<String> = horizons.iter().map(title_of).collect();
    say(
        "order",
        format!(
            "the horizons as declared, top to bottom: `{}` - never sorted by height",
            order.join("` over `")
        ),
    );
    if built.holes > 0 {
        say(
            "holes",
            format!(
                "{} horizon node(s) on the pillars have no height: written as NaN",
                built.holes
            ),
        );
    }
    if undefined > 0 {
        say(
            "undefined-cells",
            format!(
                "{undefined} of {total} cell(s) have a corner with no height: `CellGeometryIsDefined` false (RESQML TRG 2.2 §5.1 p. 94); the others, in the zones the holes do not bound, are kept"
            ),
        );
    }
    if cell_defined.iter().any(|defined| !defined) || pillar_defined.iter().any(|defined| !defined) {
        say(
            "no-activity",
            "no `active` property is written: a cell with no geometry is said by `CellGeometryIsDefined`, and which cells a simulator uses is a decision about the model (RESQML Technical Usage Guide 2.2, §9.17 p. 168), for `grid-properties` to write".to_string(),
        );
    }
    if built.crossed > 0 {
        say(
            "pinched-crossings",
            format!(
                "{} pillar node(s) had a horizon above the one declared over it: brought up to it, as asked (`crossing = pinch`) - the upper horizon wins, the older one stops at it",
                built.crossed
            ),
        );
    }
    if built.pinched > 0 {
        say(
            "zero-thickness",
            format!(
                "{} pillar node(s) are where a zone has zero thickness: its cells there are flat, a pinch-out",
                built.pinched
            ),
        );
    }
    if built.trimmed != [0, 0] {
        say(
            "trimmed",
            format!(
                "a pillar every {} node(s): the last {} column(s) of nodes along x and {} row(s) along y lie past the last pillar and are left out",
                built.step, built.trimmed[0], built.trimmed[1]
            ),
        );
    }
    if cut.is_some() {
        say(
            "left-out",
            "no parametric pillar, no `IntervalStratigraphicUnits`, no `SplitColumnEdges` (optional; the column edges a face index needs, RESQML Technical Usage Guide 2.2, §9.11.2 p. 151) and no `GridConnectionSetRepresentation` along the faults: explicit vertical pillars, split where a fault runs".to_string(),
        );
    } else {
        say(
            "left-out",
            "no split coordinate line (a fault through the grid), no parametric pillar, no `IntervalStratigraphicUnits`: explicit vertical pillars on the horizons' nodes".to_string(),
        );
    }
    says.extend(frame.says);

    let mut objects = frame.objects;
    objects.extend(represented.objects);
    objects.push(NewObject {
        uuid,
        qualified_type: "resqml22.IjkGridRepresentation".to_string(),
        role: "representation".to_string(),
        title: title.clone(),
        json: object,
    });
    let mut parameters = frame.parameters;
    parameters.push(double("ni", ni as f64));
    parameters.push(double("nj", nj as f64));
    parameters.push(double("nk", nk as f64));
    parameters.push(double("gapCount", gaps as f64));
    parameters.push(double("holeCount", built.holes as f64));
    parameters.push(double("undefinedCellCount", undefined as f64));
    parameters.push(double("crossedCount", built.crossed as f64));
    parameters.push(double("zeroThicknessCount", built.pinched as f64));
    if let Some((cut, _)) = cut {
        says.extend(cut.says);
        parameters.extend(cut.parameters);
    }
    Ok(Plan {
        media: Vec::new(),
        blocked: frame.blocked.clone(),
        objects,
        datasets,
        shapes: vec![shape],
        activity: ActivityDraft {
            recipe,
            title: format!("{recipe} → {title}"),
            seed,
            parameters,
        },
        says,
        dropped: Vec::new(),
    })
}

/// An integer array of the external file.
fn integers(file: &str, path: &str, dimensions: &[i64]) -> Value {
    json!({
        "$type": "eml23.IntegerExternalArray",
        "ArrayIntegerType": "arrayOfInt64LE",
        "NullValue": -1,
        "CountPerValue": 1,
        "Values": support::external(file, path, dimensions),
    })
}
