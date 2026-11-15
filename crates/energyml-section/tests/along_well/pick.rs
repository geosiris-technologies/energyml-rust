//! `along_well::pick`: what lies under the pointer, and the way back.

use energyml_geometry::around_well::projection::WellProjections;
use energyml_section::along_well::{AlongWellRefusal, pick, pixel_of};

use super::{inclined, model_on, view};

const NODES: [f64; 4] = [100.0, 200.0, 300.0, 400.0];
const ROWS: [f64; 3] = [-10.0, 0.0, 10.0];

/// `pick` is the way back of `pixel_of`: 20 points of the section, in each of the three projections that
/// can invert, come back to the measured depth and the offset they were placed at, to 1e-6. The one
/// arithmetic between drawing units and pixels is the plot frame's, used by both.
///
/// To break it: write a second window arithmetic in one of the two (the points come back shifted).
#[test]
fn pick_is_the_way_back_of_pixel_of() {
    let projections = WellProjections::standard();
    for id in ["md_tvd", "developed", "vertical_section"] {
        let model = model_on(inclined(60.0), id, &NODES, &ROWS, (1..=12).map(f64::from).collect(), false);
        for i in 0..20 {
            let md = 120.0 + 13.0 * i as f64;
            let dn = -8.0 + 0.8 * i as f64;
            let pixel = pixel_of(&model, &view(), &projections, md, dn).expect("a pixel");
            let readout = pick(&model, &view(), &projections, pixel, None)
                .expect("no refusal")
                .unwrap_or_else(|| panic!("{id}: nothing under the pixel of ({md}, {dn})"));
            assert!((readout.md - md).abs() < 1e-6, "{id}: md {} for {md}", readout.md);
            assert!((readout.dn - dn).abs() < 1e-6, "{id}: dn {} for {dn}", readout.dn);
        }
    }
}

/// Under the pointer the value is the cell's own (no value is made up); with an interpolation asked it is
/// the interpolation along the MD, within the row. Two nodes at MD 20 and 60 hold 10 and 40: at MD 30, a
/// quarter of the way, the cell says 10 and `linear` says 17.5.
///
/// To break it: interpolate by default (the first answer is 17.5).
#[test]
fn the_value_is_the_cells_own_unless_an_interpolation_is_asked() {
    let projections = WellProjections::standard();
    let model = model_on(
        inclined(60.0),
        "md_tvd",
        &[20.0, 60.0],
        &[-10.0, 10.0],
        vec![10.0, 10.0, 40.0, 40.0],
        false,
    );
    let pixel = pixel_of(&model, &view(), &projections, 30.0, 5.0).expect("a pixel");

    let own = pick(&model, &view(), &projections, pixel, None).unwrap().unwrap();
    assert_eq!((own.node, own.row), (Some(0), Some(1)));
    assert_eq!(own.value, Some(10.0));

    let linear = pick(&model, &view(), &projections, pixel, Some("linear")).unwrap().unwrap();
    assert!((linear.value.unwrap() - 17.5).abs() < 1e-4, "{:?}", linear.value);

    assert!(matches!(
        pick(&model, &view(), &projections, pixel, Some("nowhere")),
        Err(AlongWellRefusal::UnknownInterpolation { .. })
    ));
}

/// The readout gives the distances to the boundaries at the node: with 8 m above and 5 m below, a point at
/// offset 3 is 5 m under the upper boundary and 8 m over the lower one.
///
/// To break it: take the radius as a distance from the point (the two are 8 and 5).
#[test]
fn the_readout_measures_the_distance_to_each_boundary() {
    let projections = WellProjections::standard();
    let model = model_on(
        inclined(60.0),
        "md_tvd",
        &NODES,
        &ROWS,
        (1..=12).map(f64::from).collect(),
        true,
    );
    let pixel = pixel_of(&model, &view(), &projections, 200.0, 3.0).expect("a pixel");
    let readout = pick(&model, &view(), &projections, pixel, None).unwrap().unwrap();
    assert!((readout.to_above.unwrap() - 5.0).abs() < 1e-6);
    assert!((readout.to_below.unwrap() - 8.0).abs() < 1e-6);
}

/// A pixel outside the frame has nothing under it.
///
/// To break it: answer for any pixel.
#[test]
fn a_pixel_outside_the_frame_has_nothing_under_it() {
    let projections = WellProjections::standard();
    let model = model_on(inclined(60.0), "md_tvd", &NODES, &ROWS, (1..=12).map(f64::from).collect(), false);
    assert!(pick(&model, &view(), &projections, [-5.0, -5.0], None).unwrap().is_none());
}
