//! Do the `PropertyKind`s that the table names really exist?
//!
//! The unit test of `table` verifies that each column names a kind **from its
//! own list**, which is circular: it would pass on seven invented uuids.
//! This one confronts the constants with the **official dictionary**, read by
//! `energyml-domain` from the PropertyKind dictionary v2.3 -
//! two independent sources of the same fact, one of which is not code written
//! here. It is the triangulation that the project's contribution rules ask for.
//!
//! What this catches: a mistyped uuid. A package carrying it would have a
//! dangling reference that no store resolves - precisely the fault that the
//! graph has distinguished since yesterday from a legitimate absence.

use energyml_build::table;
use energyml_domain::property_kind;

#[test]
fn every_kind_named_by_the_table_is_in_the_official_dictionary() {
    let kinds = [
        table::CODE,
        table::FORMATION_NAME,
        table::NAME,
        table::GEOLOGIC_AGE,
        table::COUNT,
        table::DESCRIPTION,
        table::UNKNOWN,
    ];
    property_kind::with_default(|dictionary| {
        for kind in &kinds {
            let found = dictionary.by_uuid(kind.uuid).unwrap_or_else(|| {
                panic!(
                    "`{}` ({}) is not in the dictionary: a package naming it would carry \
                     a reference that nothing resolves",
                    kind.title, kind.uuid
                )
            });
            // **And the title too.** A right uuid under a wrong name would write
            // in every reference a title that the dictionary contradicts - a
            // reader that displays the title of the DOR would show something other than
            // what the reference designates.
            assert_eq!(
                found.title.trim(),
                kind.title,
                "uuid {} is that of “{}”, not of “{}”",
                kind.uuid,
                found.title,
                kind.title
            );
        }
    });
}

#[test]
fn the_check_can_fail() {
    // The counter-proof: a uuid that does not exist must be unfindable. Without
    // it, an empty dictionary would make the previous test pass without proving
    // anything - two checks of this suite have already passed on empty
    // sets on the first try.
    property_kind::with_default(|dictionary| {
        assert!(
            dictionary
                .by_uuid("00000000-0000-4000-8000-000000000000")
                .is_none()
        );
        // And the dictionary is not empty, otherwise everything above is
        // vacuity.
        assert!(
            dictionary.by_uuid(table::FORMATION_NAME.uuid).is_some(),
            "the dictionary was not loaded"
        );
    });
}
