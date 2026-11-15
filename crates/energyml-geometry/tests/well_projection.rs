//! The 2D projections of a well, the axis they read and the locator (`around_well::projection`). Every path
//! is fabricated here; no file is read. Expected values are worked out by hand from the formulas of the module
//! doc (the true vertical depth is counted below the MD datum; a mark is `PlacedPath::at(md)` projected).

use std::f64::consts::PI;

use energyml_geometry::around_well::projection::{
    Developed, MdBar, MdTvd, Plan, ProjectionRefusal, VerticalSection, WellAxis, WellProjection,
    WellProjections, locator_line, locator_mark, locator_md_at, locator_spans,
};
use energyml_geometry::crs::{
    AxisOrder, AxisReading, CompassAxes, Crs, CrsKind, VerticalDirection, VerticalQuantity,
};
use energyml_geometry::fence;
use energyml_geometry::min_curvature::{Measured, positions};
use energyml_geometry::parametric::{Path, Vertical};
use energyml_geometry::well::{MdRange, PlacedPath, SurveyPlacement};

fn crs(order: Option<&str>) -> Crs {
    Crs {
        uuid: "test".into(),
        kind: "LocalDepth3dCrs",
        x_offset: 0.0,
        y_offset: 0.0,
        z_offset: 0.0,
        areal_rotation: 0.0,
        vertical_direction: VerticalDirection::Up,
        vertical_quantity: VerticalQuantity::Length,
        horizontal_uom: Some("m".into()),
        vertical_uom: Some("m".into()),
        time_uom: None,
        projected: CrsKind::Absent,
        vertical: CrsKind::Absent,
        axes: order
            .and_then(AxisOrder::from_order)
            .map_or(AxisReading::NotStated, AxisReading::Stated),
    }
}

const EN: Option<&str> = Some("easting northing");

fn placed(
    kind: i64,
    control: Vec<[f64; 3]>,
    parameters: Vec<f64>,
    tangents: Option<Vec<[f64; 3]>>,
    order: Option<&str>,
) -> PlacedPath {
    let path = Path::new(kind, control, parameters, tangents, Vertical::Up).expect("a valid line");
    PlacedPath::new(path, crs(order), "m").expect("a placeable path")
}

fn vertical() -> PlacedPath {
    placed(
        1,
        vec![[0.0, 0.0, 0.0], [0.0, 0.0, -1000.0]],
        vec![0.0, 1000.0],
        None,
        EN,
    )
}

/// A J: a quarter circle of radius 1000 m, from straight down to due east (MD end = 500 pi).
fn j() -> PlacedPath {
    placed(
        5,
        vec![[0.0, 0.0, 0.0], [1000.0, 0.0, -1000.0]],
        vec![0.0, 500.0 * PI],
        Some(vec![[0.0, 0.0, -1.0], [1.0, 0.0, 0.0]]),
        EN,
    )
}

fn horizontal() -> PlacedPath {
    placed(
        1,
        vec![[0.0, 0.0, 0.0], [0.0, 0.0, -1000.0], [1000.0, 0.0, -1000.0]],
        vec![0.0, 1000.0, 2000.0],
        None,
        EN,
    )
}

fn inclined(end: [f64; 3]) -> PlacedPath {
    placed(1, vec![[0.0, 0.0, 0.0], end], vec![0.0, 1000.0], None, EN)
}

fn close2(a: [f64; 2], b: [f64; 2], tolerance: f64) {
    for i in 0..2 {
        assert!((a[i] - b[i]).abs() <= tolerance, "{a:?} is not {b:?}");
    }
}

fn north_section() -> VerticalSection {
    VerticalSection {
        azimuth: 0.0,
        origin: [0.0, 0.0],
    }
}

/// A well that is a vertical line in section is a point in plan (a degenerate locator line).
#[test]
fn a_vertical_well_is_a_vertical_line_in_section_and_a_point_in_plan() {
    let path = vertical();
    let axis = WellAxis::new(&path, [0.0, 1000.0], 10.0).unwrap();
    let section = north_section();
    for &md in &axis.md {
        close2(section.place(&axis, md, 0.0).unwrap(), [0.0, md], 1e-9);
    }
    let plan = locator_line(&axis, &Plan, 0.01).unwrap();
    assert!(plan.degenerate);
    assert_eq!(locator_md_at(&plan, [0.0, 0.0]), None);
    close2(locator_mark(&axis, &MdBar, 600.0).unwrap().at, [0.0, 600.0], 1e-12);
}
// break: never set `degenerate`.

/// The mark is the point of the arc at that MD, not the middle of the projected line (a J is shorter in plan
/// than its MD).
#[test]
fn the_mark_falls_on_the_arc_not_halfway_along_the_projected_line() {
    let path = j();
    let axis = WellAxis::new(&path, [0.0, 500.0 * PI], 0.0).unwrap();
    let mid = 250.0 * PI;
    close2(
        locator_mark(&axis, &Plan, mid).unwrap().at,
        [292.89322, 0.0],
        1e-5,
    );
    close2(
        locator_mark(&axis, &north_section(), mid).unwrap().at,
        [0.0, 707.10678],
        1e-5,
    );
}
// break: place the mark by the cumulated length of `locator_line` (500 m along the 2D line).

#[test]
fn an_interpolated_md_gives_what_path_at_gives() {
    let path = j();
    let axis = WellAxis::new(&path, [0.0, 500.0 * PI], 0.0).unwrap();
    let md = 123.4;
    let p = path.at(md).point;
    // East then north are the first and second placed coordinates for "easting northing".
    close2(locator_mark(&axis, &Plan, md).unwrap().at, [p[0], p[1]], 1e-9);
}
// break: interpolate between two points of the line.

#[test]
fn a_mark_beyond_the_path_is_clamped_and_said() {
    let path = j();
    let axis = WellAxis::new(&path, [0.0, 500.0 * PI], 0.0).unwrap();
    let mark = locator_mark(&axis, &Plan, 2000.0).unwrap();
    assert_eq!(mark.range, MdRange::AfterLast { last: 500.0 * PI });
    close2(mark.at, Plan.place(&axis, 500.0 * PI, 0.0).unwrap(), 1e-12);
    assert_eq!(mark.md, 500.0 * PI);
    assert!(matches!(
        locator_mark(&axis, &Plan, f64::NAN),
        Err(ProjectionRefusal::BadRange { .. })
    ));
}
// break: extrapolate without saying so.

#[test]
fn md_tvd_and_developed_lower_a_sample_by_its_offset() {
    let path = horizontal();
    let axis = WellAxis::new(&path, [0.0, 2000.0], 10.0).unwrap();
    close2(MdTvd.place(&axis, 1500.0, 10.0).unwrap(), [1500.0, 990.0], 1e-6);
    // The vertical part does not advance in plan: s = 500 at MD 1500.
    close2(Developed.place(&axis, 1500.0, 10.0).unwrap(), [500.0, 990.0], 1e-6);
}
// break: a wrong `dn` term in the abscissa of `developed`.

#[test]
fn developed_moves_a_sample_on_by_dn_cos_i() {
    let path = inclined([994.5219, 0.0, -104.5285]);
    let axis = WellAxis::new(&path, [0.0, 1000.0], 0.0).unwrap();
    let at = |projection: &dyn WellProjection, dn: f64| projection.place(&axis, 500.0, dn).unwrap();
    let (a, b) = (at(&Developed, 40.0), at(&Developed, 0.0));
    close2([a[0] - b[0], a[1] - b[1]], [4.18114, -39.78088], 1e-5);
    let (a, b) = (at(&MdTvd, 40.0), at(&MdTvd, 0.0));
    close2([a[0] - b[0], a[1] - b[1]], [0.0, -39.78088], 1e-5);
}
// break: leave out `dn cos I`.

/// The abscissa of `developed` is the cumulated plan length; two doors (the axis and `fence::point_at`) agree.
#[test]
fn developed_abscissa_is_the_plan_arc_length() {
    let path = placed(
        5,
        vec![[0.0, 0.0, -1000.0], [100.0, 100.0, -1000.0]],
        vec![0.0, 50.0 * PI],
        Some(vec![[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]),
        EN,
    );
    let axis = WellAxis::new(&path, [0.0, 50.0 * PI], 1.0).unwrap();
    let trace: Vec<[f64; 2]> = axis.at.iter().map(|p| [p[0], p[1]]).collect();
    for md in [10.0, 80.0, 150.0] {
        let s = Developed.place(&axis, md, 0.0).unwrap()[0];
        assert!((s - md).abs() < 1e-3, "s = {s} at MD {md}");
        let p = path.at(md).point;
        close2(fence::point_at(&trace, s), [p[0], p[1]], 1e-3);
    }
}
// break: take the chord instead of the cumulated length.

/// WITSML TRG 2.1 section 16.7 p. 352 (`AziVertSect`, the origin of the section) and section 16.10 p. 361
/// (`VertSect`).
#[test]
fn vertical_section_matches_the_witsml_arithmetic() {
    let stations = [
        Measured {
            md: 0.0,
            inclination: Some(0.0),
            azimuth: None,
        },
        Measured {
            md: 500.0,
            inclination: Some(30.0),
            azimuth: Some(60.0),
        },
        Measured {
            md: 1000.0,
            inclination: Some(60.0),
            azimuth: Some(60.0),
        },
    ];
    let placement = SurveyPlacement {
        datum: [0.0, 0.0, 0.0],
        axes: CompassAxes {
            east: [1.0, 0.0],
            north: [0.0, 1.0],
        },
    };
    let path = PlacedPath::from_stations(&stations, &placement).unwrap();
    let located = positions(&stations).unwrap();
    let azimuth = 45.0_f64.to_radians();
    let section = VerticalSection {
        azimuth,
        origin: [10.0, 20.0],
    };
    let axis = WellAxis::new(&path, [0.0, 1000.0], 0.0).unwrap();
    for station in &located {
        let [x, y] = section.place(&axis, station.md, 0.0).unwrap();
        let expected = (station.east - 10.0) * azimuth.sin() + (station.north - 20.0) * azimuth.cos();
        assert!((x - expected).abs() < 1e-6, "MD {}: {x} against {expected}", station.md);
        assert!((y - station.tvd).abs() < 1e-6, "MD {}: {y} against {}", station.md, station.tvd);
    }
}
// break: ignore `origin`.

#[test]
fn md_bar_needs_only_the_md_extent() {
    let axis = WellAxis::md_only([0.0, 3835.0]).unwrap();
    let line = locator_line(&axis, &MdBar, 0.01).unwrap();
    assert_eq!(line.points, vec![[0.0, 0.0], [0.0, 3835.0]]);
    assert_eq!(MdTvd.place(&axis, 100.0, 0.0), Err(ProjectionRefusal::NoPath));
}
// break: ask a path of `md_bar`.

#[test]
fn the_click_on_the_line_gives_the_md_back() {
    let path = j();
    let axis = WellAxis::new(&path, [0.0, 500.0 * PI], 0.0).unwrap();
    let line = locator_line(&axis, &Plan, 0.01).unwrap();
    assert!(!line.degenerate);
    let clicked = Plan.place(&axis, 1234.5, 0.0).unwrap();
    let md = locator_md_at(&line, clicked).unwrap();
    assert!((md - 1234.5).abs() < 0.05, "{md}");
    // A span is cut out of the line with its ends interpolated.
    let pieces = locator_spans(&line, &[[100.0, 200.0], [200.0, 100.0]]);
    assert_eq!(pieces[1], Vec::<[f64; 2]>::new());
    assert!(pieces[0].len() >= 2);
    close2(pieces[0][0], Plan.place(&axis, 100.0, 0.0).unwrap(), 0.01);
    close2(*pieces[0].last().unwrap(), Plan.place(&axis, 200.0, 0.0).unwrap(), 0.01);
}
// break: return the MD of the nearest point without interpolating along its segment.

#[test]
fn invert_gives_back_md_and_offset() {
    let path = inclined([866.0254, 0.0, -500.0]);
    let axis = WellAxis::new(&path, [0.0, 1000.0], 1.0).unwrap();
    let east_section = VerticalSection {
        azimuth: PI / 2.0,
        origin: [0.0, 0.0],
    };
    let entries: [&dyn WellProjection; 3] = [&MdTvd, &Developed, &east_section];
    for projection in entries {
        let laid = projection.place(&axis, 400.0, 25.0).unwrap();
        let back = projection.invert(&axis, laid).expect("an invertible projection");
        close2(back, [400.0, 25.0], 1e-6);
    }
    assert_eq!(Plan.invert(&axis, [1.0, 1.0]), None);
}
// break: invert `dn` with the wrong sign.

/// S6: the true vertical depth is counted below the MD datum; a path that does not reach MD 0 has none.
#[test]
fn a_tvd_without_datum_is_refused() {
    let path = placed(
        1,
        vec![[0.0, 0.0, -100.0], [0.0, 0.0, -200.0]],
        vec![100.0, 200.0],
        None,
        EN,
    );
    let axis = WellAxis::new(&path, [100.0, 200.0], 0.0).unwrap();
    assert_eq!(MdTvd.place(&axis, 150.0, 0.0), Err(ProjectionRefusal::NoDatum));
}
// break: count the depth from the first knot.

/// S2: east and north are not guessed.
#[test]
fn plan_and_section_need_the_axis_order() {
    let path = placed(
        1,
        vec![[0.0, 0.0, 0.0], [0.0, 0.0, -1000.0], [1000.0, 0.0, -1000.0]],
        vec![0.0, 1000.0, 2000.0],
        None,
        None,
    );
    let axis = WellAxis::new(&path, [0.0, 2000.0], 100.0).unwrap();
    assert_eq!(
        Plan.place(&axis, 1500.0, 0.0),
        Err(ProjectionRefusal::AxisOrderUnknown)
    );
    assert_eq!(
        north_section().place(&axis, 1500.0, 0.0),
        Err(ProjectionRefusal::AxisOrderUnknown)
    );
    assert!(Developed.place(&axis, 1500.0, 0.0).is_ok());
}
// break: take east and north by default.

struct Mine;

impl WellProjection for Mine {
    fn id(&self) -> &'static str {
        "plan"
    }
    fn abscissa_is_length(&self) -> bool {
        false
    }
    fn place(&self, _: &WellAxis, _: f64, _: f64) -> Result<[f64; 2], ProjectionRefusal> {
        Ok([0.0, 0.0])
    }
    fn direction(&self, _: &WellAxis, _: f64) -> Result<Option<[f64; 2]>, ProjectionRefusal> {
        Ok(None)
    }
}

#[test]
fn the_registry_tries_the_callers_entry_first() {
    let mut registry = WellProjections::standard();
    assert_eq!(registry.ids(), ["md_bar", "md_tvd", "developed", "plan"]);
    assert!(registry.get("plan").unwrap().abscissa_is_length());
    registry.add(Box::new(Mine));
    assert!(!registry.get("plan").unwrap().abscissa_is_length());
    assert_eq!(registry.ids()[0], "plan");
    assert!(registry.get("nothing").is_none());
}
// break: add the entry at the end.

#[test]
fn too_many_samples_are_refused_before_any_evaluation() {
    let path = horizontal();
    assert!(matches!(
        WellAxis::new(&path, [0.0, 2000.0], 1e-3),
        Err(ProjectionRefusal::TooManySamples {
            max: 1_000_000,
            ..
        })
    ));
}
// break: count after evaluating, or not at all.

#[test]
fn a_range_outside_the_path_or_reversed_is_refused() {
    let path = horizontal();
    assert!(matches!(
        WellAxis::new(&path, [0.0, 3000.0], 0.0),
        Err(ProjectionRefusal::MdOutside { .. })
    ));
    assert!(matches!(
        WellAxis::new(&path, [100.0, 50.0], 0.0),
        Err(ProjectionRefusal::BadRange { .. })
    ));
    let axis = WellAxis::new(&path, [0.0, 2000.0], 0.0).unwrap();
    assert!(matches!(
        locator_line(&axis, &Plan, 0.0),
        Err(ProjectionRefusal::BadTolerance { .. })
    ));
    assert!(matches!(
        MdBar.place(&axis, 100.0, 5.0),
        Err(ProjectionRefusal::OffsetNotSupported { projection: "md_bar" })
    ));
}
// break: do not check the range against the path's extent.
