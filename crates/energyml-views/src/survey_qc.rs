//! `survey-qc`: the control of a directional survey and of the trajectory that goes with it, in panels.
//!
//! The panels, each a [`PlotFrame`] of [`crate::style`]: the plan and the vertical section (equal aspect, laid by
//! the projections of `energyml_geometry::around_well`), the inclination, the azimuth and the dogleg severity
//! against the measured depth, and the gap between the survey and a trajectory when both are given.
//!
//! Everything is computed elsewhere and only laid out here:
//! - the stations, their checks and their placement: `energyml_geometry::stations` (`stations_of`, `diagnose`,
//!   `read_declaring`, `survey_path`);
//! - the dogleg of each course: `energyml_geometry::min_curvature::courses` - the bar drawn *is* that course's
//!   severity, converted to degrees per [`RATE_LENGTH`];
//! - what an azimuth is worth, the azimuth of the section, the gap, what the file states against what is
//!   computed: `energyml_analysis::survey_qc`.
//!
//! **The north is never guessed.** A survey that states none is refused by the library, and that refusal is said
//! as a [`ViewNote`] with its own sentence; the panels that need the world (plan, section) are then not drawn, while
//! the ones that are true whatever the north (inclination, dogleg, the gap after the turn that fits best) are. A
//! north may be declared by the setting `north`; it is carried in a note and never taken as a default. Any other
//! refusal of the stations (a method other than minimum curvature, a rejected station, a datum that is missing...)
//! is passed on as it is, as a refusal of the view.
//!
//! The plan and the section are in the survey's own north, from the datum: the north the file states (or the
//! caller declares) is what the azimuths count from.

use energyml_analysis::survey_qc::{
    CompareAt, Criteria, Frames, InclinationAtLeast, PathSample, SectionAzimuths, SectionInput, Significance,
    SignificanceCriterion, StatedField, SurveyComparison, UserAzimuth, azimuth_significance, compare,
    sample_mds, section_azimuth, stated_against_computed,
};
use energyml_analysis::well_plan::RATE_LENGTH;
use energyml_domain::crs::CompassAxes;
use energyml_domain::package_index::{NodeState, PackageIndex};
use energyml_geometry::around_well::projection::{
    Plan, VerticalSection, WellAxis, WellProjection, locator_line,
};
use energyml_geometry::min_curvature::{self, Located, Measured};
use energyml_geometry::stations::{
    BetweenStations, CheckKind, DeclaredNorth, NorthSource, PlacedSurvey, RejectedStations, SurveyDeclarations,
    SurveyStations, diagnose, placement_without_north, read_declaring, stations_of, survey_path,
};
use energyml_geometry::well::{PlacedPath, SurveyPlacement};
use energyml_sheet::draw::{
    Anchor, DisplayList, Element, Item, Layer, PageUnit, Paint, Rect, Stroke, SymbolKind,
};
use serde_json::Value;

use crate::kind::{Drawn, ObjectSlot, Offer, Request, ViewContext, ViewKind, ViewNote, local_type};
use crate::refusal::ViewRefusal;
use crate::setting::ViewSetting;
use crate::style::{self, PlotFrame};

/// The id of the view.
pub const ID: &str = "survey-qc";

/// The types of the survey: a WITSML trajectory or a RESQML 2.0.1 deviation survey.
pub const SURVEY_TYPES: &[&str] = &["Trajectory", "DeviationSurveyRepresentation"];

/// The type of the trajectory the survey is compared with.
pub const COUNTERPART_TYPES: &[&str] = &["WellboreTrajectoryRepresentation"];

/// The norths a request may declare, the first the default: what the file says.
pub const NORTHS: &[&str] = &["file", "grid", "plant"];

/// What becomes of a rejected station, the first the default.
pub const REJECTED: &[&str] = &["refuse", "leave_out"];

/// The criteria that tell whether an azimuth means anything, the first the default.
pub const CRITERIA: &[&str] = &["inclination-at-least", "lateral-effect-at-least"];

/// The view.
pub struct SurveyQc;

impl ViewKind for SurveyQc {
    fn id(&self) -> &'static str {
        ID
    }

    fn about(&self) -> &'static str {
        "the control of a directional survey: plan and vertical section, inclination, azimuth and dogleg severity against MD, and the gap with a trajectory; rejected stations and stated values the recomputation contradicts are marked"
    }

    fn objects(&self) -> Vec<ObjectSlot> {
        vec![
            ObjectSlot {
                role: "survey",
                types: SURVEY_TYPES,
                least: 1,
                most: 1,
                about: "a directional survey: a WITSML trajectory or a RESQML 2.0.1 deviation survey",
            },
            ObjectSlot {
                role: "counterpart",
                types: COUNTERPART_TYPES,
                least: 0,
                most: 1,
                about: "the trajectory the survey is compared with; none: no gap panel",
            },
        ]
    }

    fn settings(&self) -> Vec<ViewSetting> {
        vec![
            ViewSetting::choice(
                "north",
                NORTHS,
                "the north of a survey that states none: `grid` or `plant`, declared by you and never a default; `file`: what the file says",
            )
            .default(Value::from(NORTHS[0])),
            ViewSetting::choice(
                "rejected",
                REJECTED,
                "what becomes of a station the file marks `rejected`: refused, or left out and marked",
            )
            .default(Value::from(REJECTED[0])),
            ViewSetting::number(
                "azimuth",
                "the azimuth of the vertical section, degrees clockwise from the survey's north; none: the file's, else the closure's, else the drain's",
            )
            .uom("dega")
            .bounds(Some(0.0), Some(360.0)),
            ViewSetting::choice(
                "significance",
                CRITERIA,
                "the rule that tells whether a station's azimuth means anything",
            )
            .default(Value::from(CRITERIA[0])),
            ViewSetting::number(
                "inclination_threshold",
                "an inclination, in degrees, from which an azimuth is significant; replaces `significance`",
            )
            .uom("dega")
            .bounds(Some(0.0), Some(180.0)),
            ViewSetting::number(
                "dls_limit",
                "a dogleg severity, in degrees per 30 m, drawn as a limit; the courses above it are marked",
            )
            .bounds(Some(0.0), None),
            ViewSetting::number(
                "stated_tolerance",
                "the gap, in metres, from which a value the file states (TVD, displacements, section abscissa) is marked as contradicted by the recomputation",
            )
            .uom("m")
            .bounds(Some(0.0), None)
            .default(Value::from(0.5)),
            ViewSetting::number(
                "dls_tolerance",
                "the gap, in degrees per 30 m, from which a stated dogleg severity is marked as contradicted",
            )
            .bounds(Some(0.0), None)
            .default(Value::from(0.5)),
            ViewSetting::number(
                "compare_step",
                "the spacing, in metres, of the gap's samples; none: at the survey's stations",
            )
            .uom("m")
            .bounds(Some(0.001), None),
            ViewSetting::number(
                "step",
                "the spacing of the samples along the hole before refining, in metres",
            )
            .uom("m")
            .bounds(Some(0.001), None)
            .default(Value::from(10.0)),
        ]
    }

    fn size(&self) -> [f64; 2] {
        [960.0, 1100.0]
    }

    fn offers(&self, index: &PackageIndex) -> Vec<Offer> {
        index
            .nodes()
            .filter(|node| {
                node.state == NodeState::Present && SURVEY_TYPES.contains(&local_type(&node.qualified_type))
            })
            .map(|node| Offer {
                view: ID,
                objects: vec![node.uri.to_string()],
                title: node.title.as_deref().unwrap_or(&node.uuid).to_string(),
                missing: Vec::new(),
            })
            .collect()
    }

    fn draw(&self, request: &Request, cx: &ViewContext<'_>) -> Result<Drawn, ViewRefusal> {
        let refuse = |why: String| ViewRefusal::computing(ID, why);
        let uri = request
            .object("survey")
            .ok_or_else(|| refuse("no survey given".to_string()))?;
        let object = cx.source.read_object_at(uri).map_err(refuse)?;
        let survey = object.as_reflect();
        let settings = &request.settings;
        let mut notes: Vec<ViewNote> = Vec::new();

        let declared = SurveyDeclarations {
            north: match settings.text("north") {
                Some("grid") => Some(DeclaredNorth::Grid),
                Some("plant") => Some(DeclaredNorth::Plant),
                _ => None,
            },
            rejected: if settings.text("rejected") == Some("leave_out") {
                RejectedStations::LeaveOut
            } else {
                RejectedStations::Refuse
            },
        };

        // The stations as the file states them, in its own units and its own north.
        let stations = stations_of(survey, cx.source).map_err(|why| refuse(why.to_string()))?;

        // The checks. A refusal of anything but the north's orientation is the view's refusal, in the library's
        // words; a refusal of the orientation leaves the panels that do not need it.
        let diagnosis = diagnose(survey, cx.source, &declared);
        let mut oriented = true;
        if !diagnosis.passed() {
            let first = read_declaring(survey, cx.source, &declared)
                .err()
                .map(|why| why.to_string());
            let blocking = diagnosis
                .failures()
                .any(|(kind, _)| !matches!(kind, CheckKind::North | CheckKind::Placement | CheckKind::NorthInFrame));
            if blocking {
                return Err(refuse(first.unwrap_or_else(|| "the survey is refused".to_string())));
            }
            oriented = false;
            for (at, (kind, finding)) in diagnosis.failures().enumerate() {
                let says = match (&first, at) {
                    (Some(sentence), 0) => sentence.clone(),
                    _ => format!("{finding:?}"),
                };
                notes.push(ViewNote {
                    key: check_key(kind),
                    says: format!(
                        "check {kind:?} failed: {says}. The plan and the vertical section are not drawn: where the hole goes in the world is not known"
                    ),
                });
            }
        }

        // The stations kept, the courses between them and their positions from the first.
        let kept = stations
            .kept(declared.rejected)
            .map_err(|why| refuse(format!("{why:?}")))?;
        let measured: &[Measured] = &kept.measured;
        let courses = min_curvature::courses(measured).map_err(|why| refuse(why.to_string()))?;
        let located = min_curvature::positions(measured).map_err(|why| refuse(why.to_string()))?;
        if !stations.rejected.is_empty() {
            notes.push(ViewNote {
                key: "rejected-stations-left-out",
                says: format!(
                    "{} rejected station(s) left out of the computing, as asked: {}",
                    stations.rejected.len(),
                    numbers(&stations.rejected)
                ),
            });
        }

        // What an azimuth is worth.
        let threshold = settings.number("inclination_threshold").map(InclinationAtLeast);
        let criteria = Criteria::builtin();
        let criterion: &dyn SignificanceCriterion = match &threshold {
            Some(own) => own,
            None => criteria
                .get(settings.text("significance").unwrap_or(CRITERIA[0]))
                .unwrap_or_else(|| criteria.first()),
        };
        let significance = azimuth_significance(measured, criterion);

        // The azimuth of the section.
        let mut sources = SectionAzimuths::builtin();
        if let Some(azimuth) = settings.number("azimuth") {
            sources.register(Box::new(UserAzimuth(azimuth)));
        }
        let chosen = section_azimuth(
            &sources,
            &SectionInput {
                stations: &stations,
                located: &located,
            },
        );

        // What the file states, against what the method computes.
        let tolerance = settings.number("stated_tolerance").unwrap_or(0.5);
        let dls_tolerance = settings.number("dls_tolerance").unwrap_or(0.5);
        let mut flags: Vec<Flag> = Vec::new();
        match stated_against_computed(&stations, &kept, &located, &courses, chosen.as_ref()) {
            Ok(report) => {
                for gap in &report.gaps {
                    let Some(station) = gap.station.checked_sub(1).and_then(|at| stations.measured.get(at)) else {
                        continue;
                    };
                    let (stated, computed, limit) = if gap.field == StatedField::Dls {
                        (rate(gap.stated), rate(gap.computed), dls_tolerance)
                    } else {
                        (gap.stated, gap.computed, tolerance)
                    };
                    if (stated - computed).abs() > limit {
                        flags.push(Flag {
                            md: station.md,
                            field: gap.field,
                            stated,
                            computed,
                        });
                    }
                }
                if !flags.is_empty() {
                    let first = &flags[0];
                    notes.push(ViewNote {
                        key: "stated-values-contradicted",
                        says: format!(
                            "{} value(s) the file states differ from the recomputation by more than the tolerance, the first {:?} at MD {} m: stated {}, computed {}",
                            flags.len(),
                            first.field,
                            short(first.md),
                            short(first.stated),
                            short(first.computed)
                        ),
                    });
                }
                if !report.not_compared.is_empty() {
                    let first = &report.not_compared[0];
                    notes.push(ViewNote {
                        key: "stated-values-not-compared",
                        says: format!(
                            "{} stated value(s) not compared, the first {:?} of station {}: {:?}",
                            report.not_compared.len(),
                            first.field,
                            first.station,
                            first.why
                        ),
                    });
                }
            }
            Err(why) => notes.push(ViewNote {
                key: "stated-values-not-compared",
                says: format!("the stated values are not compared: {why}"),
            }),
        }

        // Where the hole goes in the world, when the north is known.
        let placed: Option<PlacedSurvey> = if oriented {
            Some(read_declaring(survey, cx.source, &declared).map_err(|why| refuse(why.to_string()))?)
        } else {
            None
        };
        if let Some(PlacedSurvey {
            north: NorthSource::Declared(north),
            ..
        }) = &placed
        {
            notes.push(ViewNote {
                key: "north-declared",
                says: format!(
                    "the file states no north: {north:?} north is declared by the request, not read from the file"
                ),
            });
        }

        // The plan and the section, laid by the projections, in the survey's own north from the datum.
        let local = survey_path(measured, BetweenStations::Arc).map_err(|why| refuse(why.to_string()))?;
        let mut plan: Option<Laid> = None;
        let mut section: Option<Laid> = None;
        if oriented {
            let identity = SurveyPlacement {
                datum: [0.0; 3],
                axes: CompassAxes {
                    east: [1.0, 0.0],
                    north: [0.0, 1.0],
                },
            };
            let path = PlacedPath::from_local(local.clone(), &identity).map_err(|why| refuse(why.to_string()))?;
            let extent = path.md_extent();
            let span = (extent[1] - extent[0]).abs().max(1.0);
            let step = settings.number("step").unwrap_or(10.0);
            let axis = WellAxis::new(&path, extent, step).map_err(|why| refuse(why.to_string()))?;
            match lay(&axis, &Plan, span, &located) {
                Ok(laid) => plan = Some(laid),
                Err(why) => notes.push(ViewNote {
                    key: "plan-not-drawn",
                    says: format!("the plan is not drawn: {why}"),
                }),
            }
            match &chosen {
                Some(chosen) => {
                    let projection = VerticalSection {
                        azimuth: chosen.azimuth.to_radians(),
                        origin: chosen.origin,
                    };
                    match lay(&axis, &projection, span, &located) {
                        Ok(laid) => section = Some(laid),
                        Err(why) => notes.push(ViewNote {
                            key: "section-not-drawn",
                            says: format!("the vertical section is not drawn: {why}"),
                        }),
                    }
                }
                None => notes.push(ViewNote {
                    key: "section-not-drawn",
                    says: "the vertical section is not drawn: the file states no azimuth for it, the hole does not close and has no drain; give the setting `azimuth`".to_string(),
                }),
            }
        }

        // The gap with the trajectory.
        let counterpart = request.object("counterpart");
        let mut gap: Option<SurveyComparison> = None;
        if let Some(counterpart) = counterpart {
            match compare_with(
                cx,
                survey,
                placed.as_ref(),
                &local,
                measured,
                counterpart,
                settings.number("compare_step"),
            ) {
                Ok(comparison) => {
                    notes.push(ViewNote {
                        key: "datums-assumed-equal",
                        says: "the survey and the trajectory are taken to count their MDs from one datum: the datum each names is not compared".to_string(),
                    });
                    if let Some(fit) = &comparison.fit {
                        notes.push(ViewNote {
                            key: "north-fitted-not-stated",
                            says: format!(
                                "the survey's north is not known: the gap is measured after the turn that fits best, {}° (largest residual {} m); it is not a north the file states",
                                short(fit.angle),
                                short(fit.largest_residual)
                            ),
                        });
                    }
                    gap = Some(comparison);
                }
                Err(why) => notes.push(ViewNote {
                    key: "gap-not-drawn",
                    says: format!("the gap with the trajectory is not drawn: {why}"),
                }),
            }
        }

        // The page: the panels in order, two to a row, an odd one last across the page.
        let title = cx
            .index
            .node(uri)
            .and_then(|node| node.title.as_deref())
            .unwrap_or(uri)
            .to_string();
        let mut order: Vec<Panel> = Vec::new();
        if plan.is_some() {
            order.push(Panel::Plan);
        }
        if section.is_some() {
            order.push(Panel::Section);
        }
        order.push(Panel::Inclination);
        order.push(Panel::Azimuth);
        order.push(Panel::Dls);
        if gap.is_some() {
            order.push(Panel::Gap);
        }

        let size = request.size;
        let mut list = DisplayList::new(size, PageUnit::Pixel);
        list.title = ID.to_string();
        list.background = Some(style::PAPER);
        let mut header = Layer::new("title");
        header.elements.push(Element::new(vec![style::label_item(
            [12.0, 24.0],
            format!("{title} - survey quality control"),
            style::text(style::TITLE, style::INK, Anchor::Start),
        )]));
        list.layers.push(header);

        let mds: Vec<f64> = located.iter().map(|one| one.md).collect();
        let station_range = md_range(&stations.measured);
        let plot = Plot {
            uri,
            stations: &stations,
            measured,
            significance: &significance,
            courses: &courses,
            flags: &flags,
            limit: settings.number("dls_limit"),
            range: station_range,
        };
        for (at, panel) in order.iter().enumerate() {
            let outer = cell(size, at, order.len());
            let layers = match panel {
                Panel::Plan => {
                    let laid = plan.as_ref().expect("the plan is in the order only when it is laid");
                    let flagged = flagged_positions(&flags, &mds, laid);
                    draw_laid(
                        "plan",
                        "plan: east and north from the datum",
                        uri,
                        outer,
                        laid,
                        &mds,
                        &flagged,
                        "east (m)",
                        "north (m)",
                        false,
                    )
                }
                Panel::Section => {
                    let laid = section.as_ref().expect("the section is in the order only when it is laid");
                    let (azimuth, by) = chosen
                        .as_ref()
                        .map_or((0.0, "none"), |chosen| (chosen.azimuth, chosen.by));
                    draw_laid(
                        "section",
                        &format!("vertical section at {} deg ({by})", short(azimuth)),
                        uri,
                        outer,
                        laid,
                        &mds,
                        &[],
                        &format!("along the azimuth {} deg (m)", short(azimuth)),
                        "TVD below the MD datum (m)",
                        true,
                    )
                }
                Panel::Inclination => draw_inclination(&plot, outer),
                Panel::Azimuth => draw_azimuth(&plot, outer),
                Panel::Dls => draw_dls(&plot, outer),
                Panel::Gap => {
                    let comparison = gap.as_ref().expect("the gap is in the order only when it is computed");
                    draw_gap(counterpart.unwrap_or(uri), comparison, outer)
                }
            };
            list.layers.extend(layers);
        }
        Ok(Drawn { list, notes })
    }
}

// -- what is laid out ----------------------------------------------------------------------------

/// The panels, in the order they are laid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Panel {
    Plan,
    Section,
    Inclination,
    Azimuth,
    Dls,
    Gap,
}

/// A value the file states that the recomputation contradicts, in the units drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Flag {
    md: f64,
    field: StatedField,
    stated: f64,
    computed: f64,
}

/// A path laid flat by a projection: the line, and the place of each kept station.
struct Laid {
    line: Vec<[f64; 2]>,
    stations: Vec<[f64; 2]>,
}

/// What the series panels draw from.
struct Plot<'a> {
    uri: &'a str,
    stations: &'a SurveyStations,
    measured: &'a [Measured],
    significance: &'a [Significance],
    courses: &'a [min_curvature::Course],
    flags: &'a [Flag],
    limit: Option<f64>,
    range: [f64; 2],
}

fn check_key(kind: CheckKind) -> &'static str {
    match kind {
        CheckKind::North => "check-north-failed",
        CheckKind::Placement => "check-placement-failed",
        CheckKind::NorthInFrame => "check-north-in-frame-failed",
        _ => "check-failed",
    }
}

/// A dogleg severity from radians per metre to degrees per [`RATE_LENGTH`].
fn rate(per_metre: f64) -> f64 {
    (per_metre * RATE_LENGTH).to_degrees()
}

fn numbers(values: &[usize]) -> String {
    values.iter().map(usize::to_string).collect::<Vec<_>>().join(", ")
}

/// A number written short: no trailing zeros, two decimals at most.
fn short(value: f64) -> String {
    let text = format!("{value:.2}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    // A value that rounds to nothing is written 0, never -0.
    if text == "-0" { "0".to_string() } else { text.to_string() }
}

/// The first and the last measured depth of the stations.
fn md_range(stations: &[Measured]) -> [f64; 2] {
    let low = stations.iter().map(|s| s.md).fold(f64::INFINITY, f64::min);
    let high = stations.iter().map(|s| s.md).fold(f64::NEG_INFINITY, f64::max);
    [low, high]
}

/// The path laid flat by a projection, and each station placed by the same projection.
fn lay(
    axis: &WellAxis,
    projection: &dyn WellProjection,
    span: f64,
    located: &[Located],
) -> Result<Laid, String> {
    let line = locator_line(axis, projection, (span * 1.0e-4).max(1.0e-6)).map_err(|why| why.to_string())?;
    let mut stations = Vec::with_capacity(located.len());
    for one in located {
        stations.push(
            projection
                .place(axis, one.md, 0.0)
                .map_err(|why| why.to_string())?,
        );
    }
    Ok(Laid {
        line: line.points,
        stations,
    })
}

/// The gap between the survey and a trajectory, MD by MD, in the frame of the datum.
///
/// With the north known the two are in one frame; with none, the turn about the vertical that fits best is found
/// by the library and said by the caller. Both are taken to count their MDs from the datum the survey is placed
/// at: the links that say so (`MdInterval.Datum`) are not read here, and the caller says it.
#[allow(clippy::too_many_arguments)]
fn compare_with(
    cx: &ViewContext<'_>,
    survey: &dyn energyml_core::value::Reflect,
    placed: Option<&PlacedSurvey>,
    local: &energyml_geometry::parametric::Path,
    measured: &[Measured],
    counterpart: &str,
    step: Option<f64>,
) -> Result<SurveyComparison, String> {
    let other_object = cx.source.read_object_at(counterpart)?;
    let other = PlacedPath::of(other_object.as_reflect(), cx.source).map_err(|why| why.to_string())?;
    let (placement, path, frames) = match placed {
        Some(placed) => (placed.placement, placed.local.clone(), Frames::Same),
        None => (
            placement_without_north(survey, cx.source).map_err(|why| why.to_string())?,
            local.clone(),
            Frames::TurnedAboutVertical,
        ),
    };
    let (Some(first), Some(last)) = (measured.first(), measured.last()) else {
        return Err("no station".to_string());
    };
    let at = match step {
        Some(step) => CompareAt::Step(step),
        None => CompareAt::Mds(measured.iter().map(|one| one.md).collect()),
    };
    let mds = sample_mds([first.md, last.md], other.md_extent(), &at).map_err(|why| why.to_string())?;
    let datum = placement.datum;
    let mut mine = Vec::with_capacity(mds.len());
    let mut theirs = Vec::with_capacity(mds.len());
    for md in mds {
        let a = placement.place(path.at(md));
        let b = other.at(md).point;
        mine.push(PathSample {
            md,
            at: [a[0] - datum[0], a[1] - datum[1], a[2] - datum[2]],
        });
        theirs.push(PathSample {
            md,
            at: [b[0] - datum[0], b[1] - datum[1], b[2] - datum[2]],
        });
    }
    compare(&mine, &theirs, frames).map_err(|why| why.to_string())
}

// -- the page ------------------------------------------------------------------------------------

/// The cell of panel `index` among `count`: two to a row, an odd last one across the page.
fn cell(size: [f64; 2], index: usize, count: usize) -> Rect {
    let (left, right, top, bottom, gap_x, gap_y) = (12.0, 12.0, 40.0, 14.0, 24.0, 12.0);
    let rows = count.div_ceil(2).max(1);
    let width = (size[0] - left - right - gap_x) / 2.0;
    let height = (size[1] - top - bottom - gap_y * (rows as f64 - 1.0)) / rows as f64;
    let (row, column) = (index / 2, index % 2);
    let alone = index + 1 == count && count % 2 == 1;
    Rect {
        x: left + column as f64 * (width + gap_x),
        y: top + row as f64 * (height + gap_y),
        w: if alone { width * 2.0 + gap_x } else { width },
        h: height,
    }
}

/// The plot area inside a cell: room for the title above and the graduations and the axis title around.
fn area_of(outer: Rect) -> Rect {
    Rect {
        x: outer.x + 56.0,
        y: outer.y + 22.0,
        w: (outer.w - 66.0).max(10.0),
        h: (outer.h - 62.0).max(10.0),
    }
}

/// The title of a panel and its graduated axes, as two layers named after the panel.
fn frame_layers(
    id: &str,
    title: &str,
    outer: Rect,
    frame: &PlotFrame,
    x_title: &str,
    y_title: &str,
) -> Vec<Layer> {
    let mut head = Layer::new(format!("{id}-title"));
    head.elements.push(Element::new(vec![style::label_item(
        [outer.x + 56.0, outer.y + 14.0],
        title,
        style::text(style::TEXT, style::INK, Anchor::Start),
    )]));
    let mut axes = style::axes_layer(frame, x_title, y_title);
    axes.name = format!("{id}-axes");
    vec![head, axes]
}

fn bounds(points: &[[f64; 2]], pick: usize) -> [f64; 2] {
    let low = points.iter().map(|p| p[pick]).fold(f64::INFINITY, f64::min);
    let high = points.iter().map(|p| p[pick]).fold(f64::NEG_INFINITY, f64::max);
    [low, high]
}

fn ring(at: [f64; 2], ink: energyml_sheet::draw::Colour) -> Item {
    Item::Symbol {
        at,
        kind: SymbolKind::Circle,
        size: 11.0,
        fill: None,
        stroke: ink,
    }
}

/// The stated positions the recomputation contradicts, placed on the laid path: `(md, place)`.
fn flagged_positions(flags: &[Flag], mds: &[f64], laid: &Laid) -> Vec<(f64, [f64; 2])> {
    flags
        .iter()
        .filter(|flag| flag.field != StatedField::Dls)
        .filter_map(|flag| {
            let at = mds.iter().position(|md| (*md - flag.md).abs() < 1.0e-9)?;
            Some((flag.md, *laid.stations.get(at)?))
        })
        .collect()
}

/// The plan or the section: the line, a point per station, a ring where a stated value is contradicted.
#[allow(clippy::too_many_arguments)]
fn draw_laid(
    id: &str,
    title: &str,
    uri: &str,
    outer: Rect,
    laid: &Laid,
    mds: &[f64],
    flagged: &[(f64, [f64; 2])],
    x_title: &str,
    y_title: &str,
    y_down: bool,
) -> Vec<Layer> {
    let area = area_of(outer);
    let frame = PlotFrame::new(area, bounds(&laid.line, 0), bounds(&laid.line, 1), y_down, true);
    let mut layers = frame_layers(id, title, outer, &frame, x_title, y_title);
    let mut layer = Layer::new(id);
    layer.clip = Some(area);
    layer.elements.push(Element::keyed(
        uri.to_string(),
        "survey",
        vec![Item::Polyline {
            points: laid.line.iter().map(|p| frame.place(*p)).collect(),
            stroke: Stroke::solid(style::SUBJECT, 2.0),
        }],
    ));
    for (md, at) in mds.iter().zip(&laid.stations) {
        layer.elements.push(Element::keyed(
            format!("{uri}#md={}", short(*md)),
            "station",
            vec![Item::Symbol {
                at: frame.place(*at),
                kind: SymbolKind::Point,
                size: 4.0,
                fill: Some(style::SUBJECT),
                stroke: style::SUBJECT,
            }],
        ));
    }
    for (md, at) in flagged {
        layer.elements.push(Element::keyed(
            format!("{uri}#stated-md={}", short(*md)),
            "flagged",
            vec![ring(frame.place(*at), style::MARK)],
        ));
    }
    layers.push(layer);
    layers
}

/// The frame of a series against MD: `y` is the range of the value.
fn series_frame(plot: &Plot<'_>, outer: Rect, y: [f64; 2]) -> PlotFrame {
    PlotFrame::new(area_of(outer), plot.range, y, false, false)
}

/// The stations the file rejected, as crosses at `(md, value)`: they are left out of the computing, and shown.
fn rejected_marks(plot: &Plot<'_>, frame: &PlotFrame, value: impl Fn(&Measured) -> Option<f64>) -> Vec<Element> {
    plot.stations
        .rejected
        .iter()
        .filter_map(|number| {
            let station = plot.stations.measured.get(number.checked_sub(1)?)?;
            let at = frame.place([station.md, value(station)?]);
            Some(Element::keyed(
                format!("{}#md={}", plot.uri, short(station.md)),
                "rejected",
                vec![Item::Symbol {
                    at,
                    kind: SymbolKind::Cross,
                    size: 8.0,
                    fill: None,
                    stroke: style::MARK,
                }],
            ))
        })
        .collect()
}

/// The inclination against MD, from 0 to 90 degrees, wider when a station goes beyond.
fn draw_inclination(plot: &Plot<'_>, outer: Rect) -> Vec<Layer> {
    let highest = plot
        .stations
        .measured
        .iter()
        .filter_map(|s| s.inclination)
        .fold(90.0_f64, f64::max);
    let frame = series_frame(plot, outer, [0.0, highest]);
    let mut layers = frame_layers(
        "inclination",
        "inclination against MD",
        outer,
        &frame,
        "MD (m)",
        "inclination (deg)",
    );
    let mut layer = Layer::new("inclination");
    layer.clip = Some(frame.area);
    let points: Vec<[f64; 2]> = plot
        .measured
        .iter()
        .filter_map(|s| Some(frame.place([s.md, s.inclination?])))
        .collect();
    layer.elements.push(Element::keyed(
        plot.uri.to_string(),
        "inclination",
        vec![Item::Polyline {
            points,
            stroke: Stroke::solid(style::SUBJECT, 1.5),
        }],
    ));
    for s in plot.measured {
        let Some(value) = s.inclination else { continue };
        layer.elements.push(Element::keyed(
            format!("{}#md={}", plot.uri, short(s.md)),
            "station",
            vec![Item::Symbol {
                at: frame.place([s.md, value]),
                kind: SymbolKind::Point,
                size: 4.0,
                fill: Some(style::SUBJECT),
                stroke: style::SUBJECT,
            }],
        ));
    }
    layer
        .elements
        .extend(rejected_marks(plot, &frame, |s| s.inclination));
    layers.push(layer);
    layers
}

/// The azimuth against MD: a filled point where it means something, a hollow one where it does not, no line
/// (a line would cross the page at the passage from 360 to 0).
fn draw_azimuth(plot: &Plot<'_>, outer: Rect) -> Vec<Layer> {
    let frame = series_frame(plot, outer, [0.0, 360.0]);
    let mut layers = frame_layers(
        "azimuth",
        "azimuth against MD: filled where significant, hollow where not",
        outer,
        &frame,
        "MD (m)",
        "azimuth (deg)",
    );
    let mut layer = Layer::new("azimuth");
    layer.clip = Some(frame.area);
    for (s, worth) in plot.measured.iter().zip(plot.significance) {
        let Some(value) = s.azimuth else { continue };
        let (kind, fill, ink) = match worth {
            Significance::Significant => ("station", Some(style::SUBJECT), style::SUBJECT),
            _ => ("station-not-significant", None, style::NEUTRAL),
        };
        layer.elements.push(Element::keyed(
            format!("{}#md={}", plot.uri, short(s.md)),
            kind,
            vec![Item::Symbol {
                at: frame.place([s.md, value]),
                kind: if fill.is_some() {
                    SymbolKind::Point
                } else {
                    SymbolKind::Circle
                },
                size: 5.0,
                fill,
                stroke: ink,
            }],
        ));
    }
    layer
        .elements
        .extend(rejected_marks(plot, &frame, |s| s.azimuth));
    layers.push(layer);
    layers
}

/// The dogleg severity against MD: a bar per course, as high as that course's severity in degrees per
/// [`RATE_LENGTH`]; the largest said; a limit and the courses above it; a ring where the file states another value.
fn draw_dls(plot: &Plot<'_>, outer: Rect) -> Vec<Layer> {
    let values: Vec<f64> = plot.courses.iter().map(|course| rate(course.severity())).collect();
    let largest = values.iter().copied().fold(0.0_f64, f64::max);
    let top = plot.limit.map_or(largest, |limit| largest.max(limit));
    let frame = series_frame(plot, outer, [0.0, top]);
    let unit = format!("deg/{} m", short(RATE_LENGTH));
    let mut layers = frame_layers(
        "dls",
        "dogleg severity against MD",
        outer,
        &frame,
        "MD (m)",
        &format!("dogleg severity ({unit})"),
    );
    let mut layer = Layer::new("dls");
    layer.clip = Some(frame.area);
    let base = frame.y.at(0.0);
    for (course, value) in plot.courses.iter().zip(&values) {
        let (x0, x1) = (frame.x.at(course.md_top), frame.x.at(course.md_base));
        let over = plot.limit.is_some_and(|limit| *value > limit);
        let ink = if over { style::MARK } else { style::SUBJECT };
        let high = frame.y.at(*value);
        layer.elements.push(Element::keyed(
            format!("{}#md={}", plot.uri, short(course.md_top)),
            if over { "course-over-limit" } else { "course" },
            vec![Item::Rect {
                rect: Rect {
                    x: x0,
                    y: high,
                    w: (x1 - x0).max(0.5),
                    h: (base - high).max(0.0),
                },
                corner: 0.0,
                fill: Some(Paint::Solid(ink)),
                stroke: None,
            }],
        ));
    }
    if let Some(limit) = plot.limit {
        let y = frame.y.at(limit);
        layer.elements.push(Element::keyed(
            format!("{}#limit", plot.uri),
            "limit",
            vec![Item::Polyline {
                points: vec![[frame.area.x, y], [frame.area.x + frame.area.w, y]],
                stroke: Stroke {
                    dash: vec![6.0, 4.0],
                    ..Stroke::solid(style::MARK, 1.2)
                },
            }],
        ));
    }
    for flag in plot.flags.iter().filter(|flag| flag.field == StatedField::Dls) {
        // The course the stated value closes: the one whose base is the station.
        let Some(course) = plot.courses.iter().find(|c| (c.md_base - flag.md).abs() < 1.0e-9) else {
            continue;
        };
        let middle = (course.md_top + course.md_base) / 2.0;
        layer.elements.push(Element::keyed(
            format!("{}#stated-md={}", plot.uri, short(course.md_top)),
            "flagged",
            vec![ring(frame.place([middle, flag.stated]), style::MARK)],
        ));
    }
    // The largest, named.
    if let Some((at, value)) = values
        .iter()
        .copied()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(&b.1))
    {
        let course = &plot.courses[at];
        layer.elements.push(Element::new(vec![style::label_item(
            [frame.area.x + 6.0, frame.area.y + 12.0],
            format!(
                "max DLS {} {unit} between MD {} and {} m",
                short(value),
                short(course.md_top),
                short(course.md_base)
            ),
            style::text(style::TEXT, style::INK, Anchor::Start),
        )]));
    }
    layers.push(layer);
    layers
}

/// The gap between the survey and the trajectory against MD: the distance, its horizontal part and the size of
/// its vertical part; the largest ringed and said.
fn draw_gap(uri: &str, comparison: &SurveyComparison, outer: Rect) -> Vec<Layer> {
    let area = area_of(outer);
    let range = [
        comparison.gaps.first().map_or(0.0, |g| g.md),
        comparison.gaps.last().map_or(1.0, |g| g.md),
    ];
    let highest = comparison.gaps.iter().map(|g| g.total).fold(0.0_f64, f64::max);
    let frame = PlotFrame::new(area, range, [0.0, highest], false, false);
    let mut layers = frame_layers(
        "gap",
        "gap between the survey and the trajectory",
        outer,
        &frame,
        "MD (m)",
        "gap (m)",
    );
    let mut layer = Layer::new("gap");
    layer.clip = Some(area);
    let line = |pick: &dyn Fn(&energyml_analysis::survey_qc::PositionGap) -> f64| -> Vec<[f64; 2]> {
        comparison
            .gaps
            .iter()
            .map(|g| frame.place([g.md, pick(g)]))
            .collect()
    };
    let dashed = |ink| Stroke {
        dash: vec![5.0, 3.0],
        ..Stroke::solid(ink, 1.2)
    };
    layer.elements.push(Element::keyed(
        format!("{uri}#gap-vertical"),
        "gap-vertical",
        vec![Item::Polyline {
            points: line(&|g| g.vertical.abs()),
            stroke: dashed(style::NEUTRAL),
        }],
    ));
    layer.elements.push(Element::keyed(
        format!("{uri}#gap-horizontal"),
        "gap-horizontal",
        vec![Item::Polyline {
            points: line(&|g| g.horizontal),
            stroke: dashed(style::SUBJECT),
        }],
    ));
    layer.elements.push(Element::keyed(
        format!("{uri}#gap"),
        "gap",
        vec![Item::Polyline {
            points: line(&|g| g.total),
            stroke: Stroke::solid(style::HIGHLIGHT, 2.0),
        }],
    ));
    if let Some(worst) = comparison.gaps.get(comparison.largest) {
        let at = frame.place([worst.md, worst.total]);
        layer.elements.push(Element::keyed(
            format!("{uri}#md={}", short(worst.md)),
            "gap-largest",
            vec![
                ring(at, style::MARK),
                style::label_item(
                    [area.x + 6.0, area.y + 12.0],
                    format!(
                        "largest gap {} m at MD {} m (mean {} m)",
                        short(worst.total),
                        short(worst.md),
                        short(comparison.mean_total)
                    ),
                    style::text(style::TEXT, style::INK, Anchor::Start),
                ),
            ],
        ));
    }
    layers.push(layer);
    layers
}
