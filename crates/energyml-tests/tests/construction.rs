//! Building an instance from its type alone, with no `impl` in sight.
//!
//! Reflection could read anything and build almost nothing.
//! `Registry::create` makes a **top-level** object from a table keyed by
//! qualified type; a nested type — a `Citation`, a `Point3dLatticeArray`, an
//! `AbstractValueArray` — is in no such table and has no qualified type to key
//! it by. So anything walking the schema in order to *construct* stopped dead
//! at the first nested field: a random-data generator, a "new object" form in
//! a UI, a translator filling a target model.
//!
//! `TypeInfo::make` closes it. Every generated struct and enumeration already
//! derives `Default`, so the constructor costs one function pointer per type
//! and no generated line changes — the `TypeInfo` literal comes from the derive
//! macro, not from the emitted source.
//!
//! What is pinned here is the contract a builder relies on:
//!
//! - a concrete type builds, nested or not;
//! - an abstract choice builds **only** when the schema names a default
//!   variant, and otherwise says so rather than guessing a subtype;
//! - what comes back is the type that was asked for, and it is writable.

use energyml_core::reflect::{EnergymlType, TypeInfo, TypeKind};
use energyml_core::registry::Registry;
use energyml_core::value::OwnedValue;

fn registry() -> Registry {
    let mut registry = Registry::new();
    energyml_eml_2_0::register(&mut registry);
    energyml_eml_2_3::register(&mut registry);
    energyml_resqml_2_0_1::register(&mut registry);
    energyml_resqml_2_2::register(&mut registry);
    registry
}

#[test]
fn a_nested_type_builds_from_its_type_info() {
    // `Citation` is the case that mattered: every object has one, it is
    // required, and it is not a top-level object — so nothing could make one.
    let info = <energyml_eml_2_0::abstract_::Citation as EnergymlType>::type_info();
    let built = info.create().expect("a citation builds");
    assert_eq!(built.type_info().name, "Citation");
    assert!(
        built.type_info().qualified_type.is_none(),
        "the point of the exercise: this type is not addressable as an object"
    );
}

#[test]
fn what_comes_back_is_writable() {
    let info = <energyml_eml_2_0::abstract_::Citation as EnergymlType>::type_info();
    let mut built = info.create().expect("it builds");
    built
        .set_field("Title", OwnedValue::Str("a title".to_string()))
        .expect("a freshly built object takes values");
    let title = built.field("Title").and_then(|v| v.as_scalar()).and_then(|s| s.as_str().map(str::to_string));
    assert_eq!(title.as_deref(), Some("a title"));
}

#[test]
fn an_abstract_choice_with_no_default_refuses_to_guess() {
    // `AbstractGeometry` has no declared type: an element of it always carries
    // an `xsi:type`. Building "an AbstractGeometry" is meaningless, and
    // returning one of its subtypes would be a silent choice made for the
    // caller.
    let info = <energyml_resqml_2_2::geometry::AbstractGeometry as EnergymlType>::type_info();
    assert_eq!(info.kind, TypeKind::Choice);
    assert!(
        info.create().is_none(),
        "an abstract choice with no default variant has no instance of its own"
    );
    // And the way forward is stated by the same reflection.
    assert!(
        !info.concrete_subtypes().is_empty(),
        "a caller that needs one picks a subtype, and the list is right here"
    );
}

#[test]
fn every_concrete_subtype_of_an_abstract_field_can_actually_be_built() {
    // The property a generator depends on: `accepted_types()` offers a list,
    // and every entry on it has to be constructible, or the offer is a lie.
    let registry = registry();
    let mut checked = 0usize;
    let mut refused: Vec<&'static str> = Vec::new();

    for info in registry.types() {
        if info.kind != TypeKind::Struct {
            continue;
        }
        for field in info.fields {
            for candidate in field.accepted_types() {
                checked += 1;
                if candidate.create().is_none() {
                    refused.push(candidate.name);
                }
            }
        }
    }
    refused.sort();
    refused.dedup();
    assert!(checked > 0, "no field offered a type");
    assert!(
        refused.is_empty(),
        "these types are offered as acceptable and cannot be built: {refused:?}"
    );
}

#[test]
fn every_registered_type_agrees_with_itself_about_what_it_builds() {
    // A constructor that returned the wrong type would be worse than none:
    // a builder would set fields by name on something else and get told the
    // field does not exist, with no clue why.
    let registry = registry();
    let mut built = 0usize;
    for info in registry.types() {
        let Some(instance) = info.create() else {
            // Only an abstract choice may refuse, and only when it has no
            // declared type. Anything else refusing is a generator bug.
            assert_eq!(
                info.kind,
                TypeKind::Choice,
                "`{}` is a {:?} and refused to build",
                info.name,
                info.kind
            );
            continue;
        };
        assert!(
            std::ptr::eq(instance.type_info(), info as &'static TypeInfo),
            "`{}` built a `{}`",
            info.name,
            instance.type_info().name
        );
        built += 1;
    }
    assert!(built > 1000, "only {built} types built; the registry is thin");
}

#[test]
fn a_top_level_object_builds_the_same_way_through_both_doors() {
    // `Registry::create` keeps working, and now agrees with the reflection.
    let registry = registry();
    let through_the_registry = registry
        .create("resqml22.TriangulatedSetRepresentation")
        .expect("the registry knows it");
    let info = through_the_registry.type_info();
    let through_the_type = info.create().expect("and so does the type");
    assert!(std::ptr::eq(through_the_type.type_info(), info));
}
