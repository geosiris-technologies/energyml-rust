//! `loft` — a triangulated surface **between polylines**, in their order
//! (the scene-editing design, lofting): two lines, or successive
//! fault sticks.
//!
//! The computation is `energyml_analysis::loft`; this is the seam. Each input
//! is **one** polyline: a piece that holds several is refused rather than
//! paired i-th with i-th in an order nothing guarantees. The lines are taken in
//! the order of the slot — which is declared (`ordered`), because a loft
//! between sticks 1, 3, 2 is a different surface, and one that looks like one.
//!
//! # References
//!
//! - Christiansen and Sederberg (1978), the classic tiling between contours, and Fuchs, Kedem and Uselton (1977), the optimum, as named in `energyml-geostat` (`loft`).
//!   TODO(reference): verify — both citations are given without title, venue or DOI; complete and check them.

use energyml_analysis::loft::{LoftLine, loft};
use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::{Geometry, Patch, Topology};

use crate::declare::{Inputs, Params, Shape, Suggestions};
use crate::operation::{About, Category, Context, Need, Note, Operation, Plan, Refusal, Slot};
use crate::pieces::Pieces;

use super::polylines::said_closed;
use super::support;
use super::triangles::TrianglesInputs;
use super::written::Written;

/// The operation.
pub struct Loft;

/// What `loft` is told.
#[derive(Debug, Params)]
pub struct LoftParams {
    /// The frame, what is represented, the naming — for the writer of the result.
    #[param(flatten)]
    pub written: Written,
}

/// What `loft` takes.
#[derive(Debug, Inputs)]
pub struct LoftInputs {
    /// the polylines to join, one per input, **in order** — two, or successive fault sticks
    #[input(topology = "polylines", least = 2, ordered)]
    pub lines: Vec<Shape>,
}

/// The one polyline of a shape, and whether it closes — or why it is not one.
fn one_line(shape: &Shape) -> Result<(Vec<[f64; 3]>, bool), Refusal> {
    let mut found: Option<(Vec<[f64; 3]>, bool)> = None;
    let mut global = 0usize;
    for patch in &shape.geometry.patches {
        let Topology::Polylines(runs) = &patch.topology else { continue };
        let mut start = 0usize;
        for (local, &run) in runs.iter().enumerate() {
            let run = run as usize;
            if found.is_some() {
                return Err(Refusal::on(
                    "lines",
                    format!("`{}` holds several polylines — give one per input, in order, rather than pairing them in an order nothing guarantees", shape.name),
                ));
            }
            let points = patch.points.get(start..start + run).ok_or_else(|| Refusal::on("lines", format!("`{}`: the polylines add up to more nodes than the patch holds", shape.name)))?;
            let closed = said_closed(patch, local, shape.closed.as_deref(), global).unwrap_or(false);
            found = Some((points.to_vec(), closed));
            start += run;
            global += 1;
        }
    }
    found.ok_or_else(|| Refusal::on("lines", format!("`{}` holds no polyline", shape.name)))
}

impl Operation for Loft {
    type Params = LoftParams;
    type Inputs = LoftInputs;

    fn about(&self) -> About {
        About {
            name: "loft",
            describe: "a triangulated surface between polylines, in their order — every vertex kept, the band stitched along their lengths",
            category: Category::Geometry,
            makes: &["resqml22.TriangulatedSetRepresentation"],
            recipe: "loft",
            represents: None,
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        super::written::suggest_written(bag, None, slots);
    }

    fn check(&self, inputs: &LoftInputs) -> Result<(), Refusal> {
        for shape in &inputs.lines {
            one_line(shape)?;
        }
        Ok(())
    }

    fn plan(&self, inputs: LoftInputs, params: LoftParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
        let read: Vec<(Vec<[f64; 3]>, bool)> = inputs.lines.iter().map(one_line).collect::<Result<_, _>>()?;
        let lines: Vec<LoftLine> = inputs
            .lines
            .iter()
            .zip(&read)
            .map(|(shape, (points, closed))| LoftLine { name: &shape.name, points, closed: *closed })
            .collect();
        let made = loft(&lines).map_err(|why| Refusal::on("lines", why))?;

        let mut says = Vec::new();
        for (at, &turn) in made.turned.iter().enumerate() {
            if turn > 0 {
                says.push(Note {
                    key: "turned",
                    detail: format!("the ring `{}` was turned by {turn} vertex(es) so that its start meets `{}`'s", lines[at + 1].name, lines[at].name),
                });
            }
        }
        let triangles = made.triangles.len();
        let result = Shape {
            name: "loft".to_string(),
            geometry: Geometry {
                kind: "loft",
                patches: vec![Patch { index: 0, node_offset: 0, points: made.points, topology: Topology::Triangles(made.triangles), closed: Vec::new() }],
            },
            closed: None,
            known: inputs.lines[0].known.clone(),
        };
        let title = params.written.naming.title();
        let names: Vec<&str> = lines.iter().map(|line| line.name).collect();
        let seed = format!("loft:{}:{title}", names.join("|"));
        let mut plan = super::triangles::write(&TrianglesInputs { shape: vec![result] }, &params.written, &seed, cx)?;
        plan.activity.recipe = "loft";
        plan.activity.title = format!("loft → {title}");
        plan.activity.parameters.push(Parameter::Double { title: "bandTriangles".to_string(), value: triangles as f64, uom: None });
        plan.says.extend(says);
        Ok(plan)
    }

    fn reports(&self) -> Vec<Slot> {
        support::frame_reports(vec![Slot::required("bandTriangles", Need::Number { uom: None, least: None, most: None }, "triangles between the lines")])
    }
}
