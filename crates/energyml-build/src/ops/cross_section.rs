//! `cross-section` — a filled section: a trace, surfaces, panels.
//!
//! This is **the first operation with several inputs of different roles**,
//! and that is why it comes now: cardinality and order are wired through, and
//! nothing exercised them — a declaration nobody uses is a declaration nobody
//! knows works.
//!
//! # Why this name, and not `fence`
//!
//! The design settled the vocabulary before the code: a
//! **section** is a vertical cut along **one** trace, and a **fence**
//! is *several* sections that cross each other. What the plan called `fence`
//! is therefore a section, and calling it anything else would leave the name of
//! the composite for the part — two different things under one name in the
//! code is, six months later, a function nobody knows what it takes.
//!
//! The front end does have a `fence` recipe, which is the same thing under the old name;
//! it stays as long as its constructor is not removed, and the two activity
//! templates coexist — a template uuid never changes.
//!
//! # The two roles, and why they cannot be guessed
//!
//! A trace and surfaces are **two shapes**, and nothing in their geometry
//! says which is which: a polyline laid flat is a plausible trace
//! and just as plausible a surface cut. They are therefore two named
//! slots, and the bag carries them by name — which is possible because a piece's
//! name is the name of the slot that received it.
//!
//! # The order is data, and it is declared
//!
//! `surfaces` is `in_order`: the material is put between each consecutive
//! pair, from top to bottom. Getting it the wrong way round **and an inverted
//! section looks abnormal to no one** — this is the argument the front end's
//! drawing code already carries, and the reason the order is declared instead of
//! being guessed from a mean elevation: two
//! surfaces that cross have no order in elevation, and that is exactly
//! the case the section has to show.
//!
//! # What it does not do itself
//!
//! The computation is [`energyml_geometry::fence`] — the curves, the panels, the
//! crossings, the wedges — and the writing is that of [`super::triangles`].
//! A second writer of the same object would be a second place for the same
//! mistakes.
//!
//! # The debt, named
//!
//! The design wants **one object per panel**, grouped by a
//! `RepresentationSetRepresentation`, and the trace kept as a
//! `PolylineRepresentation`. What is written here is **one** multi-patch
//! representation, a patch per piece of panel. Both gaps have the same
//! cause: no writer of `RepresentationSetRepresentation` exists in this
//! crate, and without the group, ten scattered objects do not say they are one
//! section — that would be worse than what is written here. What the scene asked for
//! is delivered: **the artefact tree does have a node per panel**,
//! which is the granularity one clicks on.
//!
//! # References
//!
//! - The cut of a surface by a vertical plane is computed by `energyml_geometry::section`.
//!   TODO(reference): verify - no source is cited for the triangle/plane intersection it relies on; add one once checked.

use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::{Geometry, Patch, Topology};
use energyml_geometry::{fence, section};

use crate::declare::{Inputs, Params, Shape, Suggestions};
use crate::operation::{About, Category, Context, Made, Need, Note, Operation, Plan, Refusal, Slot};
use crate::pieces::Pieces;

use super::support;
use super::triangles::TrianglesInputs;
use super::written::Written;

/// The operation.
pub struct CrossSection;

/// What `cross-section` is told: the shared block of the triangle writer.
#[derive(Debug, Params)]
pub struct CrossSectionParams {
    /// The frame, what is represented, the naming — for the triangle writer.
    #[param(flatten)]
    pub written: Written,
}

/// What `cross-section` takes — **two roles, read by the slot's name**: a
/// polyline in `surfaces` is the caller's mistake, not a spare trace.
#[derive(Debug, Inputs)]
pub struct CrossSectionInputs {
    /// the line the section follows, in plan view
    #[input(topology = "polylines")]
    pub trace: Shape,
    /// the surfaces the section fills between — at least two, **from top to bottom**
    #[input(topology = "triangles", least = 2, ordered)]
    pub surfaces: Vec<Shape>,
}

/// A surface as the section reads it: its piece name, and its mesh.
struct Surface<'a> {
    /// The name of the slot that received it — it is what names the panel.
    name: &'a str,
    points: Vec<[f64; 3]>,
    triangles: Vec<[u32; 3]>,
}

/// The trace in plan, and the surfaces in the order they were given.
struct Section<'a> {
    /// The trace, in (x, y).
    trace: Vec<[f64; 2]>,
    surfaces: Vec<Surface<'a>>,
}

/// What the two bound roles say — or the refusal that names what is wrong.
fn section_of(inputs: &CrossSectionInputs) -> Result<Section<'_>, Refusal> {
    let trace: Vec<[f64; 2]> = inputs
        .trace
        .geometry
        .patches
        .iter()
        .flat_map(|patch| patch.points.iter().map(|point| [point[0], point[1]]))
        .collect();
    if trace.len() < 2 {
        return Err(Refusal::on(
            "trace",
            format!("needs at least two positions — it holds {}", trace.len()),
        ));
    }
    let mut surfaces = Vec::new();
    for one in &inputs.surfaces {
        let mut points: Vec<[f64; 3]> = Vec::new();
        let mut triangles: Vec<[u32; 3]> = Vec::new();
        let mut base = 0u32;
        for patch in &one.geometry.patches {
            let Topology::Triangles(faces) = &patch.topology else {
                continue;
            };
            points.extend(patch.points.iter().copied());
            // **Offset by the rank of the patch's first node**: a patch's
            // indices are local to it, and cutting two patches laid end to end without
            // this offset would read the second on the first's nodes.
            triangles.extend(
                faces
                    .iter()
                    .map(|face| [face[0] + base, face[1] + base, face[2] + base]),
            );
            base += patch.points.len() as u32;
        }
        if triangles.is_empty() {
            // Named rather than skipped: a surface that vanishes silently
            // shifts every pair below it by one rank.
            return Err(Refusal::on(
                "surfaces",
                format!(
                    "`{}` is not triangulated — `triangulate` makes a surface from a cloud",
                    one.name
                ),
            ));
        }
        surfaces.push(Surface {
            name: &one.name,
            points,
            triangles,
        });
    }
    Ok(Section { trace, surfaces })
}

/// A panel's geometry: one RESQML patch per continuous piece.
fn geometry_of(panel: &fence::Panel) -> Geometry {
    let mut patches = Vec::new();
    let mut node_offset = 0usize;
    for (index, piece) in panel.patches.iter().enumerate() {
        patches.push(Patch {
            index,
            node_offset,
            points: piece.points.clone(),
            topology: Topology::Triangles(piece.triangles.clone()),
            closed: Vec::new(),
        });
        node_offset += piece.points.len();
    }
    Geometry {
        kind: "cross-section",
        patches,
    }
}

impl Operation for CrossSection {
    type Params = CrossSectionParams;
    type Inputs = CrossSectionInputs;

    fn about(&self) -> About {
        About {
            name: "cross-section",
            describe: "a filled section along a trace: material between each consecutive pair of surfaces",
            category: Category::Geometry,
            makes: &["resqml22.TriangulatedSetRepresentation"],
            recipe: "cross-section",
            // **Nothing.** A section represents no feature of the
            // subsurface, and the `RepresentedObject` is left empty because
            // filling it would lie about what the object is. The activity
            // carries the provenance.
            represents: None,
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        super::written::suggest_written(bag, None, slots);
    }

    fn check(&self, inputs: &CrossSectionInputs) -> Result<(), Refusal> {
        section_of(inputs).map(|_| ())
    }

    fn plan(
        &self,
        inputs: CrossSectionInputs,
        params: CrossSectionParams,
        cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        let found = section_of(&inputs)?;

        // Each surface gives its (s, z) curve along the trace. A surface
        // that cuts nowhere gives an empty curve: that is an answer, and
        // the panel that would carry it will be empty in turn.
        let curves: Vec<fence::Curve> = found
            .surfaces
            .iter()
            .map(|one| fence::curve(&section::cut(&one.points, &one.triangles, &found.trace)))
            .collect();

        let mut panels: Vec<(String, Geometry)> = Vec::new();
        let mut says: Vec<Note> = Vec::new();
        let mut empty = 0usize;
        let mut inverted = 0usize;
        for (at, pair) in curves.windows(2).enumerate() {
            let made = fence::panel(&pair[0], &pair[1], &found.trace);
            let upper = found.surfaces[at].name;
            let lower = found.surfaces[at + 1].name;
            for note in &made.notes {
                says.push(Note {
                    key: "panel",
                    detail: format!("{upper} → {lower}: {note}"),
                });
            }
            if made.is_empty() {
                // **An answer, not a failure**: the two surfaces do not
                // meet anywhere along this trace. Staying silent would make
                // a section with holes look like a complete one.
                empty += 1;
                says.push(Note {
                    key: "panel-empty",
                    detail: format!("{upper} → {lower}: no material along this trace"),
                });
                continue;
            }
            inverted += made.patches.iter().filter(|patch| patch.inverted).count();
            panels.push((format!("{upper} → {lower}"), geometry_of(&made)));
        }
        if panels.is_empty() {
            return Err(Refusal::on(
                "surfaces",
                format!(
                    "no panel holds any material along this trace — {} pair(s) of surfaces, all empty",
                    curves.len().saturating_sub(1)
                ),
            ));
        }

        // **One panel per shape** given to the writer, typed; what was known
        // of the input objects travels through the context. The seed is the one the
        // first panel used to give the derived bag before.
        let title = params.written.naming.title();
        let seed = format!("{}:{title}", panels[0].0);
        let shapes: Vec<Shape> = panels
            .iter()
            .map(|(name, geometry)| Shape {
                name: name.clone(),
                geometry: geometry.clone(),
                closed: None,
                known: None,
            })
            .collect();
        let mut plan =
            super::triangles::write(&TrianglesInputs { shape: shapes }, &params.written, &seed, cx)?;

        plan.activity.recipe = "cross-section";
        plan.activity.title = format!("cross-section → {title}");
        plan.activity.parameters.push(Parameter::Double {
            title: "panelCount".to_string(),
            value: panels.len() as f64,
            uom: None,
        });
        plan.activity.parameters.push(Parameter::Double {
            title: "emptyPanels".to_string(),
            value: empty as f64,
            uom: None,
        });
        plan.activity.parameters.push(Parameter::Double {
            title: "invertedPatches".to_string(),
            value: inverted as f64,
            uom: None,
        });
        plan.says.append(&mut says);
        if inverted > 0 {
            // **A sign, never a correction** (crossings): the given order
            // puts one surface above another, and the geometry says
            // the opposite in places. Straightening it would be deciding in someone
            // else's place; staying silent would let a wrong section be read.
            plan.says.push(Note {
                key: "inverted",
                detail: format!(
                    "{inverted} piece(s) run the other way round: the surface given as the upper one passes below the other there"
                ),
            });
        }
        plan.shapes = tree_of(&plan.shapes, &panels);
        Ok(plan)
    }

    fn reports(&self) -> Vec<Slot> {
        support::frame_reports(vec![
            Slot::optional(
                "panelCount",
                Need::Integer {
                    least: Some(0),
                    most: None,
                },
                "how many panels hold material",
            ),
            Slot::optional(
                "emptyPanels",
                Need::Integer {
                    least: Some(0),
                    most: None,
                },
                "how many pairs of surfaces meet nowhere along the trace",
            ),
            Slot::optional(
                "invertedPatches",
                Need::Integer {
                    least: Some(0),
                    most: None,
                },
                "how many pieces run the other way round — the order given disagrees with the geometry there",
            ),
        ])
    }
}

/// The tree: a **grouping** root, a node per panel.
///
/// **The root draws nothing**, and that is the point. It carries the uuid of
/// the written object, its children carry the material; leaving it the merged
/// geometry that `triangles` made would draw each panel **twice**
/// — once by the root, once by its child — and two exactly
/// superimposed surfaces fight pixel by pixel with nothing saying
/// why.
///
/// It is also what the scene-tree design already states: hiding the cross
/// sections without hiding the node would leave a lit line that draws
/// nothing — a group root is a line that draws nothing, by construction.
fn tree_of(written: &[Made], panels: &[(String, Geometry)]) -> Vec<Made> {
    let Some(first) = written.first() else {
        return Vec::new();
    };
    let root = Made {
        geometry: Geometry {
            kind: "cross-section",
            patches: Vec::new(),
        },
        ..first.clone()
    };
    let mut out = vec![root.clone()];
    for (at, (title, geometry)) in panels.iter().enumerate() {
        out.push(Made {
            id: format!("{at}-panel"),
            parent: Some(root.id.clone()),
            title: title.clone(),
            geometry: geometry.clone(),
            // **No object of its own**: the panels are the patches of the
            // representation the root names. Inventing one for them would make
            // believe they can be promoted separately, which a coherent section needs
            // and this operation does not do yet.
            object: None,
        });
    }
    out
}
