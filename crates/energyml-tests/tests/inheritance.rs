//! The inheritance chain, all the way up.
//!
//! # The hole this file exists to keep from coming back
//!
//! The generator wrote `base = …` on **structs** and not on **choices**, that
//! is, not on abstract types. The chain therefore stopped at the first abstract
//! ancestor: `TriangulatedSetRepresentation` reached
//! `AbstractSurfaceRepresentation` and no higher, so that
//! `is_a(AbstractRepresentation)` answered **false** for every surface in the
//! model. Silently - which is the only way it could do harm: nothing fails, a
//! business rule naming an abstract class merely covers a fraction of what it
//! announces, and a version translator reads a truncated hierarchy.
//!
//! The assertions therefore come from the **specification** (the RESQML UML
//! model: a triangulated representation *is* a representation), not from a file -
//! the house rule. What the corpus would say does not enter here.

use energyml_core::value::Reflect;

/// An object's concepts: its own, then the ones it descends from.
fn concepts(object: &dyn Reflect) -> Vec<&'static str> {
    let info = object.type_info();
    let mut out = vec![info.type_key];
    let mut base = info.base;
    while let Some(next) = base {
        let info = next();
        out.push(info.type_key);
        base = info.base;
    }
    out
}

fn assert_descends(object: &dyn Reflect, ancestor: &str) {
    let chain = concepts(object);
    assert!(
        chain.contains(&ancestor),
        "{} does not descend from {ancestor}: {chain:?}",
        object.type_info().type_key
    );
}

#[test]
fn a_concrete_representation_descends_from_the_abstract_one() {
    // Two different shapes of the same requirement: a surface goes through an
    // intermediate abstract class (`AbstractSurfaceRepresentation`), a
    // sub-representation derives directly. It was the first that broke.
    assert_descends(
        &energyml_resqml_2_0_1::TriangulatedSetRepresentation::default(),
        "AbstractRepresentation",
    );
    assert_descends(
        &energyml_resqml_2_2::TriangulatedSetRepresentation::default(),
        "AbstractRepresentation",
    );
    assert_descends(
        &energyml_resqml_2_0_1::SubRepresentation::default(),
        "AbstractRepresentation",
    );
    assert_descends(
        &energyml_resqml_2_0_1::IjkGridRepresentation::default(),
        "AbstractRepresentation",
    );
}

#[test]
fn a_concrete_property_descends_from_the_abstract_one() {
    // Two storeys of abstract above a concrete property -
    // `AbstractValuesProperty`, then `AbstractProperty` - so the chain had to
    // step over two choices to reach the top.
    for object in [
        &energyml_resqml_2_0_1::ContinuousProperty::default() as &dyn Reflect,
        &energyml_resqml_2_0_1::DiscreteProperty::default(),
        &energyml_resqml_2_0_1::CategoricalProperty::default(),
    ] {
        assert_descends(object, "AbstractValuesProperty");
        assert_descends(object, "AbstractProperty");
    }
}

#[test]
fn an_interpretation_and_a_feature_reach_their_abstract_class() {
    assert_descends(
        &energyml_resqml_2_0_1::FaultInterpretation::default(),
        "AbstractFeatureInterpretation",
    );
    assert_descends(
        &energyml_resqml_2_0_1::HorizonInterpretation::default(),
        "AbstractFeatureInterpretation",
    );
    assert_descends(
        &energyml_resqml_2_0_1::TectonicBoundaryFeature::default(),
        "AbstractFeature",
    );
    assert_descends(
        &energyml_resqml_2_0_1::OrganizationFeature::default(),
        "AbstractFeature",
    );
}

#[test]
fn every_top_level_type_reaches_the_object_class_of_its_package() {
    // The broad measure, the one that would have shouted: **every** top-level
    // object of a release descends from its package's object class - that is
    // the definition of a top-level object in the model. A type that does not
    // get there has its chain cut somewhere.
    let registry = energyml_schemas::all();
    let mut checked = 0usize;
    let mut broken = Vec::new();
    for info in registry.top_level_types() {
        // Each package names its root differently; the chain is asked to reach
        // **one** known root rather than a single spelling.
        //
        // The list also states a **deliberate limit**: a base declared in the
        // *depended-on* crate is not emitted (see `render_struct` - "only
        // reference a base that this crate actually defines"), so a chain stops
        // at its crate's edge. `AbstractCitedDataObject` (resqml) and
        // `AbstractActiveObject` / `AbstractMdGrowingObject` (witsml) are those
        // edges: their own base is `eml:AbstractObject`. What this test defends
        // is the storey below - the **local** abstract classes, which were all
        // being skipped.
        const ROOTS: &[&str] = &[
            "AbstractObject",
            "AbstractCitedDataObject",
            "AbstractDataObject",
            "AbstractContextualObject",
            "AbstractActiveObject",
            "AbstractMdGrowingObject",
        ];
        let mut chain = vec![info.type_key];
        let mut base = info.base;
        while let Some(next) = base {
            let parent = next();
            chain.push(parent.type_key);
            base = parent.base;
        }
        checked += 1;
        if !chain.iter().any(|key| ROOTS.contains(key)) {
            broken.push(format!("{}::{} → {:?}", info.package, info.name, chain));
        }
    }
    assert!(checked > 300, "only {checked} top-level types walked");
    assert!(
        broken.is_empty(),
        "{} type(s) never reach an object root: {:?}",
        broken.len(),
        &broken[..broken.len().min(5)]
    );
}

/// A part written under an ancestor's element names an ancestor that has it.
///
/// PRODML 2.3 gives `FlowTestActivity` a global element and none of its
/// subtypes (FlowTestActivity.xsd l. 185-190: *"This object is abstract; you
/// must choose one of the available types"*), so an injection test is
/// `<prodml:FlowTestActivity xsi:type="prodml:InjectionFlowTest">`. For every
/// type the generator marks so: it is a part with an identity of its own, and
/// the element it names is a **same-package ancestor's**, which has an element
/// of its own - a `root_element` naming anything else writes a root the schema
/// does not declare.
#[test]
fn a_type_written_under_an_ancestors_element_names_one_it_descends_from() {
    let registry = energyml_schemas::all();
    let mut seen = 0;
    for info in registry.types() {
        let Some(element) = info.root_element else {
            continue;
        };
        seen += 1;
        let name = info.qualified_type.unwrap_or(info.name);
        assert!(
            info.is_top_level && info.content_type.is_some(),
            "{name} is a part with an identity of its own"
        );
        let mut at = info.base.map(|base| base());
        let found = loop {
            let Some(ancestor) = at else { break None };
            if ancestor.is_top_level && ancestor.root_element.is_none() && ancestor.xml_name == element {
                break Some(ancestor);
            }
            at = ancestor.base.map(|base| base());
        };
        let found = found.unwrap_or_else(|| panic!("{name}: `{element}` is no element of an ancestor"));
        assert_eq!(found.package, info.package, "{name}");
    }
    // The one case that forced the rule must be among them - without it this
    // loop passes on an empty registry.
    let injection = registry
        .by_qualified_type("prodml23.InjectionFlowTest")
        .expect("registered")
        .info;
    assert_eq!(injection.root_element, Some("FlowTestActivity"));
    assert!(seen >= 1);
}
