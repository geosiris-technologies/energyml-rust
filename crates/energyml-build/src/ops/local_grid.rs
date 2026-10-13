//! `local-grid` - a refined window of an `IjkGridRepresentation` of the
//! package: a child grid whose geometry is *derived*, never stored, from a
//! `ParentWindow` and a `Regrid` per axis (RESQML TRG 2.2 §5.31 p. 127),
//! grouped with its parent under a `LocalGridSet`.
//!
//! In the reference model: a `LocalGridSet` + a child `IjkGridRepresentation` -
//! window I 10-13, J 8-11, all K, refined 2 × 2 × 1 → 8 × 8 × 10 = 640
//! cells; an `IjkParentWindow` (TRG §5.31 p. 127); and
//! `ParentWindow.ParentIjkGridRepresentation` 1..1 → the main grid, the window
//! being in the main grid.
//!
//! # A general operation, not an invention
//!
//! A local grid is ordinary RESQML - `Category::Geometry`, like
//! `ijk-grid-from-horizons`, and **no `synthetic` parameter**.
//!
//! # What the file says, and where the TRG says it
//!
//! - **the window is a box of the parent's cells, 0-based** - *"0-based index
//!   for the placement of the window on the parent grid"* (`Regrid.
//!   InitialIndexOnParentGrid`, `Grids.xsd`; TRG §5.31 p. 127) - `i0..=i1`,
//!   `j0..=j1` inclusive parent cell indices; `k0..=k1` left out for the
//!   parent's whole K range (all of K);
//! - **a `Regrid` per axis** (`IRegrid`, `JRegrid`, `KRegrid`), each an
//!   `InitialIndexOnParentGrid` and, for a refinement greater than one,
//!   `Intervals` with **one interval per parent cell** - *« writers should set
//!   the parent count per interval to 1 »* (RESQML TUG 2.2 §9.8.2, p. 142) -
//!   and *"If there is a 1:1 correspondence between the parent and child
//!   cells, then this object is not needed"* (`Grids.xsd`, `Intervals`), so a
//!   refinement of one on an axis omits it;
//! - **no `Geometry` at all on the child**: a local grid's points are the
//!   parent's, subdivided. `crates/energyml-geometry/src/parent.rs` is the
//!   reader that already does this at read time, through a hyperslab of the
//!   parent's `Points` - confirmed here by reading it before writing this file.
//!   Nothing here writes a `Points` array for the child;
//! - **a `LocalGridSet`** groups the child under its parent (`ChildGrid`).
//!   Checked in `crates/generated/energyml-resqml-2-2/src/grids.rs`: it exists
//!   in RESQML 2.2, with `Citation` and a `ChildGrid` list of
//!   `DataObjectReference`.
//!
//! # The computed shape's calculation lives here - a named exception
//!
//! The house rule is that a computation lives in `energyml-analysis`, never in an
//! operation (ADR-0017). This file keeps its
//! trilinear interpolation and its cell winding **inside itself**, because
//! no third file under `energyml-analysis` was available when it was written. Moving the
//! computation there, the way `ijk-grid-from-horizons` keeps its own in
//! `energyml_analysis::ijk`, is named debt for whoever next touches this
//! operation.
//!
//! # What is refused, by name, before any computation
//!
//! - a parent that is no `IjkGridRepresentation`;
//! - a window past the parent's extent, or reversed (`i1 < i0`, and the same
//!   for J and K);
//! - **a split coordinate line (a fault) or a K gap inside the window** - the
//!   child's node there would have two positions, and the reader of the child
//!   refuses both for that reason (`crates/energyml-geometry/src/parent.rs`).
//!   **Outside the window, or at its edge, neither matters**: the window is
//!   read off the parent's own cells (`window_of`), which name the points they
//!   use whatever the faults and gaps elsewhere. That is what lets the child be
//!   written at all - the reference model's main grid has a K gap (an unmeshed
//!   cover) and a fault, and the well's impact is in the Chalk, below the gap
//!   and clear of the fault (until then any gap or fault anywhere was refused);
//! - a parent cell the window covers with no location - dropped by the reader,
//!   or a corner with no height (`NaN`), which trilinear interpolation would
//!   otherwise carry through every child node it touches, "silently, because
//!   NaN propagates without erroring" (the geometry crate's rules).
//!
//! # Identity
//!
//! From what the local grid **is**: its parent, the window, the refinement,
//! the title. A different window or a different refinement is another grid -
//! its cell count would no longer fit a property written on the old one.

use serde_json::{Value, json};

use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::{Geometry, Patch, Topology};

use crate::declare::{Inputs, Params, Shape, Suggestions, known_of};
use crate::operation::{
    About, ActivityDraft, Category, Context, Need, NewObject, Note, Operation, Plan, Refusal, Slot,
};
use crate::pieces::{Pieces, Ref};

use super::support;
use super::written::{NamingParams, RepresentedParams};

/// The operation.
pub struct LocalGrid;

/// What `local-grid` is told.
#[derive(Debug, Params)]
pub struct LocalGridParams {
    /// first parent cell along I the window starts at - 0-based, as RESQML counts it (`InitialIndexOnParentGrid`, TRG 2.2 §5.31 p. 127)
    pub i0: usize,
    /// last parent cell along I the window ends at, inclusive
    pub i1: usize,
    /// first parent cell along J the window starts at - 0-based
    pub j0: usize,
    /// last parent cell along J the window ends at, inclusive
    pub j1: usize,
    /// first parent cell along K the window starts at - 0-based; left out, the window starts at the parent's first layer
    pub k0: Option<usize>,
    /// last parent cell along K the window ends at, inclusive; left out, the window runs to the parent's last layer - all of K
    pub k1: Option<usize>,
    /// how many child cells each parent cell along I is cut into - uniform across the whole window
    #[param(default = 2_usize, min = 1)]
    pub ri: usize,
    /// how many child cells each parent cell along J is cut into
    #[param(default = 2_usize, min = 1)]
    pub rj: usize,
    /// how many child cells each parent cell along K is cut into
    #[param(default = 1_usize, min = 1)]
    pub rk: usize,
    /// What the local grid represents, when named - left out, it represents nothing on its own.
    #[param(flatten)]
    pub represented: RepresentedParams,
    /// The title and the file.
    #[param(flatten)]
    pub naming: NamingParams,
}

/// What `local-grid` takes.
#[derive(Debug, Inputs)]
pub struct LocalGridInputs {
    /// the parent `IjkGridRepresentation` the window is cut from - an object of the package, named by uuid in `ParentWindow`
    #[input(types = "resqml22.IjkGridRepresentation", topology = "hexahedra")]
    pub parent: Shape,
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

/// The parent as the geometry reader hands it: one patch of hexahedra over
/// the file's raw `Points` - every level, K gap levels included, and every
/// coordinate line, split lines included - each cell naming its eight corners
/// there (`crates/energyml-geometry/src/ijk.rs`, `read`).
///
/// **Read through its cells, never through the layout of `points`.** How many
/// levels and lines that array holds depends on the gaps and the faults, which
/// the shape does not carry; the cells say which point each corner of each
/// cell is, and that is the whole of what a window needs.
struct ParentGrid<'a> {
    ni: usize,
    nj: usize,
    nk: usize,
    points: &'a [[f64; 3]],
    cells: &'a [[u32; 8]],
    /// The RESQML number of each entry of `cells`, or empty when they coincide
    /// (`Topology::Hexahedra`): the reader drops a cell with no location.
    index: &'a [u32],
}

fn parent_grid(shape: &Shape) -> Result<ParentGrid<'_>, Refusal> {
    let [
        Patch {
            topology:
                Topology::Hexahedra {
                    cells,
                    dimensions,
                    index,
                },
            points,
            ..
        },
    ] = shape.geometry.patches.as_slice()
    else {
        return Err(Refusal::on(
            "parent",
            // Not the object's title: this is what `fitness` says, and a
            // title may spell anything - a release included.
            "is no IJK grid: a local grid refines the corner-point cells of one",
        ));
    };
    let [ni, nj, nk] = *dimensions;
    if index.is_empty() && cells.len() != ni * nj * nk {
        return Err(Refusal::on(
            "parent",
            format!(
                "`{}` reads as {} cell(s) for a {ni}×{nj}×{nk} grid and does not say which are missing: a window cannot find its cells",
                title_of(shape),
                cells.len()
            ),
        ));
    }
    Ok(ParentGrid {
        ni,
        nj,
        nk,
        points,
        cells,
        index,
    })
}

impl ParentGrid<'_> {
    /// The corners of parent cell `(i, j, k)`, or `None` when the reader
    /// dropped it - a cell the file marks undefined, or with a corner that has
    /// no location.
    fn cell(&self, i: usize, j: usize, k: usize) -> Option<[u32; 8]> {
        let number = (k * self.nj + j) * self.ni + i;
        let at = if self.index.is_empty() {
            Some(number)
        } else {
            self.index.binary_search(&(number as u32)).ok()
        };
        at.and_then(|at| self.cells.get(at)).copied()
    }
}

/// The window's own pillar lattice: node `(di, dj, dl)` is pillar
/// `(i0 + di, j0 + dj)` at the `dl`-th layer boundary from the top of layer
/// `k0`, laid out `dl`, then `dj`, then `di` fastest.
struct Window {
    /// Parent cells spanned on I and J - so `span + 1` pillars each way.
    span: [usize; 2],
    nodes: Vec<[f64; 3]>,
}

impl Window {
    fn at(&self, di: usize, dj: usize, dl: usize) -> [f64; 3] {
        let [si, sj] = self.span;
        self.nodes[(dl * (sj + 1) + dj) * (si + 1) + di]
    }
}

/// Read the window's lattice off the parent's cells.
///
/// Each corner of the lattice is named by every window cell that touches it,
/// and they must all name **the same point**. Two cells naming two points for
/// one corner is the refusal, and which one depends on where the two cells
/// are: side by side, a split coordinate line (a fault) runs between them;
/// one above the other, a K gap does. Either way the child's node there would
/// have two positions, and `crates/energyml-geometry/src/parent.rs`, which
/// reads the child back, refuses both for that reason - so writing one here
/// would write a grid nothing reads. Faults and gaps **outside** the window
/// change nothing: the window's cells name the points they use.
///
/// The reader lays each cell's corners as one face then the other, each face
/// `(i, j)`, `(i + 1, j)`, `(i + 1, j + 1)`, `(i, j + 1)`
/// (`energyml_geometry::ijk::read`); which face comes first depends on the
/// winding VTK asks for, so the top is told by the **point index** instead -
/// levels are stored top first, and the two corners of one pillar sit on one
/// coordinate line, so the upper one has the smaller index.
fn window_of(
    parent: &ParentGrid<'_>,
    title: &str,
    lo: [usize; 3],
    hi: [usize; 3],
) -> Result<Window, Refusal> {
    let span = [hi[0] - lo[0] + 1, hi[1] - lo[1] + 1, hi[2] - lo[2] + 1];
    let row = span[0] + 1;
    let plane = row * (span[1] + 1);
    // The point each lattice node is, and the parent layer of the cell that
    // named it first.
    let mut named: Vec<Option<(u32, usize)>> = vec![None; plane * (span[2] + 1)];
    for dk in 0..span[2] {
        for dj in 0..span[1] {
            for di in 0..span[0] {
                let (i, j, k) = (lo[0] + di, lo[1] + dj, lo[2] + dk);
                let Some(node) = parent.cell(i, j, k) else {
                    return Err(Refusal::on(
                        "parent",
                        format!(
                            "`{title}` has no geometry at cell ({i}, {j}, {k}), which the window covers: a local grid cannot interpolate a cell with no location"
                        ),
                    ));
                };
                let (top, base) = if node[0] < node[4] {
                    (&node[..4], &node[4..])
                } else {
                    (&node[4..], &node[..4])
                };
                for (corner, (ci, cj)) in [(0, 0), (1, 0), (1, 1), (0, 1)].into_iter().enumerate() {
                    for (face, dl) in [(top, dk), (base, dk + 1)] {
                        let slot = (dl * (span[1] + 1) + dj + cj) * row + di + ci;
                        let point = face[corner];
                        match named[slot] {
                            None => named[slot] = Some((point, k)),
                            Some((seen, _)) if seen == point => {}
                            Some((_, other)) if other != k => {
                                return Err(Refusal::on(
                                    "parent",
                                    format!(
                                        "`{title}` has a K gap between parent layers {} and {} and the window spans both: the child's node on that boundary would have two heights - give `k0` and `k1` on one side of it",
                                        other.min(k),
                                        other.max(k)
                                    ),
                                ));
                            }
                            Some(_) => {
                                return Err(Refusal::on(
                                    "parent",
                                    format!(
                                        "`{title}` has a split coordinate line (a fault) at pillar ({}, {}), inside the window: the child's node there would have one position per side - give a window clear of it",
                                        i + ci,
                                        j + cj
                                    ),
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
    let mut nodes = Vec::with_capacity(named.len());
    for (slot, point) in named.into_iter().enumerate() {
        let (point, _) = point.expect("every cell of the window named its eight corners");
        let at = parent.points[point as usize];
        if !at.iter().all(|c| c.is_finite()) {
            let (dl, rest) = (slot / plane, slot % plane);
            let (i, j) = (lo[0] + rest % row, lo[1] + rest / row);
            return Err(Refusal::on(
                "parent",
                format!(
                    "`{title}` has no height at pillar ({i}, {j}), level {} - a corner of a cell the window covers: a local grid cannot interpolate a cell with no location",
                    lo[2] + dl
                ),
            ));
        }
        nodes.push(at);
    }
    Ok(Window {
        span: [span[0], span[1]],
        nodes,
    })
}

/// A child node's position on one axis of the window: the low lattice index,
/// the high one and the fraction between them - `0.0` and `1.0` exact at a
/// node on the window's boundary, so it **copies** the parent's corner rather
/// than interpolating it: the window's edge nodes coincide exactly with the
/// parent's.
fn axis_node(span: usize, refine: usize, index: usize) -> (usize, usize, f64) {
    let mut cell = index / refine;
    let step = index % refine;
    let mut frac = step as f64 / refine as f64;
    if cell == span {
        // Only the very last node (`index == span * refine`) lands here: fold
        // it onto the last cell's high side, at fraction 1.0 exactly, rather
        // than reading one cell past the window.
        cell -= 1;
        frac = 1.0;
    }
    (cell, cell + 1, frac)
}

/// The window's lattice, trilinearly interpolated at a child node - the same
/// construction as `crates/energyml-geometry/src/parent.rs::interpolate`,
/// specialised to a uniform integer refinement: bilinear across the four
/// pillars around the node, then linear between the two levels. **In pillar
/// space**, which is what keeps the child watertight
/// (the geometry crate's rules, on IJK grids). A weight of exactly
/// `0.0` skips its branch, which keeps a boundary node an exact copy rather
/// than a sum with a zero term.
fn interpolate(
    window: &Window,
    i: (usize, usize, f64),
    j: (usize, usize, f64),
    k: (usize, usize, f64),
) -> [f64; 3] {
    let (li, hi, fi) = i;
    let (lj, hj, fj) = j;
    let (lk, hk, fk) = k;
    let mut out = [0.0f64; 3];
    for (level, wk) in [(lk, 1.0 - fk), (hk, fk)] {
        if wk == 0.0 {
            continue;
        }
        for (row, wj) in [(lj, 1.0 - fj), (hj, fj)] {
            if wj == 0.0 {
                continue;
            }
            for (col, wi) in [(li, 1.0 - fi), (hi, fi)] {
                let weight = wi * wj * wk;
                if weight == 0.0 {
                    continue;
                }
                let point = window.at(col, row, level);
                for (axis, value) in out.iter_mut().enumerate() {
                    *value += point[axis] * weight;
                }
            }
        }
    }
    out
}

/// Put the eight corners in VTK's order.
///
/// A **simplification** of `energyml_geometry::ijk::oriented` - private to
/// that crate, and out of reach from an operation, so not reused (the
/// alternative to writing this: leaving every child cell's winding to chance).
/// It measures one triple product rather than the signed volume over six
/// faces that crate documents as the faithful test, so it can be wrong on a
/// heavily warped cell the way that crate's own history records. It is right
/// on a regular hexahedron, which is what this operation produces on
/// reasonably-shaped parents, and it decides nothing this file's own
/// correctness depends on: the node **positions** are what the task's
/// triangulation test checks, and the true reader
/// (`energyml_geometry::parent::ijk_grid`) computes its own winding
/// independently when a package is read back.
fn oriented(node: [u32; 8], points: &[[f64; 3]]) -> [u32; 8] {
    let p = |n: u32| points[n as usize];
    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let (c0, c1, c3, c4) = (p(node[0]), p(node[1]), p(node[3]), p(node[4]));
    let (e1, e2, e3) = (sub(c1, c0), sub(c3, c0), sub(c4, c0));
    let cross = [
        e1[1] * e2[2] - e1[2] * e2[1],
        e1[2] * e2[0] - e1[0] * e2[2],
        e1[0] * e2[1] - e1[1] * e2[0],
    ];
    let dot = cross[0] * e3[0] + cross[1] * e3[1] + cross[2] * e3[2];
    if dot >= 0.0 {
        node
    } else {
        [
            node[4], node[5], node[6], node[7], node[0], node[1], node[2], node[3],
        ]
    }
}

/// A `Regrid`: no `Intervals` for a 1:1 correspondence (`Grids.xsd`: *"If there
/// is a 1:1 correspondence between the parent and child cells, then this
/// object is not needed"*), and otherwise **one interval per parent cell** -
/// *« For grid refinement, the number of parent cells in each interval is 1
/// […] so that a child cell does not span 2 (or more) parent cells, writers
/// should set the parent count per interval to 1 »* (RESQML TUG 2.2 §9.8.2,
/// p. 141-142). The two counts are then constant arrays: 1 parent and
/// `refine` children in each of `span` intervals. `ChildCellWeights` is left
/// out, so the children of a parent cell are equal (`Grids.xsd`).
fn regrid(start: usize, span: usize, refine: usize) -> Value {
    if refine == 1 {
        return json!({ "$type": "resqml22.Regrid", "InitialIndexOnParentGrid": start });
    }
    let constant =
        |value: usize| json!({ "$type": "eml23.IntegerConstantArray", "Value": value, "Count": span });
    json!({
        "$type": "resqml22.Regrid",
        "InitialIndexOnParentGrid": start,
        "Intervals": {
            "$type": "resqml22.Intervals",
            "IntervalCount": span,
            "ParentCountPerInterval": constant(1),
            "ChildCountPerInterval": constant(refine),
        },
    })
}

impl Operation for LocalGrid {
    type Params = LocalGridParams;
    type Inputs = LocalGridInputs;

    fn about(&self) -> About {
        About {
            name: "local-grid",
            describe: "a local grid - a refined window of an IJK grid of the package, its geometry derived from the parent's, grouped with it in a LocalGridSet",
            category: Category::Geometry,
            makes: &["resqml22.IjkGridRepresentation", "resqml22.LocalGridSet"],
            recipe: "local-grid",
            represents: None,
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        let Some(known) = known_of(bag, "parent") else {
            return;
        };
        if let Some(interpretation) = &known.interpretation {
            slots.inferred("interpretation", interpretation.dor(), interpretation.clone());
        }
        if let Some(reference) = &known.reference {
            slots.inferred(
                "title",
                json!(format!("{} - local grid", reference.title)),
                reference.clone(),
            );
        }
    }

    fn check(&self, inputs: &LocalGridInputs) -> Result<(), Refusal> {
        if inputs
            .parent
            .known
            .as_ref()
            .and_then(|known| known.reference.as_ref())
            .is_none()
        {
            return Err(Refusal::on(
                "parent",
                "is no object of the package: a local grid names its parent by uuid (`ParentIjkGridRepresentation`)",
            ));
        }
        parent_grid(&inputs.parent).map(|_| ())
    }

    fn plan(
        &self,
        inputs: LocalGridInputs,
        params: LocalGridParams,
        cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        self.check(&inputs)?;
        let LocalGridInputs { parent } = inputs;
        let parent_ref = parent
            .known
            .as_ref()
            .and_then(|known| known.reference.clone())
            .expect("checked above");
        let grid = parent_grid(&parent)?;

        // -- the window, checked before any computation ------------------------------------
        let (i0, i1, j0, j1) = (params.i0, params.i1, params.j0, params.j1);
        let k0 = params.k0.unwrap_or(0);
        let k1 = params.k1.unwrap_or_else(|| grid.nk.saturating_sub(1));
        let axis = |name: &'static str, lo: usize, hi: usize, extent: usize| -> Result<(), Refusal> {
            if hi < lo {
                return Err(Refusal::on(
                    name,
                    format!("ends at parent cell {hi}, before it starts at {lo}"),
                ));
            }
            if hi >= extent {
                return Err(Refusal::on(
                    name,
                    format!(
                        "names parent cell {hi}, and `{}` has {extent} cell(s) along this axis (0..{extent})",
                        title_of(&parent)
                    ),
                ));
            }
            Ok(())
        };
        axis("i1", i0, i1, grid.ni)?;
        axis("j1", j0, j1, grid.nj)?;
        axis("k1", k0, k1, grid.nk)?;
        let (span_i, span_j, span_k) = (i1 - i0 + 1, j1 - j0 + 1, k1 - k0 + 1);
        let (ri, rj, rk) = (params.ri, params.rj, params.rk);
        // The binder already refuses a refinement of zero (`Need::Integer`
        // bounds `min = 1`) on every path but a direct call to `plan`, which
        // this crate's own tests use to reach an operation without a package -
        // so the same refusal is stated here, named, rather than dividing by
        // zero further down.
        if ri == 0 || rj == 0 || rk == 0 {
            return Err(Refusal::on(
                "ri",
                "a refinement of zero would cut a parent cell into no child cell",
            ));
        }
        cx.stop_if_cancelled()?;

        // -- the window's lattice, read off the parent's own cells ---------------------------
        let window = window_of(&grid, &title_of(&parent), [i0, j0, k0], [i1, j1, k1])?;
        cx.stop_if_cancelled()?;

        // -- the child's nodes, trilinearly interpolated in the parent's lattice ------------
        let (ni, nj, nk) = (span_i * ri, span_j * rj, span_k * rk);
        let mut points = Vec::with_capacity((ni + 1) * (nj + 1) * (nk + 1));
        for ck in 0..=nk {
            let k = axis_node(span_k, rk, ck);
            for cj in 0..=nj {
                let j = axis_node(span_j, rj, cj);
                for ci in 0..=ni {
                    let i = axis_node(span_i, ri, ci);
                    points.push(interpolate(&window, i, j, k));
                }
            }
        }
        let plane = (ni + 1) * (nj + 1);
        let row = ni + 1;
        let mut cells = Vec::with_capacity(ni * nj * nk);
        for k in 0..nk {
            for j in 0..nj {
                for i in 0..ni {
                    let corner =
                        |dk: usize, dj: usize, di: usize| ((k + dk) * plane + (j + dj) * row + i + di) as u32;
                    let node = [
                        corner(0, 0, 0),
                        corner(0, 0, 1),
                        corner(0, 1, 1),
                        corner(0, 1, 0),
                        corner(1, 0, 0),
                        corner(1, 0, 1),
                        corner(1, 1, 1),
                        corner(1, 1, 0),
                    ];
                    cells.push(oriented(node, &points));
                }
            }
        }
        cx.stop_if_cancelled()?;

        // -- the objects ----------------------------------------------------------------------
        let title = params.naming.title();
        let seed = format!(
            "local-grid:{}:{i0}-{i1}:{j0}-{j1}:{k0}-{k1}:{ri}x{rj}x{rk}:{title}",
            parent_ref.uuid.to_ascii_lowercase()
        );
        let mut rank = 0usize;
        let represented = support::represented_in(
            &params.represented,
            None,
            None,
            cx.identity,
            cx.when,
            &seed,
            &title,
            &mut rank,
        )?;
        let uuid = support::next_uuid(cx.identity, &format!("child:{seed}"), &mut rank)?;
        let set_uuid = support::next_uuid(cx.identity, &format!("set:{seed}"), &mut rank)?;

        let mut child = json!({
            "$type": "resqml22.IjkGridRepresentation",
            "uuid": uuid,
            "schemaVersion": "2.2",
            "Citation": support::citation(cx.when, &title),
            "Ni": ni,
            "Nj": nj,
            "Nk": nk,
            "ParentWindow": {
                "$type": "resqml22.IjkParentWindow",
                "ParentIjkGridRepresentation": parent_ref.dor(),
                "IRegrid": regrid(i0, span_i, ri),
                "JRegrid": regrid(j0, span_j, rj),
                "KRegrid": regrid(k0, span_k, rk),
            },
        });
        if let Some(reference) = represented.reference {
            child["RepresentedObject"] = reference;
        }
        let set_title = format!("{title} - local grid set");
        let child_ref = Ref::new(uuid.clone(), "resqml22.IjkGridRepresentation", title.clone());
        let local_grid_set = json!({
            "$type": "resqml22.LocalGridSet",
            "uuid": set_uuid,
            "schemaVersion": "2.2",
            "Citation": support::citation(cx.when, &set_title),
            "ChildGrid": [child_ref.dor()],
        });

        // **The computed shape**: the nodes, trilinearly interpolated in the
        // parent - see the module doc on what is deliberately
        // left here rather than in `energyml-analysis`.
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
                        index: Vec::new(),
                    },
                    closed: Vec::new(),
                }],
            },
        );

        // -- what is said -----------------------------------------------------------------------
        let mut says = vec![Note {
            key: "window",
            detail: format!(
                "parent `{}` cells I {i0}..={i1}, J {j0}..={j1}, K {k0}..={k1} ({span_i} × {span_j} × {span_k}), refined {ri} × {rj} × {rk} → {ni} × {nj} × {nk} = {} child cell(s)",
                title_of(&parent),
                ni * nj * nk
            ),
        }];
        if params.k0.is_none() && params.k1.is_none() {
            says.push(Note {
                key: "all-k",
                detail: format!(
                    "no K window given: every one of the parent's {} layer(s)",
                    grid.nk
                ),
            });
        }
        says.push(Note {
            key: "left-out",
            detail: "no `OmitParentCells`, no `CellOverlap`: the window is the whole box, at one uniform resolution. No `Geometry` on the child - its points are the parent's, subdivided, read at need by `energyml_geometry::parent::ijk_grid` rather than stored twice".to_string(),
        });

        let mut objects = represented.objects;
        objects.push(NewObject {
            uuid: uuid.clone(),
            qualified_type: "resqml22.IjkGridRepresentation".to_string(),
            role: "representation".to_string(),
            title: title.clone(),
            json: child,
        });
        objects.push(NewObject {
            uuid: set_uuid,
            qualified_type: "resqml22.LocalGridSet".to_string(),
            role: "local-grid-set".to_string(),
            title: set_title,
            json: local_grid_set,
        });

        let double = |title: &str, value: f64| Parameter::Double {
            title: title.to_string(),
            value,
            uom: None,
        };
        Ok(Plan {
            media: Vec::new(),
            blocked: None,
            objects,
            datasets: Vec::new(),
            shapes: vec![shape],
            activity: ActivityDraft {
                recipe: "local-grid",
                title: format!("local-grid → {title}"),
                seed,
                parameters: vec![
                    double("ni", ni as f64),
                    double("nj", nj as f64),
                    double("nk", nk as f64),
                    double("cellCount", (ni * nj * nk) as f64),
                ],
            },
            says,
            dropped: Vec::new(),
        })
    }

    /// `ni`, `nj`, `nk` and `cellCount` are computed, not asked - recorded
    /// under the parameters the projection declares, the way
    /// `ijk-grid-from-horizons` reports its own counts.
    fn reports(&self) -> Vec<Slot> {
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
            count(
                "ni",
                "child cells along I - parent cells in the window, times the I refinement",
            ),
            count("nj", "child cells along J"),
            count("nk", "child cells along K"),
            count("cellCount", "child cells total - ni × nj × nk"),
        ]
    }
}
