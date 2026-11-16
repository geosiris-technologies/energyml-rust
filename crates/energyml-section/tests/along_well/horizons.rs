//! `along_well::horizons`: the horizon cut by the curtain, and the tops of the hole itself.
//!
//! No solver is written in the module (`tops::tops` is reused): these tests hold it to facts that are not
//! its own, a plane at a known depth and a hole at a known angle.

use energyml_geometry::around_well::projection::{Developed, WellProjection};
use energyml_section::along_well::{PlacedMesh, computed_tops, horizon_in_curtain};

use super::{axis_of, drain_east, inclined};

/// A horizontal square of 20 km at the height `z` (z up, metres).
fn plane_at(z: f64) -> PlacedMesh {
    PlacedMesh {
        uri: "eml:///resqml22.TriangulatedSetRepresentation(b0000000-0000-4000-8000-000000000001)"
            .to_string(),
        points: vec![
            -1e4, -1e4, z, //
            1e4, -1e4, z, //
            1e4, 1e4, z, //
            -1e4, 1e4, z,
        ],
        triangles: vec![0, 1, 2, 0, 2, 3],
        quads: Vec::new(),
    }
}

/// A horizontal drain, and a plane 5.5 m under it: in the plane of the curtain every column crosses the
/// horizon at `dn = -5.5`, the true vertical depth is the hole's (1000 m) plus 5.5, and the abscissa is the
/// length along the drain (a horizontal hole has `cos I = 0`: the offset does not move it). Two facts that
/// are not `tops`'s own: the offset of a plane under a horizontal hole, and the length of a straight line.
///
/// To break it: pass the measured depth, not `dn`, to `tops` (the crossings are at `md`, far outside the
/// range, and none is found).
#[test]
fn a_flat_horizon_under_a_horizontal_drain_is_at_minus_five_and_a_half() {
    let path = drain_east();
    let axis = axis_of(&path);
    let mesh = plane_at(-1005.5);
    let heights = mesh.heights().unwrap();
    let columns = [1100.0, 1300.0, 1500.0, 1700.0, 1900.0];
    let trace = horizon_in_curtain(
        &path,
        &axis,
        &columns,
        [-20.0, 20.0],
        1.0,
        &heights,
        &mesh.uri,
        &Developed,
    )
    .unwrap();
    assert_eq!(trace.surface, mesh.uri);
    assert_eq!(trace.columns_without_frame, 0);
    assert_eq!(trace.breaks, 0);
    assert_eq!(trace.spans.len(), 1, "the five crossings are one line");
    let line = &trace.spans[0];
    assert_eq!(line.len(), 5);
    for (point, md) in line.iter().zip(columns) {
        assert!((point.dn + 5.5).abs() < 1e-9, "dn {}", point.dn);
        assert!((point.at[1] - 1005.5).abs() < 1e-9);
        assert!((point.at[0] - (md - 1000.0)).abs() < 1e-6);
        let again = Developed.place(&axis, md, point.dn).unwrap();
        assert_eq!(point.at, again, "the point is placed by the one function");
    }
}

/// A column at the vertical part of the hole (under 1 degree: no high side, S4) is not looked at and
/// counted; the others are.
///
/// To break it: take the high side of the nearest column (no column is counted).
#[test]
fn a_column_without_high_side_is_counted_and_not_looked_at() {
    let path = drain_east();
    let axis = axis_of(&path);
    let mesh = plane_at(-1005.5);
    let heights = mesh.heights().unwrap();
    let trace = horizon_in_curtain(
        &path,
        &axis,
        &[500.0, 1300.0],
        [-20.0, 20.0],
        1.0,
        &heights,
        &mesh.uri,
        &Developed,
    )
    .unwrap();
    assert_eq!(trace.columns_without_frame, 1);
    assert_eq!(trace.spans.len(), 1);
    assert_eq!(trace.spans[0].len(), 1);
}

/// A straight hole at 30 degrees from the vertical crosses the horizontal plane at TVD 100 where
/// `md cos 30 = 100`: `md = 100 / cos 30`, whatever the sampling.
///
/// To break it: shift the nodes the tops are sampled at (the root moves by the shift).
#[test]
fn computed_tops_cross_a_plane_at_the_analytic_md() {
    let path = inclined(30.0);
    let mesh = plane_at(-100.0);
    let heights = mesh.heights().unwrap();
    let found = computed_tops(&path, [0.0, 1000.0], 1.0, &heights).unwrap();
    assert_eq!(found.crossings.len(), 1);
    let expected = 100.0 / 30.0_f64.to_radians().cos();
    assert!(
        (found.crossings[0].md - expected).abs() < 1e-9,
        "{}",
        found.crossings[0].md
    );
}
