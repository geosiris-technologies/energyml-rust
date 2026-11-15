//! `along_well::boundaries`: the limits of a depth of detection, found by the **value** of the angles.

use std::f64::consts::{PI, TAU};

use energyml_geometry::around_well::projection::MdTvd;
use energyml_section::along_well::{BoundaryMode, BoundaryRefusal, CurtainLayout, boundaries, curtain_cells};

use super::{axis_of, inclined};

/// 16 angles in a scrambled order: 0 sits at rank 5 and pi at rank 11. The radius is the rank plus one, plus
/// 100 per node, so the radius of each side says which angle was taken. A producer that mirrors its angles
/// (`theta -> -theta`) gives the same boundaries.
///
/// To break it: take the ranks 0 and 8 (the first and the middle): the radii are 1 and 9.
#[test]
fn angles_are_found_by_value_never_by_rank() {
    let path = inclined(60.0);
    let axis = axis_of(&path);
    let mut angles: Vec<f64> = (0..16).map(|k| k as f64 * TAU / 16.0).collect();
    angles.swap(0, 5);
    angles.swap(8, 11);
    assert_eq!(angles[5], 0.0);
    assert_eq!(angles[11], PI);
    let nodes = [10.0, 20.0];
    let radii: Vec<f64> = (0..2)
        .flat_map(|n| (0..16).map(move |k| (k + 1) as f64 + 100.0 * n as f64))
        .collect();

    let found = boundaries(&axis, &nodes, &radii, &angles, 16, BoundaryMode::InPlane, &MdTvd).unwrap();
    for n in 0..2 {
        assert_eq!(found.above[n].unwrap().dn, 6.0 + 100.0 * n as f64);
        assert_eq!(found.below[n].unwrap().dn, -(12.0 + 100.0 * n as f64));
    }
    assert_eq!((found.absent, found.refused), (0, 0));

    let mirrored: Vec<f64> = angles.iter().map(|angle| -angle).collect();
    let again = boundaries(
        &axis,
        &nodes,
        &radii,
        &mirrored,
        16,
        BoundaryMode::InPlane,
        &MdTvd,
    )
    .unwrap();
    assert_eq!(found, again);
}

/// Angles that miss 0 and pi by 0.1 rad put nothing in the plane of the section: a named refusal, never an
/// empty pair of limits.
///
/// To break it: return the empty limits.
#[test]
fn no_angle_in_the_plane_is_refused() {
    let axis = axis_of(&inclined(60.0));
    let angles: Vec<f64> = (0..4).map(|k| 0.1 + k as f64 * PI / 2.0).collect();
    let radii = [1.0, 2.0, 3.0, 4.0];
    let refused = boundaries(&axis, &[10.0], &radii, &angles, 4, BoundaryMode::InPlane, &MdTvd);
    assert!(matches!(refused, Err(BoundaryRefusal::NoAngleInPlane { .. })));
}

/// A boundary and the centre of the curtain's row at its radius are one point, **to the bit**: both are
/// `projection.place(axis, md, dn)` with the same arguments.
///
/// To break it: recompute the boundary as `at + r h` by hand (the last bits differ).
#[test]
fn a_boundary_and_the_curtain_row_at_its_radius_are_one_point() {
    let path = inclined(60.0);
    let axis = axis_of(&path);
    let nodes = [10.0, 20.0, 30.0];
    let layout = CurtainLayout::new(&nodes, &[-10.0, 0.0, 10.0], 0.0).unwrap();
    let geometry = curtain_cells(&axis, &layout, &MdTvd).unwrap();
    let found = boundaries(
        &axis,
        &nodes,
        &[10.0, 10.0, 10.0],
        &[0.0],
        1,
        BoundaryMode::InPlane,
        &MdTvd,
    )
    .unwrap();
    for n in 0..3u32 {
        let centre = geometry
            .cells
            .iter()
            .find(|cell| cell.node == n && cell.row == 2)
            .unwrap()
            .centre;
        assert_eq!(found.above[n as usize].unwrap().at, centre);
    }
}

/// Two angles at 0 on one node: which radius is the boundary is not decided here.
///
/// To break it: take the first one.
#[test]
fn two_radii_at_zero_on_one_node_are_refused() {
    let axis = axis_of(&inclined(60.0));
    let refused = boundaries(
        &axis,
        &[10.0],
        &[1.0, 2.0],
        &[0.0, 0.0],
        2,
        BoundaryMode::InPlane,
        &MdTvd,
    );
    assert!(matches!(
        refused,
        Err(BoundaryRefusal::TwoAnglesInPlane { node: 0, side: "0" })
    ));
}

/// The radii and the angles must lie on the same rings.
///
/// To break it: truncate to the shorter of the two.
#[test]
fn radii_and_angles_on_different_rings_are_refused() {
    let axis = axis_of(&inclined(60.0));
    let refused = boundaries(
        &axis,
        &[10.0, 20.0],
        &[1.0; 6],
        &[0.0; 4],
        3,
        BoundaryMode::InPlane,
        &MdTvd,
    );
    assert!(matches!(refused, Err(BoundaryRefusal::DodShapeMismatch { .. })));
}

/// In `Projected` mode every finite radius is laid at `r cos(theta)` on the high-side axis; a radius that is
/// NaN or negative is counted, never drawn.
///
/// To break it: lay every radius at `r` (the point at 90 degrees is at 5, not at 0).
#[test]
fn the_projected_mode_lays_every_radius_at_r_cos_theta() {
    let axis = axis_of(&inclined(60.0));
    let angles = [0.0, PI / 2.0, PI];
    let radii = [5.0, 5.0, f64::NAN];
    let found = boundaries(
        &axis,
        &[10.0],
        &radii,
        &angles,
        3,
        BoundaryMode::Projected,
        &MdTvd,
    )
    .unwrap();
    let dn: Vec<f64> = found.projected[0].iter().map(|point| point.dn).collect();
    assert_eq!(dn.len(), 2);
    assert!((dn[0] - 5.0).abs() < 1e-12);
    assert!(dn[1].abs() < 1e-12);
    assert_eq!(found.absent, 1);
    assert!(found.below[0].is_none());
}
