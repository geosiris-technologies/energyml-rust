//! `survey-qc`: the panels, the refusals passed on, the marks (not significant, rejected, contradicted), the gap.
//!
//! The surveys are fabricated here, in memory, with numbers worked by hand: a hold of 30 degrees of inclination
//! toward azimuth 90 goes `sin 30 = 0.5` of its length east and `cos 30` of it down (API D20; WITSML TRG 2.1
//! section 16.10 p. 356-358); an arc of 30 degrees over 100 m is a dogleg severity of
//! `30 / 100 * 30 = 9` degrees per 30 m. No file is read.

use std::io::Cursor;
use std::sync::Arc;

use energyml_analysis::survey_qc::{InclinationAtLeast, Significance, azimuth_significance};
use energyml_core::registry::DynObject;
use energyml_eml_2_3 as eml;
use energyml_geometry::min_curvature::{Measured, courses};
use energyml_resqml_2_2 as resqml22;
use energyml_sheet::draw::{DisplayList, Element, Item, Rect};
use energyml_storage::EpcStore;
use energyml_views::survey_qc::SurveyQc;
use energyml_views::{Drawn, ViewAsk, ViewKinds, ViewRefusal, draw_list};
use energyml_witsml_2_1 as witsml;
use serde_json::json;

use crate::fixtures::{NO_MAPS, Opened, polylines, texts};

const PLANE: &str = "40000000-0000-4000-8000-000000000001";
const VERTICAL: &str = "40000000-0000-4000-8000-000000000002";
const COMPOUND: &str = "40000000-0000-4000-8000-000000000003";
const COLLAR: &str = "40000000-0000-4000-8000-000000000004";
const SURVEY: &str = "40000000-0000-4000-8000-000000000006";
const TRAJECTORY: &str = "40000000-0000-4000-8000-000000000007";

fn survey_uri() -> String {
    format!("eml:///witsml21.Trajectory({SURVEY})")
}

fn trajectory_uri() -> String {
    format!("eml:///resqml22.WellboreTrajectoryRepresentation({TRAJECTORY})")
}

fn dor(uuid: &str, qualified_type: &str) -> eml::DataObjectReference {
    eml::DataObjectReference {
        uuid: uuid.into(),
        qualified_type: qualified_type.into(),
        ..Default::default()
    }
}

fn degrees(value: f64) -> eml::PlaneAngleMeasure {
    eml::PlaneAngleMeasure {
        value,
        uom: eml::PlaneAngleUom::Dega,
    }
}

/// A station: its MD, its inclination, and its azimuth when it states one.
fn station(md: f64, incl: f64, azi: Option<f64>) -> witsml::TrajectoryStation {
    witsml::TrajectoryStation {
        uid: format!("s{md}"),
        md: eml::MeasuredDepth {
            measured_depth: eml::LengthMeasureExt {
                value: md,
                uom: eml::LengthUomExt::M,
            },
            datum: None,
        },
        type_traj_station: witsml::TrajStationType::MDINCLAndAZI,
        incl: Some(degrees(incl)),
        azi: azi.map(degrees),
        ..Default::default()
    }
}

/// A survey from the collar: `(md, inclination, azimuth)` per station, grid north unless none is asked.
fn survey(
    stations: &[(f64, f64, Option<f64>)],
    north: Option<eml::NorthReferenceKind>,
) -> witsml::Trajectory {
    let collar = || dor(COLLAR, "eml23.ReferencePointInALocalEngineeringCompoundCrs");
    let last = stations.last().map_or(0.0, |s| s.0);
    witsml::Trajectory {
        uuid: SURVEY.into(),
        schema_version: "2.1".into(),
        citation: eml::Citation {
            title: "a survey".to_string(),
            ..Default::default()
        },
        md_interval: Some(eml::MdInterval {
            md_min: 0.0,
            md_max: last,
            uom: eml::LengthUomExt::M,
            datum: Some(collar()),
            ..Default::default()
        }),
        default_md_datum: Some(collar()),
        trajectory_station: stations.iter().map(|s| station(s.0, s.1, s.2)).collect(),
        azi_ref: north,
        ..Default::default()
    }
}

fn grid() -> Option<eml::NorthReferenceKind> {
    Some(eml::NorthReferenceKind::GridNorth)
}

/// A hold: every station at 30 degrees toward azimuth 90, from the collar.
fn hold(mds: &[f64]) -> Vec<(f64, f64, Option<f64>)> {
    mds.iter().map(|md| (*md, 30.0, Some(90.0))).collect()
}

fn doubles(values: &[f64]) -> eml::AbstractFloatingPointArray {
    eml::AbstractFloatingPointArray::FloatingPointXmlArray(eml::FloatingPointXmlArray {
        count_per_value: 1,
        values: values.to_vec(),
        ..Default::default()
    })
}

/// A straight trajectory in the compound, from the collar `shift` metres east of it, 30 degrees toward the east,
/// over `md_max` of hole: `0.5 * md_max` east, `cos 30 * md_max` down.
fn trajectory(shift: f64, md_max: f64) -> resqml22::WellboreTrajectoryRepresentation {
    use resqml22::{
        AbstractParametricLineGeometry, AbstractPoint3dArray, ParametricLineGeometry, Point3d,
        Point3dLatticeArray, Point3dLatticeDimension,
    };
    resqml22::WellboreTrajectoryRepresentation {
        uuid: TRAJECTORY.to_string(),
        schema_version: "2.2".into(),
        citation: eml::Citation {
            title: "a trajectory".to_string(),
            ..Default::default()
        },
        md_interval: eml::MdInterval {
            md_min: 0.0,
            md_max,
            uom: eml::LengthUomExt::M,
            datum: None,
            ..Default::default()
        },
        geometry: Some(AbstractParametricLineGeometry::ParametricLineGeometry(
            ParametricLineGeometry {
                local_crs: dor(COMPOUND, "eml23.LocalEngineeringCompoundCrs"),
                control_point_parameters: Some(doubles(&[0.0, md_max])),
                control_points: Some(AbstractPoint3dArray::Point3dLatticeArray(Point3dLatticeArray {
                    all_dimensions_are_orthogonal: None,
                    origin: Point3d {
                        coordinate1: 10.0 + shift,
                        coordinate2: 0.0,
                        coordinate3: 5.0,
                    },
                    dimension: vec![Point3dLatticeDimension {
                        direction: Point3d {
                            coordinate1: 0.5 * md_max,
                            coordinate2: 0.0,
                            coordinate3: 30f64.to_radians().cos() * md_max,
                        },
                        spacing: Some(doubles(&[1.0])),
                    }],
                })),
                knot_count: 2,
                line_kind_index: 1,
                ..Default::default()
            },
        )),
        ..Default::default()
    }
}

fn empty_store() -> EpcStore {
    EpcStore::from_reader(
        Box::new(Cursor::new(
            energyml_storage::extract::empty_package(None).expect("an empty package"),
        )),
        "survey.epc",
        Arc::new(energyml_schemas::all()),
    )
    .expect("it opens")
}

fn put(store: &mut EpcStore, object: impl DynObject + 'static) {
    store.put(Box::new(object)).expect("it goes in");
}

/// Lambert-93 at (1000, 2000), not turned; a depth axis from 35 m above NGF-IGN69; the collar 10 m along x, 5 m
/// deep (world: 1010, 2000, elevation 30).
fn package(survey: witsml::Trajectory, counterpart: Option<(f64, f64)>) -> EpcStore {
    let mut store = empty_store();
    put(
        &mut store,
        eml::LocalEngineering2dCrs {
            uuid: PLANE.into(),
            schema_version: "2.3".into(),
            azimuth: eml::PlaneAngleMeasureExt {
                value: 0.0,
                uom: eml::PlaneAngleUomExt::Dega,
            },
            azimuth_reference: eml::NorthReferenceKind::GridNorth,
            origin_projected_coordinate1: 1000.0,
            origin_projected_coordinate2: 2000.0,
            horizontal_axes: eml::HorizontalAxes {
                direction1: eml::AxisDirectionKind::East,
                direction2: eml::AxisDirectionKind::North,
                uom: eml::LengthAndTimeUomExt::M,
                is_time: false,
            },
            origin_projected_crs: eml::ProjectedCrs {
                abstract_projected_crs: Some(eml::AbstractProjectedCrs::ProjectedEpsgCrs(
                    eml::ProjectedEpsgCrs { epsg_code: 2154 },
                )),
                ..Default::default()
            },
            ..Default::default()
        },
    );
    put(
        &mut store,
        eml::VerticalCrs {
            uuid: VERTICAL.into(),
            schema_version: "2.3".into(),
            direction: eml::VerticalDirection::Up,
            uom: eml::LengthUomExt::M,
            abstract_vertical_crs: Some(eml::AbstractVerticalCrs::VerticalEpsgCrs(eml::VerticalEpsgCrs {
                epsg_code: 5720,
            })),
            ..Default::default()
        },
    );
    put(
        &mut store,
        eml::LocalEngineeringCompoundCrs {
            uuid: COMPOUND.into(),
            schema_version: "2.3".into(),
            vertical_crs: dor(VERTICAL, "eml23.VerticalCrs"),
            origin_vertical_coordinate: 35.0,
            vertical_axis: eml::VerticalAxis {
                direction: eml::VerticalDirection::Down,
                uom: eml::LengthAndTimeUomExt::M,
                is_time: false,
            },
            local_engineering2d_crs: dor(PLANE, "eml23.LocalEngineering2dCrs"),
            ..Default::default()
        },
    );
    put(
        &mut store,
        eml::ReferencePointInALocalEngineeringCompoundCrs {
            uuid: COLLAR.into(),
            schema_version: "2.3".into(),
            horizontal_coordinates: Some(eml::HorizontalCoordinates {
                coordinate1: 10.0,
                coordinate2: 0.0,
            }),
            vertical_coordinate: Some(5.0),
            crs: dor(COMPOUND, "eml23.LocalEngineeringCompoundCrs"),
            ..Default::default()
        },
    );
    put(&mut store, survey);
    if let Some((shift, md_max)) = counterpart {
        put(&mut store, trajectory(shift, md_max));
    }
    store
}

fn kinds() -> ViewKinds {
    let mut kinds = ViewKinds::empty();
    kinds.register(Box::new(SurveyQc));
    kinds
}

fn try_draw(store: EpcStore, ask: ViewAsk) -> Result<Drawn, ViewRefusal> {
    let opened = Opened::of(store);
    draw_list(&kinds(), &ask, &opened.context(&NO_MAPS)).map(|drawn| drawn.0)
}

fn drawn(store: EpcStore, ask: ViewAsk) -> Drawn {
    try_draw(store, ask).expect("drawn")
}

fn ask() -> ViewAsk {
    ViewAsk::new("survey-qc").object(survey_uri())
}

fn elements<'a>(list: &'a DisplayList, layer: &str, kind: &str) -> Vec<&'a Element> {
    list.layers
        .iter()
        .filter(|one| one.name == layer)
        .flat_map(|one| one.elements.iter())
        .filter(|element| element.kind == kind)
        .collect()
}

fn has_layer(list: &DisplayList, layer: &str) -> bool {
    list.layers.iter().any(|one| one.name == layer)
}

fn note<'a>(drawn: &'a Drawn, key: &str) -> Option<&'a str> {
    drawn
        .notes
        .iter()
        .find(|one| one.key == key)
        .map(|one| one.says.as_str())
}

fn rects(list: &DisplayList, layer: &str, kind: &str) -> Vec<Rect> {
    elements(list, layer, kind)
        .into_iter()
        .flat_map(|element| element.items.iter())
        .filter_map(|item| match item {
            Item::Rect { rect, .. } => Some(*rect),
            _ => None,
        })
        .collect()
}

fn symbol_centres(list: &DisplayList, layer: &str, kind: &str) -> Vec<(String, [f64; 2])> {
    elements(list, layer, kind)
        .into_iter()
        .flat_map(|element| {
            let key = element.key.clone().unwrap_or_default();
            element.items.iter().filter_map(move |item| match item {
                Item::Symbol { at, .. } => Some((key.clone(), *at)),
                _ => None,
            })
        })
        .collect()
}

fn measured(stations: &[(f64, f64, Option<f64>)]) -> Vec<Measured> {
    stations
        .iter()
        .map(|(md, incl, azi)| Measured {
            md: *md,
            inclination: Some(*incl),
            azimuth: *azi,
        })
        .collect()
}

#[test]
fn a_survey_is_offered_and_the_trajectory_alone_is_not() {
    let opened = Opened::of(package(survey(&hold(&[0.0, 100.0]), grid()), Some((0.0, 100.0))));
    let offers = kinds().offers(&opened.index);
    let objects: Vec<&str> = offers.iter().map(|offer| offer.objects[0].as_str()).collect();
    assert_eq!(objects, vec![survey_uri().as_str()], "{objects:?}");
    // Break it: take the trajectory's type among the survey types.
}

#[test]
fn the_dls_drawn_is_the_dls_of_the_course() {
    // 0 to 100: an arc of 30 degrees, 100 to 200: of 15 degrees, 200 to 300: none.
    let stations = [
        (0.0, 0.0, None),
        (100.0, 30.0, Some(90.0)),
        (200.0, 45.0, Some(90.0)),
        (300.0, 45.0, Some(90.0)),
    ];
    let result = drawn(package(survey(&stations, grid()), None), ask());
    let bars = rects(&result.list, "dls", "course");
    assert_eq!(bars.len(), 3);
    // Triangulation: the severity `courses` gives for the same stations, in degrees per 30 m.
    let expected: Vec<f64> = courses(&measured(&stations))
        .expect("the stations join")
        .iter()
        .map(|course| (course.severity() * 30.0).to_degrees())
        .collect();
    assert!((expected[0] - 9.0).abs() < 1.0e-9, "by hand: 30 / 100 * 30 = 9");
    for (bar, value) in bars.iter().zip(&expected) {
        assert!(
            (bar.h - value * bars[0].h / expected[0]).abs() < 1.0e-6,
            "a bar is as high as its course's severity: {} px for {value}",
            bar.h
        );
    }
    assert!(bars[2].h.abs() < 1.0e-9, "no dogleg, no bar");
    // The largest is named, in the unit of the rate.
    let words = texts(&result.list);
    assert!(
        words
            .iter()
            .any(|text| text.contains("max DLS 9 deg/30 m between MD 0 and 100 m")),
        "{words:?}"
    );
    // Break it: draw the dogleg in radians per metre, or the inclination's change instead of the course's dogleg.
}

#[test]
fn the_plan_runs_east_and_the_section_follows_the_closure_when_the_file_gives_no_azimuth() {
    let result = drawn(
        package(survey(&hold(&[0.0, 100.0, 200.0, 300.0, 400.0]), grid()), None),
        ask(),
    );
    // The plan: an eastward hold, a horizontal line left to right, a point per station keyed by its MD.
    let plan = &polylines(&result.list, "plan", "survey")[0];
    assert!(
        (plan[0][1] - plan[plan.len() - 1][1]).abs() < 1.0e-6,
        "no northing"
    );
    assert!(plan[plan.len() - 1][0] > plan[0][0], "east to the right");
    let stations = elements(&result.list, "plan", "station");
    assert_eq!(stations.len(), 5);
    assert!(
        stations
            .iter()
            .any(|element| element.key.as_deref() == Some(format!("{}#md=100", survey_uri()).as_str()))
    );
    // The section: toward the closure's azimuth, 90, said; depth down the page; one scale on both axes, so the
    // line's width over its height is east over depth: 0.5 / cos 30.
    assert!(
        texts(&result.list)
            .iter()
            .any(|text| text.contains("vertical section at 90 deg (closure)")),
        "{:?}",
        texts(&result.list)
    );
    let section = &polylines(&result.list, "section", "survey")[0];
    let (first, last) = (section[0], section[section.len() - 1]);
    assert!(last[1] > first[1], "the hole goes down the page");
    let ratio = (last[0] - first[0]) / (last[1] - first[1]);
    assert!(
        (ratio - 0.5 / 30f64.to_radians().cos()).abs() < 1.0e-3,
        "width over height is {ratio}"
    );
    // Break it: lay the section with an unequal aspect, or the plan with northing down the page.
}

#[test]
fn an_azimuth_near_the_vertical_is_hollow_and_the_rule_is_the_librarys() {
    let stations = [
        (0.0, 0.0, None),
        (100.0, 1.0, Some(45.0)),
        (200.0, 10.0, Some(45.0)),
        (300.0, 10.0, Some(45.0)),
    ];
    let result = drawn(package(survey(&stations, grid()), None), ask());
    let hollow = elements(&result.list, "azimuth", "station-not-significant");
    let filled = elements(&result.list, "azimuth", "station");
    // Triangulation: `azimuth_significance` over the same stations, with its default threshold.
    let worth = azimuth_significance(&measured(&stations), &InclinationAtLeast(3.0));
    let not_significant = worth
        .iter()
        .filter(|w| **w == Significance::NotSignificant)
        .count();
    let significant = worth.iter().filter(|w| **w == Significance::Significant).count();
    assert_eq!((hollow.len(), filled.len()), (not_significant, significant));
    assert_eq!(
        (hollow.len(), filled.len()),
        (1, 2),
        "the station at 1 degree is the hollow one"
    );
    assert_eq!(
        hollow[0].key.as_deref(),
        Some(format!("{}#md=100", survey_uri()).as_str())
    );
    // Break it: take every stated azimuth as significant.
}

#[test]
fn a_survey_that_states_no_north_is_not_placed_and_says_why_until_one_is_declared() {
    let stations = [
        (0.0, 0.0, None),
        (100.0, 30.0, Some(90.0)),
        (200.0, 30.0, Some(90.0)),
    ];
    let result = drawn(package(survey(&stations, None), None), ask());
    assert!(!has_layer(&result.list, "plan") && !has_layer(&result.list, "section"));
    assert!(has_layer(&result.list, "inclination") && has_layer(&result.list, "dls"));
    let says = note(&result, "check-north-failed").expect("the failed check is a note");
    assert!(says.starts_with("check North failed"), "{says}");
    // Declared by the request: placed, and said, never silently.
    let declared = drawn(
        package(survey(&stations, None), None),
        ask().set("north", json!("grid")),
    );
    assert!(has_layer(&declared.list, "plan"));
    assert!(note(&declared, "check-north-failed").is_none());
    let says = note(&declared, "north-declared").expect("the declaration is said");
    assert!(says.contains("Grid"), "{says}");
    // Break it: read grid north when the file states none.
}

#[test]
fn a_rejected_station_is_refused_or_left_out_and_marked() {
    let stations = [
        (0.0, 0.0, None),
        (100.0, 10.0, Some(90.0)),
        (200.0, 20.0, Some(90.0)),
        (300.0, 20.0, Some(90.0)),
    ];
    let mut with_rejected = survey(&stations, grid());
    with_rejected.trajectory_station[1].status_traj_station = Some(witsml::TrajStationStatus::Rejected);
    let refused = try_draw(package(with_rejected.clone(), None), ask()).expect_err("refused");
    assert!(
        matches!(&refused, ViewRefusal::Computing { why, .. } if why.contains("rejected")),
        "{refused}"
    );
    let result = drawn(
        package(with_rejected, None),
        ask().set("rejected", json!("leave_out")),
    );
    let marks = symbol_centres(&result.list, "inclination", "rejected");
    assert_eq!(marks.len(), 1);
    assert_eq!(marks[0].0, format!("{}#md=100", survey_uri()));
    assert_eq!(
        elements(&result.list, "inclination", "station").len(),
        3,
        "the others stay"
    );
    assert!(note(&result, "rejected-stations-left-out").is_some());
    // Break it: drop the rejected station silently instead of marking it, or leave it in the computing.
}

#[test]
fn a_stated_value_the_recomputation_contradicts_is_ringed_on_its_station() {
    // East at MD 200 is 200 * sin 30 = 100 m; at MD 100, 50 m. The file states 50 (right) and 105 (wrong).
    let mut stated = survey(&hold(&[0.0, 100.0, 200.0, 300.0]), grid());
    let metres = |value: f64| {
        Some(eml::LengthMeasure {
            value,
            uom: eml::LengthUom::M,
        })
    };
    stated.trajectory_station[1].disp_ew = metres(50.0);
    stated.trajectory_station[2].disp_ew = metres(105.0);
    let result = drawn(package(stated, None), ask());
    let rings = symbol_centres(&result.list, "plan", "flagged");
    assert_eq!(rings.len(), 1, "only the contradicted value is ringed");
    assert_eq!(rings[0].0, format!("{}#stated-md=200", survey_uri()));
    // The ring is on the station it is about.
    let at_station = symbol_centres(&result.list, "plan", "station")
        .into_iter()
        .find(|(key, _)| *key == format!("{}#md=200", survey_uri()))
        .expect("the station")
        .1;
    assert!((rings[0].1[0] - at_station[0]).abs() < 1.0e-9 && (rings[0].1[1] - at_station[1]).abs() < 1.0e-9);
    let says = note(&result, "stated-values-contradicted").expect("said");
    assert!(says.contains("stated 105, computed 100"), "{says}");
    // Break it: compare the stated displacement with the station's MD, or ring every stated value.
}

#[test]
fn the_gap_is_the_shift_between_the_survey_and_the_trajectory() {
    // The trajectory is the survey's hold, 2 m east of it: a gap of 2 m at every MD, all of it horizontal.
    let store = package(
        survey(&hold(&[0.0, 100.0, 200.0, 300.0, 400.0]), grid()),
        Some((2.0, 300.0)),
    );
    let result = drawn(store, ask().object(trajectory_uri()));
    let total = &polylines(&result.list, "gap", "gap")[0];
    assert_eq!(total.len(), 4, "the stations both paths reach: MD 0 to 300");
    assert!(
        total.iter().all(|p| (p[1] - total[0][1]).abs() < 1.0e-6),
        "the same gap everywhere"
    );
    let vertical = &polylines(&result.list, "gap", "gap-vertical")[0];
    assert!(
        vertical.iter().all(|p| (p[1] - vertical[0][1]).abs() < 1.0e-6),
        "no vertical part: flat, and at the foot of the plot"
    );
    assert!(
        vertical[0][1] > total[0][1],
        "the vertical part is under the total: it is zero, the total is 2 m"
    );
    let words = texts(&result.list);
    assert!(
        words
            .iter()
            .any(|text| text.contains("largest gap 2 m at MD 0 m (mean 2 m)")),
        "{words:?}"
    );
    assert!(
        note(&result, "north-fitted-not-stated").is_none(),
        "the north is known: no turn"
    );
    assert!(note(&result, "datums-assumed-equal").is_some());
    // Break it: measure the gap from the survey's stations to the trajectory's nodes by index, or drop the shift.
}

#[test]
fn with_no_north_the_gap_is_measured_after_a_fitted_turn_that_is_said() {
    let stations = hold(&[0.0, 100.0, 200.0, 300.0, 400.0]);
    let store = package(survey(&stations, None), Some((2.0, 300.0)));
    let result = drawn(store, ask().object(trajectory_uri()));
    assert!(!has_layer(&result.list, "plan"), "no north, no plan");
    let says = note(&result, "north-fitted-not-stated").expect("the turn is said");
    assert!(
        says.contains("fits best, 0 deg") || says.contains("fits best, 0°"),
        "{says}"
    );
    assert!(
        texts(&result.list)
            .iter()
            .any(|text| text.contains("largest gap 2 m")),
        "the 2 m shift cannot be turned away"
    );
    // Break it: apply the fitted turn to the survey without saying so, or draw the plan in the fitted north.
}
