//! `blocked-wellbore` — a `BlockedWellboreRepresentation`: *« the information
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
//! (`chordExcess`) — a dense frame on a curved hole is what makes it small.
//!
//! # What is written
//!
//! - **a node at every entry and exit**, from the first entry to the last exit;
//!   an interval between two cells where the path is in none — a K gap, a hole
//!   in the grid, between two grids — is **null** (*« Null values signify that
//!   the interval is not within a grid »*, TRG §5.32 `IntervalGridCells`
//!   p. 128; *« a value of -1 is used »*, usage guide §11.5 p. 214);
//! - **`CellCount`**, *« the number of non-null entries in the grid indices
//!   array »* (same page), `GridIndices` and `CellIndices` one per interval,
//!   `LocalFacePairPerCellIndices` two per interval — *« the entry and exit
//!   intersection faces of the trajectory in the cell. Use null for missing
//!   intersections, e.g., when a trajectory originates or terminates within a
//!   cell »* — all in the external file (`Plan::datasets`), `NullValue` −1;
//! - **the local faces** of an IJK cell: *« 0=top, 1=bottom, 2-5 are side
//!   faces following the IJ circular node ordering »* (usage guide §9.12.3
//!   p. 159) — 0 the face at the smaller K, which is the top of a grid whose
//!   `KDirection` is `down` (the one `ijk-grid-from-horizons` writes), and the
//!   plan says so;
//! - **the grids in the order given**: where two hold one stretch, the first
//!   listed holds it — in the reference model the local grid is listed before its parent.
//!
//! # A cell in index order, from a shape in VTK's
//!
//! The reader hands a hexahedron in VTK's order: its two quadrilaterals
//! swapped when the cell measured inside out (`energyml_geometry::ijk::
//! oriented`), the circular order kept within each. Which quadrilateral is the
//! smaller K is read off the node numbers: a column-layer grid stores its
//! points **level by level** — *« Coordinate line nodes for all of the
//! coordinate lines, with NKL nodes per line »* in an array whose slowest
//! dimension is the level (RESQML TRG 2.2 §5.1 p. 94, and the reader) — so of
//! two corners on one coordinate line, the smaller number is the smaller K.
//!
//! # What is refused, by name
//!
//! A frame whose depths the package does not state, or that names no
//! trajectory; a grid that is no IJK grid, in another frame than the path, or
//! whose pillars lean (`energyml_analysis::cells`); a stretch two cells of one
//! grid both hold; a path through no cell — a blocked wellbore with no cell
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
    /// What the blocked wellbore represents — suggested from the frame.
    #[param(flatten)]
    pub represented: RepresentedParams,
    /// The title and the file.
    #[param(flatten)]
    pub naming: NamingParams,
}

/// What `blocked-wellbore` takes.
#[derive(Debug, Inputs)]
pub struct BlockedWellboreInputs {
    /// the wellbore frame the path is — its measured depths, its nodes on the trajectory it names
    #[input(topology = "polylines", types = "resqml22.WellboreFrameRepresentation")]
    pub frame: Shape,
    /// the IJK grids the path is blocked in, in order: where two hold one stretch, the first listed holds it — a local grid before its parent
    #[input(types = "resqml22.IjkGridRepresentation", least = 1, ordered)]
    pub grids: Vec<Shape>,
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

/// A grid's cells in index order — see the module header.
fn grid_of(shape: &Shape) -> Result<(Grid, usize), Refusal> {
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
                title_of(shape)
            ),
        ));
    };
    let mut out = Vec::with_capacity(cells.len());
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
        out.push(Hexahedron {
            index: number,
            corners: order.map(|n| points[n as usize]),
        });
    }
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
            describe: "a blocked wellbore — the cells of IJK grids a wellbore frame crosses, with the MD of each entry and exit and the faces it goes through",
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
                json!(format!("{} — blocked", frame.title)),
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
                    "`{}`: its measured depths could not be read — a blocked wellbore's MD are the frame's, never derived from its drawing",
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
                        "`{}` is in `{}` and `{}` in `{}`: a path and cells read in two frames meet where neither is — bring them into one frame first",
                        frame_ref.title,
                        a.title,
                        title_of(grid),
                        b.title
                    ),
                ));
            }
            let (cells, holes) = grid_of(grid)?;
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
                    format!(" — grid {grid} is `{}`", title_of(&grids[*grid]))
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
                detail: "local faces: 0 at the smaller K, 1 at the larger — top and bottom for a KDirection down — and 2–5 the sides after the IJ circular node order (0,0) (1,0) (1,1) (0,1) (RESQML Technical Usage Guide 2.2 §9.12.3 p. 159, Figure 9-18 p. 152); −1 where the path starts or ends in a cell, or leaves a grid listed earlier".to_string(),
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
                detail: format!("{outside} interval(s) between two cells are in none — a K gap, an undefined cell, the space between two grids: written null (−1)"),
            });
        }
        for (grid, holes) in grids.iter().zip(&undefined) {
            if *holes > 0 {
                says.push(Note {
                    key: "undefined-cells",
                    detail: format!(
                        "`{}` leaves {holes} cell(s) without geometry: the path there is in no cell",
                        title_of(grid)
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
                        .map(|grid| format!("`{}`", title_of(grid)))
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
            count("nodeCount", "nodes — an entry or an exit each"),
            count("cellCount", "intervals in a cell — CellCount"),
            count("outsideIntervalCount", "intervals in no cell, written null"),
            count(
                "chordExcess",
                "the largest shortfall of a chord against its MD step, as a fraction",
            ),
        ]
    }
}
