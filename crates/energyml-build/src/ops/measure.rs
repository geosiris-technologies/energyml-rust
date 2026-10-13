//! `measure` - the area of a surface, the length of polylines, the volume of an
//! IJK grid: one value per element, written as a continuous property, and the
//! totals in the activity.
//!
//! The rule behind it: a `bulk volume` per cell is a legitimate object, rather than a figure in a terminal (ADR 0017). The
//! computation is [`energyml_analysis::measure`]; this is the seam, and the
//! computation is [`energyml_analysis::measure`]; this is the seam, and the
//! writing is [`super::property`]'s, the way `thickness` does it.
//!
//! # What is measured follows the shape, and the element with it
//!
//! | the shape | the measure | per | the kind (dictionary, by title) |
//! |---|---|---|---|
//! | triangles | area | `triangles` | `area` |
//! | grid cells, quadrilaterals | area | `cells` | `area` |
//! | polylines | length | `lines` | `length` |
//! | an IJK grid's hexahedra | volume | `cells` | `volume` |
//!
//! The reference model names `bulk volume`; the Energistics 2.3 dictionary holds no kind
//! of that title, and `volume` - *“a measure of the space occupied by a
//! body”* - is the one it has. A kind the dictionary does not hold would be a
//! reference no store resolves.
//!
//! A shape mixing two kinds of element - triangles beside grid cells - is
//! refused: a property holds one value per element of **one** kind. So is an
//! unstructured grid (its faces are fanned by the reader, which is exact only
//! for planar faces), and a trajectory: its length is the measured-depth span
//! it states, and the chord sum of its stations under-reads every arc.
//!
//! # Along or in plan, and it is said
//!
//! `measured` is `along` (in three dimensions) or `plan` (the projection on the
//! horizontal). The property holds the one asked for; the activity reports
//! **both totals** for an area or a length, so that a reader of the package
//! never has to wonder which one a number is. A volume has no plan projection:
//! `plan` is refused for it.
//!
//! # The unit is the frame's, and it has to be one
//!
//! An area is a length squared, so the unit comes from the coordinates:
//!
//! - **a frame in degrees is refused.** A degree of longitude is 111 km at the
//!   equator and none at the pole: an area in square degrees is no area. Bring
//!   the shape into a projected frame first;
//! - **a measure in three dimensions needs one unit on the three axes**: a
//!   horizontal in metres and a vertical in feet is refused, naming both - the
//!   sum of the two squared would add apples to oranges. The same for a time
//!   vertical: a length along a surface in metres and milliseconds is not a
//!   length. The plan measure reads the horizontal only, and is left alone;
//! - **when the frame does not say**, `lengthUom` is asked for; given, it must
//!   agree with what the frame says. The unit written is the square or the
//!   cube of it - `m2`, `ft3` - from a table of the spellings the Energistics
//!   unit dictionary uses; a unit outside it is refused by name rather than
//!   spelled by guess.

use serde_json::json;

use energyml_analysis::measure::{self as analysis};
use energyml_domain::provenance::Parameter;
use energyml_domain::units::Dimension;
use energyml_geometry::geometry::Topology;

use crate::declare::{
    Choice, Inputs, ObjectRef, Params, PropertyKindRef, Shape, Suggestions, Title, known_of,
};
use crate::operation::{About, Category, Context, Need, Note, Operation, Plan, Refusal, Slot};
use crate::pieces::{Known, Pieces};

/// The operation.
pub struct Measure;

/// Along the shape, or in plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Choice)]
pub enum Measured {
    /// Along the shape itself, in three dimensions.
    Along,
    /// Its projection on the horizontal.
    Plan,
}

/// What `measure` is told.
#[derive(Debug, Params)]
pub struct MeasureParams {
    /// along the shape itself (in three dimensions) or its projection on the horizontal - a volume is always along
    #[param(default = Measured::Along)]
    pub measured: Measured,
    /// the representation the values are laid on - the shape's
    pub representation: ObjectRef,
    /// the length unit of the coordinates; left out, the frame's - given, it must agree with it
    pub length_uom: Option<String>,
    /// what is measured; left out, the kind of the standard dictionary - area, length or volume
    pub property_kind: Option<PropertyKindRef>,
    /// the property's title; left out, the shape's and the measure's
    ///
    /// Not suggested: a suggestion is settled before `plan`, so it could not
    /// follow `measured` - see `surface-map`.
    pub title: Option<Title>,
    /// the external file the values go into
    #[param(default = String::from("data.h5"))]
    pub data_file: String,
}

/// What `measure` takes.
#[derive(Debug, Inputs)]
pub struct MeasureInputs {
    /// the shape measured - a surface (triangles or grid cells), polylines, or an IJK grid
    ///
    /// `triangles` ranks the form's list; polylines and hexahedra are measured
    /// too, and [`Operation::check`] is what decides.
    #[input(topology = "triangles")]
    pub shape: Shape,
}

/// What a shape measures, read off its topology.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum What {
    Triangles,
    Cells,
    Lines,
    Volume,
}

impl What {
    fn of(shape: &Shape) -> Result<What, String> {
        let mut found: Option<What> = None;
        for patch in &shape.geometry.patches {
            let this = match &patch.topology {
                Topology::Triangles(_) => What::Triangles,
                Topology::Quads(_) | Topology::Grid { .. } => What::Cells,
                Topology::Polylines(_) => What::Lines,
                Topology::Hexahedra { .. } => What::Volume,
                Topology::Points => return Err("holds points, and a point measures nothing".to_string()),
                Topology::Polyhedra(_) => {
                    return Err("is an unstructured grid: its faces of any shape are fanned by the reader, exact only when planar - its volume is not measured here yet".to_string());
                }
                Topology::Polygons(_) | Topology::Segments(_) => {
                    return Err("holds rings or loose edges, neither a surface, a line nor a volume a property can lie on".to_string());
                }
            };
            match found {
                Some(before) if before != this => {
                    return Err(format!(
                        "mixes {} and {}: a property holds one value per element of one kind",
                        before.elements(),
                        this.elements()
                    ));
                }
                _ => found = Some(this),
            }
        }
        if found == Some(What::Volume) && shape.geometry.patches.len() != 1 {
            return Err(format!(
                "holds {} patches of hexahedra, where an IJK grid has one",
                shape.geometry.patches.len()
            ));
        }
        found.ok_or_else(|| "has no patch".to_string())
    }

    fn elements(self) -> &'static str {
        match self {
            What::Triangles => "triangles",
            What::Cells | What::Volume => "cells",
            What::Lines => "lines",
        }
    }

    fn word(self) -> &'static str {
        match self {
            What::Triangles | What::Cells => "area",
            What::Lines => "length",
            What::Volume => "volume",
        }
    }

    /// The power of the length unit the measure is in.
    fn power(self) -> u8 {
        match self {
            What::Triangles | What::Cells => 2,
            What::Lines => 1,
            What::Volume => 3,
        }
    }
}

/// The spelling of a length unit squared or cubed, as the Energistics unit
/// dictionary writes it - only the ones this table knows; anything else is
/// refused rather than spelled by pattern.
fn powered(unit: &str, power: u8) -> Option<String> {
    const KNOWN: &[&str] = &["m", "km", "cm", "mm", "ft", "in"];
    match power {
        1 => Some(unit.to_string()),
        2 | 3 if KNOWN.contains(&unit) => Some(format!("{unit}{power}")),
        _ => None,
    }
}

/// The title the reference of the shape gives it, or its role.
fn title_of(shape: &Shape) -> String {
    shape
        .known
        .as_ref()
        .and_then(|known| known.reference.as_ref())
        .map(|r| r.title.clone())
        .unwrap_or_else(|| "shape".to_string())
}

/// The length unit of the coordinates - or why there is none to measure in.
fn length_unit(known: Option<&Known>, three_d: bool, given: Option<&str>) -> Result<String, Refusal> {
    let placement = known.and_then(|k| k.placement.as_ref());
    let horizontal = placement.and_then(|p| p.horizontal_uom.clone());
    let vertical = placement
        .and_then(|p| p.vertical_axis_uom().map(str::to_string))
        .or_else(|| known.and_then(|k| k.vertical_uom.clone()));

    // Degrees: by the class of the frame, or by the unit it states.
    let geographic = known.and_then(|k| k.crs.as_ref()).is_some_and(|crs| {
        matches!(
            crate::crs::frame_kind(&crs.qualified_type),
            Ok(crate::crs::FrameKind::Geographic)
        )
    });
    let angular = horizontal.as_deref().and_then(Dimension::of) == Some(Dimension::PlaneAngle);
    if geographic || angular {
        return Err(Refusal::new(
            "degrees",
            "the frame is in degrees: a degree of longitude is 111 km at the equator and nothing at the pole, so a length or an area in degrees measures nothing - bring the shape into a projected frame first (`change-frame`)",
        ));
    }
    if three_d {
        let time = placement.is_some_and(|p| p.is_time())
            || vertical.as_deref().and_then(Dimension::of) == Some(Dimension::Time);
        if time {
            return Err(Refusal::new(
                "units",
                format!(
                    "the heights are times (`{}`): a measure along the shape would add metres and seconds - ask for the plan measure, or convert to depth first",
                    vertical.as_deref().unwrap_or("time")
                ),
            ));
        }
        if let (Some(h), Some(v)) = (&horizontal, &vertical)
            && h != v
        {
            return Err(Refusal::new(
                "units",
                format!(
                    "the frame counts x and y in `{h}` and z in `{v}`: a measure in three dimensions needs one unit on the three axes - convert one of them first, or ask for the plan measure"
                ),
            ));
        }
    }
    let stated: Vec<&String> = [
        horizontal.as_ref(),
        if three_d { vertical.as_ref() } else { None },
    ]
    .into_iter()
    .flatten()
    .collect();
    let unit = match (given, horizontal.as_ref()) {
        (Some(given), _) => {
            if let Some(disagrees) = stated.iter().find(|unit| unit.as_str() != given) {
                return Err(Refusal::on(
                    "lengthUom",
                    format!(
                        "is `{given}` and the frame counts its coordinates in `{disagrees}` - the measure would be read in the wrong unit"
                    ),
                ));
            }
            given.to_string()
        }
        (None, Some(h)) => h.clone(),
        (None, None) => {
            return Err(Refusal::on(
                "lengthUom",
                match vertical {
                    Some(v) => format!("is required: the frame states its vertical unit (`{v}`) and not its horizontal one - name the length unit of the coordinates"),
                    None => "is required: the frame does not say the unit of its coordinates - name it, a measure without one is a number".to_string(),
                },
            ));
        }
    };
    if Dimension::of(&unit) != Some(Dimension::Length) {
        return Err(Refusal::on(
            "lengthUom",
            format!("`{unit}` is not a length unit this build knows"),
        ));
    }
    Ok(unit)
}

impl Operation for Measure {
    type Params = MeasureParams;
    type Inputs = MeasureInputs;

    fn about(&self) -> About {
        About {
            name: "measure",
            describe: "the area of a surface, the length of polylines or the volume of an IJK grid - one value per element as a continuous property, the totals in the activity",
            category: Category::Analysis,
            makes: &["resqml22.ContinuousProperty"],
            recipe: "measure",
            represents: None,
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        let Some(known) = known_of(bag, "shape") else {
            return;
        };
        if let Some(reference) = &known.reference {
            slots.inferred("representation", reference.dor(), reference.clone());
        }
    }

    fn check(&self, inputs: &MeasureInputs) -> Result<(), Refusal> {
        What::of(&inputs.shape)
            .map(|_| ())
            .map_err(|why| Refusal::on("shape", why))?;
        if let Some(reference) = inputs.shape.known.as_ref().and_then(|k| k.reference.as_ref()) {
            let class = reference.qualified_type.rsplit('.').next().unwrap_or_default();
            if class.ends_with("WellboreTrajectoryRepresentation") {
                return Err(Refusal::on(
                    "shape",
                    "is a trajectory: its length is the measured-depth span it states, and the chord sum of its stations under-reads every arc",
                ));
            }
            if class == "PolylineRepresentation" {
                return Err(Refusal::on(
                    "shape",
                    "is a single `PolylineRepresentation`, which has no `lines` element to hold one length per line - write it as a polyline set first",
                ));
            }
        }
        Ok(())
    }

    fn plan(&self, inputs: MeasureInputs, params: MeasureParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
        let MeasureInputs { shape } = inputs;
        let what = What::of(&shape).map_err(|why| Refusal::on("shape", why))?;
        if what == What::Volume && params.measured == Measured::Plan {
            return Err(Refusal::on(
                "measured",
                "is `plan`, and a volume has no projection on the horizontal - ask for `along`",
            ));
        }
        let three_d = what == What::Volume || params.measured == Measured::Along;
        let length = length_unit(shape.known.as_ref(), three_d, params.length_uom.as_deref())?;
        let uom = powered(&length, what.power()).ok_or_else(|| {
            Refusal::on(
                "lengthUom",
                format!("`{length}` has no {} spelling this build knows (m, km, cm, mm, ft, in) - convert the coordinates first", if what.power() == 2 { "squared" } else { "cubed" }),
            )
        })?;

        // The per-element measures, both ways where there are two.
        let mut says: Vec<Note> = Vec::new();
        let (values, along, in_plan, undefined) = match what {
            What::Triangles | What::Cells => {
                let (mut along, mut plan, mut unplaced, mut folded) =
                    (Vec::new(), Vec::new(), 0usize, 0usize);
                for patch in &shape.geometry.patches {
                    let found = match &patch.topology {
                        Topology::Triangles(list) => analysis::triangle_areas(&patch.points, list),
                        Topology::Quads(list) => analysis::quad_areas(&patch.points, list),
                        Topology::Grid { fastest, slowest } => {
                            // Row by row, the fast axis first; the ring goes round the cell.
                            let mut quads =
                                Vec::with_capacity(fastest.saturating_sub(1) * slowest.saturating_sub(1));
                            let fast = *fastest as u32;
                            for row in 0..slowest.saturating_sub(1) as u32 {
                                for column in 0..fast.saturating_sub(1) {
                                    let a = row * fast + column;
                                    quads.push([a, a + 1, a + fast + 1, a + fast]);
                                }
                            }
                            analysis::quad_areas(&patch.points, &quads)
                        }
                        _ => unreachable!("What::of admitted only surfaces here"),
                    }
                    .map_err(|e| Refusal::on("shape", e.to_string()))?;
                    along.extend(found.along);
                    plan.extend(found.plan);
                    unplaced += found.unplaced;
                    folded += found.folded;
                }
                if what == What::Cells {
                    says.push(Note {
                        key: "bilinear",
                        detail: "a grid cell is the bilinear patch through its corners, never cut by a diagonal: its plan area is exact, its area along it integrated by a 4 × 4 Gauss-Legendre rule".to_string(),
                    });
                }
                if unplaced > 0 {
                    says.push(Note {
                        key: "unplaced",
                        detail: format!(
                            "{unplaced} element(s) with a corner that is no finite point: NaN, not zero"
                        ),
                    });
                }
                if folded > 0 {
                    says.push(Note {
                        key: "folded",
                        detail: format!("{folded} cell(s) fold over themselves in plan - a bow-tie has no single plan area: NaN in plan"),
                    });
                }
                let chosen = if params.measured == Measured::Along {
                    along.clone()
                } else {
                    plan.clone()
                };
                let undefined = chosen.iter().filter(|v| !v.is_finite()).count();
                (chosen, Some(along), Some(plan), undefined)
            }
            What::Lines => {
                let (mut along, mut plan, mut unplaced, mut unsaid, mut at) =
                    (Vec::new(), Vec::new(), 0usize, 0usize, 0usize);
                for patch in &shape.geometry.patches {
                    let Topology::Polylines(runs) = &patch.topology else {
                        unreachable!("What::of admitted only lines here")
                    };
                    // The closure the shape says, the one reading of it; a
                    // closure nobody said is open, and counted.
                    let closed: Vec<bool> = (0..runs.len())
                        .map(|local| {
                            let said = super::polylines::said_closed(
                                patch,
                                local,
                                shape.closed.as_deref(),
                                at + local,
                            );
                            if said.is_none() {
                                unsaid += 1;
                            }
                            said.unwrap_or(false)
                        })
                        .collect();
                    at += runs.len();
                    let found = analysis::polyline_lengths(&patch.points, runs, &closed)
                        .map_err(|e| Refusal::on("shape", e.to_string()))?;
                    along.extend(found.along);
                    plan.extend(found.plan);
                    unplaced += found.unplaced;
                }
                if unsaid > 0 {
                    says.push(Note {
                        key: "closure-unsaid",
                        detail: format!("{unsaid} line(s) say nothing of their closure: measured open, without a closing segment nobody stated"),
                    });
                }
                if unplaced > 0 {
                    says.push(Note {
                        key: "unplaced",
                        detail: format!(
                            "{unplaced} line(s) with a node that is no finite point: NaN, not zero"
                        ),
                    });
                }
                let chosen = if params.measured == Measured::Along {
                    along.clone()
                } else {
                    plan.clone()
                };
                let undefined = chosen.iter().filter(|v| !v.is_finite()).count();
                (chosen, Some(along), Some(plan), undefined)
            }
            What::Volume => {
                let patch = &shape.geometry.patches[0];
                let Topology::Hexahedra {
                    cells, dimensions, ..
                } = &patch.topology
                else {
                    unreachable!("What::of admitted only hexahedra here")
                };
                let found = analysis::hexahedron_volumes(&patch.points, cells)
                    .map_err(|e| Refusal::on("shape", e.to_string()))?;
                cx.stop_if_cancelled()?;
                // One value per cell **as the grid numbers them**: a cell the
                // reader dropped - undefined, or with no located pillar - is a
                // NaN at its own number, never a shift of every later cell.
                let count = dimensions[0] * dimensions[1] * dimensions[2];
                let mut values = vec![f64::NAN; count];
                for (at, volume) in found.values.iter().enumerate() {
                    let number = patch
                        .topology
                        .cell_at(at)
                        .map(|n| n as usize)
                        .filter(|n| *n < count)
                        .ok_or_else(|| {
                            Refusal::on(
                                "shape",
                                format!("cell {at} has no number within the grid's {count} cells"),
                            )
                        })?;
                    values[number] = *volume;
                }
                let absent = count - cells.len();
                if absent > 0 {
                    says.push(Note {
                        key: "undefined-cells",
                        detail: format!(
                            "{absent} of {count} cell(s) have no geometry in the grid: NaN, not zero"
                        ),
                    });
                }
                if found.unplaced > 0 {
                    says.push(Note {
                        key: "unplaced",
                        detail: format!(
                            "{} cell(s) with a corner that is no finite point: NaN",
                            found.unplaced
                        ),
                    });
                }
                if found.twisted > 0 {
                    says.push(Note {
                        key: "twisted",
                        detail: format!("{} cell(s) turn inside out within themselves - their volume would subtract the overlap: NaN", found.twisted),
                    });
                }
                if found.flat > 0 {
                    says.push(Note {
                        key: "flat",
                        detail: format!(
                            "{} cell(s) are collapsed - a pinch-out: a volume of zero, kept",
                            found.flat
                        ),
                    });
                }
                says.push(Note {
                    key: "trilinear",
                    detail: "a cell is the trilinear map through its eight corners: its volume is exact, faces that are not planar included".to_string(),
                });
                let undefined = values.iter().filter(|v| !v.is_finite()).count();
                (values, None, None, undefined)
            }
        };
        if values.is_empty() {
            return Err(Refusal::on("shape", format!("holds no {}", what.elements())));
        }
        cx.stop_if_cancelled()?;

        // The kind of the standard dictionary, looked up by title: the
        // dictionary is the authority for its own identifiers.
        let kind = match params.property_kind {
            Some(given) => given.0,
            None => {
                let wanted = what.word();
                let found = energyml_domain::property_kind::with_default(|dictionary| {
                    dictionary
                        .by_title(wanted)
                        .first()
                        .map(|kind| (kind.uuid.clone(), kind.title.clone()))
                });
                let (uuid, title) = found.ok_or_else(|| {
                    Refusal::on(
                        "propertyKind",
                        format!(
                            "the dictionary in use holds no kind called `{wanted}`: name one of the package"
                        ),
                    )
                })?;
                json!({ "$type": "eml23.DataObjectReference", "Uuid": uuid, "QualifiedType": "eml23.PropertyKind", "Title": title })
            }
        };
        let label = match (what, params.measured) {
            (What::Volume, _) => "cell volume".to_string(),
            (_, Measured::Along) => what.word().to_string(),
            (_, Measured::Plan) => format!("plan {}", what.word()),
        };
        let title = match params.title {
            Some(given) => given.0,
            None => format!("{} - {label}", title_of(&shape)),
        };

        let (total, _) = analysis::total(&values);
        let over = if shape.known.is_some() {
            "shape (object)"
        } else {
            "shape"
        }
        .to_string();
        let name = format!("{label} of shape");
        let field = crate::declare::Field {
            name: name.clone(),
            over,
            on: what.elements().to_string(),
            values: values.clone(),
            uom: Some(uom.clone()),
        };
        let property = super::property::PropertyParams {
            representation: params.representation,
            property_kind: PropertyKindRef(kind),
            uom: uom.clone(),
            title: Title(title.clone()),
            data_file: params.data_file,
        };
        let mut plan = super::property::write(
            &field,
            shape.known.as_ref(),
            &property,
            &format!("{name}:{title}"),
            cx,
        )?;
        // Our count is exact - one value per element of the shape - so the
        // writer's note that it could not check it is not this plan's news.
        plan.says.retain(|note| note.key != "count-unchecked");
        plan.says.extend(says);

        plan.activity.recipe = "measure";
        plan.activity.title = format!("measure → {title}");
        let parameters = &mut plan.activity.parameters;
        parameters.push(Parameter::Text {
            title: "measure".to_string(),
            value: what.word().to_string(),
        });
        parameters.push(Parameter::Text {
            title: "measured".to_string(),
            value: params.measured.word().to_string(),
        });
        parameters.push(Parameter::Double {
            title: "total".to_string(),
            value: total,
            uom: Some(uom.clone()),
        });
        if let (Some(along), Some(plan_values)) = (&along, &in_plan) {
            parameters.push(Parameter::Double {
                title: "totalAlong".to_string(),
                value: analysis::total(along).0,
                uom: Some(uom.clone()),
            });
            parameters.push(Parameter::Double {
                title: "totalPlan".to_string(),
                value: analysis::total(plan_values).0,
                uom: Some(uom.clone()),
            });
        }
        parameters.push(Parameter::Double {
            title: "elementCount".to_string(),
            value: values.len() as f64,
            uom: None,
        });
        parameters.push(Parameter::Double {
            title: "undefinedCount".to_string(),
            value: undefined as f64,
            uom: None,
        });
        plan.says.push(Note {
            key: "total",
            detail: format!(
                "{} {label} over {} {}: {total} {uom}",
                what.word(),
                values.len() - undefined,
                what.elements()
            ),
        });
        Ok(plan)
    }

    /// `measured` is an input, recorded under the parameter the projection
    /// already declares - a title cannot be both.
    fn reports(&self) -> Vec<Slot> {
        let number = || Need::Number {
            uom: None,
            least: None,
            most: None,
        };
        vec![
            Slot::required("indexable", Need::Text, "the indexable element the values lie on"),
            Slot::required("valueCount", number(), "values written"),
            Slot::required(
                "measure",
                Need::Text,
                "what was measured - area, length or volume",
            ),
            Slot::required("total", number(), "the sum of the values written, in their unit"),
            Slot::optional(
                "totalAlong",
                number(),
                "an area or a length: the total along the shape, in three dimensions",
            ),
            Slot::optional("totalPlan", number(), "an area or a length: the total in plan"),
            Slot::required("elementCount", number(), "elements measured"),
            Slot::required(
                "undefinedCount",
                number(),
                "elements with no measure, written as NaN",
            ),
        ]
    }
}
