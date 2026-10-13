//! `surface-map` - what a triangulated horizon says about itself, as a
//! continuous property at every node: dip, dip azimuth, or one of the four
//! curvatures.
//!
//! The first of the geological operations.
//! The computation is
//! [`energyml_analysis::surface`], written and tested against shapes whose
//! answer is known analytically; this is the seam that turns one of its
//! answers into an object of the package - and it is an **operation**, so the
//! CLI and the front end reach it by the door they already have, with no command
//! and no form written for it.
//!
//! # One map per run
//!
//! `what` picks one, and the plan writes one property. Two maps are two runs,
//! which is what [`super::property`] itself says of two fields: one property
//! per field. It also keeps the derived uuid honest - the field's name carries
//! the map, so `dip` and `azimuth` of one surface never collide.
//!
//! # Which way is up is read, never guessed
//!
//! The module this calls refuses to guess it, for the reason it gives: a bowl
//! and a dome are the same triangles. Here it is a required choice, and it is
//! **suggested from the package** when the reader knew the CRS's vertical
//! direction (`Known::vertical_up`). When the package does not say, nothing is
//! suggested and the caller answers.
//!
//! # What has no value, and why it is not an error
//!
//! The azimuth of a horizontal node, and every curvature of a boundary node,
//! do not exist - the analysis writes `NaN` there and counts them, and so does
//! the property. A note says how many. Extrapolating them would draw a rim of
//! invented structure exactly where people look for the edge of the data.
//!
//! A surface of several patches is one mesh here, its patches concatenated in
//! their own order - which is the node order of the representation, so value
//! *i* lands on node *i*. Nothing welds two patches that meet, because nothing
//! in the document says they do: a node on such a seam is on the boundary of
//! its patch, so its curvature is `NaN` like any other rim node, and the note
//! counts it.
//!
//! # References
//!
//! - Dip and dip azimuth from central differences on a lattice: Zevenbergen & Thorne (1987), *Quantitative analysis of land surface topography*, Earth Surface Processes and Landforms 12(1):47-56 (as cited by `energyml_analysis::surface::lattice_slope`).
//!   TODO(reference): verify - the page range is repeated from that module and was not checked against the paper here.
//! - The four curvature operators on triangulated surfaces are those of `energyml_analysis::surface`.
//!   TODO(reference): verify - no publication is cited for the triangle-mesh curvature operators; add one once checked.

use serde_json::json;

use energyml_analysis::surface::{self as analysis, Up};
use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::Topology;

use crate::declare::{
    Choice, Field, Inputs, ObjectRef, Params, PropertyKindRef, Shape, Suggestions, Title, known_of,
};
use crate::operation::{About, Category, Context, Need, Note, Operation, Plan, Refusal, Slot};
use crate::pieces::Pieces;

use super::property::PropertyParams;
use super::thickness::Vertical;

/// The operation.
pub struct SurfaceMap;

/// The maps on offer, in the order a form shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Choice)]
pub enum What {
    /// Dip angle.
    Dip,
    /// Dip azimuth.
    Azimuth,
    /// Mean curvature.
    MeanCurvature,
    /// Gaussian curvature.
    GaussianCurvature,
    /// The larger principal curvature.
    MaximumCurvature,
    /// The smaller principal curvature.
    MinimumCurvature,
}

/// What each map is: the kind of the standard dictionary it is measured
/// under, its unit, and the words for a title and a message.
struct Map {
    /// The `Title` of the kind in the Energistics dictionary.
    kind: &'static str,
    /// An Energistics uom code.
    uom: &'static str,
    /// For a derived title.
    label: &'static str,
}

fn map_of(what: What) -> Map {
    match what {
        What::Dip => Map {
            kind: "dip angle",
            uom: "dega",
            label: "dip",
        },
        What::Azimuth => Map {
            kind: "dip azimuth",
            uom: "dega",
            label: "dip azimuth",
        },
        What::MeanCurvature => Map {
            kind: "curvature",
            uom: "1/m",
            label: "mean curvature",
        },
        What::GaussianCurvature => Map {
            kind: "curvature",
            uom: "1/m",
            label: "gaussian curvature",
        },
        What::MaximumCurvature => Map {
            kind: "curvature",
            uom: "1/m",
            label: "maximum curvature",
        },
        What::MinimumCurvature => Map {
            kind: "curvature",
            uom: "1/m",
            label: "minimum curvature",
        },
    }
}

/// What `surface-map` is told.
#[derive(Debug, Params)]
pub struct SurfaceMapParams {
    /// which map
    #[param(default = What::Dip)]
    pub what: What,
    /// what the third coordinate measures - an elevation (larger is higher) or a depth (larger is lower)
    ///
    /// Suggested from the CRS the reader read, never from the shape: a bowl
    /// and a dome are the same triangles.
    pub vertical: Vertical,
    /// the representation the values are laid on
    pub representation: ObjectRef,
    /// what is measured; left out, the kind of the standard dictionary the map is measured under
    pub property_kind: Option<PropertyKindRef>,
    /// the unit of the values; left out, the map's own - degrees, or 1/m
    pub uom: Option<String>,
    /// the property's title; left out, the surface's and the map's
    ///
    /// **Not suggested**, deliberately: a suggestion is settled before `plan`
    /// runs, so one written here would be the same whatever `what` says - and
    /// it was: every map of a surface came out titled *dip*.
    pub title: Option<Title>,
    /// the external file the values go into
    #[param(default = String::from("data.h5"))]
    pub data_file: String,
}

/// What `surface-map` takes.
#[derive(Debug, Inputs)]
pub struct SurfaceMapInputs {
    /// the surface to measure
    #[input(topology = "triangles")]
    pub shape: Shape,
}

/// The surface as one mesh: the coordinates flat, the triangles global.
///
/// The patches are concatenated in their own order, each one's indices moved
/// by the nodes before it - the node numbering a representation states, so a
/// value lands on the node it was computed for.
fn mesh_of(shape: &Shape) -> (Vec<f64>, Vec<u32>) {
    let mut points: Vec<f64> = Vec::new();
    let mut triangles: Vec<u32> = Vec::new();
    for patch in &shape.geometry.patches {
        let base = (points.len() / 3) as u32;
        points.extend(patch.points.iter().flatten().copied());
        if let Topology::Triangles(list) = &patch.topology {
            triangles.extend(list.iter().flatten().map(|&i| i + base));
        }
    }
    (points, triangles)
}

/// The lattice a shape is, when it is **one** 2D grid - `(fastest, slowest)`.
///
/// A `Grid2dRepresentation` has no listed face: its cells are the lattice's,
/// and reading them as triangles would cut each in two by a diagonal the file
/// never chose. Its slope is read by central differences instead
/// (`energyml_analysis::surface::lattice_slope`, 25/09 - the DTM of Lutetia).
fn lattice_of(shape: &Shape) -> Option<(usize, usize)> {
    match shape.geometry.patches.as_slice() {
        [one] => match one.topology {
            Topology::Grid { fastest, slowest } => Some((fastest, slowest)),
            _ => None,
        },
        _ => None,
    }
}

impl Operation for SurfaceMap {
    type Params = SurfaceMapParams;
    type Inputs = SurfaceMapInputs;

    fn about(&self) -> About {
        About {
            name: "surface-map",
            describe: "dip, dip azimuth or curvature of a triangulated surface, dip and dip azimuth of a 2D grid - one value per node, written as a continuous property",
            category: Category::Analysis,
            makes: &["resqml22.ContinuousProperty"],
            recipe: "surface-map",
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
        if let Some(up) = known.vertical_up {
            slots.set("vertical", json!(if up { "elevation" } else { "depth" }));
        }
    }

    fn check(&self, inputs: &SurfaceMapInputs) -> Result<(), Refusal> {
        if mesh_of(&inputs.shape).1.is_empty() && lattice_of(&inputs.shape).is_none() {
            return Err(Refusal::on(
                "shape",
                "the shape has no triangle and is no 2D grid: dip and curvature are read off the faces, and a point cloud or a polyline has none",
            ));
        }
        Ok(())
    }

    fn plan(
        &self,
        inputs: SurfaceMapInputs,
        params: SurfaceMapParams,
        cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        let what = params.what;
        let map = map_of(what);
        let up = match params.vertical {
            Vertical::Elevation => Up::PlusZ,
            Vertical::Depth => Up::MinusZ,
        };
        let (points, triangles) = mesh_of(&inputs.shape);
        let source = inputs.shape.name.clone();
        let refuse = |e: energyml_analysis::Error| Refusal::on("shape", e);

        // The computation, and the honesty counters that come with it.
        let (values, undefined, aside, unreached) = match what {
            What::Dip | What::Azimuth => {
                let found = match lattice_of(&inputs.shape) {
                    Some((fastest, slowest)) => {
                        analysis::lattice_slope(&points, fastest, slowest, up).map_err(refuse)?
                    }
                    None => analysis::slope(&points, &triangles, up).map_err(refuse)?,
                };
                let undefined = if what == What::Dip {
                    found.unreached
                } else {
                    found.flat + found.sheer + found.unreached
                };
                let values = if what == What::Dip {
                    found.dip
                } else {
                    found.azimuth
                };
                (values, undefined, found.set_aside, found.unreached)
            }
            _ if lattice_of(&inputs.shape).is_some() => {
                return Err(Refusal::on(
                    "what",
                    "the curvature of a 2D grid is not written yet: the operators here need triangles, and cutting each cell in two would be a choice the file never made - dip and dip azimuth are read on a grid",
                ));
            }
            _ => {
                let found = analysis::curvature(&points, &triangles, up).map_err(refuse)?;
                let undefined = found.boundary + found.unreached;
                let values = match what {
                    What::MeanCurvature => found.mean,
                    What::GaussianCurvature => found.gaussian,
                    What::MaximumCurvature => found.maximum,
                    _ => found.minimum,
                };
                (values, undefined, found.set_aside, found.unreached)
            }
        };
        cx.stop_if_cancelled()?;

        // The kind of the standard dictionary, unless one was named. Looked up
        // by title rather than written as a uuid here: the dictionary is the
        // authority for its own identifiers, and a uuid typed into this file
        // would be a second copy of one.
        let kind = match params.property_kind {
            Some(given) => given.0,
            None => {
                let found = energyml_domain::property_kind::with_default(|dictionary| {
                    dictionary
                        .by_title(map.kind)
                        .first()
                        .map(|kind| (kind.uuid.clone(), kind.title.clone()))
                });
                let (uuid, title) = found.ok_or_else(|| {
                    Refusal::on(
                        "propertyKind",
                        format!("the dictionary in use holds no kind called `{}`: name a `propertyKind` of the package", map.kind),
                    )
                })?;
                json!({ "$type": "eml23.DataObjectReference", "Uuid": uuid, "QualifiedType": "eml23.PropertyKind", "Title": title })
            }
        };
        let known = inputs.shape.known.as_ref();
        let title = match params.title {
            Some(given) => given.0,
            None => {
                let of = known
                    .and_then(|k| k.reference.as_ref().map(|r| r.title.clone()))
                    .unwrap_or_else(|| source.clone());
                format!("{of} - {}", map.label)
            }
        };
        let uom = params.uom.unwrap_or_else(|| map.uom.to_string());

        // The values as a field over the surface, handed to the property writer
        // **typed**. The field's name carries the map, which is what keeps two
        // maps of one surface apart in the derived uuid.
        let count = values.len();
        let name = format!("{} of {source}", what.word());
        let field = Field {
            name: name.clone(),
            over: source.clone(),
            on: "nodes".to_string(),
            values,
            uom: Some(uom.clone()),
        };
        let property = PropertyParams {
            representation: params.representation,
            property_kind: PropertyKindRef(kind),
            uom: uom.clone(),
            title: Title(title.clone()),
            data_file: params.data_file,
        };
        let mut plan = super::property::write(&field, known, &property, &format!("{name}:{title}"), cx)?;

        plan.activity.recipe = "surface-map";
        plan.activity.title = format!("surface-map → {title}");
        plan.activity.parameters.push(Parameter::Text {
            title: "what".to_string(),
            value: what.word().to_string(),
        });
        plan.activity.parameters.push(Parameter::Text {
            title: "vertical".to_string(),
            value: params.vertical.word().to_string(),
        });
        plan.activity.parameters.push(Parameter::Double {
            title: "undefinedCount".to_string(),
            value: undefined as f64,
            uom: None,
        });
        if undefined > 0 {
            plan.says.push(Note {
                key: "undefined",
                detail: format!(
                    "{undefined} of {count} node(s) have no {}: written as NaN rather than extrapolated{}",
                    map.label,
                    if unreached > 0 {
                        format!(", {unreached} of them reached by no usable triangle")
                    } else {
                        String::new()
                    }
                ),
            });
        }
        if aside > 0 {
            plan.says.push(Note {
                key: "set-aside",
                detail: format!(
                    "{aside} triangle(s) set aside: no area, or a corner that is not a finite point"
                ),
            });
        }
        Ok(plan)
    }

    /// `what` and `vertical` are **not** here, though the activity records
    /// them: a title cannot be an input and an output of one template, and
    /// they are inputs - the writer files them under the parameter the
    /// projection already declares.
    fn reports(&self) -> Vec<Slot> {
        vec![
            Slot::required("indexable", Need::Text, "the indexable element the values lie on"),
            Slot::required(
                "valueCount",
                Need::Number {
                    uom: None,
                    least: None,
                    most: None,
                },
                "values written",
            ),
            Slot::required(
                "undefinedCount",
                Need::Number {
                    uom: None,
                    least: None,
                    most: None,
                },
                "nodes with no value, written as NaN",
            ),
        ]
    }
}
