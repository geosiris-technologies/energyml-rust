//! What a caller can learn about a type without a schema in hand.
//!
//! Two questions a UI, a form generator or a repair tool has to answer, and
//! the schema makes both awkward:
//!
//! 1. **What may go in this field?** For a field typed by an abstract type the
//!    document chooses a concrete one with `xsi:type`, and the list of
//!    candidates is spread across the substitution group.
//! 2. **What may this value be?** For an enumeration, the declared values -
//!    and, for the `…Ext` ones, *any other string as well*. Nothing in a value
//!    of that type says which of the two kinds it is, so a caller offering a
//!    closed list where the schema allows free text silently blocks legitimate
//!    data, and one offering free text where the schema does not lets invalid
//!    data through.
//!
//! Both are answered from the generated reflection alone. This suite pins the
//! answers, because they are what a form is built from.

use energyml_core::reflect::{EnergymlType, TypeKind};
use energyml_core::value::{OwnedValue, Reflect};
use energyml_core::xml;

/// The field of a type, by XML name.
fn field(
    info: &'static energyml_core::reflect::TypeInfo,
    name: &str,
) -> &'static energyml_core::reflect::FieldInfo {
    info.fields
        .iter()
        .find(|f| f.xml_name == name)
        .unwrap_or_else(|| panic!("`{}` has no field `{name}`", info.name))
}

#[test]
fn a_field_typed_by_an_abstract_type_lists_the_concrete_ones() {
    // `TrianglePatch.Triangles` is an `AbstractIntegerArray`: the document
    // picks one of its concrete forms with `xsi:type`, and this is the list a
    // caller has to offer.
    let info = <energyml_resqml_2_2::TrianglePatch as EnergymlType>::type_info();
    let accepted = field(info, "Triangles").accepted_types();

    let names: Vec<&str> = accepted.iter().map(|t| t.name).collect();
    assert!(names.len() > 3, "got {names:?}");
    assert!(names.contains(&"IntegerExternalArray"), "{names:?}");
    assert!(names.contains(&"IntegerConstantArray"), "{names:?}");
    // Only concrete ones: an abstract type is never a legal answer, because a
    // document cannot name it in `xsi:type`.
    for info in &accepted {
        assert!(!info.is_abstract, "{} is abstract", info.name);
        assert_ne!(info.kind, TypeKind::Choice, "{} is a choice", info.name);
    }
}

#[test]
fn a_choice_says_which_xsi_type_selects_each_of_its_types() {
    // The other half of the same question: not just *what* may go there, but
    // what the document has to write to put it there.
    let info = <energyml_resqml_2_2::AnyPointGeometry as EnergymlType>::type_info();
    assert_eq!(info.kind, TypeKind::Choice);
    assert!(info.variants.len() >= 2);
    for variant in info.variants {
        let xsi = variant
            .xsi_type
            .unwrap_or_else(|| panic!("{} has no xsi:type", variant.name));
        assert!(xsi.contains(':'), "an xsi:type carries its prefix: {xsi}");
        assert!(variant.ty.is_some(), "a choice variant names its payload type");
    }
}

#[test]
fn a_closed_enumeration_offers_its_values_and_says_they_are_all() {
    let info = <energyml_resqml_2_2::TriangulatedSetRepresentation as EnergymlType>::type_info();
    let role = field(info, "SurfaceRole");
    let values: Vec<&str> = role.ty.allowed_values().iter().map(|v| v.value).collect();

    assert_eq!(values, vec!["map", "pick"]);
    let ty = role.ty.type_info().expect("an enumeration has a type");
    assert!(
        !ty.is_open,
        "`SurfaceRole` is closed: a caller may offer exactly these two"
    );
}

#[test]
fn an_open_enumeration_offers_its_values_and_says_a_string_is_allowed_too() {
    // The `…Ext` types. The declared values are a *suggestion list*, not a
    // constraint - which is exactly what a form needs to know before it
    // decides between a dropdown and a combo box.
    let info = <energyml_eml_2_3::PropertyKind as EnergymlType>::type_info();
    let class = field(info, "QuantityClass");
    let ty = class.ty.type_info().expect("an enumeration has a type");

    assert!(ty.is_open, "`QuantityClassKindExt` is an open enumeration");
    assert!(
        class.ty.allowed_values().len() > 50,
        "the declared values are still worth offering: {}",
        class.ty.allowed_values().len()
    );
}

#[test]
fn a_value_outside_an_open_enumeration_is_accepted_and_kept_verbatim() {
    // And it really is accepted, not merely advertised: set through the
    // reflection setter, read back through reflection, and written out to XML
    // as what was set.
    let mut kind = energyml_eml_2_3::PropertyKind::default();
    kind.set_field("QuantityClass", OwnedValue::Str("torque per angstrom".into()))
        .expect("an open enumeration takes any string");

    let value = kind
        .field("QuantityClass")
        .and_then(|f| f.as_scalar())
        .and_then(|s| s.as_str().map(str::to_string))
        .expect("it reads back");
    assert_eq!(
        value, "torque per angstrom",
        "kept verbatim, not folded to a variant"
    );

    let written =
        energyml_core::registry::to_part(&kind, xml::XmlWriteOptions::default()).expect("it writes");
    assert!(
        written.contains("torque per angstrom"),
        "the string has to survive to the document: {written}"
    );
}

#[test]
fn a_value_outside_a_closed_enumeration_is_refused_and_named() {
    let mut representation = energyml_resqml_2_2::TriangulatedSetRepresentation::default();
    let error = representation
        .set_field("SurfaceRole", OwnedValue::Str("banana".into()))
        .expect_err("`SurfaceRole` is closed");
    let message = error.to_string();
    assert!(
        message.contains("banana"),
        "the refusal names the value that was refused: {message}"
    );
}

#[test]
fn the_two_kinds_of_enumeration_are_distinguishable_across_the_whole_model() {
    // The guarantee that makes the two tests above generalisable: every
    // enumeration reachable from a registered type says which kind it is, and
    // an open one always has somewhere to put the string.
    let mut registry = energyml_core::registry::Registry::new();
    energyml_eml_2_3::register(&mut registry);
    energyml_resqml_2_2::register(&mut registry);

    let mut open = 0usize;
    let mut closed = 0usize;
    let mut exceptions: Vec<&'static str> = Vec::new();
    for info in registry.types() {
        for field in info.fields {
            let Some(ty) = field.ty.type_info() else { continue };
            if ty.kind != TypeKind::Enumeration {
                continue;
            }
            assert!(
                !ty.variants.is_empty(),
                "`{}` is an enumeration with no declared value at all",
                ty.name
            );
            if ty.is_open {
                open += 1;
                if !ty.name.ends_with("Ext") {
                    // Worth collecting rather than asserting away: the `…Ext`
                    // convention is **not** the whole story. The union types
                    // that fold in the legacy units - `MassPerVolumeUomWithLegacy`
                    // and its kin - are open too, and a caller keying on the
                    // name alone would offer a closed dropdown for them.
                    // Openness is a property of the type, which is exactly why
                    // it is a flag and not a naming rule.
                    exceptions.push(ty.name);
                }
            } else {
                closed += 1;
            }
        }
    }
    assert!(open > 0 && closed > 0, "open {open}, closed {closed}");
    exceptions.sort_unstable();
    exceptions.dedup();
    for name in &exceptions {
        assert!(
            name.contains("WithLegacy"),
            "`{name}` is open and named neither `…Ext` nor `…WithLegacy`; the flag is still \
             right, but the surprise is worth knowing about"
        );
    }
    eprintln!(
        "{open} open enumeration field(s), {closed} closed; open types not named `…Ext`: {exceptions:?}"
    );
}

// ---------------------------------------------------------------------------
// An enumeration slot offers values, never a class
// ---------------------------------------------------------------------------

/// `FaultThrow.Throw` is `Vec<ThrowKindExt>`: a **repeated open enumeration**,
/// which is the shape that made the bug reachable from an ordinary edit.
///
/// `accepted_types()` used to answer with the enumeration itself, because an
/// enumeration carries a `TypeInfo` like everything else. Every consumer read
/// that as "this slot holds one object of this class": an editor offered
/// *create a `ThrowKindExt`*, wrote `{}` into the field, and the JSON reader
/// asked the registry for `resqml22.ThrowKindExt` - which is deliberately not
/// registered, since an enumeration is written as its lexical value and never
/// as an object. The report was
///
/// > unknown type `resqml22.ThrowKindExt` (not registered)
///
/// naming a type that exists and is not the problem.
#[test]
fn an_enumeration_field_accepts_no_class() {
    let info = <energyml_resqml_2_2::FaultThrow as EnergymlType>::type_info();
    let throw = field(info, "Throw");

    // The enumeration is still what the field is typed by …
    let declared = throw.ty.type_info().expect("`Throw` is typed by an enumeration");
    assert_eq!(declared.name, "ThrowKindExt");
    assert_eq!(declared.kind, TypeKind::Enumeration);

    // … and there is no class to instantiate in it.
    assert!(
        throw.accepted_types().is_empty(),
        "an enumeration is a value, not a class: got {:?}",
        throw.accepted_types().iter().map(|t| t.name).collect::<Vec<_>>()
    );

    // The offer moved rather than disappearing: the values are still there, and
    // the type still says a value outside them is legal.
    assert!(declared.is_open, "`ThrowKindExt` is an `…Ext` type");
    let values: Vec<&str> = declared
        .variants
        .iter()
        .filter(|v| !v.is_other)
        .map(|v| v.value)
        .collect();
    assert!(values.contains(&"normal"), "got {values:?}");
}

/// The other half of the same guarantee, one layer down: if an object reaches
/// an enumeration slot anyway - by another page, by a hand-written document -
/// the refusal names the **shape**, not a registry entry.

#[test]
fn an_object_in_an_enumeration_slot_is_refused_by_name() {
    let registry = energyml_schemas::all();
    let info = <energyml_resqml_2_2::FaultThrow as EnergymlType>::type_info();
    let throw = field(info, "Throw");

    // `match`, not `expect_err`: the success half carries `Box<dyn DynObject>`,
    // which is deliberately not `Debug`, so the ordinary helpers do not compile
    // over this result.
    let message = match energyml_core::json::read_field_value(&serde_json::json!({}), throw, &registry) {
        Err(refusal) => refusal.to_string(),
        Ok(_) => panic!("an enumeration slot must refuse an object"),
    };

    assert!(
        message.contains("Throw") && message.contains("ThrowKindExt"),
        "the refusal has to name the field and the enumeration: {message}"
    );
    assert!(
        message.contains("enumeration"),
        "it has to say what is wrong with the value, not report a missing type: {message}"
    );
    assert!(
        !message.contains("not registered"),
        "reporting the registry sends the reader after a crate that is not missing: {message}"
    );

    // And the value it *should* have been is accepted.
    match energyml_core::json::read_field_value(&serde_json::json!("normal"), throw, &registry) {
        Ok((value, _)) => {
            assert!(
                matches!(value, OwnedValue::Str(ref s) if s == "normal"),
                "got {value:?}"
            )
        }
        Err(why) => panic!("a declared value is legal: {why}"),
    }
}

// ---------------------------------------------------------------------------
// Reaching a type the registry does not index
// ---------------------------------------------------------------------------

/// `Registry::find_type` answers for the nested types too - which is most of
/// them, and all of the ones an editor opens.
///
/// This used to live in `energyml-cli`, so the command line could describe a
/// class the web could not: `describe('resqml22.ThrowKindExt')` came back
/// *"is not a type this build knows"* about a type the same build had just
/// listed as the one the field accepts.
#[test]
fn a_nested_type_is_reachable_by_name_and_by_qualified_type() {
    let registry = energyml_schemas::all();

    // A registered top-level object: the fast path, still working.
    let top = registry
        .find_type("resqml22.TriangulatedSetRepresentation")
        .expect("a registered type resolves");
    assert_eq!(top.name, "TriangulatedSetRepresentation");

    // A nested type, by the name a document writes.
    let patch = registry
        .find_type("resqml22.TrianglePatch")
        .expect("a nested type resolves by qualified type");
    assert_eq!(patch.name, "TrianglePatch");

    // An enumeration - registered nowhere, and the case that started this.
    let enumeration = registry
        .find_type("resqml22.ThrowKindExt")
        .expect("an enumeration resolves even though it is not registered");
    assert_eq!(enumeration.name, "ThrowKindExt");
    assert_eq!(enumeration.kind, TypeKind::Enumeration);
    assert!(
        registry.by_qualified_type("resqml22.ThrowKindExt").is_none(),
        "if this starts being registered the fallback is no longer what is under test"
    );

    // A bare name, which is what a command line passes.
    assert_eq!(
        registry.find_type("TrianglePatch").map(|info| info.name),
        Some("TrianglePatch")
    );

    // And a name nothing answers to is still `None` rather than a near miss.
    assert!(registry.find_type("resqml22.NotAType").is_none());
    assert!(registry.find_type("NotAType").is_none());
    assert!(registry.find_type("   ").is_none());
}

/// The dot is what tells the two spellings apart, and it has to: a bare name
/// matched loosely would let one release answer for another.
#[test]
fn a_qualified_name_is_not_matched_as_a_bare_one() {
    let registry = energyml_schemas::all();

    // `TrianglePatch` exists in resqml 2.0.1 and 2.2. Asked for by qualified
    // type, each must come back as itself - not as whichever the walk reached
    // first.
    for (asked, version) in [
        ("resqml22.TrianglePatch", "2.2"),
        ("resqml20.TrianglePatch", "2.0.1"),
    ] {
        let found = registry
            .find_type(asked)
            .unwrap_or_else(|| panic!("`{asked}` resolves"));
        assert_eq!(found.name, "TrianglePatch");
        assert_eq!(found.version, version, "`{asked}` came back as {}", found.version);
    }

    // A qualified type whose *release* does not exist finds nothing, even
    // though the bare name would.
    assert!(registry.find_type("resqml99.TrianglePatch").is_none());
}

/// The documentation dump covers the whole model, not just what the registry
/// constructs.
///
/// Enumerations and choices are deliberately absent from the registry - they
/// are written as lexical values or selected by `xsi:type`, never built by
/// name - so a dump that iterated the registry alone shipped 3 345 structs
/// and **not one** of the vocabularies people actually look up. The closure
/// walk is what fixes that, and this test is what keeps it fixed: each kind
/// must be present, and the eml 2.3 release must hold open enumerations,
/// because the `…Ext` pattern lives there and a dump without them tells a
/// reader every list is closed.
#[test]
fn the_docs_dump_reaches_the_types_the_registry_does_not_construct() {
    let registry = energyml_schemas::all();
    let releases = energyml_core::docs::registry_docs(&registry);

    let mut kinds = std::collections::BTreeMap::new();
    let mut open_enums_in_eml23 = 0usize;
    for release in releases.as_array().into_iter().flatten() {
        // Runtime support is not a release: `AnyXml` carries no version, and
        // grouped naively it minted a one-type release called `eml-` in every
        // index - the first thing a person testing `/docs` tripped over.
        assert!(
            release["package"].as_str().is_some_and(|p| !p.is_empty())
                && release["version"].as_str().is_some_and(|v| !v.is_empty()),
            "a release without a package or version: {release}"
        );
        for info in release["types"].as_array().into_iter().flatten() {
            *kinds
                .entry(info["kind"].as_str().unwrap_or("?").to_string())
                .or_insert(0usize) += 1;
            if release["package"] == "eml"
                && release["version"] == "2.3"
                && info["kind"] == "enumeration"
                && info["open"] == true
            {
                open_enums_in_eml23 += 1;
            }
        }
    }
    assert!(kinds.get("struct").copied().unwrap_or(0) > 1000, "{kinds:?}");
    assert!(kinds.get("enumeration").copied().unwrap_or(0) > 500, "{kinds:?}");
    assert!(kinds.get("choice").copied().unwrap_or(0) > 50, "{kinds:?}");
    assert!(
        open_enums_in_eml23 > 0,
        "eml 2.3 declares the `…Ext` vocabularies"
    );

    // And a field's offer survives the trip: an enumeration field carries its
    // values, an abstract field its concrete subtypes - the two answers a
    // form, or an AI reading this file, needs.
    let a_field_with_values = releases
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|release| release["types"].as_array().into_iter().flatten())
        .flat_map(|info| info["fields"].as_array().into_iter().flatten())
        .any(|field| !field["allowedValues"].as_array().is_none_or(Vec::is_empty));
    assert!(a_field_with_values, "no field offers its enumeration values");
}

/// **A description names the key the JSON writes.** The front end's editor, the
/// type documentation and the Python bridge described a field by
/// `FieldInfo::json_name` - the XML name - while the JSON writer keys the text
/// of a simple content `_`. So every `*Measure` and `StringMeasure` was
/// described as `Value` and written as `_`, and the editor opened on an empty
/// field (the editor's user reported that the Value part looked empty and only
/// the "name" was filled in).
///
/// Two independent sources of the same fact: the keys a written document
/// actually carries, and the names the documentation announces.
#[test]
fn every_key_the_json_writes_is_a_name_the_description_announces() {
    let measure = energyml_eml_2_3::StringMeasure {
        value: "stage".into(),
        uom: None,
    };
    let written = energyml_core::json::write_value(&measure);
    let keys: Vec<&str> = written
        .as_object()
        .expect("an object")
        .keys()
        .map(String::as_str)
        .collect();
    assert!(keys.contains(&"_"), "the text is keyed `_`: {written}");

    let info = <energyml_eml_2_3::StringMeasure as EnergymlType>::type_info();
    let described = energyml_core::docs::type_doc(info);
    let announced: Vec<&str> = described["fields"]
        .as_array()
        .expect("fields")
        .iter()
        .filter_map(|field| field["jsonName"].as_str())
        .collect();
    for key in keys.iter().filter(|key| !key.starts_with('$')) {
        assert!(
            announced.contains(key),
            "`{key}` is written but described as one of {announced:?}"
        );
    }
    assert!(
        !announced.contains(&"Value"),
        "the text is not written under `Value`: {announced:?}"
    );
}

/// A union keeps **every** enumerated member, not the first.
///
/// `UnitOfMeasureExt` is `<union memberTypes="eml:LegacyUnitOfMeasure
/// eml:UnitOfMeasure eml:EnumExtensionPattern"/>`, and a union accepts a value
/// valid against *any* of its members. The generator took the first enumerated
/// one, so the type offered the 68 legacy units and none of the 1 457 current
/// ones: `m`, `ft` and `Euc` were free text.
///
/// **Nothing could see it**, and that is why this test exists. The value was
/// still carried in `Other`, so a written document was byte-identical and the
/// round-trip campaign over 425 real parts stayed green; and no code outside the
/// generated crates matches on these variants - `units.rs` works from strings.
/// The defect broke nothing. It only made every unit unvalidatable, and made the
/// type say something untrue about the schema. Found on 07/09 by confronting a
/// BRGM package with three other validators.
#[test]
fn a_union_of_several_enumerations_keeps_them_all() {
    let info = <energyml_eml_2_3::UnitOfMeasureExt as EnergymlType>::type_info();
    let listed: Vec<&str> = info.variants.iter().map(|v| v.value).collect();

    // The metre is not a niche unit. It is the one that proves the modern list
    // is there at all, and its absence is what the defect looked like.
    for expected in ["m", "ft", "Euc", "s", "Pa"] {
        assert!(
            listed.contains(&expected),
            "`{expected}` is a UnitOfMeasure value and must be offered; the type lists {} values",
            listed.len()
        );
    }
    // And the legacy member is still there - a union adds, it does not replace.
    assert!(
        listed.contains(&"1E6 bbl"),
        "the legacy units stay: a union is the sum of its members"
    );
    assert!(
        listed.len() > 1400,
        "68 values is the legacy list alone - the sign of the 07/09 defect; found {}",
        listed.len()
    );
}

/// And what falls outside the list still has a shape to respect.
///
/// The widening member is `EnumExtensionPattern`, `<xs:pattern value=".*:.*"/>`:
/// an authority and a name around a **mandatory colon**. Dropping it made *open*
/// mean *anything*, so a `uom="unknown"` - which is in neither list and has no
/// colon - passed here while the python tool, webstudio and Xerces all refused
/// it.
#[test]
fn an_open_enumeration_still_refuses_free_text_of_the_wrong_shape() {
    use energyml_core::validate::validate_with;

    let good = energyml_eml_2_3::DoubleQuantityParameter {
        uom: energyml_eml_2_3::UnitOfMeasureExt::Other("brgm:sondage".into()),
        ..Default::default()
    };
    assert!(
        validate_with(&good, &[])
            .issues
            .iter()
            .all(|i| i.code != "pattern"),
        "a value bearing the colon is what the schema widens to"
    );

    let bad = energyml_eml_2_3::DoubleQuantityParameter {
        uom: energyml_eml_2_3::UnitOfMeasureExt::Other("unknown".into()),
        ..Default::default()
    };
    let report = validate_with(&bad, &[]);
    assert!(
        report.issues.iter().any(|i| i.code == "pattern"),
        "`unknown` is in neither list and has no colon: {:?}",
        report.issues
    );
}
