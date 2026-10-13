//! `wells-from-polylines` - a wellbore per polyline of a set.
//!
//! The need: there is already an operation that creates a well from
//! a polyline; it should also be possible to take a list of polylines
//! to create a list of wells - the first of the two ways
//! to do it: `well` taking a list, the cardinality declared since
//! the first day and exercised by nothing (`Slot::least`/`most`); the second,
//! a chaining of `well-from-polyline` run once per polyline, waits on
//! `yields` and the workflow interpreter.
//!
//! # Delegates, does not copy
//!
//! `crates/energyml-build/ARCHITECTURE.md`: an operation that writes the same object
//! as another delegates to it. Every wellbore is [`super::well::write`], the
//! same datum [`super::well::datum_in`], the same measured depths, the same
//! vertical detection - a second writer would be a second place for the same
//! mistakes. What differs from `well-from-polyline` is only what a **set**
//! adds: several polylines instead of refusing more than one
//! (`super::well::polylines_of`, shared rather than duplicated), and titles
//! that must tell the wells apart.
//!
//! # What a bulk creation decides that one well does not have to
//!
//! - **titles are derived, and it says so**: one `title` names the *set*, and
//!   each well is `"<title> - well <n>"`, `n` counted from 1 in the order the
//!   polylines were found - a shape's patches, then the feature collection's
//!   lines. A single shared title would collide: two wells would derive the
//!   same uuid;
//! - **every well gets its own feature, interpretation and datum** - this
//!   operation only ever *creates*, `well-from-polyline`'s "continue an
//!   existing interpretation" has no bulk form here, because there is no one
//!   interpretation N new polylines could all mean;
//! - **the ground is each polyline's own topmost point** - `well-from-polyline`
//!   only *suggests* it there, and a form usually keeps the suggestion; a
//!   single `ground` shared by every hole would put the wrong wells at the
//!   wrong depth. Reading it here rather than asking is what a *set* buys
//!   over running the single operation N times by hand;
//! - **one frame for the whole set**: creating a `LocalEngineeringCompoundCrs`
//!   per well would derive N identical systems under N different uuids -
//!   `crs::names` derives from what distinguishes a frame, and nothing does
//!   here.

use serde_json::{Value, json};

use energyml_domain::provenance::Parameter;

use crate::crs::FrameKind;
use crate::declare::{Features, Inputs, Params, Shape as ShapeIn, Suggestions};
use crate::operation::{
    About, ActivityDraft, Category, Context, Need, NewObject, Note, Operation, Plan, Refusal, Slot,
};
use crate::pieces::Pieces;

use super::support;
use super::well::{self, Datum, DatumKind, Found, TrajectoryShape, Wellbore};
use super::written::{FrameParams, NamingParams};

/// The operation.
pub struct WellsFromPolylines;

/// What `wells-from-polylines` is told - one frame and one set of curve
/// settings for every well; no `datum` and no `interpretation` overrides,
/// because a set only ever creates (see the module doc).
#[derive(Debug, Params)]
pub struct SetParams {
    /// Where the coordinates are.
    #[param(flatten)]
    pub frame: FrameParams,
    /// The title of the set, and the external file - each well is titled from it.
    #[param(flatten)]
    pub naming: NamingParams,
    /// the unit of the measured depths - the frame's vertical unit
    #[param(default = String::from("m"), optional)]
    pub md_uom: String,
    /// what the created datum is, at each wellhead
    #[param(default = DatumKind::GroundLevel, optional)]
    pub datum_kind: DatumKind,
    /// the trajectory through each polyline's points: a linear spline invents nothing; the cubics smooth; a vertical line is detected and written as such
    #[param(default = TrajectoryShape::Linear, optional)]
    pub shape: TrajectoryShape,
    /// a frame node every so many units of MD; left out, one per control point
    #[param(min = 0.0)]
    pub spacing: Option<f64>,
}

/// What `wells-from-polylines` takes: a **set** of polylines - one or more
/// shapes, or a feature collection whose lines are each a trace.
///
/// **`trace`, not `shape`**: this operation already has a setting named
/// `shape`, the *kind* of curve - the same reason `well-from-polyline`
/// declares it so.
#[derive(Debug, Inputs)]
pub struct SetInputs {
    /// shapes holding polylines - each patch's runs, and each shape given, become their own wellbore
    #[input(topology = "polylines", least = 1)]
    pub trace: Vec<ShapeIn>,
    /// a feature collection holding lines, each in its own system
    pub features: Option<Features>,
}

/// Every polyline of the set, found in order: each shape's patches, then the
/// collection's lines - the order titles and uuids are derived by.
fn every_polyline(inputs: &SetInputs) -> Vec<Found> {
    let mut out = Vec::new();
    for shape in &inputs.trace {
        out.extend(well::polylines_of(Some(shape), None));
    }
    out.extend(well::polylines_of(None, inputs.features.as_ref()));
    out
}

impl Operation for WellsFromPolylines {
    type Params = SetParams;
    type Inputs = SetInputs;

    fn about(&self) -> About {
        About {
            name: "wells-from-polylines",
            describe: "a wellbore per polyline of a set: the datum at the ground, the measured depths, a trajectory through the points, a frame on it - one feature and interpretation per well",
            category: Category::Wells,
            makes: &[
                "eml23.ReferencePointInALocalEngineeringCompoundCrs",
                "eml23.ReferencePointInACrs",
                "resqml22.WellboreFeature",
                "resqml22.WellboreInterpretation",
                "resqml22.WellboreTrajectoryRepresentation",
                "resqml22.WellboreFrameRepresentation",
            ],
            recipe: "wells-from-polylines",
            represents: Some(("resqml22.WellboreFeature", "resqml22.WellboreInterpretation")),
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        super::written::suggest_written(bag, None, slots);
    }

    fn check(&self, inputs: &SetInputs) -> Result<(), Refusal> {
        let found = every_polyline(inputs);
        if found.is_empty() {
            return Err(Refusal::on(
                "trace",
                "it takes one or more polylines - a shape, or a feature collection with a line; the input holds none",
            ));
        }
        for (at, line) in found.iter().enumerate() {
            well::valid_polyline(line)
                .map_err(|why| Refusal::on("trace", format!("polyline {at}: {why}")))?;
        }
        Ok(())
    }

    fn plan(&self, inputs: SetInputs, params: SetParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
        let identity = cx.identity;
        let when = cx.when;
        let mut rank = 0usize;
        let known = cx.known();

        // **One frame for the whole set** (see the module doc): a well needs
        // its own coordinates, not its own system.
        let frame = support::frame_in(&known, &params.frame, identity, when, &mut rank)?;
        // A wellbore is a vertical extent, exactly the refusal
        // `well-from-polyline` makes and for the same reason: a degree horizontal beside a metre vertical is not
        // a frame any reader reconciles.
        if frame.kind == Some(FrameKind::Geographic) {
            let (slot, what) = match params.frame.crs.as_ref() {
                Some(_) => (
                    "crs",
                    format!(
                        "the frame `{}` named",
                        frame.crs["Title"].as_str().unwrap_or_default()
                    ),
                ),
                None => ("project", "the frame created without `project`".to_string()),
            };
            return Err(Refusal::on(
                slot,
                format!(
                    "a wellbore is a vertical extent, and {what} measures its horizontal axes in degrees while its depths are linear: every hole would be written between a hundred and ten thousand times longer than the model is wide - name a projected system in `project` (EPSG:2154 for mainland France), or a LocalEngineeringCompoundCrs as `crs`"
                ),
            ));
        }
        let vertical_up = if frame.objects.is_empty() {
            known.first().and_then(|k| k.vertical_up)
        } else {
            Some(true)
        };

        let found = every_polyline(&inputs);
        if found.is_empty() {
            return Err(Refusal::on(
                "trace",
                "it takes one or more polylines; the input holds none",
            ));
        }
        for (at, line) in found.iter().enumerate() {
            well::valid_polyline(line)
                .map_err(|why| Refusal::on("trace", format!("polyline {at}: {why}")))?;
        }

        let base = params.naming.title();
        let uom = params.md_uom.clone();
        let file = params.naming.data_file.clone();
        let cite = |title: &str| -> Value {
            json!({
                "$type": "eml23.Citation",
                "Title": title,
                "Originator": when.originator,
                "Format": energyml_core::CITATION_FORMAT,
                "Creation": when.creation,
            })
        };

        let mut objects = frame.objects.clone();
        let mut datasets = Vec::new();
        let mut says = Vec::new();
        let mut parameters = frame.parameters.clone();
        let mut titles: Vec<String> = Vec::new();
        let mut total_nodes = 0usize;

        for (at, line) in found.iter().enumerate() {
            let title = format!("{base} - well {}", at + 1);
            let seed = cx.seed(&title);

            let placed: Vec<[f64; 3]> = line.placed_in(&frame)?;
            let ground = well::topmost(&line.zs(), vertical_up);

            let (datum, datum_objects) = well::datum_in(
                &Datum {
                    named: None,
                    head: [placed[0][0], placed[0][1]],
                    ground,
                    kind: params.datum_kind,
                    crs: &frame.crs,
                    vertical_crs: frame.vertical.as_ref(),
                    title: &title,
                    seed: &seed,
                    citation: &cite,
                },
                identity,
                &mut rank,
            )?;
            objects.extend(datum_objects);

            let feature_uuid = support::next_uuid(identity, &format!("well-feature:{seed}"), &mut rank)?;
            let feature_title = format!("{title} - feature");
            objects.push(NewObject {
                uuid: feature_uuid.clone(),
                qualified_type: "resqml22.WellboreFeature".to_string(),
                role: "feature".to_string(),
                title: feature_title.clone(),
                json: json!({
                    "$type": "resqml22.WellboreFeature",
                    "uuid": feature_uuid,
                    "schemaVersion": "2.2",
                    "Citation": cite(&feature_title),
                    "IsWellKnown": false,
                }),
            });
            let interpretation_uuid =
                support::next_uuid(identity, &format!("well-interpretation:{seed}"), &mut rank)?;
            let interpretation_title = format!("{title} - interpretation");
            objects.push(NewObject {
                uuid: interpretation_uuid.clone(),
                qualified_type: "resqml22.WellboreInterpretation".to_string(),
                role: "interpretation".to_string(),
                title: interpretation_title.clone(),
                json: json!({
                    "$type": "resqml22.WellboreInterpretation",
                    "uuid": interpretation_uuid,
                    "schemaVersion": "2.2",
                    "Citation": cite(&interpretation_title),
                    "Domain": "depth",
                    "IsDrilled": false,
                    "InterpretedFeature": {
                        "$type": "eml23.DataObjectReference",
                        "Uuid": feature_uuid,
                        "QualifiedType": "resqml22.WellboreFeature",
                        "Title": feature_title,
                    },
                }),
            });
            let interpretation = json!({
                "$type": "eml23.DataObjectReference",
                "Uuid": interpretation_uuid,
                "QualifiedType": "resqml22.WellboreInterpretation",
                "Title": interpretation_title,
            });

            let written = well::write(
                &Wellbore {
                    title: &title,
                    seed: &seed,
                    line: &placed,
                    mds: None,
                    ground,
                    uom: &uom,
                    shape: params.shape,
                    tangents: None,
                    spacing: params.spacing,
                    datum: &datum,
                    interpretation: &interpretation,
                    file: &file,
                    vertical_up,
                    crs: &frame.crs,
                    citation: &cite,
                    keep: None,
                },
                identity,
                &mut rank,
            )?;
            total_nodes += written.nodes;
            objects.extend(written.objects);
            datasets.extend(written.datasets);
            for note in written.says {
                says.push(Note {
                    key: note.key,
                    detail: format!("`{title}`: {}", note.detail),
                });
            }
            titles.push(title);
        }

        parameters.push(Parameter::Double {
            title: "wellCount".to_string(),
            value: found.len() as f64,
            uom: None,
        });
        parameters.push(Parameter::Double {
            title: "frameNodeCount".to_string(),
            value: total_nodes as f64,
            uom: None,
        });
        says.push(Note {
            key: "wells",
            detail: format!(
                "{} wellbore(s) from {} polyline(s), titled {}",
                titles.len(),
                found.len(),
                titles.join(", ")
            ),
        });
        says.extend(frame.says.clone());

        Ok(Plan {
            media: Vec::new(),
            blocked: frame.blocked,
            objects,
            datasets,
            // No shape: same reason as `well-from-polyline` - a parametric
            // line's control points are its trajectory only when linear.
            shapes: Vec::new(),
            activity: ActivityDraft {
                recipe: "wells-from-polylines",
                title: format!("wells-from-polylines → {base}"),
                seed: base,
                parameters,
            },
            says,
            dropped: Vec::new(),
        })
    }

    fn reports(&self) -> Vec<Slot> {
        support::frame_reports(vec![
            Slot::required(
                "wellCount",
                Need::Number {
                    uom: None,
                    least: None,
                    most: None,
                },
                "wellbores written",
            ),
            Slot::required(
                "frameNodeCount",
                Need::Number {
                    uom: None,
                    least: None,
                    most: None,
                },
                "frame nodes written, total",
            ),
        ])
    }
}
