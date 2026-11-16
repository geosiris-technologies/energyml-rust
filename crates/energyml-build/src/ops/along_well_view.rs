//! `along-well-view` - a geosteering section drawn, stored in the package: its
//! SVG (and PNG) attached to the frame of the curtain, and an `Activity`
//! whose template says it is a geosteering section and whose inputs are the
//! objects it was computed from.
//!
//! The model is [`super::section_view`]'s, applied to the geosteering section
//! of `energyml_section::along_well`: **the drawing is the input, and the
//! recipe is read from it** - one fact. The `<metadata>` of the SVG is the
//! `along-well-view` element, read back by
//! `energyml_section::along_well::read_metadata`; a setting given beside it
//! is held to it, never taken in its place. The operation draws nothing: a PNG
//! is the caller's, made by `energyml_sheet::draw::png::render` **from the
//! SVG it passes**.
//!
//! One operation per sort of view result, because the template of an
//! `Activity` declares each of its parameters (D6, S15) and a generic
//! template cannot.
//!
//! The reading is `read::stored_views`, which finds the activity by the uuid
//! of its template.
//!
//! # What is checked
//!
//! - the SVG has an `along-well-view` element of this format;
//! - every uri the request names (the frame, the trajectory, the curtain's
//!   property and axes, the boundaries' properties, the surfaces) is one of
//!   `objects`, **compared on the full uri**; one that is not held is refused
//!   naming it;
//! - every setting given agrees with the request and the view;
//! - the PNG is a PNG (its signature, never its name) of the SVG's proportions.

use energyml_domain::provenance::Parameter;
use energyml_section::along_well::{AxisLinkAsk, Request, View, read_metadata};
use serde_json::{Value, json};

use super::media::Document;
use super::section_view::{Named, attachment_named, folder_named, named_in, part_named, png_size};
use crate::declare::{Inputs, ObjectIn, Params, Suggestions};
use crate::operation::{About, Category, Context, Need, Note, Operation, Plan, Refusal, Slot};
use crate::pieces::{Known, Pieces, Ref, Source};

/// The operation.
pub struct AlongWellView;

/// What `along-well-view` is told. Every setting of the drawing is a
/// parameter of the template: suggested from the drawing, and refused when an
/// answer disagrees with it (the rule of `section-view`).
#[derive(Debug, Params)]
pub struct AlongWellViewParams {
    /// the SVG's part name in the package - left out, `media/along-well/`, a folder named after the frame, and the file's name
    pub svg_path: Option<String>,
    /// the PNG's part name in the package - left out, beside the SVG
    pub png_path: Option<String>,
    /// what the images are, in a word (64 characters at most)
    #[param(default = String::from("AlongWellView"), optional)]
    pub category: String,
    /// the projection the drawing was made in - `md_tvd`, `developed`, `vertical_section`
    pub projection: String,
    /// for `vertical_section`: the azimuth of the plane, degrees clockwise from grid north
    pub section_azimuth: Option<f64>,
    /// the lateral row shown, by its rank along the lateral axis
    pub lateral_index: Option<f64>,
    /// the first measured depth drawn, in metres
    pub md_from: Option<f64>,
    /// the last measured depth drawn, in metres
    pub md_to: Option<f64>,
    /// the sampling step of the horizons and the tops, in metres
    pub step: Option<f64>,
    /// whether the tops computed from the surfaces were drawn
    #[param(default = false, optional)]
    pub computed_tops: bool,
}

/// What `along-well-view` takes.
#[derive(Debug, Inputs)]
pub struct AlongWellViewInputs {
    /// the geosteering section, drawn as SVG - what it was computed from is read from its metadata
    pub svg: Document,
    /// the same drawing as a PNG, for a reader that draws no SVG
    pub png: Option<Document>,
    /// the objects of the package the section may have been computed from - those it names are recorded as the activity's inputs, and one it names that the package does not hold is refused
    #[input(
        types = "resqml22.WellboreFrameRepresentation, resqml22.WellboreTrajectoryRepresentation, resqml22.ContinuousProperty, eml23.ColumnBasedTable, resqml22.TriangulatedSetRepresentation, resqml22.Grid2dRepresentation, resqml20.obj_WellboreFrameRepresentation, resqml20.obj_WellboreTrajectoryRepresentation, resqml20.obj_ContinuousProperty, resqml20.obj_TriangulatedSetRepresentation, resqml20.obj_Grid2dRepresentation",
        every
    )]
    pub objects: Vec<ObjectIn>,
}

// -- the drawing's own words -------------------------------------------------------------

/// The drawing as text.
fn text_of(svg: &Document) -> Result<&str, Refusal> {
    std::str::from_utf8(&svg.bytes)
        .map_err(|_| Refusal::on("svg", format!("`{}` is not UTF-8 text, and an SVG is", svg.file)))
}

/// The request and the view of a drawing, or why it is refused.
fn metadata_of(svg: &Document) -> Result<(Request, View), Refusal> {
    read_metadata(text_of(svg)?).map_err(|why| {
        Refusal::on(
            "svg",
            format!("`{}` states no geosteering section: {why}", svg.file),
        )
    })
}

/// What the drawing states of each setting, slot by slot.
fn drawn(request: &Request) -> Vec<(&'static str, Value)> {
    let mut out = vec![
        ("projection", json!(request.projection)),
        ("computedTops", json!(request.computed_tops)),
    ];
    if let Some(section) = &request.section {
        out.push(("sectionAzimuth", json!(section.azimuth.to_degrees())));
    }
    if let Some(curtain) = &request.curtain {
        out.push(("lateralIndex", json!(curtain.lateral_index as f64)));
    }
    if let Some([from, to]) = request.md_range {
        out.push(("mdFrom", json!(from)));
        out.push(("mdTo", json!(to)));
    }
    if let Some(step) = request.step {
        out.push(("step", json!(step)));
    }
    out
}

/// What the parameters say, slot by slot.
fn given(p: &AlongWellViewParams) -> Vec<(&'static str, Option<Value>)> {
    vec![
        ("projection", Some(json!(p.projection))),
        ("computedTops", Some(json!(p.computed_tops))),
        ("sectionAzimuth", p.section_azimuth.map(|v| json!(v))),
        ("lateralIndex", p.lateral_index.map(|v| json!(v))),
        ("mdFrom", p.md_from.map(|v| json!(v))),
        ("mdTo", p.md_to.map(|v| json!(v))),
        ("step", p.step.map(|v| json!(v))),
    ]
}

/// Two JSON values are one value whatever their spelling (`3` and `3.0`), a
/// number up to the rounding of a conversion of angle.
fn same(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) if a.is_number() && b.is_number() => (x - y).abs() <= 1e-9 * x.abs().max(1.0),
        _ => a == b,
    }
}

// -- the objects the request names ---------------------------------------------------------

/// Every uri the request names, with the field it comes from, the frame first.
fn named_by(request: &Request) -> Vec<(&'static str, String)> {
    let mut out = vec![("frame", request.frame.clone())];
    if let Some(trajectory) = &request.trajectory {
        out.push(("trajectory", trajectory.clone()));
    }
    if let Some(curtain) = &request.curtain {
        out.push(("curtain property", curtain.property.clone()));
        for link in &curtain.links {
            match link {
                AxisLinkAsk::Table { table, .. } => out.push(("axis table", table.clone())),
                AxisLinkAsk::PairedProperty { property, .. } => out.push(("axis property", property.clone())),
                AxisLinkAsk::WitsmlChannel { channel } => out.push(("axis channel", channel.clone())),
            }
        }
    }
    if let Some(boundaries) = &request.boundaries {
        out.push(("boundary radius", boundaries.radius.clone()));
        out.push(("boundary angle", boundaries.angle.clone()));
    }
    for surface in &request.surfaces {
        out.push(("surface", surface.clone()));
    }
    out
}

// -- the pictures --------------------------------------------------------------------------

/// A part name for a file: the one stated, or `media/along-well/<the frame>/`
/// and its name - a folder per frame, so that drawing a section again writes
/// over its own files and nobody else's.
fn part_of(stated: &Option<String>, frame_title: &str, file: &Document) -> String {
    part_named(
        stated,
        "media/along-well",
        &folder_named(frame_title, "frame"),
        file,
    )
}

/// One image attached to the frame, by the one writer of attachments, named by
/// the frame and what it is (`SVG`, `PNG`): the same section stored twice
/// writes the same object over itself.
#[allow(clippy::too_many_arguments)]
fn attached(
    cx: &Context<'_>,
    rank: &mut usize,
    target: &ObjectIn,
    category: &str,
    file: Document,
    part: String,
    content_type: &str,
    what: &str,
    frame: &Ref,
) -> Result<Plan, Refusal> {
    attachment_named(
        cx,
        rank,
        target.clone(),
        file,
        part,
        category,
        content_type,
        format!("{} - {what}", frame.title),
        &format!(
            "along-well-view:attachment:{}:{what}",
            frame.uuid.to_ascii_lowercase()
        ),
    )
}

impl Operation for AlongWellView {
    type Params = AlongWellViewParams;
    type Inputs = AlongWellViewInputs;

    fn about(&self) -> About {
        About {
            name: "along-well-view",
            describe: "a geosteering section drawn, stored in the package: its SVG and PNG attached to the frame of the curtain, and an activity naming the objects it was computed from",
            category: Category::Create,
            makes: &["eml23.Attachment"],
            recipe: "along-well-view",
            represents: None,
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        // What the drawing was computed with, read off the drawing itself.
        if let Some(Source::Document {
            name,
            content_type,
            bytes,
        }) = bag.get("svg")
        {
            let svg = Document {
                name: "svg".to_string(),
                file: name.clone(),
                declared: content_type.clone(),
                bytes: bytes.clone(),
            };
            if let Ok((request, _)) = metadata_of(&svg) {
                for (slot, value) in drawn(&request) {
                    slots.set(slot, value);
                }
            }
        }
    }

    fn plan(
        &self,
        inputs: AlongWellViewInputs,
        params: AlongWellViewParams,
        cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        // -- the drawing, read back, and what was said of it held to it
        let (request, view) = metadata_of(&inputs.svg)?;
        let states = drawn(&request);
        for (slot, told) in given(&params) {
            let Some(told) = told else { continue };
            match states.iter().find(|(name, _)| *name == slot) {
                Some((_, value)) if same(value, &told) => {}
                Some((_, value)) => {
                    return Err(Refusal::on(
                        slot,
                        format!(
                            "the drawing was made with {value}, and {told} is given: the activity would describe a run that did not happen - draw it again with {told}, or leave `{slot}` out"
                        ),
                    ));
                }
                // A flag that is false says nothing more than its default; a
                // projection is always stated.
                None if told == json!(false) => {}
                None => {
                    return Err(Refusal::on(
                        slot,
                        format!(
                            "the drawing says nothing of `{slot}`, and {told} is given - draw it again with it, or leave it out"
                        ),
                    ));
                }
            }
        }

        // -- the PNG: a PNG, of the same proportions
        let png = match &inputs.png {
            None => None,
            Some(file) => {
                let Some([w, h]) = png_size(&file.bytes) else {
                    return Err(Refusal::on(
                        "png",
                        format!(
                            "`{}` is not a PNG - it does not start with the PNG signature",
                            file.file
                        ),
                    ));
                };
                let size = view.size;
                let (drawn_ratio, given_ratio) = (size[0] / size[1], f64::from(w) / f64::from(h));
                // A pixel of rounding on either side, and no more.
                let slack = 1.0 / f64::from(w.min(h)).max(1.0) + 1.0 / size[0].min(size[1]).max(1.0);
                if (given_ratio / drawn_ratio - 1.0).abs() > slack {
                    return Err(Refusal::on(
                        "png",
                        format!(
                            "`{}` is {w}x{h} px, and the drawing is {}x{} px: not the same proportions, so not the same drawing",
                            file.file, size[0], size[1]
                        ),
                    ));
                }
                Some(file.clone())
            }
        };

        // -- the objects the request names, each held
        let held: Vec<&Ref> = inputs
            .objects
            .iter()
            .filter_map(|one| one.known.reference.as_ref())
            .collect();
        let mut named: Vec<&Ref> = Vec::new();
        for (field, uri) in named_by(&request) {
            let found = match named_in(&held, &uri) {
                Named::One(found) => found,
                Named::None => {
                    return Err(Refusal::on(
                        "objects",
                        format!(
                            "the drawing was computed from `{uri}` (its {field}), which this package does not hold - it was drawn from another package, or before that object was written"
                        ),
                    ));
                }
                Named::Several(uris) => {
                    return Err(Refusal::on(
                        "objects",
                        format!(
                            "the drawing was computed from `{uri}` (its {field}), which names an object held in several versions - {} - name one with its version",
                            uris.join(", ")
                        ),
                    ));
                }
            };
            if !named.iter().any(|one| one.same_object(found)) {
                named.push(found);
            }
        }
        // The frame is the first: it carries the images.
        let frame: &Ref = named[0];
        let frame_in = inputs
            .objects
            .iter()
            .find(|one| one.known.reference.as_ref().is_some_and(|r| r.same_object(frame)))
            .map(|one| one.known.clone())
            .unwrap_or_default();

        // -- the images, attached to the frame by the one writer of attachments
        let target = ObjectIn {
            name: "target".to_string(),
            known: Known {
                reference: Some(frame.clone()),
                ..frame_in
            },
        };
        let mut rank = 0usize;
        let svg_part = part_of(&params.svg_path, &frame.title, &inputs.svg);
        let mut plan = attached(
            cx,
            &mut rank,
            &target,
            &params.category,
            inputs.svg.clone(),
            svg_part.clone(),
            "image/svg+xml",
            "SVG",
            frame,
        )?;
        let mut parameters = vec![Parameter::Text {
            title: "svgPart".to_string(),
            value: svg_part.clone(),
        }];
        if let Some(file) = png {
            let part = part_of(&params.png_path, &frame.title, &file);
            let more = attached(
                cx,
                &mut rank,
                &target,
                &params.category,
                file,
                part.clone(),
                "image/png",
                "PNG",
                frame,
            )?;
            plan.objects.extend(more.objects);
            plan.media.extend(more.media);
            parameters.push(Parameter::Text {
                title: "pngPart".to_string(),
                value: part,
            });
        }
        for one in &named {
            parameters.push(Parameter::Object {
                title: "input".to_string(),
                reference: one.dor(),
            });
        }
        plan.says.insert(
            0,
            Note {
                key: "drawing",
                detail: format!(
                    "the images are the drawing's: the request and the view are read from `{}`, and the objects it names are the run's inputs",
                    inputs.svg.file
                ),
            },
        );
        plan.activity.recipe = "along-well-view";
        plan.activity.title = format!("along-well view → {}", frame.title);
        plan.activity.seed = format!("along-well-view:{}:{svg_part}", frame.uuid.to_ascii_lowercase());
        plan.activity.parameters = parameters;
        Ok(plan)
    }

    fn reports(&self) -> Vec<Slot> {
        vec![
            Slot::required("svgPart", Need::Text, "the SVG's part name in the package"),
            Slot::optional("pngPart", Need::Text, "the PNG's part name in the package"),
        ]
    }
}
