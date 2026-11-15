//! The unit tests of `survey_qc` (`energyml-analysis` has no `tests/main.rs`: a module here adds no test binary).

use energyml_geometry::min_curvature::{Measured, courses, positions};
use energyml_geometry::stations::{
    BetweenStations, DatumFacts, Kept, MethodFacts, NorthCorrections, StatedSection, StatedStation, StatedTvd,
    SurveyKind, SurveyStations, TvdReference, UnitFault, survey_path,
};

use super::*;

// -- fixtures ----------------------------------------------------------------------------------------------------

fn station(md: f64, inclination: f64, azimuth: f64) -> Measured {
    Measured {
        md,
        inclination: Some(inclination),
        azimuth: Some(azimuth),
    }
}

/// A survey object around some stations: nothing stated, nothing rejected.
fn survey_of(measured: Vec<Measured>) -> SurveyStations {
    let stated = (1..=measured.len())
        .map(|station| StatedStation {
            station,
            ..StatedStation::default()
        })
        .collect();
    SurveyStations {
        kind: SurveyKind::WitsmlTrajectory,
        measured,
        rejected: Vec::new(),
        datums: DatumFacts::default(),
        methods: MethodFacts::default(),
        north: None,
        corrections: NorthCorrections::default(),
        stated,
        section: StatedSection::default(),
        first_station_location: None,
    }
}

/// A constant build of 3 degrees per 30 m towards the east: 31 stations, vertical to horizontal.
fn circle_stations() -> Vec<Measured> {
    (0..=30)
        .map(|k| station(30.0 * k as f64, 3.0 * k as f64, 90.0))
        .collect()
}

/// The radius of that build: 30 m of hole per 3 degrees.
fn radius() -> f64 {
    30.0 / 3.0_f64.to_radians()
}

/// The circle by hand, in the datum frame: x east, y north, z up.
fn circle_at(md: f64) -> [f64; 3] {
    let r = radius();
    let theta = md / r;
    [r * (1.0 - theta.cos()), 0.0, -r * theta.sin()]
}

fn arc_samples(stations: &[Measured], mds: &[f64]) -> Vec<PathSample> {
    let path = survey_path(stations, BetweenStations::Arc).expect("a path");
    mds.iter().map(|&md| PathSample { md, at: path.at(md) }).collect()
}

fn mds_over(stations: &[Measured], step: f64) -> Vec<f64> {
    let (first, last) = (stations[0].md, stations[stations.len() - 1].md);
    sample_mds([first, last], [first, last], &CompareAt::Step(step)).expect("mds")
}

/// `[east, north]` turned clockwise by `degrees`, written out here so that the test does not use the code's own.
fn turn(a: [f64; 2], degrees: f64) -> [f64; 2] {
    let t = degrees.to_radians();
    [a[0] * t.cos() + a[1] * t.sin(), a[1] * t.cos() - a[0] * t.sin()]
}

fn kept_all(measured: &[Measured]) -> Kept {
    Kept {
        measured: measured.to_vec(),
        stations: (1..=measured.len()).collect(),
    }
}

// -- F12 : significance ------------------------------------------------------------------------------------------

#[test]
fn the_azimuth_below_the_threshold_is_not_significant() {
    let stations = [station(0.0, 2.9, 10.0), station(30.0, 3.0, 10.0)];
    let answers = azimuth_significance(&stations, &InclinationAtLeast(3.0));
    // The threshold itself is included (`>=`).
    assert_eq!(answers, vec![Significance::NotSignificant, Significance::Significant]);
}

#[test]
fn a_station_without_azimuth_has_no_significance() {
    struct Panics;
    impl SignificanceCriterion for Panics {
        fn id(&self) -> &'static str {
            "panics"
        }
        fn significant(&self, _at: &StationContext<'_>) -> bool {
            panic!("a station without azimuth must not be asked");
        }
    }
    let stations = [Measured {
        md: 0.0,
        inclination: Some(0.0),
        azimuth: None,
    }];
    assert_eq!(azimuth_significance(&stations, &Panics), vec![Significance::NoAzimuth]);
}

#[test]
fn the_lateral_effect_counts_the_course_length() {
    // 30 m of hole at 3 degrees, a degree of azimuth error: 30 sin(3 deg) 0.017453 = 0.0274 m sideways.
    let stations = [station(0.0, 3.0, 0.0), station(30.0, 3.0, 0.0)];
    let at = |length| azimuth_significance(&stations, &LateralEffectAtLeast { azimuth_error: 1.0, length });
    assert_eq!(at(0.02)[1], Significance::Significant);
    assert_eq!(at(0.03)[1], Significance::NotSignificant);
}

#[test]
fn a_registered_criterion_is_tried_first() {
    struct Mine;
    impl SignificanceCriterion for Mine {
        fn id(&self) -> &'static str {
            "mine"
        }
        fn significant(&self, _at: &StationContext<'_>) -> bool {
            true
        }
    }
    let mut criteria = Criteria::builtin();
    assert_eq!(criteria.first().id(), "inclination-at-least");
    criteria.register(Box::new(Mine));
    assert_eq!(criteria.first().id(), "mine");
    assert!(criteria.get("lateral-effect-at-least").is_some());
    assert!(criteria.get("nothing").is_none());
}

// -- F13 : the section's azimuth ---------------------------------------------------------------------------------

fn chosen(survey: &SurveyStations, sources: &SectionAzimuths) -> Option<ChosenSection> {
    let located = positions(&survey.measured).expect("positions");
    section_azimuth(
        sources,
        &SectionInput {
            stations: survey,
            located: &located,
        },
    )
}

#[test]
fn a_stated_section_azimuth_is_taken_first() {
    let mut survey = survey_of(vec![station(0.0, 60.0, 90.0), station(100.0, 60.0, 90.0)]);
    survey.section.azimuth = Some(Ok(45.0));
    let section = chosen(&survey, &SectionAzimuths::builtin()).expect("an answer");
    assert_eq!(section.by, "stated");
    assert_eq!(section.azimuth, 45.0);
}

#[test]
fn the_closure_of_a_straight_hole_is_its_azimuth() {
    let survey = survey_of(vec![
        station(0.0, 60.0, 90.0),
        station(100.0, 60.0, 90.0),
        station(200.0, 60.0, 90.0),
    ]);
    let section = chosen(&survey, &SectionAzimuths::builtin()).expect("an answer");
    assert_eq!(section.by, "closure");
    assert!((section.azimuth - 90.0).abs() < 1e-9, "{}", section.azimuth);
}

#[test]
fn the_drain_mean_ignores_the_build() {
    let survey = survey_of(vec![
        station(0.0, 30.0, 10.0),
        station(100.0, 30.0, 10.0),
        station(200.0, 85.0, 100.0),
        station(300.0, 85.0, 100.0),
    ]);
    let located = positions(&survey.measured).expect("positions");
    let input = SectionInput {
        stations: &survey,
        located: &located,
    };
    let azimuth = DrainMeanAzimuth { above: 80.0 }.azimuth(&input).expect("a drain");
    assert!((azimuth - 100.0).abs() < 1e-9, "{azimuth}");
}

#[test]
fn a_user_azimuth_registered_comes_first() {
    let mut survey = survey_of(vec![station(0.0, 60.0, 90.0), station(100.0, 60.0, 90.0)]);
    survey.section.azimuth = Some(Ok(45.0));
    let mut sources = SectionAzimuths::builtin();
    sources.register(Box::new(UserAzimuth(-30.0)));
    let section = chosen(&survey, &sources).expect("an answer");
    assert_eq!(section.by, "user");
    assert_eq!(section.azimuth, 330.0);
    assert!(sources.get("closure").is_some());
}

#[test]
fn no_source_answers_on_a_vertical_hole() {
    let vertical = |md| Measured {
        md,
        inclination: Some(0.0),
        azimuth: None,
    };
    let survey = survey_of(vec![vertical(0.0), vertical(100.0), vertical(200.0)]);
    assert_eq!(chosen(&survey, &SectionAzimuths::builtin()), None);
}

#[test]
fn half_a_stated_origin_is_no_origin() {
    let mut survey = survey_of(vec![station(0.0, 60.0, 90.0), station(100.0, 60.0, 90.0)]);
    survey.section.azimuth = Some(Ok(10.0));
    survey.section.origin_north = Some(Ok(5.0));
    let half = chosen(&survey, &SectionAzimuths::builtin()).expect("an answer");
    assert_eq!(half.origin, [0.0, 0.0]);
    assert!(!half.origin_stated);

    // Both stated: `[east, north]`, the order of the vertical section.
    survey.section.origin_east = Some(Ok(3.0));
    let both = chosen(&survey, &SectionAzimuths::builtin()).expect("an answer");
    assert_eq!(both.origin, [3.0, 5.0]);
    assert!(both.origin_stated);
}

// -- F14 : compare -----------------------------------------------------------------------------------------------

#[test]
fn a_survey_against_its_own_circle_has_no_gap() {
    let stations = circle_stations();
    let mds = mds_over(&stations, 7.0);
    let survey = arc_samples(&stations, &mds);
    let other: Vec<PathSample> = mds.iter().map(|&md| PathSample { md, at: circle_at(md) }).collect();
    let result = compare(&survey, &other, Frames::Same).expect("a comparison");
    assert!(result.gaps.len() > 100);
    for gap in &result.gaps {
        assert!(gap.total < 1e-6, "MD {}: {}", gap.md, gap.total);
    }
}

#[test]
fn a_node_moved_one_metre_is_the_largest_gap_at_its_md() {
    let stations = circle_stations();
    let mds = mds_over(&stations, 7.0);
    let survey = arc_samples(&stations, &mds);
    let mut other = survey.clone();
    other[5].at[0] += 1.0;
    let result = compare(&survey, &other, Frames::Same).expect("a comparison");
    assert_eq!(result.largest, 5);
    let gap = result.gaps[5];
    assert!((gap.total - 1.0).abs() < 1e-12);
    assert!((gap.horizontal - 1.0).abs() < 1e-12);
    assert!(gap.vertical.abs() < 1e-12);
    // The mean is not the maximum.
    assert!(result.mean_total < 0.5);
}

#[test]
fn the_vertical_gap_does_not_depend_on_the_north() {
    let stations = circle_stations();
    let mds = mds_over(&stations, 7.0);
    let survey = arc_samples(&stations, &mds);
    let other: Vec<PathSample> = survey
        .iter()
        .map(|s| {
            let xy = turn([s.at[0], s.at[1]], 37.0);
            PathSample {
                md: s.md,
                at: [xy[0], xy[1], s.at[2] - 0.5],
            }
        })
        .collect();
    let result = compare(&survey, &other, Frames::TurnedAboutVertical).expect("a comparison");
    let fit = result.fit.expect("a fit");
    assert!((fit.angle - 37.0).abs() < 1e-9, "{}", fit.angle);
    for gap in &result.gaps {
        assert!((gap.vertical + 0.5).abs() < 1e-9, "{}", gap.vertical);
        assert!(gap.horizontal < 1e-9, "{}", gap.horizontal);
    }
}

#[test]
fn samples_outside_the_overlap_are_refused() {
    let at = CompareAt::Step(10.0);
    assert!(matches!(
        sample_mds([0.0, 100.0], [200.0, 300.0], &at),
        Err(CompareError::NoOverlap { .. })
    ));
    // Listed MDs are kept inside the overlap only, sorted, with no repeat.
    let kept = sample_mds([0.0, 100.0], [50.0, 300.0], &CompareAt::Mds(vec![90.0, 10.0, 60.0, 60.0, 200.0])).unwrap();
    assert_eq!(kept, vec![60.0, 90.0]);
}

#[test]
fn a_step_too_fine_is_refused_before_allocating() {
    assert!(matches!(
        sample_mds([0.0, 1000.0], [0.0, 1000.0], &CompareAt::Step(1e-6)),
        Err(CompareError::TooManySamples { .. })
    ));
    assert!(matches!(
        sample_mds([0.0, 1000.0], [0.0, 1000.0], &CompareAt::Step(0.0)),
        Err(CompareError::BadStep { .. })
    ));
    // Both ends are included, even when the step does not divide the overlap.
    let mds = sample_mds([0.0, 100.0], [0.0, 100.0], &CompareAt::Step(30.0)).unwrap();
    assert_eq!(mds, vec![0.0, 30.0, 60.0, 90.0, 100.0]);
}

#[test]
fn samples_that_do_not_line_up_are_refused() {
    let a = [PathSample { md: 0.0, at: [0.0; 3] }];
    let b = [PathSample { md: 5.0, at: [0.0; 3] }];
    assert!(matches!(compare(&a, &b, Frames::Same), Err(CompareError::MdsDiffer { index: 0, .. })));
    assert!(matches!(compare(&a, &[], Frames::Same), Err(CompareError::LengthsDiffer { .. })));
    assert!(matches!(compare(&[], &[], Frames::Same), Err(CompareError::NoSample)));
    let nan = [PathSample {
        md: 0.0,
        at: [f64::NAN, 0.0, 0.0],
    }];
    assert!(matches!(compare(&a, &nan, Frames::Same), Err(CompareError::NotFinite { index: 0 })));
}

// -- F15 : rotation_fit ------------------------------------------------------------------------------------------

#[test]
fn a_compound_turned_thirty_degrees_is_found() {
    let measured: Vec<Measured> = (0..=10).map(|k| station(30.0 * k as f64, 4.0 * k as f64, 20.0)).collect();
    let located = positions(&measured).expect("positions");
    let survey: Vec<[f64; 2]> = located.iter().map(|l| [l.east, l.north]).collect();
    let other: Vec<[f64; 2]> = survey.iter().map(|&a| turn(a, 30.0)).collect();
    let fit = rotation_fit(&survey, &other).expect("a fit");
    assert!((fit.angle - 30.0).abs() < 1e-9, "{}", fit.angle);
    assert!(fit.largest_residual < 1e-9);
    assert!(fit.rms_residual < 1e-9);
    assert_eq!(fit.samples, survey.len());
    // The hand check: north to east is a quarter turn clockwise.
    let quarter = rotation_fit(&[[0.0, 1.0]], &[[1.0, 0.0]]).expect("a fit");
    assert!((quarter.angle - 90.0).abs() < 1e-9);
}

#[test]
fn rotation_fit_refuses_what_has_no_horizontal_extent() {
    let vertical = [[0.0, 0.0], [0.0, 0.0]];
    assert_eq!(rotation_fit(&vertical, &vertical), Err(FitError::NoHorizontalExtent));
    assert!(matches!(
        rotation_fit(&vertical, &vertical[..1]),
        Err(FitError::LengthsDiffer { .. })
    ));
}

// -- F16 : stated against computed -------------------------------------------------------------------------------

const DATUM: &str = "eml:///witsml21.ReferencePoint(11111111-1111-4111-8111-111111111111)";
const OTHER_POINT: &str = "eml:///witsml21.ReferencePoint(22222222-2222-4222-8222-222222222222)";

fn compared(survey: &SurveyStations, kept: &Kept, section: Option<&ChosenSection>) -> StatedReport {
    let located = positions(&kept.measured).expect("positions");
    let courses = courses(&kept.measured).expect("courses");
    stated_against_computed(survey, kept, &located, &courses, section).expect("a report")
}

#[test]
fn a_stated_tvd_off_by_half_a_metre_is_listed() {
    let measured = circle_stations();
    let located = positions(&measured).expect("positions");
    let mut survey = survey_of(measured.clone());
    survey.datums.default = Some(DATUM.to_string());
    for (k, stated) in survey.stated.iter_mut().enumerate() {
        let off = if stated.station == 4 { 0.5 } else { 0.0 };
        stated.tvd = Some(Ok(StatedTvd {
            metres: located[k].tvd + off,
            from: TvdReference::Point(DATUM.to_string()),
        }));
    }
    let report = compared(&survey, &kept_all(&measured), None);
    assert_eq!(report.gaps.len(), measured.len());
    for gap in &report.gaps {
        let difference = gap.stated - gap.computed;
        let expected = if gap.station == 4 { 0.5 } else { 0.0 };
        assert!((difference - expected).abs() < 1e-9, "station {}: {difference}", gap.station);
    }
    // A TVD counted from another point is not compared.
    survey.stated[0].tvd = Some(Ok(StatedTvd {
        metres: 0.0,
        from: TvdReference::Point(OTHER_POINT.to_string()),
    }));
    let report = compared(&survey, &kept_all(&measured), None);
    assert_eq!(report.not_compared.len(), 1);
    assert_eq!(report.not_compared[0].why, NotComparedWhy::TvdFromAnotherPoint);
}

#[test]
fn a_stated_dls_is_compared_with_its_course() {
    let measured = circle_stations();
    let mut survey = survey_of(measured.clone());
    for stated in &mut survey.stated {
        stated.dls = Some(Ok(3.0_f64.to_radians() / 30.0));
    }
    let report = compared(&survey, &kept_all(&measured), None);
    assert_eq!(report.gaps.len(), measured.len() - 1);
    for gap in &report.gaps {
        assert_eq!(gap.field, StatedField::Dls);
        assert!((gap.stated - gap.computed).abs() < 1e-12, "station {}", gap.station);
    }
    assert_eq!(report.not_compared.len(), 1);
    assert_eq!(report.not_compared[0].station, 1);
    assert_eq!(report.not_compared[0].why, NotComparedWhy::FirstStation);
}

#[test]
fn a_tvd_from_a_vertical_crs_is_not_compared() {
    let measured = circle_stations();
    let mut survey = survey_of(measured.clone());
    survey.stated[2].tvd = Some(Ok(StatedTvd {
        metres: 12.0,
        from: TvdReference::VerticalCrs(None),
    }));
    survey.stated[3].disp_ns = Some(Err(UnitFault {
        field: "DispNs",
        unit: None,
    }));
    let report = compared(&survey, &kept_all(&measured), None);
    assert!(report.gaps.is_empty());
    assert_eq!(report.not_compared[0].why, NotComparedWhy::TvdFromAVerticalCrs);
    assert!(matches!(report.not_compared[1].why, NotComparedWhy::Unit(_)));
}

#[test]
fn a_left_out_station_is_not_compared() {
    let all = [
        station(0.0, 0.0, 90.0),
        station(30.0, 3.0, 90.0),
        station(60.0, 6.0, 90.0),
        station(90.0, 9.0, 90.0),
    ];
    let mut survey = survey_of(all.to_vec());
    survey.rejected = vec![3];
    survey.stated[2].dls = Some(Ok(0.001));
    let kept = Kept {
        measured: vec![all[0], all[1], all[3]],
        stations: vec![1, 2, 4],
    };
    let report = compared(&survey, &kept, None);
    assert!(report.gaps.is_empty());
    assert_eq!(
        report.not_compared,
        vec![NotCompared {
            station: 3,
            field: StatedField::Dls,
            why: NotComparedWhy::LeftOut
        }]
    );
}

#[test]
fn the_section_abscissa_goes_through_the_section_and_needs_one() {
    let measured = circle_stations();
    let located = positions(&measured).expect("positions");
    let mut survey = survey_of(measured.clone());
    survey.stated[10].vert_sect = Some(Ok(located[10].east));
    let report = compared(&survey, &kept_all(&measured), None);
    assert_eq!(report.not_compared[0].why, NotComparedWhy::NoSection);
    let section = ChosenSection {
        azimuth: 90.0,
        origin: [0.0, 0.0],
        origin_stated: false,
        by: "user",
    };
    let report = compared(&survey, &kept_all(&measured), Some(&section));
    assert_eq!(report.gaps.len(), 1);
    assert!((report.gaps[0].stated - report.gaps[0].computed).abs() < 1e-9);
}

#[test]
fn lengths_that_do_not_match_the_kept_stations_are_refused() {
    let measured = circle_stations();
    let survey = survey_of(measured.clone());
    let located = positions(&measured).expect("positions");
    let result = stated_against_computed(&survey, &kept_all(&measured), &located[1..], &[], None);
    assert!(matches!(result, Err(StatedError::LengthsDiffer { .. })));
}

// -- F17 : pairings ----------------------------------------------------------------------------------------------

fn uri(n: u8) -> String {
    format!("eml:///witsml21.Trajectory({n:02x}{n:02x}{n:02x}{n:02x}-0000-4000-8000-000000000000)")
}

fn refs(n: u8, kind: SurveyObjectKind) -> TrajectoryRefs {
    TrajectoryRefs {
        uri: uri(n),
        kind,
        witsml_trajectory: None,
        deviation_survey: None,
        md_datum: None,
        md_range: None,
        activities: Vec::new(),
    }
}

#[test]
fn pairing_prefers_what_the_file_states() {
    let datum = Some("eml:///witsml21.Datum(aaaaaaaa-0000-4000-8000-000000000000)".to_string());
    let mut subject = refs(1, SurveyObjectKind::WitsmlTrajectory);
    subject.md_datum = datum.clone();
    subject.md_range = Some([0.0, 1000.0]);
    // Names the survey, but sits on another datum.
    let mut named = refs(2, SurveyObjectKind::ResqmlTrajectory);
    named.witsml_trajectory = Some(subject.uri.clone());
    named.md_datum = Some("eml:///witsml21.Datum(bbbbbbbb-0000-4000-8000-000000000000)".to_string());
    named.md_range = Some([0.0, 1000.0]);
    let mut same_a = refs(3, SurveyObjectKind::ResqmlTrajectory);
    same_a.md_datum = datum.clone();
    same_a.md_range = Some([100.0, 900.0]);
    let mut same_b = refs(4, SurveyObjectKind::ResqmlTrajectory);
    same_b.md_datum = datum;
    same_b.md_range = Some([0.0, 500.0]);

    let all = [named.clone(), same_a.clone(), same_b.clone()];
    let stated = pairings(&subject, &all);
    assert_eq!(stated.by, Some(PairedBy::WitsmlTrajectory));
    assert_eq!(stated.candidates, vec![named.uri.clone()]);
    assert_eq!(stated.chosen, Some(named.uri.clone()));

    // Without the link: the datum proposes, the user chooses.
    named.witsml_trajectory = None;
    let all = [named, same_a.clone(), same_b.clone()];
    let proposed = pairings(&subject, &all);
    assert_eq!(proposed.by, Some(PairedBy::SameDatum));
    assert_eq!(proposed.candidates, vec![same_a.uri, same_b.uri]);
    assert_eq!(proposed.chosen, None);
}

#[test]
fn an_activity_pairs_only_objects_of_two_kinds() {
    let activity = "eml:///resqml20.obj_Activity(cccccccc-0000-4000-8000-000000000000)".to_string();
    let mut subject = refs(1, SurveyObjectKind::ResqmlTrajectory);
    subject.activities = vec![activity.clone()];
    let mut same_kind = refs(2, SurveyObjectKind::ResqmlTrajectory);
    same_kind.activities = vec![activity.clone()];
    let mut other_kind = refs(3, SurveyObjectKind::WitsmlTrajectory);
    other_kind.activities = vec![activity];

    let alone = pairings(&subject, &[same_kind.clone()]);
    assert_eq!(alone.by, None);
    assert!(alone.candidates.is_empty());

    let both = pairings(&subject, &[same_kind, other_kind.clone()]);
    assert_eq!(both.by, Some(PairedBy::Activity));
    assert_eq!(both.candidates, vec![other_kind.uri.clone()]);
    assert_eq!(both.chosen, Some(other_kind.uri));
}

#[test]
fn the_subject_is_never_its_own_candidate() {
    let mut subject = refs(1, SurveyObjectKind::DeviationSurvey);
    subject.deviation_survey = Some(subject.uri.clone());
    let answer = pairings(&subject, &[subject.clone()]);
    assert_eq!(answer.by, None);
    assert!(answer.candidates.is_empty());
    assert_eq!(answer.chosen, None);
}
