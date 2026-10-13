//! `color::derived_colour` against the front end's `colourFor`: the same table is
//! asserted in a front end's colour test, so the two implementations of one
//! fact cannot drift apart without one of the two suites saying so.

use energyml_domain::color::derived_colour;

/// Seeds and the colours `colourFor` gives them, read off the front end (03/10).
const TABLE: [(&str, &str); 6] = [
    ("4a6b5ef5-11c8-57ad-8cb9-68fff5e59290", "#61d17f"),
    ("0d445ed7-3820-500c-9b4a-cc6a03a336d8", "#61a8d1"),
    ("fdf4510a-3aa1-578d-8935-0c9962e3773b", "#d1b161"),
    ("", "#cfd161"),
    ("a", "#a461d1"),
    ("seed 7", "#d17261"),
];

#[test]
fn the_derived_colour_is_the_pages() {
    for (seed, colour) in TABLE {
        assert_eq!(derived_colour(seed), colour, "seed {seed:?}");
    }
}
