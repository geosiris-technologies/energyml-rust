//! `grid-properties` — a pressure or a temperature by a sourced law of each
//! cell's own vertical depth below the ground, or a porosity, a net-to-gross
//! ratio or a permeability uniform within each K layer — laid on the cells of
//! an `IjkGridRepresentation`.
//!
//! This is the grid-properties step of the reference model's build plan,
//! next to `ijk-grid-from-horizons`, which writes the main grid and carries no
//! property of its own. Four properties are named for the grid:
//!
//! > a `ContinuousProperty` porosity (cells) — `porosity`
//! >
//! > a `ContinuousProperty` permeability — `absolute permeability`;
//! > `ValueCountPerIndexableElement` \[3], `LabelPerComponent` kx ky kz, `Facet`
//! > direction
//! >
//! > a `ContinuousProperty` NTG — `net gross thickness ratio`
//!
//! and the time-series step (`property-time-series`) names the fifth: a
//! pressure on the cells of the main grid, which `grid-properties` lays —
//! a pressure and a temperature by the same sourced
//! law of depth `well-logs` lays on wellbore frames, here laid on the grid's
//! cells instead of a frame's nodes.
//!
//! # The computation is `energyml_analysis::well_logs`
//!
//! The house rule: a computation lives in `energyml-analysis`, never in an
//! operation (ADR-0017). That module already holds the law a temperature or a
//! hydrostatic pressure follows with depth — the one `well-logs` uses — and
//! this operation reuses it rather than writing a second one: the physics of
//! a geothermal gradient or a column of water does not change because the
//! indexable element is a cell instead of a node. Only the reading of *the
//! depth of this element below the ground* is new, because a cell has a
//! centre and no node.
//!
//! # A cell's own depth, in the RESQML order
//!
//! The grid arrives as `energyml-geometry` reads it — a grid is never reprojected —
//! one patch, `Topology::Hexahedra {
//! cells, dimensions: [ni, nj, nk], index }`, `i` fastest then `j` then `k`
//! (the geometry crate's rules, and the reader's own comment:
//! *« Cell index in RESQML's own order — i fastest, then j, then k »*). For
//! each of the `ni × nj × nk` cells RESQML declares, in that order:
//!
//! - **its position in `cells`** is found by [`Topology::position_of`], the
//!   one translation from a RESQML cell number to where it sits in a
//!   (possibly compacted) list — never re-derived here, since a second
//!   reading of `index` could disagree with the reader's;
//! - **absent** (`position_of` returns `None`): the cell has no defined
//!   geometry — an undefined pillar, TRG 2.2 §5.1 p. 94 — so it has no centre
//!   and no depth to read. Written as `NaN` and counted (`undefined-cells`),
//!   never invented;
//! - **present**: its centre is the mean of its eight corners — a cheap and
//!   honest reading for a law that only wants *how deep*, not the cell's true
//!   shape.
//!
//! Pressure and temperature need a **ground** — `ground`, a surface read
//! exactly as `well-logs` reads it (`support::Faces`, `energyml_analysis::heights`)
//! under each cell centre's `(x, y)`, in the grid's own frame. A cell outside
//! it, or under a fold, is refused by its number rather than silently
//! skipped: unlike an undefined pillar, this is not a hole the file states,
//! it is a ground that does not cover the grid.
//!
//! # A K-layer value does not read the geometry at all
//!
//! Porosity, net-to-gross and permeability are **uniform within a K layer**:
//! the same number on every cell of it, top to bottom. A RESQML cell index is
//! `i + ni·(j + nj·k)` (the reader's own order, see above), so `k` is `cell /
//! (ni·nj)` — arithmetic on the index, nothing computed from a corner point.
//! **Every** cell gets a value this way, defined geometry or not: a porosity
//! is a fact about the layer, and a corner the reader could not place does
//! not change what rock a cell belongs to.
//!
//! # A tensor property this build does not write
//!
//! The permeability property above is quoted whole because it is not entirely followed: a single
//! `ContinuousProperty` with `ValueCountPerIndexableElement = [3]` and
//! `LabelPerComponent = [kx, ky, kz]` is what the reference model asks for, but two
//! things stood in the way of writing it honestly:
//!
//! - **the writer is `property::write`** (a `ContinuousProperty` is always written
//!   through `property::write`), and it
//!   hardcodes `ValueCountPerIndexableElement: [1]` — a second, tensor-shaped
//!   writer would be exactly the second writer that rule forbids, and this
//!   file may not edit `property.rs`;
//! - **`Facet` is one value per property**, and RESQML's own list
//!   (`eml23.Facet`) has no member naming *all three* of I, J and K at once —
//!   only one direction at a time. Writing a single `Facet` on a
//!   three-component property would be a guess the schema cannot even state
//!   correctly, and *refuse rather than guess* is the rule this crate lives
//!   by.
//!
//! So permeability is written as **three scalar properties**, one per
//! direction — kx with `Facet` `I`, ky with `J`, kz with `K` — all of the same
//! kind (`absolute permeability`), each one `property::write`'s ordinary
//! case. Each names its direction unambiguously, which the reference model's single
//! tensor property, read strictly, cannot. Turning this into the one property
//! described above is future work **in `property.rs`**, not here.
//!
//! # A law is sourced, and what it lays says so
//!
//! No constant without a source: a plausible value is declared
//! synthetic (reference-model rule, ADR-0019): `source` is required and a blank
//! one refused, for every case — a K-layer value is exactly as invented as a
//! law of depth. And the rule on invented objects — `ExtensionNameValue` `synthetic = true` and a
//! sentence in `Citation/Description` — is written on every property.
//!
//! # What is not suggested, and why
//!
//! Nothing: there is no `title` slot (each property names itself after the
//! grid and what it holds) and no `crs` slot (the grid already has one, and a
//! property does not carry a frame of its own).
//!
//! # References
//!
//! - The hydrostatic and geothermal laws of `energyml-analysis` (`well_logs`).
//!   TODO(reference): verify — the geothermal gradient and the hydrostatic law carry no published source (the requirement itself says the source is still to be found).

use serde_json::json;

use energyml_analysis::heights::{Height, Heights};
use energyml_analysis::well_logs::{self as analysis, Coefficient, Law, LogError};
use energyml_domain::provenance::Parameter;
use energyml_domain::units;
use energyml_geometry::geometry::{Geometry, Patch, Topology};

use crate::declare::{Field, Inputs, ObjectRef, Params, PropertyKindRef, Shape, Title};
use crate::operation::{
    About, ActivityDraft, Category, Context, Identity, Need, Note, Operation, Plan, Refusal, Slot,
};
use crate::pieces::{Known, Ref};

use super::property::PropertyParams;
use super::support;
use super::well_logs::{HydrostaticParams, TemperatureParams};

/// The operation.
pub struct GridProperties;

/// What `grid-properties` is told.
#[derive(Debug, Params)]
pub struct GridPropertiesParams {
    /// what is laid on the grid's cells.
    #[param(flatten)]
    pub what: WhatParams,
    /// where the law or the values come from — a document with its page or paragraph; a value stated as plausible says so (synthetic, and why)
    pub source: String,
    /// the HDF5 file the value arrays go into
    #[param(default = String::from("data.h5"), group = "naming")]
    pub data_file: String,
}

/// what is laid on the grid's cells — a temperature or a hydrostatic pressure by a sourced law of each cell's own vertical depth below the ground, or a porosity, a net-to-gross ratio or a permeability uniform within each K layer
///
/// **No default**: which quantity is the question, and a default would
/// answer it for whoever did not.
#[derive(Debug, Params)]
#[params(tag = "what")]
pub enum WhatParams {
    /// A temperature rising linearly with depth, read below the ground at each cell's own centre.
    Temperature(TemperatureParams),
    /// The weight of a column of water below a water table, read below the ground at each cell's own centre.
    Pressure(HydrostaticParams),
    /// A porosity, the same value on every cell of a K layer, top to bottom.
    Porosity(LayerParams),
    /// A net-to-gross thickness ratio, the same value on every cell of a K layer, top to bottom.
    #[case(name = "ntg")]
    NetGross(LayerParams),
    /// An absolute permeability — kx, ky, kz — the same three values on every cell of a K layer, top to bottom.
    Permeability(PermeabilityParams),
}

/// A value stated once per K layer of the grid, top to bottom.
#[derive(Debug, Params)]
pub struct LayerParams {
    /// one value per K layer, top to bottom — the same value on every cell of that layer
    #[param(least = 1, ordered)]
    pub layers: Vec<f64>,
    /// the unit of the values — an Energistics uom code, `Euc` for none
    pub uom: String,
}

/// What a permeability is told — kx, ky, kz, one value per K layer each.
#[derive(Debug, Params)]
pub struct PermeabilityParams {
    /// kx: one value per K layer, top to bottom
    #[param(least = 1, ordered)]
    pub kx: Vec<f64>,
    /// ky: one value per K layer, top to bottom
    #[param(least = 1, ordered)]
    pub ky: Vec<f64>,
    /// kz: one value per K layer, top to bottom
    #[param(least = 1, ordered)]
    pub kz: Vec<f64>,
    /// the unit of the values — an Energistics uom code, `mD` for millidarcy
    pub uom: String,
}

/// What `grid-properties` takes.
#[derive(Debug, Inputs)]
pub struct GridPropertiesInputs {
    /// the IJK grid whose cells receive the property
    #[input(types = "resqml22.IjkGridRepresentation", topology = "hexahedra")]
    pub grid: Shape,
    /// the ground — a surface's triangles or a grid's cells, in the grid's own frame; its height under each cell centre's (x, y) is where the depth is counted from — pressure and temperature only
    #[input(topology = "triangles")]
    pub ground: Option<Shape>,
}

/// The dimensions of an IJK grid's one patch of hexahedra, or why it is none.
fn hexahedra_dims(geometry: &Geometry) -> Result<[usize; 3], String> {
    let [patch] = geometry.patches.as_slice() else {
        return Err(format!(
            "holds {} patch(es): an IJK grid is one patch of hexahedra",
            geometry.patches.len()
        ));
    };
    let Topology::Hexahedra { dimensions, .. } = &patch.topology else {
        return Err(
            "is no IJK grid: a property on grid cells is laid on an `IjkGridRepresentation`, read as hexahedra — make one with `ijk-grid-from-horizons` first"
                .to_string(),
        );
    };
    let [ni, nj, nk] = *dimensions;
    if ni == 0 || nj == 0 || nk == 0 {
        return Err(format!(
            "declares {ni} x {nj} x {nk} cells: an IJK grid has at least one cell along each axis"
        ));
    }
    Ok([ni, nj, nk])
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

/// The centre of the cell at `position` in `cells` — the mean of its eight corners.
fn cell_centre(patch: &Patch, position: usize) -> [f64; 3] {
    let Topology::Hexahedra { cells, .. } = &patch.topology else {
        unreachable!("checked by hexahedra_dims before any position is read")
    };
    let mut centre = [0.0f64; 3];
    for &node in &cells[position] {
        let point = patch.points[node as usize];
        for (axis, value) in centre.iter_mut().enumerate() {
            *value += point[axis] / 8.0;
        }
    }
    centre
}

/// The property kind of the standard dictionary, by its title — never a uuid
/// typed here, the dictionary is the authority for its own identifiers.
fn kind_ref(title: &str) -> Result<(serde_json::Value, String), Refusal> {
    let found = energyml_domain::property_kind::with_default(|dictionary| {
        dictionary
            .by_title(title)
            .first()
            .map(|kind| (kind.uuid.clone(), kind.title.clone()))
    });
    let (uuid, kind_title) = found.ok_or_else(|| {
        Refusal::on(
            "what",
            format!(
                "the dictionary in use holds no kind called `{title}`: name a `propertyKind` of the package"
            ),
        )
    })?;
    let dor = json!({ "$type": "eml23.DataObjectReference", "Uuid": uuid, "QualifiedType": "eml23.PropertyKind", "Title": kind_title });
    Ok((dor, uuid))
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

/// The law's settings, as `well-logs` states them, glued to
/// `energyml_analysis::well_logs::Law` — copied rather than shared, since
/// `well_logs`'s own `Told` is private to that file and this crate writes one
/// file per operation.
#[derive(Clone, Copy)]
enum Told {
    Temperature {
        at: f64,
        from: f64,
        gradient_km: f64,
    },
    Hydrostatic {
        level: f64,
        density: f64,
        gravity: f64,
        at_table_bar: f64,
    },
}

impl Told {
    fn temperature(t: &TemperatureParams) -> Told {
        Told::Temperature {
            at: t.reference_temperature,
            from: t.reference_depth,
            gradient_km: t.gradient,
        }
    }

    fn hydrostatic(h: &HydrostaticParams) -> Told {
        Told::Hydrostatic {
            level: h.water_level,
            density: h.density,
            gravity: h.gravity,
            at_table_bar: h.table_pressure,
        }
    }

    /// The law under a cell whose ground stands at `ground` metres of
    /// elevation, in the SI units the analysis counts in — `bar` pascals to
    /// the bar.
    fn law(&self, ground: f64, bar: f64) -> Law {
        match *self {
            Told::Temperature {
                at,
                from,
                gradient_km,
            } => Law::Geothermal {
                at,
                from,
                gradient: gradient_km / 1000.0,
            },
            Told::Hydrostatic {
                level,
                density,
                gravity,
                at_table_bar,
            } => Law::Hydrostatic {
                table: ground - level,
                density,
                gravity,
                at_table: at_table_bar * bar,
            },
        }
    }

    fn word(&self) -> &'static str {
        match self {
            Told::Temperature { .. } => "temperature",
            Told::Hydrostatic { .. } => "hydrostatic",
        }
    }

    /// The title of the kind in the standard dictionary.
    fn kind(&self) -> &'static str {
        match self {
            Told::Temperature { .. } => "temperature",
            Told::Hydrostatic { .. } => "pressure",
        }
    }

    fn uom(&self) -> &'static str {
        match self {
            Told::Temperature { .. } => "degC",
            Told::Hydrostatic { .. } => "bar",
        }
    }

    /// From what the analysis gives (°C, Pa) to what is written.
    fn scale(&self, bar: f64) -> f64 {
        match self {
            Told::Temperature { .. } => 1.0,
            Told::Hydrostatic { .. } => 1.0 / bar,
        }
    }

    /// Its coefficients, exactly — what the seed carries.
    fn key(&self) -> String {
        match *self {
            Told::Temperature {
                at,
                from,
                gradient_km,
            } => format!("temperature:{at:?}:{from:?}:{gradient_km:?}"),
            Told::Hydrostatic {
                level,
                density,
                gravity,
                at_table_bar,
            } => {
                format!("hydrostatic:{level:?}:{density:?}:{gravity:?}:{at_table_bar:?}")
            }
        }
    }

    /// The law, in the words a description and a note use.
    fn words(&self) -> String {
        match *self {
            Told::Temperature {
                at,
                from,
                gradient_km,
            } => format!(
                "a temperature of {at} degC at {from} m below the ground, changing by {gradient_km} degC per km of vertical depth below it"
            ),
            Told::Hydrostatic {
                level,
                density,
                gravity,
                at_table_bar,
            } => format!(
                "a hydrostatic pressure below a water table at {level} m of elevation: {at_table_bar} bar there, and {density} kg/m3 × {gravity} m/s2 per metre of water below it"
            ),
        }
    }
}

/// The slot a coefficient of the analysis is told in.
fn slot_of(which: Coefficient) -> &'static str {
    match which {
        Coefficient::Temperature => "referenceTemperature",
        Coefficient::From => "referenceDepth",
        Coefficient::Gradient => "gradient",
        Coefficient::Table => "waterLevel",
        Coefficient::Density => "density",
        Coefficient::Gravity => "gravity",
        Coefficient::AtTable => "tablePressure",
    }
}

/// A refusal of the analysis, under the slot its coefficient is told in.
fn refusal_of(why: &LogError) -> Refusal {
    match why {
        LogError::NotFinite { which, .. }
        | LogError::NotPositive { which, .. }
        | LogError::Negative { which, .. } => Refusal::on(slot_of(*which), why),
        LogError::BadDepth { .. } => Refusal::on("grid", why),
    }
}

impl Operation for GridProperties {
    type Params = GridPropertiesParams;
    type Inputs = GridPropertiesInputs;

    fn about(&self) -> About {
        About {
            name: "grid-properties",
            describe: "a pressure or a temperature by a sourced law of each cell's own vertical depth below the ground, or a porosity, a net-to-gross ratio or a permeability uniform within each K layer — laid on the cells of an IJK grid, marked synthetic",
            category: Category::Synthetic,
            makes: &["resqml22.ContinuousProperty"],
            recipe: "grid-properties",
            represents: None,
        }
    }

    fn check(&self, inputs: &GridPropertiesInputs) -> Result<(), Refusal> {
        hexahedra_dims(&inputs.grid.geometry).map_err(|why| Refusal::on("grid", why))?;
        if let Some(ground) = &inputs.ground
            && !support::Faces::of(&ground.geometry).any()
        {
            return Err(Refusal::on(
                "ground",
                format!(
                    "`{}` has no face — the ground is read on triangles or a grid's cells, and points or lines have none",
                    ground.name
                ),
            ));
        }
        Ok(())
    }

    fn plan(
        &self,
        inputs: GridPropertiesInputs,
        params: GridPropertiesParams,
        cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        let GridPropertiesInputs { grid, ground } = inputs;

        let source = params.source.trim().to_string();
        if source.is_empty() {
            return Err(Refusal::on(
                "source",
                "is empty: a law or a set of values is sourced — a document with its page or paragraph, or a plausible value said to be one (reference-model rule: no constant without a source)",
            ));
        }

        let [ni, nj, nk] = hexahedra_dims(&grid.geometry).map_err(|why| Refusal::on("grid", why))?;
        let total = ni * nj * nk;
        let per_layer = ni * nj;
        let grid_title = title_of(&grid);
        let known = grid.known.as_ref();
        let reference = known.and_then(|k| k.reference.as_ref()).ok_or_else(|| {
            Refusal::on(
                "grid",
                format!("`{grid_title}` names no object of the package for a property to hang on"),
            )
        })?;
        let representation = reference.dor();

        match &params.what {
            WhatParams::Temperature(t) => plan_depth(
                &grid,
                ground.as_ref(),
                known,
                reference,
                &representation,
                &grid_title,
                total,
                Told::temperature(t),
                &source,
                &params.data_file,
                cx,
            ),
            WhatParams::Pressure(h) => plan_depth(
                &grid,
                ground.as_ref(),
                known,
                reference,
                &representation,
                &grid_title,
                total,
                Told::hydrostatic(h),
                &source,
                &params.data_file,
                cx,
            ),
            WhatParams::Porosity(layer) => plan_layer(
                reference,
                &representation,
                &grid_title,
                total,
                per_layer,
                nk,
                "porosity",
                layer,
                &source,
                &params.data_file,
                cx,
            ),
            WhatParams::NetGross(layer) => plan_layer(
                reference,
                &representation,
                &grid_title,
                total,
                per_layer,
                nk,
                "net gross thickness ratio",
                layer,
                &source,
                &params.data_file,
                cx,
            ),
            WhatParams::Permeability(perm) => plan_permeability(
                reference,
                &representation,
                &grid_title,
                total,
                per_layer,
                nk,
                perm,
                &source,
                &params.data_file,
                cx,
            ),
        }
    }

    /// `what`'s settings and `source` are inputs, recorded under the
    /// parameters the projection already declares — a title cannot be both.
    fn reports(&self) -> Vec<Slot> {
        let number = || Need::Number {
            uom: None,
            least: None,
            most: None,
        };
        vec![
            Slot::required("cellCount", number(), "cells the grid declares — Ni × Nj × Nk"),
            Slot::optional(
                "layerCount",
                number(),
                "K layers — for a porosity, a net-to-gross ratio or a permeability",
            ),
            Slot::optional(
                "undefinedCellCount",
                number(),
                "cells with no defined geometry — no depth to read, written as NaN; pressure and temperature only",
            ),
            Slot::optional(
                "aboveGroundCount",
                number(),
                "cell centres above the ground — no law of the subsurface holds there, written as NaN; pressure and temperature only",
            ),
            Slot::optional(
                "shallowCount",
                number(),
                "cell centres below the ground and above where the law starts, written as NaN; pressure and temperature only",
            ),
        ]
    }
}

/// A pressure or a temperature by a sourced law of each cell's own vertical
/// depth below the ground.
#[allow(clippy::too_many_arguments)]
fn plan_depth(
    grid: &Shape,
    ground: Option<&Shape>,
    known: Option<&Known>,
    reference: &Ref,
    representation: &serde_json::Value,
    grid_title: &str,
    total: usize,
    told: Told,
    source: &str,
    data_file: &str,
    cx: &Context<'_>,
) -> Result<Plan, Refusal> {
    let vertical_up = known.and_then(|k| k.vertical_up).ok_or_else(|| {
        Refusal::on(
            "grid",
            format!(
                "`{grid_title}` does not say which way its vertical axis points — a depth below the ground is read on it, and up and down cannot be guessed"
            ),
        )
    })?;
    let vertical_uom = known.and_then(|k| k.vertical_uom.as_deref()).ok_or_else(|| {
        Refusal::on(
            "grid",
            format!("`{grid_title}` does not say in which unit its vertical axis counts — a depth in feet read as metres is 3.28 times too deep"),
        )
    })?;
    let metres = units::metres(1.0, vertical_uom)
        .map_err(|why| Refusal::on("grid", format!("`{grid_title}` counts its vertical axis in `{vertical_uom}`, and a depth below the ground is a length: {why}")))?;

    let ground = ground.ok_or_else(|| {
        Refusal::on(
            "ground",
            "the depths are counted from the ground, and nothing says where it is: give `ground`, a surface to read it on under each cell's own centre",
        )
    })?;
    let ground_known = ground.known.as_ref();
    let ground_title = title_of(ground);
    match (
        known.and_then(|k| k.crs.as_ref()),
        ground_known.and_then(|k| k.crs.as_ref()),
    ) {
        (Some(mine), Some(theirs)) if mine.uuid.eq_ignore_ascii_case(&theirs.uuid) => {}
        (Some(mine), Some(theirs)) => {
            return Err(Refusal::on(
                "ground",
                format!(
                    "`{ground_title}` is in `{}` and `{grid_title}` in `{}` — a height read in one frame and compared in another is a height from somewhere else",
                    theirs.title, mine.title
                ),
            ));
        }
        _ => {
            return Err(Refusal::on(
                "ground",
                format!(
                    "`{ground_title}` or `{grid_title}` states no frame, so the ground cannot be compared with the grid's cells: give both from the package"
                ),
            ));
        }
    }
    let faces = support::Faces::of(&ground.geometry);
    let heights = Heights::of(faces.surface(), "ground", "under the grid's cells")
        .map_err(|why| Refusal::on("ground", why))?;

    let bar = units::pascals(1.0, "bar").map_err(|why| Refusal::new("units", why.to_string()))?;
    told.law(0.0, bar).check().map_err(|why| refusal_of(&why))?;

    let (kind, kind_uuid) = kind_ref(told.kind())?;

    let patch = &grid.geometry.patches[0];
    let mut values = vec![f64::NAN; total];
    let mut undefined = 0usize;
    let mut above_ground = 0usize;
    let mut shallow = 0usize;
    let mut range: Option<[f64; 2]> = None;

    for (cell_index, value) in values.iter_mut().enumerate() {
        cx.stop_if_cancelled()?;
        let Some(position) = patch.topology.position_of(cell_index as u32) else {
            undefined += 1;
            continue;
        };
        let centre = cell_centre(patch, position);
        let elevation_m = (if vertical_up { centre[2] } else { -centre[2] }) * metres;
        let ground_z = match heights.height(centre[0], centre[1]) {
            Height::At(z) => z,
            Height::Outside => {
                return Err(Refusal::on(
                    "ground",
                    format!(
                        "cell {cell_index} of `{grid_title}`, centred at ({}, {}), is outside `{ground_title}`: no face lies under it",
                        centre[0], centre[1]
                    ),
                ));
            }
            Height::Folded => {
                return Err(Refusal::on(
                    "ground",
                    format!(
                        "`{ground_title}` folds under cell {cell_index} of `{grid_title}`, at ({}, {}): several faces disagree on its height there",
                        centre[0], centre[1]
                    ),
                ));
            }
        };
        let ground_m = (if vertical_up { ground_z } else { -ground_z }) * metres;
        let depth = ground_m - elevation_m;
        let law = told.law(ground_m, bar);
        let logged = analysis::lay(&law, std::slice::from_ref(&depth)).map_err(|why| match &why {
            LogError::BadDepth { .. } => {
                Refusal::on("grid", format!("cell {cell_index} of `{grid_title}`: {why}"))
            }
            other => refusal_of(other),
        })?;
        *value = logged.values[0];
        above_ground += logged.above_ground;
        shallow += logged.shallow;
        if let Some([low, high]) = logged.range {
            range = Some(match range {
                None => [low, high],
                Some([a, b]) => [a.min(low), b.max(high)],
            });
        }
    }

    let scale = told.scale(bar);
    for value in values.iter_mut() {
        *value *= scale;
    }
    if let Some([low, high]) = range.as_mut() {
        *low *= scale;
        *high *= scale;
    }

    // **The one count a pure plan can verify**: a value on each cell the grid
    // declares, never more, never fewer — `values` is built exactly `total`
    // long above, and this is what would catch a change that broke it.
    if values.len() != total {
        return Err(Refusal::on(
            "grid",
            format!(
                "{total} cell(s) declared and {} value(s) computed — a value is laid on each cell",
                values.len()
            ),
        ));
    }

    let field = Field {
        name: format!("{} of {grid_title}", told.kind()),
        over: grid.name.clone(),
        on: "cells".to_string(),
        values,
        uom: Some(told.uom().to_string()),
    };
    let title = format!("{grid_title} — {}", told.kind());
    let seed = format!(
        "grid-properties:{}:{}:{kind_uuid}:{}",
        reference.uuid,
        told.key(),
        digest(&field.values)
    );
    let property = PropertyParams {
        representation: ObjectRef(representation.clone()),
        property_kind: PropertyKindRef(kind),
        uom: told.uom().to_string(),
        title: Title(title.clone()),
        data_file: data_file.to_string(),
    };
    let mut drawn = super::property::write(&field, known, &property, &seed, cx)?;

    let description = format!(
        "Laid by a law, not measured: {}, at each cell's own vertical depth below the ground read on `{ground_title}`. Source: {source}",
        told.words()
    );
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
        // The reference-model rule on invented objects (ADR-0019).
        object.json["ExtensionNameValue"] = json!([{
            "$type": "eml23.ExtensionNameValue",
            "Name": "synthetic",
            "Value": { "$type": "eml23.StringMeasure", "_": "true" },
            "Description": format!("laid by `grid-properties` by a law of depth ({}), not measured", told.word()),
        }]);
    }

    let mut notes = vec![
        Note { key: "law", detail: format!("{} — source: {source}", told.words()) },
        Note {
            key: "vertical-depth",
            detail: "the law is read at the vertical depth below the ground under each cell's own centre, in the grid's own frame".to_string(),
        },
        Note {
            key: "synthetic",
            detail: "the property is laid, not measured: it carries `synthetic = true` and a description of its law and its source".to_string(),
        },
    ];
    if undefined > 0 {
        notes.push(Note {
            key: "undefined-cells",
            detail: format!("{undefined} cell(s) of `{grid_title}` have no defined geometry (an undefined pillar, TRG 2.2 §5.1 p. 94): no depth to read, written as NaN"),
        });
    }
    if above_ground > 0 {
        notes.push(Note {
            key: "above-ground",
            detail: format!("{above_ground} cell centre(s) lie above the ground: no law of the subsurface holds there, written as NaN"),
        });
    }
    if shallow > 0 {
        notes.push(Note {
            key: "shallow",
            detail: format!("{shallow} cell centre(s) lie below the ground and above where the law starts: written as NaN, not extrapolated"),
        });
    }
    if let Some([low, high]) = range {
        notes.push(Note {
            key: "range",
            detail: format!("the values laid run from {low} to {high} {}", told.uom()),
        });
    }
    notes.append(&mut drawn.says);

    let mut parameters = vec![Parameter::Object {
        title: "input".to_string(),
        reference: representation.clone(),
    }];
    if let Some(g) = ground_known.and_then(|k| k.reference.as_ref()) {
        parameters.push(Parameter::Object {
            title: "input".to_string(),
            reference: g.dor(),
        });
    }
    parameters.push(count("cellCount", total));
    parameters.push(count("undefinedCellCount", undefined));
    parameters.push(count("aboveGroundCount", above_ground));
    parameters.push(count("shallowCount", shallow));

    Ok(Plan {
        media: Vec::new(),
        blocked: None,
        objects: drawn.objects,
        datasets: drawn.datasets,
        // **No shape**: a property is laid on a geometry, it
        // makes none.
        shapes: Vec::new(),
        activity: ActivityDraft {
            recipe: "grid-properties",
            title: format!("grid-properties → {} on {total} cell(s)", told.kind()),
            seed,
            parameters,
        },
        says: notes,
        dropped: Vec::new(),
    })
}

/// A porosity or a net-to-gross ratio, the same value on every cell of a K
/// layer, top to bottom — no geometry read at all.
#[allow(clippy::too_many_arguments)]
fn plan_layer(
    reference: &Ref,
    representation: &serde_json::Value,
    grid_title: &str,
    total: usize,
    per_layer: usize,
    nk: usize,
    kind_title: &'static str,
    layer: &LayerParams,
    source: &str,
    data_file: &str,
    cx: &Context<'_>,
) -> Result<Plan, Refusal> {
    if layer.layers.len() != nk {
        return Err(Refusal::on(
            "layers",
            format!(
                "the grid has {nk} K layer(s) and {} value(s) were given — one per layer, top to bottom",
                layer.layers.len()
            ),
        ));
    }
    for (k, &v) in layer.layers.iter().enumerate() {
        if !(v.is_finite() && (0.0..=1.0).contains(&v)) {
            return Err(Refusal::on(
                "layers",
                format!("layer {k} (top to bottom): {v} is not a ratio between 0 and 1"),
            ));
        }
    }
    cx.stop_if_cancelled()?;
    let values: Vec<f64> = (0..total)
        .map(|cell_index| layer.layers[cell_index / per_layer])
        .collect();
    if values.len() != total {
        return Err(Refusal::on(
            "grid",
            format!(
                "{total} cell(s) declared and {} value(s) computed — a value is laid on each cell",
                values.len()
            ),
        ));
    }

    let (kind, kind_uuid) = kind_ref(kind_title)?;
    let field = Field {
        name: format!("{kind_title} of {grid_title}"),
        over: reference.title.clone(),
        on: "cells".to_string(),
        values,
        uom: Some(layer.uom.clone()),
    };
    let title = format!("{grid_title} — {kind_title}");
    let seed = format!(
        "grid-properties:{}:{kind_title}:{kind_uuid}:{}",
        reference.uuid,
        digest(&field.values)
    );
    let property = PropertyParams {
        representation: ObjectRef(representation.clone()),
        property_kind: PropertyKindRef(kind),
        uom: layer.uom.clone(),
        title: Title(title.clone()),
        data_file: data_file.to_string(),
    };
    let mut drawn = super::property::write(&field, None, &property, &seed, cx)?;

    let description = format!(
        "Laid by a value per K layer, not measured: {kind_title} — one value per layer, top to bottom: {:?} {}. Source: {source}",
        layer.layers, layer.uom
    );
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
        object.json["ExtensionNameValue"] = json!([{
            "$type": "eml23.ExtensionNameValue",
            "Name": "synthetic",
            "Value": { "$type": "eml23.StringMeasure", "_": "true" },
            "Description": "laid by `grid-properties`, one value per K layer, not measured",
        }]);
    }

    let mut notes = vec![
        Note { key: "layers", detail: format!("one value per K layer, top to bottom, the same on every cell of it — source: {source}") },
        Note {
            key: "synthetic",
            detail: "the property is laid, not measured: it carries `synthetic = true` and a description of its values and their source".to_string(),
        },
        Note {
            key: "geometry-independent",
            detail: "written on every cell regardless of whether its own geometry is defined: a K-layer value does not depend on a cell's corner points".to_string(),
        },
    ];
    notes.append(&mut drawn.says);

    let parameters = vec![
        Parameter::Object {
            title: "input".to_string(),
            reference: representation.clone(),
        },
        count("cellCount", total),
        count("layerCount", nk),
    ];

    Ok(Plan {
        media: Vec::new(),
        blocked: None,
        objects: drawn.objects,
        datasets: drawn.datasets,
        shapes: Vec::new(),
        activity: ActivityDraft {
            recipe: "grid-properties",
            title: format!("grid-properties → {kind_title} on {total} cell(s)"),
            seed,
            parameters,
        },
        says: notes,
        dropped: Vec::new(),
    })
}

/// An absolute permeability — kx, ky, kz — written as three scalar
/// properties, one per direction (see the module doc: *A tensor property this
/// build does not write*).
#[allow(clippy::too_many_arguments)]
fn plan_permeability(
    reference: &Ref,
    representation: &serde_json::Value,
    grid_title: &str,
    total: usize,
    per_layer: usize,
    nk: usize,
    perm: &PermeabilityParams,
    source: &str,
    data_file: &str,
    cx: &Context<'_>,
) -> Result<Plan, Refusal> {
    for (name, values) in [("kx", &perm.kx), ("ky", &perm.ky), ("kz", &perm.kz)] {
        if values.len() != nk {
            return Err(Refusal::on(
                name,
                format!(
                    "the grid has {nk} K layer(s) and {} value(s) were given — one per layer, top to bottom",
                    values.len()
                ),
            ));
        }
        for (k, &v) in values.iter().enumerate() {
            if !(v.is_finite() && v >= 0.0) {
                return Err(Refusal::on(
                    name,
                    format!("layer {k} (top to bottom): {v} is not a permeability (0 or more)"),
                ));
            }
        }
    }
    cx.stop_if_cancelled()?;

    let (kind, kind_uuid) = kind_ref("absolute permeability")?;
    let mut objects = Vec::new();
    let mut datasets = Vec::new();
    let mut says = Vec::new();

    for (written, (component, facet, values)) in [
        ("kx", "I", &perm.kx),
        ("ky", "J", &perm.ky),
        ("kz", "K", &perm.kz),
    ]
    .into_iter()
    .enumerate()
    {
        let per_cell: Vec<f64> = (0..total)
            .map(|cell_index| values[cell_index / per_layer])
            .collect();
        if per_cell.len() != total {
            return Err(Refusal::on(
                "grid",
                format!(
                    "{total} cell(s) declared and {} value(s) computed — a value is laid on each cell",
                    per_cell.len()
                ),
            ));
        }
        let field = Field {
            name: format!("{component} of {grid_title}"),
            over: reference.title.clone(),
            on: "cells".to_string(),
            values: per_cell,
            uom: Some(perm.uom.clone()),
        };
        let title = format!("{grid_title} — permeability ({component})");
        let seed = format!(
            "grid-properties:{}:permeability:{component}:{kind_uuid}:{}",
            reference.uuid,
            digest(&field.values)
        );
        let property = PropertyParams {
            representation: ObjectRef(representation.clone()),
            property_kind: PropertyKindRef(kind.clone()),
            uom: perm.uom.clone(),
            title: Title(title.clone()),
            data_file: data_file.to_string(),
        };
        // **One uuid each, in the plan's order** — the same trick `well-logs`
        // uses for several properties in one run: under a given list, the
        // rank picks the one this property gets; a derived identity differs
        // by its seed already, and is passed through as it is.
        let identity = match cx.identity {
            Identity::Given(_) => Identity::Given(vec![cx.identity.uuid("", written)?]),
            derived => derived.clone(),
        };
        let one = Context {
            identity: &identity,
            when: cx.when,
            purpose: cx.purpose,
            feedback: cx.feedback,
            bag: cx.bag,
        };
        let mut drawn = super::property::write(&field, None, &property, &seed, &one)?;

        let description = format!(
            "Laid by a value per K layer, not measured: absolute permeability, direction {component} — one value per layer, top to bottom: {values:?} {}. Source: {source}",
            perm.uom
        );
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
            // One direction per property — see the module's *A tensor
            // property this build does not write*.
            object.json["Facet"] =
                json!([{ "$type": "eml23.PropertyKindFacet", "Facet": facet, "Kind": "direction" }]);
            object.json["ExtensionNameValue"] = json!([{
                "$type": "eml23.ExtensionNameValue",
                "Name": "synthetic",
                "Value": { "$type": "eml23.StringMeasure", "_": "true" },
                "Description": "laid by `grid-properties`, one value per K layer, not measured",
            }]);
        }
        objects.append(&mut drawn.objects);
        datasets.append(&mut drawn.datasets);
        says.append(&mut drawn.says);
    }

    let mut notes = vec![
        Note { key: "layers", detail: format!("one value per K layer, top to bottom, the same on every cell of it — source: {source}") },
        Note {
            key: "components",
            detail: "kx, ky, kz are written as three scalar properties of the same kind (`absolute permeability`), one direction each (`Facet` I, J, K): a single `Facet` cannot name three directions on one property, and this build's writer (`property::write`) is scalar-only — see the module doc".to_string(),
        },
        Note {
            key: "synthetic",
            detail: "the properties are laid, not measured: each carries `synthetic = true` and a description of its values and their source".to_string(),
        },
        Note {
            key: "geometry-independent",
            detail: "written on every cell regardless of whether its own geometry is defined: a K-layer value does not depend on a cell's corner points".to_string(),
        },
    ];
    notes.append(&mut says);

    let parameters = vec![
        Parameter::Object {
            title: "input".to_string(),
            reference: representation.clone(),
        },
        count("cellCount", total),
        count("layerCount", nk),
    ];

    Ok(Plan {
        media: Vec::new(),
        blocked: None,
        objects,
        datasets,
        shapes: Vec::new(),
        activity: ActivityDraft {
            recipe: "grid-properties",
            title: format!("grid-properties → permeability on {total} cell(s)"),
            seed: format!("grid-properties:{}:permeability:{kind_uuid}", reference.uuid),
            parameters,
        },
        says: notes,
        dropped: Vec::new(),
    })
}
