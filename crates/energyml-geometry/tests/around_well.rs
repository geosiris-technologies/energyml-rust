//! The frame of the hole, the polar layers, the transverse grid and the envelope and ribbon meshes
//! (`energyml_geometry::around_well`). Every path is fabricated here; no file is read.
//!
//! The expected values are written from the definitions (the high side is the upward vertical projected on the
//! plane normal to the hole, the right is the physical right of someone looking down the hole), not from what
//! the code returns.

use std::f64::consts::{FRAC_PI_2, PI};

use energyml_geometry::around_well::envelope::{Angles, EnvelopeRefusal, PolarSamples, envelope, ribbon};
use energyml_geometry::around_well::hole_frame::{
    FrameOptions, FrameRefusal, Frames, HoleFrame, frames_along, high_side_of, hole_frame, north_in_plane,
    offset_point,
};
use energyml_geometry::around_well::polar::{
    AngleConvention, AngleOrigin, AngleSense, PolarRefusal, ViewedFrom, plane_direction, polar_points,
    regular_sectors, sector_bounds, to_space,
};
use energyml_geometry::around_well::transverse::{
    AxisSign, GridAxis, GridRefusal, PlaneAxis, RibbonPlane, TransverseGrid,
};
use energyml_geometry::crs::{
    AxisOrder, AxisReading, CompassAxes, Crs, CrsKind, Handedness, VerticalDirection, VerticalQuantity,
};
use energyml_geometry::parametric::{Path, Vertical};
use energyml_geometry::well::PlacedPath;

const ONE_DEGREE: f64 = PI / 180.0;

fn crs(direction: VerticalDirection, order: Option<&str>) -> Crs {
    Crs {
        uuid: "test".into(),
        kind: "LocalDepth3dCrs",
        x_offset: 0.0,
        y_offset: 0.0,
        z_offset: 0.0,
        areal_rotation: 0.0,
        vertical_direction: direction,
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

fn placed(
    kind: i64,
    control: Vec<[f64; 3]>,
    parameters: Vec<f64>,
    tangents: Option<Vec<[f64; 3]>>,
    direction: VerticalDirection,
    order: Option<&str>,
) -> PlacedPath {
    let path = Path::new(kind, control, parameters, tangents, Vertical::Up).expect("a valid line");
    PlacedPath::new(path, crs(direction, order), "m").expect("a placeable path")
}

fn close3(a: [f64; 3], b: [f64; 3], tolerance: f64) {
    for i in 0..3 {
        assert!((a[i] - b[i]).abs() <= tolerance, "{a:?} is not {b:?}");
    }
}

fn close2(a: [f64; 2], b: [f64; 2], tolerance: f64) {
    for i in 0..2 {
        assert!((a[i] - b[i]).abs() <= tolerance, "{a:?} is not {b:?}");
    }
}

const EN: Option<&str> = Some("easting northing");

fn right(tangent: [f64; 3]) -> HoleFrame {
    hole_frame(0.0, [0.0; 3], tangent, Handedness::Right, ONE_DEGREE).expect("a frame")
}

// ---------------------------------------------------------------------------------------------------
// hole frame
// ---------------------------------------------------------------------------------------------------

/// The high side is `u - (u . t) t`, normalised; for a hole 60 degrees from vertical towards the east it is
/// (cos 60, 0, sin 60) (the upward direction of the vertical plane holding the hole, perpendicular to it).
#[test]
fn a_hole_inclined_sixty_degrees_east_has_its_high_side_up_and_east() {
    let a = 60.0_f64.to_radians();
    let (h, inclination) = high_side_of(0.0, [a.sin(), 0.0, -a.cos()], ONE_DEGREE).unwrap();
    close3(h, [0.5, 0.0, 0.8660254], 1e-7);
    assert!((inclination - a).abs() < 1e-12);
}
// break: drop the `cos(I) a` term (take the vertical itself, normalised on the plane).

/// Sawaryn and Thorogood, SPE 84246; the contract's `right = tangent x high_side` in a direct frame.
#[test]
fn a_horizontal_well_heading_north_has_east_on_its_right() {
    let frame = right([0.0, 1.0, 0.0]);
    close3(frame.high_side, [0.0, 0.0, 1.0], 1e-12);
    close3(frame.right, [1.0, 0.0, 0.0], 1e-12);
}
// break: compute `high_side x tangent`.

#[test]
fn a_horizontal_well_heading_east_has_south_on_its_right() {
    close3(right([1.0, 0.0, 0.0]).right, [0.0, -1.0, 0.0], 1e-12);
}
// break: compute `high_side x tangent`.

#[test]
fn the_frame_is_direct() {
    let a = 45.0_f64.to_radians();
    let f = right([0.0, a.sin(), -a.cos()]);
    close3(f.high_side, [0.0, a.cos(), a.sin()], 1e-12);
    close3(f.right, [1.0, 0.0, 0.0], 1e-12);
    // det(t, h, r) = t . (h x r)
    let h_cross_r = [
        f.high_side[1] * f.right[2] - f.high_side[2] * f.right[1],
        f.high_side[2] * f.right[0] - f.high_side[0] * f.right[2],
        f.high_side[0] * f.right[1] - f.high_side[1] * f.right[0],
    ];
    let det = f.tangent[0] * h_cross_r[0] + f.tangent[1] * h_cross_r[1] + f.tangent[2] * h_cross_r[2];
    assert!((det - 1.0).abs() < 1e-12, "{det}");
}
// break: compute `high_side x tangent`.

/// S4: under 1 degree from vertical (down or up) the high side does not exist.
#[test]
fn the_threshold_is_one_degree_and_named() {
    let at = |degrees: f64| {
        let a = degrees.to_radians();
        high_side_of(7.0, [a.sin(), 0.0, -a.cos()], ONE_DEGREE)
    };
    assert!(matches!(
        at(0.99),
        Err(FrameRefusal::HighSideUndefined { md, .. }) if md == 7.0
    ));
    assert!(at(1.01).is_ok());
    assert!(matches!(at(179.5), Err(FrameRefusal::HighSideUndefined { .. })));
}
// break: set the threshold to 0.

#[test]
fn a_near_vertical_node_is_refused_by_name_and_the_others_kept() {
    let path = placed(
        1,
        vec![[0.0, 0.0, 0.0], [0.0, 0.0, -1000.0], [500.0, 0.0, -1500.0]],
        vec![0.0, 1000.0, 1707.1],
        None,
        VerticalDirection::Up,
        EN,
    );
    let frames = frames_along(&path, &[500.0, 1300.0], &FrameOptions::default()).unwrap();
    assert!(matches!(
        frames.frames[0],
        Err(FrameRefusal::HighSideUndefined { .. })
    ));
    assert!(frames.frames[1].is_ok());
    assert_eq!(frames.refused(), 1);
    assert_eq!(frames.given().count(), 1);
}
// break: stop at the first refusal (collect into a Result).

/// The same horizontal well to the north, written z up and z down: the physical right is the same (the trap of
/// a CRS "z down" is avoided because the frame is computed after the direction has gone through the CRS).
#[test]
fn the_same_well_written_with_depth_down_has_the_same_right() {
    let up = placed(
        1,
        vec![[0.0, 0.0, 0.0], [0.0, 0.0, -1000.0], [0.0, 1000.0, -1000.0]],
        vec![0.0, 1000.0, 2000.0],
        None,
        VerticalDirection::Up,
        EN,
    );
    let down = placed(
        1,
        vec![[0.0, 0.0, 0.0], [0.0, 0.0, 1000.0], [0.0, 1000.0, 1000.0]],
        vec![0.0, 1000.0, 2000.0],
        None,
        VerticalDirection::Down,
        EN,
    );
    for path in [&up, &down] {
        let frames = frames_along(path, &[1500.0], &FrameOptions::default()).unwrap();
        let frame = frames.frames[0].as_ref().unwrap();
        close3(frame.right, [1.0, 0.0, 0.0], 1e-9);
        close3(frame.high_side, [0.0, 0.0, 1.0], 1e-9);
    }
}
// break: compute the frame on the local coordinates (no `direction_to_projected`).

/// RESQML TRG 2.0.1 section 2.1 p. 10: the placed coordinates are (northing, easting, up), an indirect frame;
/// the right stays the physical right, so for a well going north it is the east, the second coordinate.
#[test]
fn a_northing_easting_crs_keeps_the_right_on_the_east() {
    let path = placed(
        1,
        vec![[0.0, 0.0, 0.0], [0.0, 0.0, -1000.0], [1000.0, 0.0, -1000.0]],
        vec![0.0, 1000.0, 2000.0],
        None,
        VerticalDirection::Up,
        Some("northing easting"),
    );
    let frames = frames_along(&path, &[1500.0], &FrameOptions::default()).unwrap();
    let frame = frames.frames[0].as_ref().unwrap();
    close3(frame.right, [0.0, 1.0, 0.0], 1e-9);
    let east = path.compass().unwrap().east;
    assert!((frame.right[0] * east[0] + frame.right[1] * east[1] - 1.0).abs() < 1e-9);
}
// break: forget the sign `s` of an indirect frame.

/// S2: an unstated order is refused unless declared; a declaration never overrides the file.
#[test]
fn an_unstated_axis_order_is_refused_unless_declared() {
    let control = vec![[0.0, 0.0, 0.0], [0.0, 0.0, -1000.0], [1000.0, 0.0, -1000.0]];
    let unstated = placed(
        1,
        control.clone(),
        vec![0.0, 1000.0, 2000.0],
        None,
        VerticalDirection::Up,
        None,
    );
    assert_eq!(
        frames_along(&unstated, &[1500.0], &FrameOptions::default()),
        Err(FrameRefusal::AxisOrderUnknown)
    );
    let declared = FrameOptions {
        handedness: Some(Handedness::Right),
        ..FrameOptions::default()
    };
    assert!(frames_along(&unstated, &[1500.0], &declared).is_ok());

    let stated = placed(
        1,
        control,
        vec![0.0, 1000.0, 2000.0],
        None,
        VerticalDirection::Up,
        EN,
    );
    let against = FrameOptions {
        handedness: Some(Handedness::Left),
        ..FrameOptions::default()
    };
    assert_eq!(
        frames_along(&stated, &[1500.0], &against),
        Err(FrameRefusal::DeclaredAgainstFile {
            stated: Handedness::Right,
            declared: Handedness::Left
        })
    );
}
// break: take `Right` when nothing is stated.

#[test]
fn a_frame_beyond_the_trajectory_is_refused() {
    let path = placed(
        1,
        vec![[0.0, 0.0, 0.0], [1000.0, 0.0, -1000.0]],
        vec![0.0, 2000.0],
        None,
        VerticalDirection::Up,
        EN,
    );
    let frames = frames_along(&path, &[2500.0], &FrameOptions::default()).unwrap();
    assert_eq!(
        frames.frames[0],
        Err(FrameRefusal::MdOutside {
            md: 2500.0,
            first: 0.0,
            last: 2000.0
        })
    );
}
// break: extrapolate instead of refusing.

#[test]
fn north_in_plane_of_a_hole_heading_east_points_left() {
    let compass = CompassAxes {
        east: [1.0, 0.0],
        north: [0.0, 1.0],
    };
    let east = right([1.0, 0.0, 0.0]);
    close2(north_in_plane(&east, &compass).unwrap(), [-1.0, 0.0], 1e-12);
    let north = hole_frame(5.0, [0.0; 3], [0.0, 1.0, 0.0], Handedness::Right, ONE_DEGREE).unwrap();
    assert_eq!(
        north_in_plane(&north, &compass),
        Err(FrameRefusal::NorthAlongHole { md: 5.0 })
    );
}
// break: return [0, 1] without projecting north on the plane.

#[test]
fn an_offset_point_is_the_centre_moved_along_the_high_side() {
    let inclined = placed(
        1,
        vec![[0.0, 0.0, 0.0], [866.0254, 0.0, -500.0]],
        vec![0.0, 1000.0],
        None,
        VerticalDirection::Up,
        EN,
    );
    let centre = inclined.at(500.0).point;
    let moved = offset_point(&inclined, 500.0, 10.0).unwrap();
    close3(
        [moved[0] - centre[0], moved[1] - centre[1], moved[2] - centre[2]],
        [5.0, 0.0, 8.660254],
        1e-6,
    );

    let vertical = placed(
        1,
        vec![[0.0, 0.0, 0.0], [0.0, 0.0, -1000.0]],
        vec![0.0, 1000.0],
        None,
        VerticalDirection::Up,
        EN,
    );
    assert!(matches!(
        offset_point(&vertical, 500.0, 10.0),
        Err(FrameRefusal::HighSideUndefined { .. })
    ));
    assert_eq!(
        offset_point(&vertical, 500.0, 0.0).unwrap(),
        vertical.at(500.0).point
    );
}
// break: move along the upward vertical instead of the high side.

// ---------------------------------------------------------------------------------------------------
// polar
// ---------------------------------------------------------------------------------------------------

fn convention(origin: AngleOrigin, sense: AngleSense, viewed: ViewedFrom, offset: f64) -> AngleConvention {
    AngleConvention {
        origin,
        sense,
        viewed_from: viewed,
        offset,
    }
}

fn clockwise_from_top() -> AngleConvention {
    convention(
        AngleOrigin::HighSide,
        AngleSense::Clockwise,
        ViewedFrom::LookingDownhole,
        0.0,
    )
}

#[test]
fn a_missing_convention_is_refused_by_name() {
    assert_eq!(
        polar_points(&[1.0], &[0.0], None, None),
        Err(PolarRefusal::ConventionUndeclared)
    );
}
// break: fall back on a default convention.

#[test]
fn clockwise_from_high_side_puts_ninety_degrees_on_the_right() {
    close2(
        plane_direction(FRAC_PI_2, &clockwise_from_top(), None).unwrap(),
        [1.0, 0.0],
        1e-12,
    );
}
// break: invert the sense.

#[test]
fn looking_uphole_mirrors_left_and_right() {
    let mirrored = convention(
        AngleOrigin::HighSide,
        AngleSense::Clockwise,
        ViewedFrom::LookingUphole,
        0.0,
    );
    close2(
        plane_direction(FRAC_PI_2, &mirrored, None).unwrap(),
        [-1.0, 0.0],
        1e-12,
    );
}
// break: ignore `viewed_from`.

#[test]
fn the_origin_and_the_offset_turn_the_whole_ring() {
    let c = convention(
        AngleOrigin::Right,
        AngleSense::Clockwise,
        ViewedFrom::LookingDownhole,
        FRAC_PI_2,
    );
    close2(plane_direction(0.0, &c, None).unwrap(), [0.0, -1.0], 1e-12);
}
// break: ignore `offset`.

#[test]
fn north_origin_needs_the_north_across_the_hole() {
    let c = convention(
        AngleOrigin::North,
        AngleSense::Clockwise,
        ViewedFrom::LookingDownhole,
        0.0,
    );
    assert_eq!(
        plane_direction(0.0, &c, None),
        Err(PolarRefusal::NorthUnavailable)
    );
    close2(
        plane_direction(0.0, &c, Some([-1.0, 0.0])).unwrap(),
        [-1.0, 0.0],
        1e-12,
    );
}
// break: take the high side as origin whatever `origin` says.

#[test]
fn a_nan_or_negative_radius_is_a_counted_hole() {
    let ring = polar_points(
        &[1.0, f64::NAN, -2.0, 3.0],
        &[0.0, FRAC_PI_2, PI, 3.0 * FRAC_PI_2],
        Some(&clockwise_from_top()),
        None,
    )
    .unwrap();
    assert_eq!(ring.points[1], None);
    assert_eq!(ring.points[2], None);
    assert_eq!(ring.holes, 2);
    assert_eq!(ring.negative, 1);
    close2(ring.points[3].unwrap(), [-3.0, 0.0], 1e-12);
}
// break: fill a hole from its neighbour.

#[test]
fn irregular_sectors_split_at_half_angles() {
    let degrees = |v: &[f64]| v.iter().map(|d| d.to_radians()).collect::<Vec<_>>();
    let bounds = sector_bounds(&degrees(&[0.0, 90.0, 180.0, 300.0])).unwrap();
    let expected: [[f64; 2]; 4] = [[-30.0, 45.0], [45.0, 135.0], [135.0, 240.0], [240.0, 330.0]];
    for (got, want) in bounds.iter().zip(expected) {
        close2(*got, [want[0].to_radians(), want[1].to_radians()], 1e-12);
    }
}
// break: force regular sectors.

#[test]
fn decreasing_angles_split_the_other_way() {
    let degrees = |v: &[f64]| v.iter().map(|d| d.to_radians()).collect::<Vec<_>>();
    let bounds = sector_bounds(&degrees(&[0.0, -90.0, -180.0, -270.0])).unwrap();
    close2(
        bounds[0],
        [45.0_f64.to_radians(), (-45.0_f64).to_radians()],
        1e-12,
    );
}
// break: assume increasing angles.

#[test]
fn angles_that_turn_more_than_once_are_refused() {
    let degrees = |v: &[f64]| v.iter().map(|d| d.to_radians()).collect::<Vec<_>>();
    assert_eq!(
        sector_bounds(&degrees(&[0.0, 180.0, 370.0])),
        Err(PolarRefusal::AnglesNotMonotonic)
    );
    assert_eq!(
        sector_bounds(&degrees(&[0.0, 90.0, 45.0])),
        Err(PolarRefusal::AnglesNotMonotonic)
    );
    assert_eq!(
        sector_bounds(&degrees(&[0.0, 90.0])),
        Err(PolarRefusal::TooFewSectors { n: 2 })
    );
}
// break: drop the monotonic check.

#[test]
fn regular_sectors_start_on_their_first_centre() {
    let sectors = regular_sectors(4, 0.0);
    let expected = [0.0, FRAC_PI_2, PI, 3.0 * FRAC_PI_2];
    for (got, want) in sectors.iter().zip(expected) {
        assert!((got - want).abs() < 1e-12);
    }
}
// break: start on the edge of the first sector.

#[test]
fn to_space_places_a_plane_point_on_the_frame() {
    let frame = HoleFrame {
        md: 0.0,
        at: [0.0, 0.0, -1000.0],
        tangent: [1.0, 0.0, 0.0],
        high_side: [0.0, 0.0, 1.0],
        right: [0.0, -1.0, 0.0],
        inclination: FRAC_PI_2,
    };
    close3(to_space(&frame, [1.0, 2.0]), [0.0, -1.0, -998.0], 1e-12);
}
// break: swap the two plane axes.

// ---------------------------------------------------------------------------------------------------
// transverse
// ---------------------------------------------------------------------------------------------------

/// A horizontal hole heading east at (0, 0, -1000): high side up, right to the south.
fn east_frame(md: f64, x: f64) -> HoleFrame {
    HoleFrame {
        md,
        at: [x, 0.0, -1000.0],
        tangent: [1.0, 0.0, 0.0],
        high_side: [0.0, 0.0, 1.0],
        right: [0.0, -1.0, 0.0],
        inclination: FRAC_PI_2,
    }
}

fn axis(along: PlaneAxis, sign: AxisSign, offsets: &[f64]) -> GridAxis {
    GridAxis {
        along,
        sign,
        offsets: offsets.to_vec(),
    }
}

#[test]
fn a_grid_cell_sits_between_half_offsets() {
    let grid = TransverseGrid::new(
        axis(PlaneAxis::HighSide, AxisSign::Positive, &[-10.0, 0.0, 10.0, 30.0]),
        axis(PlaneAxis::Right, AxisSign::Positive, &[-5.0, 5.0]),
    )
    .unwrap();
    assert_eq!(grid.cell(1, 0), Some([[-10.0, -5.0], [0.0, 5.0]]));
    assert_eq!(grid.cell(3, 1), Some([[0.0, 20.0], [10.0, 40.0]]));
    assert_eq!(grid.cell(4, 0), None);
}
// break: put the bounds on the offsets themselves.

/// RESQML TRG 2.2 section 7.1: the top element is the slower one, so the first count is the slow dimension.
#[test]
fn the_slow_dimension_is_the_first_count() {
    let grid = TransverseGrid::new(
        axis(PlaneAxis::HighSide, AxisSign::Positive, &[0.0, 1.0, 2.0, 3.0]),
        axis(PlaneAxis::Right, AxisSign::Positive, &[0.0, 1.0]),
    )
    .unwrap();
    assert_eq!(grid.checked(&[4, 2]), Ok(()));
    assert!(matches!(
        grid.checked(&[2, 4]),
        Err(GridRefusal::ShapeMismatch { .. })
    ));
}
// break: compare in the other order.

#[test]
fn lattice_point_matches_the_definition() {
    let grid = TransverseGrid::new(
        axis(PlaneAxis::HighSide, AxisSign::Positive, &[-10.0, 0.0, 10.0]),
        axis(PlaneAxis::Right, AxisSign::Negative, &[-20.0, 20.0]),
    )
    .unwrap();
    let frame = east_frame(0.0, 0.0);
    close3(
        grid.lattice_point(&frame, 2, 1).unwrap(),
        [0.0, 20.0, -990.0],
        1e-12,
    );
    assert_eq!(grid.slice(&frame).len(), 6);
    assert_eq!(grid.lattice_point(&frame, 3, 0), None);
}
// break: ignore the declared sign.

#[test]
fn the_same_axis_twice_is_refused() {
    assert_eq!(
        TransverseGrid::new(
            axis(PlaneAxis::HighSide, AxisSign::Positive, &[0.0, 1.0]),
            axis(PlaneAxis::HighSide, AxisSign::Positive, &[0.0, 1.0]),
        ),
        Err(GridRefusal::SameAxisTwice)
    );
}
// break: drop the check.

fn bend_frames(mds: &[f64]) -> Frames {
    let path = placed(
        5,
        vec![[0.0, 0.0, 0.0], [20.0, 0.0, -20.0]],
        vec![0.0, 10.0 * PI],
        Some(vec![[0.0, 0.0, -1.0], [1.0, 0.0, 0.0]]),
        VerticalDirection::Up,
        EN,
    );
    frames_along(&path, mds, &FrameOptions::default()).unwrap()
}

/// A quarter circle of radius 20 m: a slice offset by 40 m towards the centre folds, one offset by 10 m does not.
#[test]
fn tight_bend_folds_are_found() {
    let mds: Vec<f64> = (1..=15).map(|i| 2.0 * i as f64).collect();
    let frames = bend_frames(&mds);
    assert_eq!(frames.refused(), 0);
    let wide = TransverseGrid::new(
        axis(PlaneAxis::HighSide, AxisSign::Positive, &[-40.0, 40.0]),
        axis(PlaneAxis::Right, AxisSign::Positive, &[-1.0, 1.0]),
    )
    .unwrap();
    assert!(!wide.check_folds(&frames).is_empty());
    let narrow = TransverseGrid::new(
        axis(PlaneAxis::HighSide, AxisSign::Positive, &[-10.0, 10.0]),
        axis(PlaneAxis::Right, AxisSign::Positive, &[-1.0, 1.0]),
    )
    .unwrap();
    assert!(narrow.check_folds(&frames).is_empty());
}
// break: make `check_folds` always return an empty list.

/// A refused node cuts the check: two crossing slices on either side of it are not compared.
#[test]
fn a_refused_frame_cuts_the_fold_check() {
    let both = bend_frames(&[2.0, 30.0]);
    let (a, b) = (both.frames[0].clone().unwrap(), both.frames[1].clone().unwrap());
    let grid = TransverseGrid::new(
        axis(PlaneAxis::HighSide, AxisSign::Positive, &[-40.0, 40.0]),
        axis(PlaneAxis::Right, AxisSign::Positive, &[-1.0, 1.0]),
    )
    .unwrap();
    // The guard: without the hole between them, the two slices do cross.
    assert_eq!(grid.check_folds(&both).len(), 1);
    let cut = Frames {
        frames: vec![
            Ok(a),
            Err(FrameRefusal::HighSideUndefined {
                md: 15.0,
                inclination: 0.0,
            }),
            Ok(b),
        ],
    };
    assert!(grid.check_folds(&cut).is_empty());
}
// break: compare the frames on either side of a refused one.

// ---------------------------------------------------------------------------------------------------
// envelope and ribbon
// ---------------------------------------------------------------------------------------------------

fn straight_frames() -> Frames {
    let path = placed(
        1,
        vec![[0.0, 0.0, -1000.0], [300.0, 0.0, -1000.0]],
        vec![0.0, 300.0],
        None,
        VerticalDirection::Up,
        EN,
    );
    frames_along(&path, &[100.0, 150.0, 200.0], &FrameOptions::default()).unwrap()
}

const QUARTERS: [f64; 4] = [0.0, FRAC_PI_2, PI, 3.0 * FRAC_PI_2];

fn ring_of(
    radii: &[f64],
    frames: &Frames,
    c: &AngleConvention,
) -> Result<energyml_geometry::around_well::envelope::EnvelopeMesh, EnvelopeRefusal> {
    envelope(
        frames,
        &PolarSamples {
            sectors: 4,
            radii,
            angles: Angles::Shared(&QUARTERS),
        },
        Some(c),
        None,
        1.0,
    )
}

#[test]
fn angle_zero_is_the_high_side() {
    let mesh = ring_of(&[1.0; 12], &straight_frames(), &clockwise_from_top()).unwrap();
    close3(mesh.points[0], [100.0, 0.0, -999.0], 1e-9);
}
// break: shift the origin.

#[test]
fn clockwise_looking_downhole_turns_to_the_right() {
    let mesh = ring_of(&[1.0; 12], &straight_frames(), &clockwise_from_top()).unwrap();
    // The right of a well heading east is the south.
    close3(mesh.points[1], [100.0, -1.0, -1000.0], 1e-9);
}
// break: invert the sense.

#[test]
fn the_faces_look_outward_in_both_senses() {
    for sense in [AngleSense::Clockwise, AngleSense::Anticlockwise] {
        let c = convention(AngleOrigin::HighSide, sense, ViewedFrom::LookingDownhole, 0.0);
        let mesh = ring_of(&[1.0; 12], &straight_frames(), &c).unwrap();
        assert_eq!(mesh.triangles.len(), 16);
        for tri in &mesh.triangles {
            let [a, b, c] = tri.map(|i| mesh.points[i as usize]);
            let normal = [
                (b[1] - a[1]) * (c[2] - a[2]) - (b[2] - a[2]) * (c[1] - a[1]),
                (b[2] - a[2]) * (c[0] - a[0]) - (b[0] - a[0]) * (c[2] - a[2]),
                (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]),
            ];
            let centroid = [
                (a[0] + b[0] + c[0]) / 3.0,
                (a[1] + b[1] + c[1]) / 3.0,
                (a[2] + b[2] + c[2]) / 3.0,
            ];
            let radial = [0.0, centroid[1], centroid[2] + 1000.0];
            let outward = normal[0] * radial[0] + normal[1] * radial[1] + normal[2] * radial[2];
            assert!(outward > 0.0, "{sense:?}: a face looks inward");
        }
    }
}
// break: do not correct the winding.

/// RESQML TRG 2.2 section 7.1: node slowest, then sector.
#[test]
fn sample_of_follows_the_normative_order() {
    let mesh = ring_of(&[1.0; 12], &straight_frames(), &clockwise_from_top()).unwrap();
    assert_eq!(mesh.sample_of, (0..12).collect::<Vec<u32>>());
}
// break: transpose node and sector.

#[test]
fn a_nan_radius_makes_a_counted_hole() {
    let mut radii = [1.0; 12];
    radii[6] = f64::NAN; // node 1, sector 2
    let mesh = ring_of(&radii, &straight_frames(), &clockwise_from_top()).unwrap();
    assert_eq!(mesh.points.len(), 11);
    assert_eq!(mesh.holes, 4);
    assert_eq!(mesh.triangles.len(), 8);
}
// break: fill the hole from a neighbour.

#[test]
fn a_refused_frame_cuts_the_band_without_a_bridge() {
    let mut frames = straight_frames();
    frames.frames[1] = Err(FrameRefusal::HighSideUndefined {
        md: 150.0,
        inclination: 0.0,
    });
    let mesh = ring_of(&[1.0; 12], &frames, &clockwise_from_top()).unwrap();
    assert!(mesh.triangles.is_empty());
    assert_eq!(mesh.holes, 8);
    assert_eq!(mesh.points.len(), 8);
}
// break: join node 0 to node 2.

#[test]
fn a_missing_convention_and_wrong_counts_are_refused() {
    let frames = straight_frames();
    let samples = |sectors, radii| PolarSamples {
        sectors,
        radii,
        angles: Angles::Shared(&QUARTERS),
    };
    assert_eq!(
        envelope(&frames, &samples(4, &[1.0; 12]), None, None, 1.0),
        Err(EnvelopeRefusal::ConventionUndeclared)
    );
    let c = clockwise_from_top();
    assert_eq!(
        envelope(&frames, &samples(2, &[1.0; 6]), Some(&c), None, 1.0),
        Err(EnvelopeRefusal::TooFewSectors { k: 2 })
    );
    assert_eq!(
        envelope(&frames, &samples(4, &[1.0; 11]), Some(&c), None, 1.0),
        Err(EnvelopeRefusal::CountMismatch {
            nodes: 3,
            sectors: 4,
            values: 11
        })
    );
    assert_eq!(
        envelope(&frames, &samples(4, &[1.0; 12]), Some(&c), None, 0.0),
        Err(EnvelopeRefusal::BadRadialFactor { factor: 0.0 })
    );
}
// break: use a default convention.

#[test]
fn the_ribbon_follows_one_column_of_the_cube() {
    let grid = TransverseGrid::new(
        axis(PlaneAxis::HighSide, AxisSign::Positive, &[-10.0, 0.0, 10.0]),
        axis(PlaneAxis::Right, AxisSign::Positive, &[-20.0, 20.0]),
    )
    .unwrap();
    let mesh = ribbon(&straight_frames(), &grid, RibbonPlane::FixedFast(1)).unwrap();
    assert_eq!(mesh.points.len(), 9);
    // node 0, row 2: the slow index 2 (+10 along the high side), the fast index 1 (+20 to the south).
    close3(mesh.points[2], [100.0, -20.0, -990.0], 1e-9);
    assert_eq!(mesh.sample_of[2], 5); // 0 * (3 * 2) + 2 * 2 + 1
    assert_eq!(mesh.triangles.len(), 8);
    assert!(matches!(
        ribbon(&straight_frames(), &grid, RibbonPlane::FixedFast(2)),
        Err(EnvelopeRefusal::RibbonIndexOutside { index: 2, count: 2 })
    ));
}
// break: drop the factor `m n` of the node in `sample_of`.
