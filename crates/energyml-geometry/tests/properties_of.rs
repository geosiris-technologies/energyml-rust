//! `scene::PropertiesOf` hands each representation exactly the properties
//! `attributes_for` would have kept from the whole package.
//!
//! Two roads to the same list: the index (`energyml_domain::property::Index`,
//! built once), and each property read on its own and asked what it hangs on —
//! the very test `attributes_for` applies before anything else. Same members,
//! same order; so handing `attributes_for` the index's answer instead of every
//! property changes what it attaches by nothing, and costs one read per
//! property instead of one per property **per object** (29/09).
//!
//! On every public reference package; skipped, and said, when there is none.

use std::sync::Arc;

use energyml_core::identity;
use energyml_geometry::scene::PropertiesOf;
use energyml_storage::EpcStore;

#[test]
fn each_representation_gets_the_properties_that_name_it_in_package_order() {
    let packages = crate::fixtures::public_corpus();
    if packages.is_empty() {
        eprintln!("skipped: no reference package (scripts/fetch-fixtures.py)");
        return;
    }
    let mut compared = 0usize;
    for path in packages {
        let Ok(store) = EpcStore::open(&path, Arc::new(energyml_schemas::all())) else {
            continue;
        };
        let resources = store.resources();
        let objects: Vec<_> = resources
            .iter()
            .filter(|summary| summary.qualified_type.ends_with("Property"))
            .filter_map(|summary| store.get_object(&summary.uri).ok())
            .collect();
        let properties: Vec<&dyn energyml_core::value::Reflect> =
            objects.iter().map(|one| one.as_reflect()).collect();
        let grouped = PropertiesOf::new(&properties);

        for summary in resources
            .iter()
            .filter(|summary| summary.qualified_type.contains("Representation"))
        {
            let by_index: Vec<String> = grouped
                .of(&summary.uuid)
                .iter()
                .filter_map(|one| identity::uuid_of_reflect(*one).map(str::to_string))
                .collect();
            let by_reading: Vec<String> = properties
                .iter()
                .filter(|one| {
                    energyml_domain::property::read(**one).is_ok_and(|read| {
                        read.supporting_representation
                            .is_some_and(|support| support.eq_ignore_ascii_case(&summary.uuid))
                    })
                })
                .filter_map(|one| identity::uuid_of_reflect(*one).map(str::to_string))
                .collect();
            assert_eq!(by_index, by_reading, "{}: {}", path.display(), summary.uri);
            compared += by_index.len();
        }
    }
    if compared == 0 {
        eprintln!("skipped: no property hangs on a representation in testdata/");
    }
    assert!(
        compared == 0 || compared > 10,
        "{compared} property link(s) compared"
    );
}
