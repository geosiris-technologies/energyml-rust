//! `blocked-wellbore` - a `BlockedWellboreRepresentation`: *« the information
//! that allows you to locate, on one or several grids (existing or planned),
//! the intersection of volume (cells) and surface (faces) elements with a
//! wellbore trajectory »* (RESQML TRG 2.2 §12.1 p. 297).
//!
//! This is the blocked-wellbore step of the reference model's build plan: two
//! blocked wellbores, one on a grid and its local refinement, the other going
//! through a fault, each with its `IntervalGridCells`. Every `IntervalGridCells.Grid`
//! (1..∞) points to the grids concerned, and the cells are those the
//! trajectory **really** crosses. Its `Trajectory` is the well's.
//!
//! # The computation is `energyml_analysis::cells::block`
//!
//! The house rule: a computation lives in `energyml-analysis`, never in an
//! operation (ADR-0017). That module intersects the path with the cells and decides the
//! nodes; this one reads the bag, turns each grid's shape into cells in index
//! order, and writes what came out.
//!
//! # The path is a wellbore frame
//!
//! A blocked wellbore *« is a well marker frame where each node typically
//! represents a limit between grid cells »* (RESQML Technical Usage Guide 2.2,
//! draft, §11.5 p. 214), and its MD *« must be consistent with the trajectory
//! representation »* (Wells.xsd, `NodeMd`). So the path is a **frame** of the
//! package: its measured depths as the package states them
//! (`Known::node_mds`, read at the boundary, never derived from the drawing),
//! its nodes placed on its trajectory by the geometry reader, and its
//! `Trajectory` the one the blocked wellbore names. Between two frame nodes
//! the path runs straight and the MD is spread along the chord: exact at every
//! node, and the most a chord falls short of its MD step is **reported**
//! (`chordExcess`) - a dense frame on a curved hole is what makes it small.
//!
//! # What is written
//!
//! - **a node at every entry and exit**, from the first entry to the last exit;
//!   an interval between two cells where the path is in none - a K gap, a hole
//!   in the grid, between two grids - is **null** (*« Null values signify that
//!   the interval is not within a grid »*, TRG §5.32 `IntervalGridCells`
//!   p. 128; *« a value of -1 is used »*, usage guide §11.5 p. 214);
//! - **`CellCount`**, *« the number of non-null entries in the grid indices
//!   array »* (same page), `GridIndices` and `CellIndices` one per interval,
//!   `LocalFacePairPerCellIndices` two per interval - *« the entry and exit
//!   intersection faces of the trajectory in the cell. Use null for missing
//!   intersections, e.g., when a trajectory originates or terminates within a
//!   cell »* - all in the external file (`Plan::datasets`), `NullValue` −1;
//! - **the local faces** of an IJK cell: *« 0=top, 1=bottom, 2-5 are side
//!   faces following the IJ circular node ordering »* (usage guide §9.12.3
//!   p. 159) - 0 the face at the smaller K, which is the top of a grid whose
//!   `KDirection` is `down` (the one `ijk-grid-from-horizons` writes), and the
//!   plan says so;
//! - **the grids in the order given**: where two hold one stretch, the first
//!   listed holds it - in the reference model the local grid is listed before its parent.
//!
//! # A cell in index order, from a shape in VTK's
//!
//! The reader hands a hexahedron in VTK's order: its two quadrilaterals
//! swapped when the cell measured inside out (`energyml_geometry::ijk::
//! oriented`), the circular order kept within each. Which quadrilateral is the
//! smaller K is read off the node numbers: a column-layer grid stores its
//! points **level by level** - *« Coordinate line nodes for all of the
//! coordinate lines, with NKL nodes per line »* in an array whose slowest
//! dimension is the level (RESQML TRG 2.2 §5.1 p. 94, and the reader) - so of
//! two corners on one coordinate line, the smaller number is the smaller K.
//!
//! # What is refused, by name
//!
//! A frame whose depths the package does not state, or that names no
//! trajectory; a grid that is no IJK grid, in another frame than the path, or
//! whose pillars lean (`energyml_analysis::cells`) - in the cells the path's box
//! reaches: the others are never read (`hexahedra_near`); a stretch two cells of one
//! grid both hold; a path through no cell - a blocked wellbore with no cell
//! could not state its `CellCount`, which is 1 or more.
//!
//! # Identity
//!
//! From what it is: the frame, the grids in order, the title. Re-gridding
//! rewrites the same object, the way a new layering of the grid does not
//! change which frame and grids a blocked wellbore joins.

use serde_json::{Value, json};

use energyml_analysis::cells::{self, CellsError, Grid, Hexahedron};
use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::{Patch, Topology};

use crate::declare::{Inputs, Params, Shape, Suggestions, known_of};
use crate::operation::{
    About, ActivityDraft, Category, Context, Dataset, Need, NewObject, Note, Numbers, Operation, Plan,
    Refusal, Slot,
};
use crate::pieces::Pieces;

use super::support;
use super::written::{NamingParams, RepresentedParams};

/// The operation.
pub struct BlockedWellbore;

/// What `blocked-wellbore` is told.
#[derive(Debug, Params)]
pub struct BlockedWellboreParams {
    /// What the blocked wellbore represents - suggested from the frame.
    #[param(flatten)]
    pub represented: RepresentedParams,
    /// The title and the file.
    #[param(flatten)]
    pub naming: NamingParams,
}

/// What `blocked-wellbore` takes.
#[derive(Debug, Inputs)]
pub struct BlockedWellboreInputs {
    /// the wellbore frame the path is - its measured depths, its nodes on the trajectory it names
    #[input(topology = "polylines", types = "resqml22.WellboreFrameRepresentation")]
    pub frame: Shape,
    /// the IJK grids the path is blocked in, in order: where two hold one stretch, the first listed holds it - a local grid before its parent
    #[input(types = "resqml22.IjkGridRepresentation", least = 1, ordered)]
    pub grids: Vec<Shape>,
}

/// The path's box, grown a hair: a cell whose box misses it is crossed by no
/// segment of the path.
fn path_box(path: &[[f64; 3]]) -> ([f64; 3], [f64; 3]) {
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for p in path {
        for axis in 0..3 {
            low[axis] = low[axis].min(p[axis]);
            high[axis] = high[axis].max(p[axis]);
        }
    }
    let far = low.iter().chain(high.iter()).fold(0.0f64, |m, v| m.max(v.abs()));
    let pad = 1e-9 * (1.0 + far);
    (low.map(|v| v - pad), high.map(|v| v + pad))
}

/// The cells of a hexahedral topology that the path's box can touch, in index
/// order - see the module header.
///
/// A cell is judged by its box, read from its eight corners **before** a
/// [`Hexahedron`] is built for it: a reservoir grid of ten million cells would
/// otherwise be copied corner by corner (about 200 bytes a cell, on every plan)
/// to follow a wellbore through a few hundred. A cell whose box misses the path's
/// is crossed by no segment of it - `cells::block` skips it by the same box test -
/// so what is kept gives the same blocking; what is left out is never read, so a
/// leaning pillar nobody's path comes near refuses nothing.
fn hexahedra_near(
    cells: &[[u32; 8]],
    index: &[u32],
    points: &[[f64; 3]],
    path: &[[f64; 3]],
) -> Result<Vec<Hexahedron>, String> {
    if !index.is_empty() && index.len() != cells.len() {
        return Err(format!(
            "{} cell(s) and {} cell number(s) - one number per cell, or none",
            cells.len(),
            index.len()
        ));
    }
    let (low, high) = path_box(path);
    let mut out = Vec::new();
    for (at, cell) in cells.iter().enumerate() {
        let number = if index.is_empty() {
            at as u64
        } else {
            u64::from(index[at])
        };
        // The smaller node number is the smaller K: VTK's swap undone.
        let order: [u32; 8] = if cell[0] > cell[4] {
            [
                cell[4], cell[5], cell[6], cell[7], cell[0], cell[1], cell[2], cell[3],
            ]
        } else {
            *cell
        };
        let mut reach = [[f64::INFINITY, f64::NEG_INFINITY]; 3];
        for n in order {
            let Some(p) = points.get(n as usize) else {
                return Err(format!(
                    "cell {number} names node {n} where the grid has {}",
                    points.len()
                ));
            };
            for axis in 0..3 {
                reach[axis][0] = reach[axis][0].min(p[axis]);
                reach[axis][1] = reach[axis][1].max(p[axis]);
            }
        }
        if (0..3).any(|axis| high[axis] < reach[axis][0] || low[axis] > reach[axis][1]) {
            continue;
        }
        out.push(Hexahedron {
            index: number,
            corners: order.map(|n| points[n as usize]),
        });
    }
    Ok(out)
}

/// A grid's cells in index order, those near `path` - see the module header.
fn grid_of(shape: &Shape, path: &[[f64; 3]]) -> Result<(Grid, usize), Refusal> {
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
            "grids",
            format!(
                "`{}` is no IJK grid: a blocked wellbore is blocked in the cells of one",
                shape.title()
            ),
        ));
    };
    let out = hexahedra_near(cells, index, points, path)
        .map_err(|why| Refusal::on("grids", format!("`{}`: {why}", shape.title())))?;
    let [ni, nj, nk] = *dimensions;
    let undefined = (ni * nj * nk).saturating_sub(cells.len());
    Ok((Grid { cells: out }, undefined))
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

impl Operation for BlockedWellbore {
    type Params = BlockedWellboreParams;
    type Inputs = BlockedWellboreInputs;

    fn about(&self) -> About {
        About {
            name: "blocked-wellbore",
            describe: "a blocked wellbore - the cells of IJK grids a wellbore frame crosses, with the MD of each entry and exit and the faces it goes through",
            category: Category::Wells,
            makes: &["resqml22.BlockedWellboreRepresentation"],
            recipe: "blocked-wellbore",
            represents: None,
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        let Some(known) = known_of(bag, "frame") else {
            return;
        };
        // What the frame represents: the same wellbore.
        if let Some(interpretation) = &known.interpretation {
            slots.inferred("interpretation", interpretation.dor(), interpretation.clone());
        }
        if let Some(frame) = &known.reference {
            slots.inferred(
                "title",
                json!(format!("{} - blocked", frame.title)),
                frame.clone(),
            );
        }
    }

    fn check(&self, inputs: &BlockedWellboreInputs) -> Result<(), Refusal> {
        let no_reference = |shape: &Shape, slot: &str| {
            Refusal::on(
                slot,
                format!(
                    "`{}` is no object of the package: a blocked wellbore names it by uuid, and an artefact has none",
                    shape.name
                ),
            )
        };
        if inputs
            .frame
            .known
            .as_ref()
            .and_then(|known| known.reference.as_ref())
            .is_none()
        {
            return Err(no_reference(&inputs.frame, "frame"));
        }
        if let Some(grid) = inputs.grids.iter().find(|grid| {
            grid.known
                .as_ref()
                .and_then(|known| known.reference.as_ref())
                .is_none()
        }) {
            return Err(no_reference(grid, "grids"));
        }
        Ok(())
    }

    fn plan(
        &self,
        inputs: BlockedWellboreInputs,
        params: BlockedWellboreParams,
        cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        self.check(&inputs)?;
        let BlockedWellboreInputs { frame, grids } = inputs;
        let known = frame.known.clone().unwrap_or_default();
        let Some(frame_ref) = known.reference.clone() else {
            return Err(Refusal::on("frame", "names no object of the package"));
        };

        // -- the path: the frame's nodes and the depths the package states ----------------
        let Some(mds) = known.node_mds.clone() else {
            return Err(Refusal::on(
                "frame",
                format!(
                    "`{}`: its measured depths could not be read - a blocked wellbore's MD are the frame's, never derived from its drawing",
                    frame_ref.title
                ),
            ));
        };
        let Some(trajectory) = known.trajectory.clone() else {
            return Err(Refusal::on(
                "frame",
                format!(
                    "`{}` names no trajectory, and a blocked wellbore names the frame's",
                    frame_ref.title
                ),
            ));
        };
        let points: Vec<[f64; 3]> = match frame.geometry.patches.as_slice() {
            [
                Patch {
                    topology: Topology::Polylines(runs),
                    points,
                    ..
                },
            ] if runs.len() == 1 => points.clone(),
            _ => {
                return Err(Refusal::on(
                    "frame",
                    format!(
                        "`{}` is not one line of nodes: a frame is drawn as one polyline, top to bottom",
                        frame_ref.title
                    ),
                ));
            }
        };
        if points.len() != mds.len() {
            return Err(Refusal::on(
                "frame",
                format!(
                    "`{}` places {} node(s) and states {} measured depth(s)",
                    frame_ref.title,
                    points.len(),
                    mds.len()
                ),
            ));
        }

        // -- the grids, in the path's frame ------------------------------------------------
        let mut blocks = Vec::with_capacity(grids.len());
        let mut undefined = Vec::with_capacity(grids.len());
        for grid in &grids {
            if let (Some(a), Some(b)) = (
                known.crs.as_ref(),
                grid.known.as_ref().and_then(|k| k.crs.as_ref()),
            ) && !a.uuid.eq_ignore_ascii_case(&b.uuid)
            {
                return Err(Refusal::new(
                    "frames",
                    format!(
                        "`{}` is in `{}` and `{}` in `{}`: a path and cells read in two frames meet where neither is - bring them into one frame first",
                        frame_ref.title,
                        a.title,
                        grid.title(),
                        b.title
                    ),
                ));
            }
            let (cells, holes) = grid_of(grid, &points)?;
            blocks.push(cells);
            undefined.push(holes);
        }
        cx.stop_if_cancelled()?;
        let blocked = cells::block(&points, &mds, &blocks).map_err(|why| {
            let slot = match &why {
                CellsError::Leaning { .. }
                | CellsError::NotParallelogram { .. }
                | CellsError::Shared { .. }
                | CellsError::NoCell => "grids",
                _ => "frame",
            };
            let named = match &why {
                CellsError::Leaning { grid, .. }
                | CellsError::NotParallelogram { grid, .. }
                | CellsError::Shared { grid, .. } => {
                    format!(" - grid {grid} is `{}`", grids[*grid].title())
                }
                _ => String::new(),
            };
            Refusal::on(slot, format!("{why}{named}"))
        })?;
        cx.stop_if_cancelled()?;

        // -- the object ------------------------------------------------------------------------
        let title = params.naming.title();
        let file = params.naming.data_file.clone();
        let grid_refs: Vec<_> = grids
            .iter()
            .filter_map(|grid| grid.known.as_ref().and_then(|k| k.reference.clone()))
            .collect();
        let seed = format!(
            "blocked-wellbore:{}:{}:{title}",
            frame_ref.uuid.to_ascii_lowercase(),
            grid_refs
                .iter()
                .map(|one| one.uuid.to_ascii_lowercase())
                .collect::<Vec<_>>()
                .join(",")
        );
        let mut rank = 0usize;
        let uuid = support::next_uuid(cx.identity, &seed, &mut rank)?;
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

        let nodes = blocked.node_md.len();
        let intervals = blocked.intervals.len();
        let null = -1i64;
        let grid_indices: Vec<i64> = blocked
            .intervals
            .iter()
            .map(|one| one.map_or(null, |held| held.grid as i64))
            .collect();
        let cell_indices: Vec<i64> = blocked
            .intervals
            .iter()
            .map(|one| one.map_or(null, |held| held.cell as i64))
            .collect();
        let faces: Vec<i64> = blocked
            .intervals
            .iter()
            .flat_map(|one| match one {
                Some(held) => held.faces.map(|face| face.map_or(null, i64::from)),
                None => [null, null],
            })
            .collect();
        let path = |name: &str| format!("/resqml22/{uuid}/{name}");
        let datasets = vec![
            Dataset {
                file: file.clone(),
                path: path("nodeMd"),
                dimensions: vec![nodes as i64],
                values: Numbers::F64(blocked.node_md.clone()),
            },
            Dataset {
                file: file.clone(),
                path: path("gridIndices"),
                dimensions: vec![intervals as i64],
                values: Numbers::I64(grid_indices),
            },
            Dataset {
                file: file.clone(),
                path: path("cellIndices"),
                dimensions: vec![intervals as i64],
                values: Numbers::I64(cell_indices),
            },
            Dataset {
                file: file.clone(),
                path: path("localFacePairPerCellIndices"),
                dimensions: vec![intervals as i64, 2],
                values: Numbers::I64(faces),
            },
        ];
        let mut object = json!({
            "$type": "resqml22.BlockedWellboreRepresentation",
            "uuid": uuid,
            "schemaVersion": "2.2",
            "Citation": support::citation(cx.when, &title),
            "NodeCount": nodes,
            "NodeMd": {
                "$type": "eml23.FloatingPointExternalArray",
                "ArrayFloatingPointType": "arrayOfDouble64LE",
                "CountPerValue": 1,
                "Values": support::external(&file, &path("nodeMd"), &[nodes as i64]),
            },
            "Trajectory": trajectory.dor(),
            "IntervalGridCells": {
                "$type": "resqml22.IntervalGridCells",
                "CellCount": blocked.cell_count(),
                "GridIndices": integers(&file, &path("gridIndices"), &[intervals as i64]),
                "CellIndices": integers(&file, &path("cellIndices"), &[intervals as i64]),
                "LocalFacePairPerCellIndices": integers(&file, &path("localFacePairPerCellIndices"), &[intervals as i64, 2]),
                "Grid": grid_refs.iter().map(|one| one.dor()).collect::<Vec<_>>(),
            },
        });
        if let Some(reference) = represented.reference {
            object["RepresentedObject"] = reference;
        }

        // **The computed shape**: the nodes, where the path places them.
        let placed: Vec<[f64; 3]> = cells::positions(&points, &mds, &blocked.node_md)
            .into_iter()
            .flatten()
            .collect();
        let count = placed.len() as u32;
        let shape = support::made_root(
            &title,
            &uuid,
            support::one_patch(
                "BlockedWellboreRepresentation",
                placed,
                Topology::Polylines(vec![count]),
            ),
        );

        // -- what is said ------------------------------------------------------------------------
        let outside = intervals - blocked.cell_count();
        let mut says = vec![
            Note {
                key: "blocked",
                detail: format!(
                    "`{}` from MD {} to {}: {} interval(s), {} in a cell of {} grid(s)",
                    frame_ref.title,
                    blocked.node_md[0],
                    blocked.node_md[nodes - 1],
                    intervals,
                    blocked.cell_count(),
                    grids.len()
                ),
            },
            Note {
                key: "faces",
                detail: "local faces: 0 at the smaller K, 1 at the larger - top and bottom for a KDirection down - and 2-5 the sides after the IJ circular node order (0,0) (1,0) (1,1) (0,1) (RESQML Technical Usage Guide 2.2 §9.12.3 p. 159, Figure 9-18 p. 152); −1 where the path starts or ends in a cell, or leaves a grid listed earlier".to_string(),
            },
            Note {
                key: "chords",
                detail: format!(
                    "the MD is spread along the straight chord between two frame nodes, exact at each: the largest shortfall of a chord against its MD step is {:.3} %",
                    100.0 * blocked.chord_excess
                ),
            },
        ];
        if outside > 0 {
            says.push(Note {
                key: "outside",
                detail: format!("{outside} interval(s) between two cells are in none - a K gap, an undefined cell, the space between two grids: written null (−1)"),
            });
        }
        for (grid, holes) in grids.iter().zip(&undefined) {
            if *holes > 0 {
                says.push(Note {
                    key: "undefined-cells",
                    detail: format!(
                        "`{}` leaves {holes} cell(s) without geometry: the path there is in no cell",
                        grid.title()
                    ),
                });
            }
        }
        if grids.len() > 1 {
            says.push(Note {
                key: "grid-order",
                detail: format!(
                    "where two grids hold one stretch the first listed holds it: {}",
                    grids
                        .iter()
                        .map(|grid| format!("`{}`", grid.title()))
                        .collect::<Vec<_>>()
                        .join(" before ")
                ),
            });
        }
        says.push(Note {
            key: "left-out",
            detail:
                "no `IntervalStratigraphicUnits`, no `CellFluidPhaseUnits`: only the cells and their faces"
                    .to_string(),
        });

        let mut objects = represented.objects;
        objects.push(NewObject {
            uuid,
            qualified_type: "resqml22.BlockedWellboreRepresentation".to_string(),
            role: "representation".to_string(),
            title: title.clone(),
            json: object,
        });
        let number = |title: &str, value: f64| Parameter::Double {
            title: title.to_string(),
            value,
            uom: None,
        };
        Ok(Plan {
            media: Vec::new(),
            blocked: None,
            objects,
            datasets,
            activity: ActivityDraft {
                recipe: "blocked-wellbore",
                title: format!("blocked-wellbore → {title}"),
                seed,
                parameters: vec![
                    number("nodeCount", nodes as f64),
                    number("cellCount", blocked.cell_count() as f64),
                    number("outsideIntervalCount", outside as f64),
                    number("chordExcess", blocked.chord_excess),
                ],
            },
            says,
            dropped: Vec::new(),
            shapes: vec![shape],
        })
    }

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
            count("nodeCount", "nodes - an entry or an exit each"),
            count("cellCount", "intervals in a cell - CellCount"),
            count("outsideIntervalCount", "intervals in no cell, written null"),
            count(
                "chordExcess",
                "the largest shortfall of a chord against its MD step, as a fraction",
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A lattice of `n x n x 2` unit cells, smaller K first, its points level by
    /// level, cells numbered by position.
    fn lattice(n: usize) -> (Vec<[u32; 8]>, Vec<[f64; 3]>) {
        let side = n + 1;
        let mut points = Vec::new();
        for k in 0..3 {
            for j in 0..side {
                for i in 0..side {
                    points.push([i as f64, j as f64, -(k as f64)]);
                }
            }
        }
        let node = |i: usize, j: usize, k: usize| (k * side * side + j * side + i) as u32;
        let mut cells = Vec::new();
        for k in 0..2 {
            for j in 0..n {
                for i in 0..n {
                    cells.push([
                        node(i, j, k),
                        node(i + 1, j, k),
                        node(i + 1, j + 1, k),
                        node(i, j + 1, k),
                        node(i, j, k + 1),
                        node(i + 1, j, k + 1),
                        node(i + 1, j + 1, k + 1),
                        node(i, j + 1, k + 1),
                    ]);
                }
            }
        }
        (cells, points)
    }

    /// [build-ops-1 S4] Only the cells the path's box reaches become hexahedra: a
    /// short path in a lattice of 20 000 cells keeps a handful, in index order, and
    /// they are the cells a scan of every cell with the same box test keeps. A
    /// missing node number or a short index array is refused by name.
    #[test]
    fn only_the_cells_near_the_path_are_built() {
        let (cells, points) = lattice(100);
        let path = [[10.5, 20.5, 0.5], [12.5, 22.5, -1.5]];
        let kept = hexahedra_near(&cells, &[], &points, &path).unwrap();
        assert!(!kept.is_empty() && kept.len() < 100, "{} cells kept", kept.len());
        assert!(kept.windows(2).all(|pair| pair[0].index < pair[1].index));
        let (low, high) = path_box(&path);
        let by_scan: Vec<u64> = (0..cells.len() as u64)
            .filter(|&at| {
                let c = &cells[at as usize];
                (0..3).all(|axis| {
                    let lo = c
                        .iter()
                        .map(|&n| points[n as usize][axis])
                        .fold(f64::INFINITY, f64::min);
                    let hi = c
                        .iter()
                        .map(|&n| points[n as usize][axis])
                        .fold(f64::NEG_INFINITY, f64::max);
                    hi >= low[axis] && lo <= high[axis]
                })
            })
            .collect();
        assert_eq!(kept.iter().map(|h| h.index).collect::<Vec<_>>(), by_scan);
        // The cell the middle of the path is in is among them.
        let middle = [11.5, 21.5, -0.5];
        let holds = |h: &&Hexahedron| {
            (0..3).all(|axis| {
                let lo = h.corners.iter().map(|c| c[axis]).fold(f64::INFINITY, f64::min);
                let hi = h
                    .corners
                    .iter()
                    .map(|c| c[axis])
                    .fold(f64::NEG_INFINITY, f64::max);
                lo <= middle[axis] && middle[axis] <= hi
            })
        };
        assert!(kept.iter().any(|h| holds(&h)));
        assert!(
            hexahedra_near(&cells, &[0, 1], &points, &path)
                .unwrap_err()
                .contains("one number per cell")
        );
        let mut broken = cells.clone();
        broken[0][3] = 1_000_000;
        assert!(
            hexahedra_near(&broken, &[], &points, &[[0.5, 0.5, 0.5]])
                .unwrap_err()
                .contains("names node 1000000")
        );
        // Break it: build every cell before the box test and `kept.len()` is 20 000.
    }
}
