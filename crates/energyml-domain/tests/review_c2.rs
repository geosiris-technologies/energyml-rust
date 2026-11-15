//! Lot C2 of the review of 15/11: no whole-buffer copy where a borrow serves.
//! Built in memory; no file is read.

use std::borrow::Cow;

use energyml_domain::series::{Channel, Quality};
use energyml_domain::values::Values;

fn channel(values: Values) -> Channel {
    let quality = vec![Quality::Good; values.len()];
    Channel {
        uuid: None,
        mnemonic: "c".into(),
        uom: None,
        data_kind: None,
        property_kind: None,
        measure_class: None,
        values,
        quality,
    }
}

/// A floating channel's numbers are borrowed, not copied; an integer channel's
/// are converted, and both agree with the owned form.
#[test]
fn numbers_ref_borrows_a_floating_channel() {
    let floating = channel(Values::Floating(vec![1.0, 2.5, 4.0]));
    let borrowed = floating.numbers_ref().expect("a numeric channel");
    assert!(
        matches!(borrowed, Cow::Borrowed(_)),
        "a floating channel must not be copied"
    );
    let Values::Floating(own) = &floating.values else {
        unreachable!()
    };
    assert_eq!(
        borrowed.as_ptr(),
        own.as_ptr(),
        "the slice is the channel's own buffer"
    );
    assert_eq!(floating.numbers(), Some(vec![1.0, 2.5, 4.0]));

    let integer = channel(Values::Integer(vec![1, 2, 3]));
    assert_eq!(integer.numbers_ref().as_deref(), Some(&[1.0, 2.0, 3.0][..]));
    assert_eq!(integer.numbers(), Some(vec![1.0, 2.0, 3.0]));

    assert!(channel(Values::Boolean(vec![true])).numbers_ref().is_none());
    // Break it: make `numbers_ref` clone the floating buffer again (`Cow::Owned(values.clone())`).
}

/// The tally reads the borrowed numbers and gives the same extremes.
#[test]
fn tally_extremes_are_unchanged() {
    let tally = channel(Values::Floating(vec![3.0, -1.0, 2.0])).tally();
    assert_eq!((tally.min, tally.max, tally.good), (Some(-1.0), Some(3.0), 3));
    // Break it: index `all[at + 1]` in `Channel::tally`.
}
