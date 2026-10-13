//! `faulted-ijk-grid` - an `IjkGridRepresentation` between ordered horizon
//! grids, **cut by faults**: split coordinate lines where a fault runs between
//! two columns, the hanging wall moved by a stated throw, and the cells nearest
//! a fault removed on request.
//!
//! The need: the IJK grids of the reference model are too regular, and at
//! least one should be "cut" by the faults - perhaps a new one, displaced a
//! little downwards on one side, with cells removed near one or two faults. It
//! is also what the reference model's main grid asked for from the start:
//! 21 columns duplicated along one fault (`ColumnLayerSplitCoordinateLines`).
//!
//! # One writer, two operations
//!
//! The grid between the horizons is `ijk-grid-from-horizons`'s, and this
//! operation does not write a second one: it takes the same horizons and the
//! same settings (`#[param(flatten)]`), hands `super::ijk_grid::between` a
//! cut, and that function writes the object - an operation that writes the same
//! object as another delegates to it (ADR-0017). The cut
//! itself is `energyml_analysis::ijk_faults`, which holds the rules and their
//! pages: where a fault runs (its trace at the middle of the grid's heights, a
//! vertical staircase along the column faces), which side goes down (the side
//! it dips to, measured), by how much (`fault-throw`'s law, so the grid and the
//! torn horizons say the same throw), which pillars split and which
//! columns take each line.
//!
//! # What the grid represents - never nothing
//!
//! A grid is a representation of the model: *« Software G mainly uses the 3D
//! grid representation, which represents Peter's earth model interpretation »*
//! (RESQML Technical Usage Guide 2.2, draft, §7.3.2.2 p. 104), and what a
//! representation represents *« is either an interpretation or a technical
//! feature »* (RESQML TRG 2.2 §8.1 `AbstractRepresentation` p. 234). The schema
//! lets `RepresentedObject` be absent; this operation does not: `interpretation`
//! names one (the reference model's `EarthModelInterpretation`), or `featureType` and
//! `interpretationType` make the pair - otherwise the grid is refused by name.
//! Five representations of the reference model once represented nothing; this one is not a sixth.
//!
//! # A synthetic throw says so
//!
//! With `synthetic`, a `source` is required, and the grid carries `synthetic =
//! true` and the law with its source in `Citation/Description`
//! (the reference-model rules on invented objects, ADR-0019) - the throws of the reference model are invented.
//!
//! # Identity
//!
//! The grid between the same horizons' seed, then the faults (their uuid, or
//! their name in the bag), the throws, the senses, the law, the reach and the
//! removed cells, then the title: another throw is another grid, since the
//! nodes a property was laid on would have moved.
//!
//! # References
//!
//! - The throw law along a fault trace is a synthetic law stated in `energyml-analysis`, which cites no published source for it.
//!   TODO(reference): verify - no publication is cited; if the law follows one, add it (authors, year, title, venue, DOI) once checked.

use serde_json::json;

use energyml_analysis::fault_throw::Sense;
use energyml_analysis::ijk_faults::{self, CutError, Fault, Settings};
use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::Topology;

use crate::declare::{Inputs, Params, Shape, Suggestions};
use crate::operation::{About, Category, Context, Need, Note, Operation, Plan, Refusal, Slot};
use crate::pieces::Pieces;

use super::fault::Throw;
use super::fault_throw::Profile;
use super::ijk_grid::{self, Cut, Faulting, IjkGridParams};
use super::support;

/// The operation.
pub struct FaultedIjkGrid;

/// What `faulted-ijk-grid` is told.
#[derive(Debug, Params)]
pub struct FaultedGridParams {
    /// The grid between the horizons: its layers, its pillars, its frame.
    #[param(flatten)]
    pub grid: IjkGridParams,
    /// the largest throw of each fault, in metres, in the order of `faults` - reached on the fault where the profile is one
    #[param(least = 1, min = 0.0, uom = "m")]
    pub maximum_throw: Vec<f64>,
    /// how each fault moved, in the order of `faults` - normal lowers its hanging wall, reverse or thrust raises it
    #[param(least = 1)]
    pub throw: Vec<Throw>,
    /// how the throw varies along each trace - zero at both ends, the maximum in the middle
    ///
    /// No default: a law of throw is a statement about the fault.
    pub profile: Profile,
    /// how far from the trace, in plan, the hanging wall is dragged - the offset falls linearly from the throw on the fault to zero at this distance, in metres
    #[param(min = 0.0, uom = "m")]
    pub reach: f64,
    /// how many columns of each fault's hanging wall, counted from the fault, lose their cells, in the order of `faults` - 0 keeps them; left out, none do
    pub dead_columns: Vec<usize>,
    /// the layers, from 0 at the top, in which those columns lose their cells - left out, every layer
    pub dead_layers: Vec<usize>,
    /// the throw is invented: the grid carries `synthetic = true` and its source
    #[param(default = false, optional)]
    pub synthetic: bool,
    /// where the throw comes from - a document with its page, or why an invented one is plausible; required for a synthetic one
    pub source: Option<String>,
}

/// What `faulted-ijk-grid` takes.
#[derive(Debug, Inputs)]
pub struct FaultedGridInputs {
    /// the horizons the grid lies between, **top to bottom** - 2D grids on one lattice, one per input
    #[input(topology = "grid", least = 2, ordered)]
    pub horizons: Vec<Shape>,
    /// the fault surfaces that cut it, triangulated - in the order `maximumThrow`, `throw` and `deadColumns` follow
    #[input(topology = "triangles", least = 1, ordered)]
    pub faults: Vec<Shape>,
}

/// A count or a length, as the activity records it.
fn double(title: &str, value: f64, uom: Option<&str>) -> Parameter {
    Parameter::Double {
        title: title.to_string(),
        value,
        uom: uom.map(str::to_string),
    }
}

/// What names a shape in a message: its object's title, or its name in the bag.
fn name_of(shape: &Shape) -> String {
    shape
        .known
        .as_ref()
        .and_then(|known| known.reference.as_ref())
        .map(|one| one.title.clone())
        .unwrap_or_else(|| shape.name.clone())
}

/// The key a fault enters the seed by: its uuid, or its name in the bag.
fn key_of(shape: &Shape) -> String {
    shape
        .known
        .as_ref()
        .and_then(|known| known.reference.as_ref())
        .map(|one| one.uuid.to_ascii_lowercase())
        .unwrap_or_else(|| shape.name.clone())
}

/// A triangulated surface: its nodes and its triangles.
type Surface = (Vec<[f64; 3]>, Vec<[u32; 3]>);

/// A shape's triangles, every patch's indices shifted to the whole shape's
/// nodes - or `None` when it holds none. F1 is in two patches.
fn surface_of(shape: &Shape) -> Option<Surface> {
    let mut points = Vec::new();
    let mut triangles = Vec::new();
    for patch in &shape.geometry.patches {
        let Topology::Triangles(faces) = &patch.topology else {
            continue;
        };
        let base = points.len() as u32;
        points.extend(patch.points.iter().copied());
        triangles.extend(faces.iter().map(|f| [f[0] + base, f[1] + base, f[2] + base]));
    }
    (!triangles.is_empty()).then_some((points, triangles))
}

/// The slot a refusal of the computation belongs to.
fn refusal(why: CutError) -> Refusal {
    let slot = match why {
        CutError::BadThrow { .. } => "maximumThrow",
        CutError::BadReach { .. } => "reach",
        CutError::DeadLayer { .. } => "deadLayers",
        CutError::NothingLeft { .. } => "deadColumns",
        CutError::Overturned { .. } => "throw",
        CutError::Pillars { .. } => "horizons",
        _ => "faults",
    };
    Refusal::on(slot, why)
}

impl Operation for FaultedIjkGrid {
    type Params = FaultedGridParams;
    type Inputs = FaultedGridInputs;

    fn about(&self) -> About {
        About {
            name: "faulted-ijk-grid",
            describe: "an IJK grid between horizon grids given top to bottom, cut by fault surfaces - split pillars where a fault runs between two columns, the hanging wall moved by a stated throw, the cells next to a fault removed on request",
            category: Category::Geometry,
            makes: &["resqml22.IjkGridRepresentation"],
            recipe: "faulted-ijk-grid",
            represents: None,
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        ijk_grid::suggest_grid(bag, slots);
    }

    /// The horizons on one lattice, the faults surfaces.
    fn check(&self, inputs: &FaultedGridInputs) -> Result<(), Refusal> {
        ijk_grid::check_horizons(&inputs.horizons)?;
        for fault in &inputs.faults {
            if surface_of(fault).is_none() {
                return Err(Refusal::on(
                    "faults",
                    format!(
                        "`{}` is not a triangulated surface - where a fault runs through the grid is read on its surface",
                        name_of(fault)
                    ),
                ));
            }
        }
        Ok(())
    }

    fn plan(
        &self,
        inputs: FaultedGridInputs,
        params: FaultedGridParams,
        cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        let FaultedGridInputs { horizons, faults } = inputs;
        let count = faults.len();
        let names: Vec<String> = faults.iter().map(name_of).collect();

        // -- one entry per fault -------------------------------------------------------
        if params.maximum_throw.len() != count {
            return Err(Refusal::on(
                "maximumThrow",
                format!(
                    "{} throw(s) for {count} fault(s) - one per fault, in the order of `faults`",
                    params.maximum_throw.len()
                ),
            ));
        }
        if params.throw.len() != count {
            return Err(Refusal::on(
                "throw",
                format!(
                    "{} kind(s) of throw for {count} fault(s) - one per fault, in the order of `faults`",
                    params.throw.len()
                ),
            ));
        }
        if !params.dead_columns.is_empty() && params.dead_columns.len() != count {
            return Err(Refusal::on(
                "deadColumns",
                format!(
                    "{} count(s) for {count} fault(s) - one per fault, in the order of `faults`, or none",
                    params.dead_columns.len()
                ),
            ));
        }
        let mut senses: Vec<Sense> = Vec::with_capacity(count);
        for (one, name) in params.throw.iter().zip(&names) {
            senses.push(match one {
                Throw::Normal => Sense::Down,
                Throw::Reverse | Throw::Thrust => Sense::Up,
                other => {
                    return Err(Refusal::on(
                        "throw",
                        format!(
                            "`{name}` is given as {}: that play states no vertical sense, and which way its hanging wall goes is what the grid is cut by - give normal, reverse or thrust",
                            other.word()
                        ),
                    ));
                }
            });
        }
        let source = params
            .source
            .as_deref()
            .map(str::trim)
            .filter(|one| !one.is_empty())
            .map(str::to_string);
        if params.synthetic && source.is_none() {
            return Err(Refusal::on(
                "source",
                "a synthetic throw says where its numbers come from - a document with its page, or why they are plausible (reference-model rule: no constant without a source)",
            ));
        }

        // -- what the grid represents: never nothing ----------------------------------------
        let written = &params.grid.written;
        if written.represented.interpretation.is_none() && written.new_feature.feature_type.is_none() {
            return Err(Refusal::on(
                "interpretation",
                "a faulted grid represents the model it discretises - *« the 3D grid representation, which represents Peter's earth model interpretation »* (RESQML Technical Usage Guide 2.2, §7.3.2.2 p. 104): name the interpretation (an `EarthModelInterpretation`), or choose `featureType` and `interpretationType` to create the pair",
            ));
        }

        // -- one frame: the faults' is the horizons' -----------------------------------------
        let horizon_frame = horizons
            .first()
            .and_then(|shape| shape.known.as_ref())
            .and_then(|known| known.crs.clone());
        for (fault, name) in faults.iter().zip(&names) {
            if let (Some(own), Some(theirs)) = (
                &horizon_frame,
                fault.known.as_ref().and_then(|known| known.crs.as_ref()),
            ) && !own.uuid.eq_ignore_ascii_case(&theirs.uuid)
            {
                return Err(Refusal::on(
                    "faults",
                    format!(
                        "`{name}` is in `{}` and the horizons in `{}` - a fault read across two frames is read across two origins",
                        theirs.title, own.title
                    ),
                ));
            }
        }

        // -- the law, said once -----------------------------------------------------------------
        let law = format!(
            "throw(s) = maximum · {}, s along the fault's trace from 0 to 1; the hanging wall dragged over {} m, the offset falling linearly to zero there, straight down the vertical pillars",
            params.profile.law(),
            params.reach
        );
        let description = source.as_ref().map(|source| {
            if params.synthetic {
                format!("Faulted grid, synthetic throw, not measured: {law}. Source: {source}")
            } else {
                format!("Faulted grid. Throw: {law}. Source: {source}")
            }
        });
        if let Some(text) = &description
            && text.chars().count() > 2000
        {
            return Err(Refusal::on(
                "source",
                format!(
                    "makes a description of {} characters, and a Citation/Description holds 2000 - cite the document shorter",
                    text.chars().count()
                ),
            ));
        }

        // -- the seed: the faults and the law ------------------------------------------------------
        let throws: Vec<String> = params.maximum_throw.iter().map(|t| t.to_string()).collect();
        let kinds: Vec<&str> = params.throw.iter().map(|t| t.word()).collect();
        let dead_columns: Vec<String> = params.dead_columns.iter().map(|n| n.to_string()).collect();
        let dead_layers: Vec<String> = params.dead_layers.iter().map(|n| n.to_string()).collect();
        let fault_keys: Vec<String> = faults.iter().map(key_of).collect();
        let seed = format!(
            "{}:{}:{}:{}:{}:{}:{}",
            fault_keys.join("|"),
            throws.join(","),
            kinds.join(","),
            params.profile.word(),
            params.reach,
            dead_columns.join(","),
            dead_layers.join(",")
        );

        // -- the cut ------------------------------------------------------------------------------
        let surfaces: Vec<Surface> = faults
            .iter()
            .map(|fault| surface_of(fault).unwrap_or_default())
            .collect();
        let profile = params.profile;
        let reach = params.reach;
        let maximum = params.maximum_throw.clone();
        let dead_per_fault = params.dead_columns.clone();
        let layers_asked = params.dead_layers.clone();
        let synthetic = params.synthetic;
        let fault_names = names.clone();
        let cut = move |stacked: &energyml_analysis::ijk::Stacked,
                        pillars: &[[f64; 2]],
                        unit: Option<&str>|
              -> Result<Cut, Refusal> {
            match unit {
                Some("m") => {}
                Some(other) => {
                    return Err(Refusal::on(
                        "maximumThrow",
                        format!(
                            "the horizons count their heights in `{other}`, and a throw and a reach are given in metres - no conversion is made"
                        ),
                    ));
                }
                None => {
                    return Err(Refusal::on(
                        "horizons",
                        "nothing says in what unit the horizons count their heights, and a throw is given in metres: give horizons of the package, or create the frame here",
                    ));
                }
            }
            let cut_faults: Vec<Fault<'_>> = surfaces
                .iter()
                .zip(&fault_names)
                .zip(maximum.iter().zip(&senses))
                .enumerate()
                .map(|(at, (((points, triangles), name), (throw, sense)))| Fault {
                    name,
                    points,
                    triangles,
                    maximum_throw: *throw,
                    sense: *sense,
                    dead_columns: dead_per_fault.get(at).copied().unwrap_or(0),
                })
                .collect();
            let settings = Settings {
                profile: profile.computed(),
                reach,
                dead_layers: &layers_asked,
            };
            let faulted = ijk_faults::cut(stacked, pillars, &cut_faults, &settings).map_err(refusal)?;

            let mut says = Vec::new();
            let mut parameters = vec![
                double("splitLineCount", faulted.split_pillars.len() as f64, None),
                double("deadCellCount", faulted.dead.len() as f64, None),
            ];
            says.push(Note {
                key: "split-lines",
                detail: format!(
                    "{} split coordinate line(s) on {} pillar(s): the columns a fault puts on the moved side take them, the others keep the pillar's own line (RESQML TRG 2.2 §5.17 p. 111) - {} coordinate lines in all",
                    faulted.split_pillars.len(),
                    {
                        let mut on = faulted.split_pillars.clone();
                        on.dedup();
                        on.len()
                    },
                    faulted.lines
                ),
            });
            says.push(Note {
                key: "law",
                detail: law.clone(),
            });
            says.push(Note {
                key: "staircase",
                detail: "the pillars are vertical, so each fault runs through the grid as a vertical staircase along the column faces, placed by its trace at the middle of the heights it shares with the grid; its dip is not followed".to_string(),
            });
            for ((one, name), throw) in faulted.faults.iter().zip(&fault_names).zip(&maximum) {
                says.push(Note {
                    key: "fault",
                    detail: format!(
                        "`{name}`: traced at {:.2}, dipping {:.1}° toward {:.1}° (measured on its surface); {} column(s) in its hanging wall; the largest displacement at a split pillar is {:.3} m of {throw} m{}",
                        one.level,
                        one.orientation.dip,
                        one.orientation.dip_azimuth,
                        one.hanging_wall.iter().filter(|on| **on).count(),
                        one.largest,
                        if one.dead_columns.is_empty() {
                            String::new()
                        } else {
                            format!("; {} cell(s) of {} column(s) next to it removed", one.dead_cells, one.dead_columns.len())
                        }
                    ),
                });
                parameters.push(double("traceLevel", one.level, unit));
            }
            if !faulted.dead.is_empty() {
                says.push(Note {
                    key: "removed-cells",
                    detail: format!(
                        "{} cell(s) next to the faults removed, as asked: `CellGeometryIsDefined` false (RESQML TRG 2.2 §5.1 p. 94) - their nodes kept, the flag is what a reader obeys",
                        faulted.dead.len()
                    ),
                });
            }
            if synthetic {
                says.push(Note {
                    key: "synthetic",
                    detail: "the throw is invented: the grid carries `synthetic = true` and a description of its law and its source".to_string(),
                });
            }
            Ok(Cut {
                faulted,
                says,
                parameters,
            })
        };

        let mut plan = ijk_grid::between(
            horizons,
            params.grid,
            cx,
            Some(Faulting {
                recipe: "faulted-ijk-grid",
                seed,
                cut: Box::new(cut),
            }),
        )?;

        // -- the mark of an invented throw -------------------------------------------------------
        if let Some(text) = description
            && let Some(grid) = plan.objects.iter_mut().find(|one| one.role == "representation")
        {
            grid.json["Citation"]["Description"] = json!(text);
            if synthetic {
                // The reference-model rule on invented objects (ADR-0019).
                grid.json["ExtensionNameValue"] = json!([{
                    "$type": "eml23.ExtensionNameValue",
                    "Name": "synthetic",
                    "Value": { "$type": "eml23.StringMeasure", "_": "true" },
                    "Description": "an invented throw, cut into the grid by `faulted-ijk-grid` from a stated law, not measured",
                }]);
            }
        }
        Ok(plan)
    }

    /// The grid's counts, then the cut's: the split lines, the removed cells,
    /// and the level each fault was traced at - one per fault, so a list.
    fn reports(&self) -> Vec<Slot> {
        let mut own = ijk_grid::grid_reports();
        own.push(Slot::required(
            "splitLineCount",
            Need::Number {
                uom: None,
                least: None,
                most: None,
            },
            "split coordinate lines - extra lines on the pillars a fault runs through",
        ));
        own.push(Slot::required(
            "deadCellCount",
            Need::Number {
                uom: None,
                least: None,
                most: None,
            },
            "cells next to a fault removed - CellGeometryIsDefined false",
        ));
        own.push(Slot {
            most: None,
            ..Slot::required("traceLevel", Need::Number { uom: None, least: None, most: None }, "the height each fault is traced at, in the order of `faults` - the middle of the heights it shares with the grid")
        });
        support::frame_reports(own)
    }
}
