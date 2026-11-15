//! `along_well::cells`: the quadrilaterals of a curtain, placed by one projection.
//!
//! Spec: the geosteering section, 02 section 5.3 (the Jacobian of the developed
//! projection is `-1 + dn I'`, so the curtain folds where `dn I'` reaches 1) and
//! S7 (a gap is never bridged: the cells of a node reach half the way to its
//! neighbour, at most half the widest gap the rule bridges).

use energyml_geometry::around_well::hole_frame::FrameRefusal;
use energyml_geometry::around_well::projection::{
    Developed, MdTvd, ProjectionRefusal, VerticalSection, WellProjection,
};
use energyml_section::along_well::{CurtainLayout, CurtainRefusal, curtain_cells};

use super::{axis_of, build_up, drain_east, east_section, inclined};

/// Nodes at 0, 1, 2, 10 and 11: the median step is 1, so the widest gap bridged is 3 and a node's cells
/// reach 1.5 at most. The step of 8 between nodes 2 and 10 is a gap: nothing is drawn in `[3.5, 8.5]`.
///
/// To break it: do not cap the half widths (`half_cap: None`), and node 2 reaches 6.
#[test]
fn a_gap_is_not_bridged_by_the_cells() {
    let layout = CurtainLayout::new(&[0.0, 1.0, 2.0, 10.0, 11.0], &[-10.0, 0.0, 10.0], 0.0).unwrap();
    assert_eq!(layout.md_bounds[2], Some([1.5, 3.5]));
    assert_eq!(layout.md_bounds[3], Some([8.5, 10.5]));
    assert_eq!(layout.gaps, vec![[3.5, 8.5]]);
}

/// A drain at 90 degrees: `dn` runs straight up, whichever of the two projections lays it. The centre
/// of the row at offset 10 is 10 m above the hole (a TVD 10 m less) and at the same abscissa.
///
/// To break it: invert the sign of `dn` in the placement (the centre goes 10 m below).
#[test]
fn a_horizontal_drain_places_dn_straight_up() {
    let path = drain_east();
    let axis = axis_of(&path);
    let layout = CurtainLayout::new(&[1200.0, 1400.0, 1600.0, 1800.0], &[-10.0, 0.0, 10.0], 0.0).unwrap();
    for projection in [&MdTvd as &dyn WellProjection, &Developed] {
        let geometry = curtain_cells(&axis, &layout, projection).unwrap();
        assert_eq!(geometry.cells.len(), 12);
        for cell in geometry.cells.iter().filter(|cell| cell.row == 2) {
            let md = layout.node_md[cell.node as usize];
            let on_hole = projection.place(&axis, md, 0.0).unwrap();
            assert!((cell.centre[1] - (path.tvd(md).unwrap() - 10.0)).abs() < 1e-9);
            assert!((cell.centre[0] - on_hole[0]).abs() < 1e-9);
        }
    }
}

/// At 84 degrees from the vertical the abscissa of `developed` is moved by `dn cos I`: the row at offset 40
/// sits `40 cos 84` degrees ahead of the hole. In `md_tvd` the abscissa is the MD, whatever `dn`.
///
/// To break it: use `md_tvd` for the developed expectation (the move is 0).
#[test]
fn at_84_degrees_developed_moves_forward_by_dn_cos_i() {
    let path = inclined(84.0);
    let axis = axis_of(&path);
    let layout = CurtainLayout::new(&[300.0, 500.0, 700.0], &[0.0, 40.0], 0.0).unwrap();

    let developed = curtain_cells(&axis, &layout, &Developed).unwrap();
    let cell = developed
        .cells
        .iter()
        .find(|cell| cell.node == 1 && cell.row == 1)
        .unwrap();
    let on_hole = Developed.place(&axis, 500.0, 0.0).unwrap();
    assert!((cell.centre[0] - on_hole[0] - 40.0 * 84.0_f64.to_radians().cos()).abs() < 1e-9);

    let flat = curtain_cells(&axis, &layout, &MdTvd).unwrap();
    let cell = flat.cells.iter().find(|cell| cell.node == 1 && cell.row == 1).unwrap();
    assert_eq!(cell.centre[0], 500.0);
}

/// A build-up of radius 30 m (`I' = 1/30` rad/m): in `developed` the abscissa of the row at offset `dn` has
/// the derivative `sin I (1 - dn / 30)`, so a cell whose high border passes 30 m folds. The rows are
/// at 0, 10, 20 and 45 m: their borders are [-5, 5], [5, 15], [15, 32.5] and [32.5, 57.5], so the two
/// last rows fold. In `md_tvd` the abscissa is the MD: nothing folds.
///
/// To break it: compute the fold on the centre line alone (`d0`): nothing folds in `developed` either.
#[test]
fn md_tvd_never_folds_developed_folds_where_dn_times_i_prime_reaches_one() {
    let path = build_up(30.0);
    let axis = axis_of(&path);
    let nodes = [5.0, 10.0, 15.0, 20.0, 25.0, 30.0, 35.0, 40.0];
    let layout = CurtainLayout::new(&nodes, &[0.0, 10.0, 20.0, 45.0], 0.0).unwrap();

    let flat = curtain_cells(&axis, &layout, &MdTvd).unwrap();
    assert!(flat.cells.iter().all(|cell| !cell.folded));
    assert!(flat.folded.is_empty());

    let developed = curtain_cells(&axis, &layout, &Developed).unwrap();
    assert_eq!(developed.cells.len(), 32);
    for cell in &developed.cells {
        assert_eq!(cell.folded, cell.row >= 2, "node {} row {}", cell.node, cell.row);
    }
    assert!(!developed.folded.is_empty());
    assert!(developed.reversed.is_empty(), "the centre line itself never runs backwards here");
}

/// A node at 0.2 m of a build-up is under the 1 degree of the threshold (S4): it has no high side, so it has
/// no cell and the refusal is kept; its neighbours keep theirs, and no cell is borrowed from them.
///
/// To break it: fill the missing node from its neighbour (the node count of the cells is 8, not 6).
#[test]
fn a_node_without_high_side_has_no_cell() {
    let path = build_up(30.0);
    let axis = axis_of(&path);
    let layout = CurtainLayout::new(&[0.2, 10.0, 20.0, 30.0], &[0.0, 10.0], 0.0).unwrap();
    let geometry = curtain_cells(&axis, &layout, &MdTvd).unwrap();
    assert_eq!(geometry.missing.len(), 1);
    assert_eq!(geometry.missing[0].0, 0);
    assert!(matches!(
        geometry.missing[0].1,
        ProjectionRefusal::Frame(FrameRefusal::HighSideUndefined { .. })
    ));
    assert!(geometry.cells.iter().all(|cell| cell.node != 0));
    assert_eq!(geometry.cells.len(), 6);
}

/// A plane 5 m off the hole is exact in `md_tvd` (the lateral offset is horizontal, the abscissa is the MD),
/// accepted in `developed` (second order), and refused where the projection has no lateral offset.
///
/// To break it: accept the lateral plane in every projection.
#[test]
fn a_lateral_plane_is_refused_where_it_is_not_placed() {
    let path = inclined(60.0);
    let axis = axis_of(&path);
    let layout = CurtainLayout::new(&[100.0, 200.0, 300.0], &[-5.0, 5.0], 5.0).unwrap();
    assert!(curtain_cells(&axis, &layout, &MdTvd).is_ok());
    assert!(curtain_cells(&axis, &layout, &Developed).is_ok());
    let section = east_section();
    let vertical_section = VerticalSection {
        azimuth: section.azimuth,
        origin: section.origin,
    };
    assert!(matches!(
        curtain_cells(&axis, &layout, &vertical_section),
        Err(CurtainRefusal::LateralNotPlaced { .. })
    ));
}

/// Rows that go down (offsets from the top to the bottom) are bounded in the same order as the values.
///
/// To break it: bound the rows in ascending order whatever their order (the bounds are mirrored).
#[test]
fn rows_in_decreasing_order_keep_the_order_of_the_values() {
    let layout = CurtainLayout::new(&[0.0, 1.0], &[10.0, 0.0, -10.0], 0.0).unwrap();
    assert_eq!(layout.row_bounds, vec![[5.0, 15.0], [-5.0, 5.0], [-15.0, -5.0]]);
    assert!(matches!(
        CurtainLayout::new(&[0.0, 1.0], &[0.0, 10.0, 5.0], 0.0),
        Err(CurtainRefusal::RowsNotMonotonic { at: 2 })
    ));
    assert!(matches!(
        CurtainLayout::new(&[0.0, 1.0], &[0.0], 0.0),
        Err(CurtainRefusal::TooFewRows { given: 1 })
    ));
}
