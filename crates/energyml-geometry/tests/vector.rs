//! The vector operations on `Point`: the one cross product and the one
//! normalisation (the convention of the hole frame: right-handed placed frame,
//! z up).

use energyml_geometry::vector::{cross, dot, norm, unit, unit_above};

#[test]
fn the_cross_product_is_right_handed() {
    // x cross y = z, and the reverse order flips the sign.
    assert_eq!(cross([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]), [0.0, 0.0, 1.0]);
    assert_eq!(cross([0.0, 1.0, 0.0], [1.0, 0.0, 0.0]), [0.0, 0.0, -1.0]);
}

#[test]
fn unit_refuses_what_has_no_direction() {
    assert_eq!(unit([0.0; 3]), None);
    assert_eq!(unit([f64::NAN, 0.0, 0.0]), None);
    assert_eq!(unit([3.0, 4.0, 0.0]), Some([0.6, 0.8, 0.0]));
    // Two floors, both kept: the caller's, and the crate default.
    assert_eq!(unit_above([1e-13, 0.0, 0.0], 1e-12), None);
    let tiny = unit([1e-13, 0.0, 0.0]).expect("above f64::EPSILON");
    assert!((tiny[0] - 1.0).abs() < 1e-12 && tiny[1] == 0.0 && tiny[2] == 0.0);
}

#[test]
fn dot_and_norm_read_a_three_four_five() {
    assert_eq!(dot([3.0, 4.0, 0.0], [3.0, 4.0, 0.0]), 25.0);
    assert_eq!(norm([3.0, 4.0, 0.0]), 5.0);
}
