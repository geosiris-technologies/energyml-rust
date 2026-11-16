//! The one rule of "count against shape".
//!
//! RESQML TRG 2.2 §7.1 p. 222: *"The size of the vector is constant on all
//! elements"*; `ValueCountPerIndexableElement` is the count per element.

use energyml_domain::property::{CountFit, count_fit};

#[test]
fn one_count_rule() {
    assert_eq!(count_fit(4, 6, 24), CountFit::Fits);
    assert_eq!(
        count_fit(4, 6, 4),
        CountFit::OnePerElement { declared: 6 },
        "one value per element where six are declared"
    );
    assert_eq!(
        count_fit(4, 6, 23),
        CountFit::Mismatch {
            expected: 24,
            actual: 23
        }
    );
    assert_eq!(
        count_fit(0, 1, 0),
        CountFit::Mismatch {
            expected: 0,
            actual: 0
        },
        "a zero count is never lined up"
    );
    assert_eq!(
        count_fit(10, 1, 9),
        CountFit::Mismatch {
            expected: 10,
            actual: 9
        }
    );
}
