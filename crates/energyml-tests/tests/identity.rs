//! Identity over the real generated crates.
//!
//! The interoperability rules this pins down are the ones a hand-written model
//! cannot: that RESQML 2.0.1 keeps its `obj_` prefix in identifiers while its
//! XML tag does not, that common 2.0's `EpcExternalPartReference` is a
//! top-level object at all, and that a reference can be built in any eml
//! version from an object of any other.

use energyml_core::identity::{self, ObjectIdentity, ObjectRef};
use energyml_core::prelude::*;
use energyml_core::registry;

use energyml_eml_2_0 as eml20;
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_0_1 as resqml201;
use energyml_resqml_2_2 as resqml22;

const UUID: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";

fn activity_201() -> resqml201::Activity {
    resqml201::Activity {
        uuid: UUID.to_string(),
        citation: eml20::Citation {
            title: "History match".to_string(),
            ..Default::default()
        },
        ..Default::default()
    }
}

#[test]
fn resqml_2_0_1_identifiers_keep_the_obj_prefix() {
    let info = <resqml201::Activity as EnergymlType>::type_info();

    assert_eq!(info.xml_name, "Activity", "the XML tag is the element name");
    assert_eq!(
        info.object_type, "obj_Activity",
        "identifiers use the complexType name"
    );
    assert_eq!(info.type_key, "Activity", "grouping ignores the prefix");
    assert_eq!(
        info.content_type,
        Some("application/x-resqml+xml;version=2.0;type=obj_Activity"),
        "fesapi and energyml-utils write it this way"
    );
    assert_eq!(info.qualified_type, Some("resqml20.obj_Activity"));
}

#[test]
fn resqml_2_2_is_unaffected() {
    let info = <resqml22::TriangulatedSetRepresentation as EnergymlType>::type_info();
    assert_eq!(
        info.object_type, info.xml_name,
        "2.2 names element and type alike"
    );
    assert_eq!(info.type_key, "TriangulatedSetRepresentation");
    assert_eq!(info.schema_version, "2.2");
}

#[test]
fn resqml_2_0_1_objects_inherit_the_abstract_object_fields() {
    // AbstractCitedDataObject restricts eml:AbstractObject rather than
    // extending it. Missing that link left every 2.0.1 object without a uuid.
    let activity = activity_201();
    assert_eq!(ObjectIdentity::uuid(&activity), Some(UUID));
    assert_eq!(ObjectIdentity::title(&activity), Some("History match"));
}

#[test]
fn the_hdf5_proxy_of_common_2_0_is_a_top_level_object() {
    // Declared as a global element with no substitutionGroup - the case the
    // fallback detection exists for. It is in nearly every RESQML 2.0.1 EPC.
    let info = <eml20::EpcExternalPartReference as EnergymlType>::type_info();
    assert!(info.is_top_level, "EpcExternalPartReference is a part of its own");
    assert_eq!(info.xml_name, "EpcExternalPartReference", "the tag has no prefix");
    assert_eq!(info.object_type, "obj_EpcExternalPartReference");
    assert_eq!(
        info.content_type,
        Some("application/x-eml+xml;version=2.0;type=obj_EpcExternalPartReference")
    );
}

#[test]
fn a_reference_can_be_built_in_either_version() {
    let activity = activity_201();

    let old = eml20::DataObjectReference::referencing(&activity).unwrap();
    assert_eq!(
        old.content_type,
        "application/x-resqml+xml;version=2.0;type=obj_Activity"
    );
    assert_eq!(old.uuid, UUID);
    assert_eq!(old.title, "History match");

    let new = eml23::DataObjectReference::referencing(&activity).unwrap();
    assert_eq!(new.qualified_type, "resqml20.obj_Activity");
    assert_eq!(new.uuid, UUID);
}

#[test]
fn a_reference_converts_between_versions_both_ways() {
    let old = eml20::DataObjectReference::referencing(&activity_201()).unwrap();
    let new = eml23::DataObjectReference::referencing(&old).unwrap();
    assert_eq!(new.qualified_type, "resqml20.obj_Activity");

    let back = eml20::DataObjectReference::referencing(&new).unwrap();
    assert_eq!(
        back.content_type, old.content_type,
        "the prefix survives the round trip"
    );
    assert_eq!(back.uuid, old.uuid);
}

#[test]
fn a_2_2_object_is_referenced_by_a_2_3_reference() {
    let repr = resqml22::TriangulatedSetRepresentation {
        uuid: UUID.to_string(),
        citation: energyml_eml_2_3::Citation {
            title: "Top Horizon".to_string(),
            ..Default::default()
        },
        ..Default::default()
    };
    let dor = eml23::DataObjectReference::referencing(&repr).unwrap();
    assert_eq!(dor.qualified_type, "resqml22.TriangulatedSetRepresentation");
    assert_eq!(dor.title, "Top Horizon");

    let key = identity::key_of(&dor).unwrap();
    assert!(key.same_object(&identity::key_of(&repr).unwrap()));
}

#[test]
fn every_top_level_type_agrees_with_its_own_identifiers() {
    let mut reg = registry::Registry::new();
    resqml201::register(&mut reg);
    resqml22::register(&mut reg);
    eml20::register(&mut reg);
    eml23::register(&mut reg);

    for info in reg.types().filter(|i| i.is_top_level) {
        let content = info.content_type.expect("a top-level type has a content type");
        let qualified = info
            .qualified_type
            .expect("a top-level type has a qualified type");

        let ct = identity::parse_content_type(content).unwrap();
        let qt = identity::parse_qualified_type(qualified).unwrap();

        assert_eq!(ct.object_type, info.object_type, "content type of {}", info.name);
        assert_eq!(
            qt.object_type, info.object_type,
            "qualified type of {}",
            info.name
        );
        assert_eq!(ct.version, info.schema_version, "version of {}", info.name);
        assert_eq!(
            naming::strip_obj_prefix(info.object_type),
            info.type_key,
            "type key of {}",
            info.name
        );
    }
}

#[test]
fn one_concept_is_reachable_across_its_versions() {
    let mut reg = registry::Registry::new();
    // `Activity` is `resqml:obj_Activity` in 2.0.1 and `eml:Activity` from
    // common 2.1 on - the concept changed package as well as spelling, which is
    // exactly what `type_key` exists to bridge.
    resqml201::register(&mut reg);
    eml23::register(&mut reg);

    let activities = reg.types_by_key("Activity");
    let spellings: Vec<&str> = activities.iter().map(|i| i.object_type).collect();
    assert!(spellings.contains(&"obj_Activity"), "{spellings:?}");
    assert!(spellings.contains(&"Activity"), "{spellings:?}");

    // What a user types, in any of its spellings, finds them too.
    assert_eq!(reg.find_type_loose("obj_activity").len(), activities.len());
}

#[test]
fn a_2_0_1_content_type_still_resolves_either_way() {
    let mut reg = registry::Registry::new();
    resqml201::register(&mut reg);

    // Written by fesapi, with the prefix.
    assert!(
        reg.by_content_type("application/x-resqml+xml;version=2.0;type=obj_Activity")
            .is_some()
    );
    // Written by a tool that drops it - still readable, per the tolerant lookup.
    assert!(
        reg.by_content_type("application/x-resqml+xml; version=2.0; type=Activity")
            .is_some()
    );
}

#[test]
fn references_inside_an_object_are_found_by_reflection() {
    let mut repr = resqml22::TriangulatedSetRepresentation {
        uuid: UUID.to_string(),
        ..Default::default()
    };
    let target = resqml22::TriangulatedSetRepresentation {
        uuid: "11111111-2222-3333-4444-555555555555".to_string(),
        ..Default::default()
    };
    repr.represented_object = Some(eml23::DataObjectReference::referencing(&target).unwrap());

    let edges = identity::collect_refs(&repr);
    assert!(
        edges
            .iter()
            .any(|e| e.target.uuid == target.uuid && e.path == "RepresentedObject"),
        "{:?}",
        edges.iter().map(|e| (&e.path, e.target.uuid)).collect::<Vec<_>>()
    );
}

/// Every reference this build can make names what it points at.
///
/// # The failure this rules out
///
/// A `DataObjectReference` says *what* it points at through one of two fields -
/// `ContentType` in eml 2.0/2.1/2.2, `QualifiedType` in eml 2.3 - and one of
/// them is mandatory: with neither, the uuid is all there is, and nothing can
/// resolve it. An ETP client cannot even form the URI, which is
/// `eml:///<qualifiedType>(<uuid>)`.
///
/// `set_target_reflect` used to attempt both writes and **discard the result**,
/// so a reference type whose role table named no type field came out looking
/// built and went into the package as a dangling reference - discovered, if at
/// all, at the far end, where nothing points back at the call that made it.
///
/// The sweep is over every registered reference type rather than one of them
/// on purpose: what would break this is a *new release* whose spelling the role
/// detection does not recognise, and a test naming only the versions that exist
/// today would pass on the day that happened.
#[test]
fn every_reference_this_build_makes_names_its_target() {
    let registry = energyml_schemas::all();
    // Bound rather than inline: an `ObjectKey` **borrows** from the object it
    // was taken from, which is what makes it free to pass around.
    let target = activity_201();
    let key = identity::key_of(&target).expect("an Activity carries an identity");

    let mut checked = 0;
    for info in registry.types() {
        if !info.is_object_ref {
            continue;
        }
        let Some(make) = info.make else { continue };

        let mut reference = make();
        identity::set_target_reflect(reference.as_mut(), &key)
            .unwrap_or_else(|why| panic!("`{}` cannot be pointed at a target: {why}", info.name));

        // Not just "it did not error": read the fields back, because the
        // guarantee is about what the reference *carries*, and an assertion on
        // the return value alone would survive a write that landed nowhere.
        let roles = reference.type_info().roles;
        let mut spelled = false;
        // A plain loop: `field` borrows the reference, so a lazy chain would
        // drop the value it is reading through.
        for index in [roles.qualified_type, roles.content_type].into_iter().flatten() {
            let Some(field) = info.fields.get(index as usize) else {
                continue;
            };
            if let Some(energyml_core::value::FieldValue::Scalar(scalar)) = reference.field(field.name)
                && scalar.as_str().is_some_and(|text| !text.trim().is_empty())
            {
                spelled = true;
            }
        }
        assert!(
            spelled,
            "`{}` was built with neither a QualifiedType nor a ContentType",
            info.name
        );
        checked += 1;
    }

    // The corollary that bites: a sweep over an empty set passes.
    assert!(checked >= 4, "only {checked} reference type(s) were reached");
    eprintln!("{checked} reference type(s) name their target");
}
