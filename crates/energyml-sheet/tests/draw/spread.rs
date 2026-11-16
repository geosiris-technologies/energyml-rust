//! Label spreading: a label is moved only towards larger values, and the order
//! is kept (the desktop's `well_column::spread`, the studio's `spreadTopLabels`).

use energyml_sheet::draw::DrawError;
use energyml_sheet::draw::spread::spread;

#[test]
fn spread_pushes_labels_down_only_and_keeps_their_order() {
    // Breaks if the group is centred (pushed up as well as down).
    assert_eq!(
        spread(&[10.0, 12.0, 11.0, 40.0], 5.0).unwrap(),
        vec![10.0, 20.0, 15.0, 40.0]
    );
}

#[test]
fn a_label_alone_stays_where_it_is() {
    // Breaks if the floor starts at 0 instead of minus infinity.
    assert_eq!(spread(&[100.0], 5.0).unwrap(), vec![100.0]);
    assert_eq!(spread(&[-100.0], 5.0).unwrap(), vec![-100.0]);
    assert_eq!(spread(&[10.0, 12.0, 11.0, 40.0], 5.0).unwrap()[3], 40.0);
}

#[test]
fn a_non_finite_row_is_not_placed_and_moves_nothing() {
    // Breaks if a NaN is placed at the floor, which invents a position.
    let out = spread(&[10.0, f64::NAN, 12.0], 5.0).unwrap();
    assert_eq!(out[0], 10.0);
    assert!(out[1].is_nan());
    assert_eq!(out[2], 15.0);
}

#[test]
fn a_bad_gap_is_refused() {
    // Breaks if the gap check is removed.
    assert!(matches!(spread(&[1.0, 2.0], -1.0), Err(DrawError::BadGap { .. })));
    assert!(matches!(spread(&[1.0], f64::NAN), Err(DrawError::BadGap { .. })));
}
