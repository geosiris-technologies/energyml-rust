//! `computed-tops` — where a well crosses one or several horizons: the
//! measured depth and the position of each crossing, written as markers.
//!
//! Intersect a trajectory with a set of horizons, giving a
//! `WellboreMarkerFrameRepresentation` — the everyday geological operation. The computation is
//! [`energyml_analysis::tops`]; the well is evaluated by
//! `energyml_geometry::well::path_of` — the reader that draws it, minimum
//! curvature included — and each horizon read by [`energyml_analysis::heights`].
//! Neither is written a second time here.
//!
//! # What is written, and why not a marker frame
//!
//! **`WellboreMarkerFrameRepresentation` does not exist in RESQML 2.2**, which
//! is what this crate writes: *« Wells have been completely redesigned in this
//! version. In general, RESQML has adopted use of relevant WITSML v2.0+ data
//! objects and removed related RESQML data objects […] These data objects
//! include: wellbore markers/marker sets and wellbore »* and *« New
//! representation WellboreIntervalSet allows multiple stratigraphic
//! interpretations of markers »* (RESQML v2.2 Release Notes, p. 6). The original
//! name for the output is the 2.0.1 one; what 2.2 says is:
//!
//! - **one WITSML 2.1 `WellboreMarker` per crossing**, a root object — its `Md`
//!   the depth of the crossing, counted from the trajectory's datum in the
//!   trajectory's unit, its `WellboreMarkerKind` `stratigraphic` (*« A high
//!   level classification of what this marker represents: stratigraphic
//!   information, a point of interest in a well or something else »*, WITSML
//!   TRG v2.1 §24.6 p. 583; the two values §24.7 p. 584). A root and not a
//!   `FormationMarker` of a set, because a boundary references it
//!   (the reference model requires a boundary's `Marker` to be an
//!   object);
//! - **one RESQML 2.2 `WellboreIntervalSet`** whose `IntervalBoundaries` are the
//!   crossings in the order of the hole — *« An array of (0..*) MarkerBoundary
//!   […] defining the intervals, an array of N boundaries, will create N+1
//!   intervals »* (RESQML TRG v2.2 §12.15 p. 312) — each a `MarkerBoundary`
//!   whose `Interpretation` is the horizon's (*« 0..1 To:
//!   BoundaryFeatureInterpretation »*, §12.6 p. 302), its `GeologicBoundaryKind`
//!   `horizon` (a fault's interpretation gives `fault`, §12.5 p. 301), and its
//!   `Marker` the WITSML marker. The **N+1 `MarkerInterval`s are empty**: which
//!   unit lies between two horizons is nothing a crossing says. A boundary has
//!   no depth of its own — it has no MD on the boundary: the depth comes from the
//!   marker.
//!
//! The **position** of a crossing is the path at its measured depth — what any
//! reader gets back from the marker's `Md` and the trajectory — so it is said
//! in the plan and drawn as a point cloud, and not stored a second time. A
//! `Tvd` would need a vertical datum decision this operation has no reason to
//! make.
//!
//! # What is said rather than written
//!
//! - **a horizon crossed twice gives two markers**, both said: a well that goes
//!   down through a horizon and back up through it meets it twice;
//! - **a horizon never reached gives none**, counted in `Plan::dropped` and
//!   named with what the well did instead — stayed above or below it (and how
//!   close it came), or passed beside it;
//! - **a crossing over a fold, or where the surface is not** (a hole, its edge)
//!   is no marker: over a fold several faces have a height and which one the
//!   well meets is not a numerical choice — a ray caster's job —, and
//!   across a hole it met no face at all. Both are named with their span of
//!   depth;
//! - **no crossing at all is a refusal**: a plan with nothing to write reads
//!   exactly like *nothing to do here*.
//!
//! # One frame
//!
//! The line is evaluated in the local numbers of the trajectory's CRS, the way
//! a plan that writes reads every shape. A horizon whose CRS is another one is
//! **refused naming both**, and so is a well whose drawn nodes are not where
//! its line puts them — a shape read placed (a plan for the scene) against a
//! line read local, the frame mismatch the placement layer would otherwise
//! hide.
//!
//! # References
//!
//! - The crossing of a path and a surface is a root found by bisection between a point above and a point below the surface, down to the last representable measured depth (`energyml_analysis::tops`).
//!   TODO(reference): verify - bisection is described without a citation; add a numerical-analysis reference once checked.

use energyml_analysis::heights::Heights;
use energyml_analysis::tops::{self as analysis, Crossing, Side, Tops};
use energyml_domain::provenance::Parameter;
use energyml_eml_2_3 as eml;
use energyml_geometry::geometry::{Geometry, Patch, Topology};
use energyml_resqml_2_2 as resqml;
use energyml_witsml_2_1 as witsml;

use crate::declare::{Inputs, Params, Shape, Title};
use crate::operation::{
    About, ActivityDraft, Category, Context, Dropped, Made, NewObject, Note, Operation, Plan, Refusal, Slot,
};
use crate::operation::Need;
use crate::pieces::{Known, KnownPath};

use super::support;

/// The operation.
pub struct ComputedTops;

/// What `computed-tops` is told.
#[derive(Debug, Params)]
pub struct ComputedTopsParams {
    /// the sampling step along the hole, in the unit of its measured depths; left out, half the median edge, in plan, of the finest horizon
    ///
    /// Two crossings closer than one step along the hole cancel and are
    /// missed — the one limit of the method (`energyml_analysis::tops`). Half
    /// an edge samples every face the hole passes over at least twice: a path
    /// moves at most one unit in plan per unit of measured depth.
    #[param(min = 0.0)]
    pub step: Option<f64>,
    /// the title of the interval set; left out, the well's, followed by « computed tops »
    pub title: Option<Title>,
}

/// What `computed-tops` takes.
#[derive(Debug, Inputs)]
pub struct ComputedTopsInputs {
    /// the well — a trajectory of the package, or a frame, whose trajectory is then followed
    #[input(types = "resqml22.WellboreTrajectoryRepresentation, resqml22.WellboreFrameRepresentation")]
    pub well: Shape,
    /// the horizons — triangulated surfaces or 2D grids, in the well's frame
    ///
    /// `triangles` ranks the form's list; the cells of a grid are read too,
    /// bilinearly, by `energyml_analysis::heights`.
    #[input(topology = "triangles", least = 1)]
    pub horizons: Vec<Shape>,
}

/// The title the object behind a shape carries, or the shape's name.
fn title_of(shape: &Shape) -> String {
    shape
        .known
        .as_ref()
        .and_then(|known| known.reference.as_ref())
        .map(|reference| reference.title.clone())
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| shape.name.clone())
}

/// Which side, in words — the frame's vertical sense decides *above*.
fn side_word(side: Side, up: Option<bool>) -> &'static str {
    match (side, up) {
        (Side::Greater, Some(true)) | (Side::Less, Some(false)) => "above",
        (Side::Less, Some(true)) | (Side::Greater, Some(false)) => "below",
        (Side::Greater, None) => "on the side of larger z",
        (Side::Less, None) => "on the side of smaller z",
    }
}

/// A number as a note prints it.
fn at(p: [f64; 3]) -> String {
    format!("({:.3}, {:.3}, {:.3})", p[0], p[1], p[2])
}

/// The boundary kind an interpretation says, and whether it may be referenced
/// as a `BoundaryFeatureInterpretation` (RESQML TRG v2.2 §12.6 p. 302).
fn boundary_of(known: Option<&Known>) -> (resqml::GeologicBoundaryKind, Option<eml::DataObjectReference>) {
    let Some(interpretation) = known.and_then(|known| known.interpretation.as_ref()) else {
        return (resqml::GeologicBoundaryKind::Horizon, None);
    };
    let class = interpretation.qualified_type.rsplit('.').next().unwrap_or("");
    let kind = match class {
        "FaultInterpretation" => resqml::GeologicBoundaryKind::Fault,
        "GeobodyBoundaryInterpretation" => resqml::GeologicBoundaryKind::Geobody,
        "HorizonInterpretation" | "BoundaryFeatureInterpretation" => resqml::GeologicBoundaryKind::Horizon,
        _ => return (resqml::GeologicBoundaryKind::Horizon, None),
    };
    let reference = crate::crs::reference_of(interpretation);
    (kind, Some(reference))
}

/// The drawn nodes of the well against its line: equal when both were read in
/// one frame. A frame's nodes are its `NodeMd` on the line, a trajectory's its
/// knots.
fn drawn_elsewhere(well: &Shape, known: &Known, path: &energyml_geometry::parametric::Path) -> Option<(usize, f64)> {
    let nodes: Vec<[f64; 3]> = well.geometry.patches.iter().flat_map(|patch| patch.points.iter().copied()).collect();
    let mds: Vec<f64> = match (&known.node_mds, path.knots()) {
        (Some(mds), _) => mds.clone(),
        (None, Some(knots)) => knots.to_vec(),
        (None, None) => return None,
    };
    if mds.len() != nodes.len() {
        return None;
    }
    let mut worst: Option<(usize, f64)> = None;
    for (rank, (node, md)) in nodes.iter().zip(&mds).enumerate() {
        let line = path.at(*md);
        let gap = (0..3).map(|k| (node[k] - line[k]).powi(2)).sum::<f64>().sqrt();
        // A millimetre, plus the rounding of map coordinates: the two are the
        // same evaluation of the same line when the frames agree.
        let scale = node.iter().fold(1.0f64, |s, c| s.max(c.abs()));
        if gap.is_finite() && gap > 1e-3 + 1e-12 * scale && worst.is_none_or(|(_, w)| gap > w) {
            worst = Some((rank, gap));
        }
    }
    worst
}

/// One crossing, and the horizon it is of.
struct Found<'a> {
    horizon: usize,
    shape: &'a Shape,
    crossing: Crossing,
    /// Its rank among the crossings of that horizon, from 1, and their count.
    nth: (usize, usize),
}

impl Operation for ComputedTops {
    type Params = ComputedTopsParams;
    type Inputs = ComputedTopsInputs;

    fn about(&self) -> About {
        About {
            name: "computed-tops",
            describe: "where a well crosses one or several horizons — the measured depth and position of each crossing, written as WITSML wellbore markers gathered by a RESQML wellbore interval set",
            category: Category::Wells,
            makes: &["witsml21.WellboreMarker", "resqml22.WellboreIntervalSet"],
            recipe: "computed-tops",
            represents: None,
        }
    }

    fn also(&self) -> &'static [Category] {
        &[Category::Analysis]
    }

    fn check(&self, inputs: &ComputedTopsInputs) -> Result<(), Refusal> {
        if inputs.well.geometry.patches.iter().all(|patch| patch.points.is_empty()) {
            return Err(Refusal::on("well", format!("`{}` places no node", inputs.well.name)));
        }
        for horizon in &inputs.horizons {
            if !support::Faces::of(&horizon.geometry).any() {
                return Err(Refusal::on(
                    "horizons",
                    format!(
                        "`{}` has no face — a crossing is read under the hole on triangles or a grid's cells, and points or lines have none",
                        horizon.name
                    ),
                ));
            }
        }
        Ok(())
    }

    fn plan(&self, inputs: ComputedTopsInputs, params: ComputedTopsParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
        let ComputedTopsInputs { well, horizons } = inputs;
        let well_title = title_of(&well);

        // -- the well: its line, its depths ---------------------------------
        let known = well.known.as_ref().ok_or_else(|| {
            Refusal::on(
                "well",
                format!("`{well_title}` is no object of the package: the tops are measured depths along a trajectory, and a shape of the scene has none"),
            )
        })?;
        let KnownPath { trajectory, path, md_uom, datum } = known.path.as_ref().ok_or_else(|| {
            Refusal::on("well", format!("`{well_title}` is neither a wellbore trajectory nor a frame: there is no hole to follow"))
        })?;
        let path = path.as_ref().map_err(|why| Refusal::on("well", format!("its line cannot be followed — {why}")))?;
        let knots = path.knots().ok_or_else(|| {
            Refusal::on("well", format!("`{}` states no measured depth at its stations", trajectory.title))
        })?;
        let md_uom = md_uom.clone().ok_or_else(|| {
            Refusal::on(
                "well",
                format!("`{}` does not say what its measured depths count in (`MdInterval.Uom`): a marker's depth is a length with its unit", trajectory.title),
            )
        })?;
        energyml_domain::units::metres(1.0, &md_uom).map_err(|why| {
            Refusal::on("well", format!("`{}` counts its measured depths in `{md_uom}`, which is no length: {why}", trajectory.title))
        })?;
        let uom = <eml::LengthUomExt as energyml_core::xml::XmlText>::from_xml_text(&md_uom)
            .map_err(|why| Refusal::on("well", format!("`{md_uom}` is no length unit a marker can write: {why}")))?;
        if let Some((rank, gap)) = drawn_elsewhere(&well, known, path) {
            return Err(Refusal::on(
                "well",
                format!(
                    "`{well_title}` is drawn {gap:.3} away from where its line puts node {rank}: the shape was read in another frame than the line (placed for the scene, where the line is local to its CRS) — plan for writing, or bring the two into one frame"
                ),
            ));
        }
        let path_at = |md: f64| path.at(md);

        // -- one frame ----------------------------------------------------------
        let mut says: Vec<Note> = Vec::new();
        let well_crs = known.crs.as_ref();
        let mut unchecked = Vec::new();
        for horizon in &horizons {
            let theirs = horizon.known.as_ref().and_then(|k| k.crs.as_ref());
            match (well_crs, theirs) {
                (Some(a), Some(b)) if a.uuid != b.uuid => {
                    return Err(Refusal::on(
                        "horizons",
                        format!(
                            "`{}` is in `{}` and the well `{well_title}` in `{}`: a height read in one frame under a position of the other is a difference of two origins — bring them into one frame first",
                            title_of(horizon),
                            b.title,
                            a.title
                        ),
                    ));
                }
                (Some(_), Some(_)) => {}
                _ => unchecked.push(title_of(horizon)),
            }
            let theirs = horizon.known.as_ref().and_then(|k| k.vertical_uom.as_deref());
            if let (Some(a), Some(b)) = (known.vertical_uom.as_deref(), theirs)
                && a != b
            {
                return Err(Refusal::on(
                    "horizons",
                    format!("`{}` counts its heights in `{b}` and the well `{well_title}` in `{a}` — one unit, or no crossing", title_of(horizon)),
                ));
            }
        }
        if !unchecked.is_empty() {
            says.push(Note {
                key: "frame-unchecked",
                detail: format!(
                    "the frame of {} against the well's could not be checked — one of the two names no CRS; they are taken to be in one frame",
                    unchecked.iter().map(|t| format!("`{t}`")).collect::<Vec<_>>().join(", ")
                ),
            });
        }

        // -- the step -------------------------------------------------------------
        let faces: Vec<support::Faces> = horizons.iter().map(|h| support::Faces::of(&h.geometry)).collect();
        let length = knots[knots.len() - 1] - knots[0];
        let step = match params.step {
            Some(given) => given,
            None => {
                let finest = faces.iter().filter_map(|f| analysis::plan_spacing(&f.surface())).fold(f64::INFINITY, f64::min);
                // Never more samples than the analysis takes: a long hole
                // over a fine grid gets the step the ceiling allows, and says so.
                let floor = length / (analysis::MOST_SAMPLES as f64 / 2.0);
                let half = if finest.is_finite() { finest / 2.0 } else { length / 1000.0 };
                if half < floor {
                    says.push(Note {
                        key: "step-floored",
                        detail: format!("half the finest horizon's edge would be {half} along {length} of hole; the step is {floor}, the most samples a path takes"),
                    });
                }
                half.max(floor)
            }
        };

        // -- each horizon ---------------------------------------------------------
        let up = known.vertical_up;
        let mut outcomes: Vec<Tops> = Vec::with_capacity(horizons.len());
        for (horizon, faces) in horizons.iter().zip(&faces) {
            let heights = Heights::of(faces.surface(), "horizon", "under each sample of the hole")
                .map_err(|why| Refusal::on("horizons", format!("`{}`: {why}", title_of(horizon))))?;
            let found = analysis::tops(&path_at, knots, step, &heights).map_err(|why| Refusal::on("step", why))?;
            outcomes.push(found);
            cx.stop_if_cancelled()?;
        }

        // -- what each horizon gave -------------------------------------------------
        let mut not_reached = Vec::new();
        let (mut gaps, mut folded, mut unresolved, mut touches) = (0usize, 0usize, 0usize, 0usize);
        let mut all: Vec<Found> = Vec::new();
        for (index, (horizon, found)) in horizons.iter().zip(&outcomes).enumerate() {
            let title = title_of(horizon);
            let count = found.crossings.len();
            for (rank, crossing) in found.crossings.iter().enumerate() {
                all.push(Found { horizon: index, shape: horizon, crossing: crossing.clone(), nth: (rank + 1, count) });
            }
            if count > 1 {
                says.push(Note {
                    key: "crossed-again",
                    detail: format!(
                        "`{title}` is crossed {count} times, at MD {} {md_uom}: a marker each",
                        found.crossings.iter().map(|c| format!("{:.3}", c.md)).collect::<Vec<_>>().join(", ")
                    ),
                });
            }
            let spans = |list: &[[f64; 2]]| list.iter().map(|s| format!("{:.3}–{:.3}", s[0], s[1])).collect::<Vec<_>>().join(", ");
            if !found.folded.is_empty() {
                folded += found.folded.len();
                says.push(Note {
                    key: "folded",
                    detail: format!(
                        "`{title}`: the hole changes side over a fold of it (MD {} {md_uom}) — several faces lie there, which one it meets is not a numerical choice: no marker",
                        spans(&found.folded)
                    ),
                });
            }
            if !found.gaps.is_empty() {
                gaps += found.gaps.len();
                says.push(Note {
                    key: "through-a-gap",
                    detail: format!(
                        "`{title}`: the hole changes side where the surface is not — off its edge or over a hole (MD {} {md_uom}): no face was crossed, no marker",
                        spans(&found.gaps)
                    ),
                });
            }
            if !found.unresolved.is_empty() {
                unresolved += found.unresolved.len();
                says.push(Note {
                    key: "unresolved",
                    detail: format!(
                        "`{title}`: a crossing bracketed at MD {} {md_uom} falls where the surface has no single height: no marker",
                        spans(&found.unresolved)
                    ),
                });
            }
            if !found.touches.is_empty() {
                touches += found.touches.len();
                says.push(Note {
                    key: "touches",
                    detail: format!(
                        "`{title}`: the hole meets it at MD {} {md_uom} and leaves on the side it came from — no crossing",
                        found.touches.iter().map(|md| format!("{md:.3}")).collect::<Vec<_>>().join(", ")
                    ),
                });
            }
            if count == 0 {
                let why = match (found.side, found.closest) {
                    (Some(side), Some(closest)) => {
                        format!("stays {} it along the whole hole — {closest:.3} at the closest", side_word(side, up))
                    }
                    _ if found.over == 0 && found.over_fold == 0 => "passes beside it: no station of the hole lies over it".to_string(),
                    _ if found.over == 0 => "lies over a fold of it only".to_string(),
                    _ => "never goes through a face of it".to_string(),
                };
                not_reached.push(format!("`{title}`: the well {why}"));
            }
        }
        if all.is_empty() {
            return Err(Refusal::new(
                "no-crossing",
                format!("`{well_title}` crosses none of the horizons — {}", not_reached.join("; ")),
            ));
        }
        for why in &not_reached {
            says.push(Note { key: "not-reached", detail: why.clone() });
        }
        // The order of the hole; two horizons at one depth keep the order given.
        all.sort_by(|a, b| a.crossing.md.total_cmp(&b.crossing.md).then(a.horizon.cmp(&b.horizon)));

        // -- the objects -------------------------------------------------------------
        let citation = |title: &str, description: String| eml::Citation {
            title: title.to_string(),
            originator: cx.when.originator.clone(),
            creation: cx.when.creation.clone(),
            format: energyml_core::CITATION_FORMAT.to_string(),
            description: Some(description),
            ..Default::default()
        };
        let datum_ref = datum.as_ref().map(crate::crs::reference_of);
        let mut objects = Vec::new();
        let mut boundaries = Vec::new();
        let mut points = Vec::new();
        let mut marker_uuids = Vec::new();
        for (rank, found) in all.iter().enumerate() {
            let horizon_title = title_of(found.shape);
            let horizon_key = found
                .shape
                .known
                .as_ref()
                .and_then(|k| k.reference.as_ref())
                .map(|r| r.uuid.clone())
                .unwrap_or_else(|| horizon_title.clone());
            let (nth, count) = found.nth;
            let title = if count > 1 {
                format!("{horizon_title} — {well_title} ({nth} of {count})")
            } else {
                format!("{horizon_title} — {well_title}")
            };
            // What the marker is: the hole, the horizon, the depth. A moved
            // horizon is a second marker, never the first rewritten.
            let seed = format!("computed-tops:marker:{}:{horizon_key}:{:?}", trajectory.uuid, super::support::seed_mm(found.crossing.md));
            let uuid = cx.identity.uuid(&seed, rank)?;
            let side = side_word(found.crossing.from, up);
            let description = format!(
                "Computed by energyml-rust (computed-tops): where the trajectory `{}` crosses `{horizon_title}`, coming from {side} it, at {} in its frame.",
                trajectory.title,
                at(found.crossing.at)
            );
            let marker = witsml::WellboreMarker {
                uuid: uuid.clone(),
                schema_version: "2.1".to_string(),
                citation: citation(&title, description),
                md: eml::MeasuredDepth {
                    measured_depth: eml::LengthMeasureExt { value: found.crossing.md, uom: uom.clone() },
                    datum: datum_ref.clone(),
                },
                wellbore_marker_kind: Some(witsml::WellboreMarkerKindExt::Stratigraphic),
                ..Default::default()
            };
            objects.push(NewObject {
                uuid: uuid.clone(),
                qualified_type: "witsml21.WellboreMarker".to_string(),
                role: "marker".to_string(),
                title: title.clone(),
                json: energyml_core::json::write_value(&marker),
            });
            let (kind, interpretation) = boundary_of(found.shape.known.as_ref());
            boundaries.push(resqml::AnyMarkerBoundary::MarkerBoundary(resqml::MarkerBoundary {
                geologic_boundary_kind: kind,
                interpretation,
                marker: Some(crate::crs::reference_to(&uuid, "witsml21.WellboreMarker", &title)),
                ..Default::default()
            }));
            says.push(Note {
                key: "crossing",
                detail: format!(
                    "`{horizon_title}` at MD {:.3} {md_uom}, {} — coming from {side} it",
                    found.crossing.md,
                    at(found.crossing.at)
                ),
            });
            points.push(found.crossing.at);
            marker_uuids.push(uuid);
        }
        let uninterpreted: Vec<String> = horizons
            .iter()
            .filter(|h| boundary_of(h.known.as_ref()).1.is_none())
            .map(title_of)
            .collect();
        if !uninterpreted.is_empty() {
            says.push(Note {
                key: "no-interpretation",
                detail: format!(
                    "{}: no boundary interpretation to reference — the boundary says `horizon` and names none",
                    uninterpreted.iter().map(|t| format!("`{t}`")).collect::<Vec<_>>().join(", ")
                ),
            });
        }

        let set_title = match params.title {
            Some(given) if !given.0.trim().is_empty() => given.0.trim().to_string(),
            _ => format!("{well_title} — computed tops"),
        };
        let seed = format!("computed-tops:set:{}:{}", trajectory.uuid, marker_uuids.join(","));
        let set_uuid = cx.identity.uuid(&seed, all.len())?;
        let represented = known
            .interpretation
            .as_ref()
            .map(crate::crs::reference_of);
        let set = resqml::WellboreIntervalSet {
            uuid: set_uuid.clone(),
            schema_version: "2.2".to_string(),
            citation: citation(
                &set_title,
                format!(
                    "Computed by energyml-rust (computed-tops): the crossings of the trajectory `{}` with {} horizon(s), in the order of the hole.",
                    trajectory.title,
                    horizons.len()
                ),
            ),
            represented_object: represented,
            interval_boundaries: boundaries,
            // N boundaries make N+1 intervals (TRG v2.2 §12.15 p. 312); what
            // lies between two horizons is no fact a crossing states.
            marker_interval: vec![resqml::MarkerInterval::default(); all.len() + 1],
            ..Default::default()
        };
        objects.push(NewObject {
            uuid: set_uuid.clone(),
            qualified_type: "resqml22.WellboreIntervalSet".to_string(),
            role: "interval-set".to_string(),
            title: set_title.clone(),
            json: energyml_core::json::write_value(&set),
        });
        says.push(Note {
            key: "empty-intervals",
            detail: format!("{} interval(s) between and around the markers, none naming a unit: a crossing says where a boundary is, not what lies between two", all.len() + 1),
        });

        let dropped = if not_reached.is_empty() {
            Vec::new()
        } else {
            vec![Dropped { count: not_reached.len(), why: "horizon(s) the well does not cross — no marker".to_string() }]
        };
        let count = |title: &str, value: usize| Parameter::Double { title: title.to_string(), value: value as f64, uom: None };
        Ok(Plan {
            objects,
            datasets: Vec::new(),
            media: Vec::new(),
            activity: ActivityDraft {
                recipe: "computed-tops",
                title: format!("computed-tops → {set_title}"),
                seed,
                parameters: vec![
                    Parameter::Double { title: "stepUsed".to_string(), value: step, uom: Some(md_uom.clone()) },
                    count("markerCount", all.len()),
                    count("notReachedCount", not_reached.len()),
                    count("foldedCount", folded),
                    count("gapCount", gaps + unresolved),
                    count("touchCount", touches),
                ],
            },
            says,
            dropped,
            shapes: vec![Made {
                id: "0".to_string(),
                parent: None,
                title: set_title,
                geometry: Geometry {
                    kind: "computed-tops",
                    patches: vec![Patch { index: 0, node_offset: 0, points, topology: Topology::Points, closed: Vec::new() }],
                },
                object: Some(set_uuid),
            }],
            blocked: None,
        })
    }

    fn reports(&self) -> Vec<Slot> {
        let count = |name: &'static str, about: &'static str| {
            Slot::required(name, Need::Number { uom: None, least: None, most: None }, about)
        };
        vec![
            count("stepUsed", "the sampling step along the hole, in the unit of its measured depths"),
            count("markerCount", "markers written — one per crossing"),
            count("notReachedCount", "horizons the well does not cross"),
            count("foldedCount", "changes of side over a fold, not decided"),
            count("gapCount", "changes of side where the surface is not, or has no single height"),
            count("touchCount", "places where the hole meets a horizon without crossing it"),
        ]
    }
}
