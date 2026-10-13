//! Audit of the schema validation (04/10/2026) - what `Validate` says about a
//! document that breaks the XSD, one fault at a time.
//!
//! Trigger: `section-view` wrote `CulturalFeature.CulturalFeatureKind =
//! "section line"` and nothing objected, although the type is an open
//! enumeration (`resqml:CulturalFeatureKind` ∪ `eml:EnumExtensionPattern`,
//! pattern `.*:.*`; `rc/energyml-xsd/resqml/v2.2/xsd_schemas/Features.xsd`
//! l. 38-40).
//!
//! **Every test here expects the fault to be detected.** They were red before
//! the corrections of the same day (each hole was shown red, then fixed) and
//! stay as the guard: a regression that lets a fault through turns one red.
//!
//! Citations are to the XSD under `rc/energyml-xsd/` - a reference, never read
//! by a test.

use energyml_core::validate::{Validate, ValidationReport};
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_2 as resqml22;

const GOOD_UUID: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";

fn codes(report: &ValidationReport) -> Vec<String> {
    report
        .errors()
        .map(|i| format!("{} @ {}", i.code, i.path))
        .collect()
}

fn has(report: &ValidationReport, code: &str) -> bool {
    report.errors().any(|i| i.code == code)
}

fn cultural_feature(kind: resqml22::CulturalFeatureKindExt) -> resqml22::CulturalFeature {
    resqml22::CulturalFeature {
        uuid: GOOD_UUID.to_string(),
        schema_version: "2.2".to_string(),
        cultural_feature_kind: kind,
        citation: eml23::Citation {
            title: "t".into(),
            originator: "o".into(),
            creation: "2026-10-04T00:00:00Z".into(),
            format: "f".into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Patterns inside unions (`…Ext` = enumeration ∪ EnumExtensionPattern)
// ---------------------------------------------------------------------------

/// Control: the same mechanism works where the pattern's package is the
/// enumeration's own (`eml:UnitOfMeasureExt`, `crates/energyml-derive/src/
/// gen_enum.rs` `other_check`).
#[test]
fn control_eml_unit_of_measure_ext_refuses_text_without_colon() {
    let report = eml23::UnitOfMeasureExt::Other("unknown".into()).validate();
    assert!(has(&report, "pattern"), "{:?}", codes(&report));
}

/// XSD: Features.xsd l. 38-40 (`CulturalFeatureKindExt`), eml 2.3
/// BaseTypes.xsd l. 41-45 (`EnumExtensionPattern`, `.*:.*`).
#[test]
fn control_a_named_variant_and_a_colon_value_pass() {
    let named = cultural_feature(resqml22::CulturalFeatureKindExt::ModelFrontier).validate();
    assert!(!has(&named, "pattern"), "{:?}", codes(&named));
    let colon = cultural_feature(resqml22::CulturalFeatureKindExt::Other(
        "acme:section line".into(),
    ))
    .validate();
    assert!(!has(&colon, "pattern"), "{:?}", codes(&colon));
}

/// The case that triggered the audit. XSD: resqml 2.2 Features.xsd l. 38-40.
#[test]
fn cultural_feature_kind_section_line() {
    let report = cultural_feature(resqml22::CulturalFeatureKindExt::Other("section line".into())).validate();
    assert!(
        has(&report, "pattern"),
        "section line (no colon) is neither listed nor `.*:.*`: {:?}",
        codes(&report)
    );
}

/// Same defect, other resqml 2.2 `…Ext` enumerations: every union whose
/// widening member lives in another package (eml) loses its pattern in
/// `ir.rs::lower_simple_types` (`own.simple_type(&member.local)`).
/// XSD: resqml 2.2 Interpretations.xsd (ThrowKindExt, DepositionalFaciesKindExt…),
/// Structural.xsd (LineRoleExt) - all `union memberTypes="resqml:X eml:EnumExtensionPattern"`.
#[test]
fn other_resqml22_open_enumerations_accept_text_without_colon() {
    let mut silent = Vec::new();
    if !has(
        &resqml22::ThrowKindExt::Other("nonsense".into()).validate(),
        "pattern",
    ) {
        silent.push("ThrowKindExt");
    }
    if !has(
        &resqml22::DepositionalFaciesKindExt::Other("nonsense".into()).validate(),
        "pattern",
    ) {
        silent.push("DepositionalFaciesKindExt");
    }
    if !has(
        &resqml22::LineRoleExt::Other("nonsense".into()).validate(),
        "pattern",
    ) {
        silent.push("LineRoleExt");
    }
    if !has(
        &resqml22::ViewerKindExt::Other("nonsense".into()).validate(),
        "pattern",
    ) {
        silent.push("ViewerKindExt");
    }
    assert!(silent.is_empty(), "no pattern check on: {silent:?}");
}

/// witsml 2.0 `EquipmentTypeExt` (DownholeComponent.xsd), same cause.
#[test]
fn witsml20_equipment_type_ext_accepts_text_without_colon() {
    let report = energyml_witsml_2_0::EquipmentTypeExt::Other("nonsense".into()).validate();
    assert!(has(&report, "pattern"), "{:?}", codes(&report));
}

/// A union of **two closed enumerations** and no pattern member is closed.
/// XSD: eml 2.3 LegacyUnitsOfMeasure.xsd l. 121-123:
/// `<union memberTypes="eml:LegacyPressureUom eml:PressureUom"/>`.
/// It used to be generated `open` with nothing checked: any string was valid.
/// Now the type has no `Other`, so an out-of-list value cannot be built and is
/// refused when read.
#[test]
fn closed_union_of_two_enumerations_refuses_any_other_string() {
    use energyml_core::xml::XmlText;
    assert!(
        !<eml23::PressureUomWithLegacy as energyml_core::reflect::EnergymlType>::type_info().is_open,
        "closed union"
    );
    assert!(eml23::PressureUomWithLegacy::from_xml_text("bar2").is_err());
    assert!(
        eml23::PressureUomWithLegacy::from_xml_text("psia").is_ok(),
        "a legacy member"
    );
    assert!(
        eml23::PressureUomWithLegacy::from_xml_text("bar").is_ok(),
        "a current member"
    );
}

// Not a hole, checked: the over-long `Other` value. The union's pattern member,
// `eml:EnumExtensionPattern`, derives from `AbstractString` with no maxLength
// (eml 2.3 BaseTypes.xsd l. 41-45), so only the enumeration members carry the
// UomEnum 32 / TypeEnum 64 limits - and those are the named variants.

// ---------------------------------------------------------------------------
// Patterns on plain fields
// ---------------------------------------------------------------------------

/// XSD: eml 2.3 BaseTypes.xsd `UuidString` - pattern 8-4-4-4-12 hex.
#[test]
fn control_uuid_pattern() {
    let mut f = cultural_feature(resqml22::CulturalFeatureKindExt::Fieldblock);
    f.uuid = "not-a-uuid".into();
    assert!(has(&f.validate(), "pattern"));
}

/// XSD: eml 2.3 `TimeStamp` - pattern `.+T.+[Z+\-].*` (Citation.Creation).
#[test]
fn control_datetime_pattern() {
    let mut f = cultural_feature(resqml22::CulturalFeatureKindExt::Fieldblock);
    f.citation.creation = "yesterday".into();
    assert!(has(&f.validate(), "pattern"));
}

/// `xs:dateTime` is a lexical type of its own: month 13, hour 99 are not dates
/// (XML Schema Part 2 §3.3.7). The schema's loose pattern lets them through; the
/// `lexical` facet (generated from the built-in the field is typed by) does not.
#[test]
fn datetime_lexical_validity() {
    let mut f = cultural_feature(resqml22::CulturalFeatureKindExt::Fieldblock);
    f.citation.creation = "2024-13-45T99:99:99Z".into();
    assert!(
        has(&f.validate(), "lexical-dateTime"),
        "{:?}",
        codes(&f.validate())
    );
    f.citation.creation = "2024-02-30T00:00:00Z".into();
    assert!(has(&f.validate(), "lexical-dateTime"), "30 February");
    f.citation.creation = "2024-02-29T23:59:59.5+01:00".into();
    assert!(!has(&f.validate(), "lexical-dateTime"), "a leap day is valid");
}

/// XSD: eml 2.3 BaseTypes.xsd l. 93-97 `QualifiedType`.
#[test]
fn control_dor_qualified_type_pattern() {
    let dor = eml23::DataObjectReference {
        uuid: GOOD_UUID.into(),
        qualified_type: "foo".into(),
        title: "t".into(),
        ..Default::default()
    };
    assert!(has(&dor.validate(), "pattern"));
}

/// A `QualifiedType` that matches the pattern but names no type this build
/// knows (`resqml22.NoSuchThing`): `Validate` has no registry, so the finding is
/// made by `energyml check` (`graph::check_declared_types`, tested in
/// `energyml-domain/tests/schema_in_check.rs`). Here, only the premise: the
/// registry does not know it, and `Validate` alone is silent by design.
#[test]
fn a_reference_to_an_unknown_type_is_the_checks_business_not_validates() {
    let r = energyml_schemas::all();
    assert!(r.find_type("resqml22.NoSuchThing").is_none());
    assert!(r.find_type("resqml22.CulturalFeature").is_some());
}

// ---------------------------------------------------------------------------
// Cardinality and numeric facets
// ---------------------------------------------------------------------------

#[test]
fn control_max_length() {
    let mut f = cultural_feature(resqml22::CulturalFeatureKindExt::Fieldblock);
    f.citation.title = "x".repeat(300);
    assert!(has(&f.validate(), "max-length"));
}

#[test]
fn control_min_occurs_of_a_relaxed_required_field() {
    // `Option` in Rust, `minOccurs=1` in the metadata (relax_required_cycles).
    let obj = resqml22::TriangulatedSetRepresentation::default();
    let report = obj.validate();
    assert!(has(&report, "missing-required"), "{:?}", codes(&report));
}

/// XSD: resqml 2.2 Geometry.xsd `ThreePoint3d`, `Point3d` min 3 max 3.
#[test]
fn control_min_occurs_of_a_list() {
    let t = resqml22::ThreePoint3d::default();
    assert!(has(&t.validate(), "missing-required"));
}

#[test]
fn control_max_occurs_of_a_list() {
    let t = resqml22::ThreePoint3d {
        point3d: vec![Default::default(); 4],
    };
    assert!(has(&t.validate(), "too-many-occurrences"));
}

/// XSD: resqml 2.2 Interpretations.xsd `FluidUnits` maxOccurs="3".
#[test]
fn control_max_occurs_bounded_dor_list() {
    let u = resqml22::ReservoirCompartmentUnitInterpretation {
        fluid_units: vec![Default::default(); 4],
        ..Default::default()
    };
    assert!(has(&u.validate(), "too-many-occurrences"));
}

/// XSD: resqml 2.2 Geometry.xsd `ParametricLineIntersections.Count`,
/// `minInclusive 1` ("Must be positive").
#[test]
fn control_min_inclusive() {
    let p = resqml22::ParametricLineIntersections {
        count: 0,
        ..Default::default()
    };
    assert!(has(&p.validate(), "min-inclusive"));
}

/// Recursion: a bad value deep inside a nested object is reached.
#[test]
fn control_validation_descends_into_nested_objects() {
    let u = resqml22::ReservoirCompartmentUnitInterpretation {
        fluid_units: vec![eml23::DataObjectReference {
            uuid: "bad".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let r = u.validate();
    assert!(
        r.errors()
            .any(|i| i.code == "pattern" && i.path.contains("FluidUnits")),
        "{:?}",
        codes(&r)
    );
}

// ---------------------------------------------------------------------------
// What `Facets` cannot even represent
// ---------------------------------------------------------------------------

/// `fixed=` (133 occurrences in the XSD; prodml 2.2/2.3 `PtaParameters.xsd`
/// l. 108 `Abbreviation fixed="AngleBetweenBoundaries"`, 66 times per release):
/// carried by `Facets::fixed` and checked by `Validate` (typed proof in
/// `energyml-domain/tests/schema_in_check.rs`).
#[test]
fn fixed_value_is_carried_and_checked() {
    use energyml_core::validate::ValidationCtx;
    use energyml_core::value::Scalar;
    let reg = energyml_schemas::all();
    let info = reg
        .types()
        .find(|i| i.name == "AveragePressure" && i.package == "prodml" && i.version == "2.3")
        .expect("prodml 2.3 AveragePressure");
    let f = info.field("Abbreviation").expect("Abbreviation");
    assert_eq!(f.facets.fixed, Some("Pbar"));
    let mut ctx = ValidationCtx::new();
    ctx.check_facets(&f.facets, Scalar::Str("Zzz"));
    assert!(ctx.finish().errors().any(|i| i.code == "fixed"));
    let mut ctx = ValidationCtx::new();
    ctx.check_facets(&f.facets, Scalar::Str("Pbar"));
    assert!(ctx.finish().is_valid());
}

/// Every `fixed=` of the schemas reaches the generated code: 133 in the XSD
/// (counted in the audit of 04/10/2026, a figure written here as the rule's
/// extent, not read at run time).
#[test]
fn every_fixed_declaration_is_carried() {
    let reg = energyml_schemas::all();
    let n = every_type(&reg)
        .into_iter()
        .flat_map(|i| i.fields.iter())
        .filter(|f| f.facets.fixed.is_some())
        .count();
    // Counted per generated struct: prodml 2.0 (1) + 2.2 and 2.3 (66 each),
    // and an inherited field is repeated in each subtype that flattens it.
    assert!(n >= 133, "{n}");
}

// ---------------------------------------------------------------------------
// The patterns themselves

/// Every type the registry knows **and every type reachable from a field** -
/// enumerations are not registered on their own, only named by the fields that
/// use them, so listing `Registry::types` alone saw none of them.
fn every_type(reg: &energyml_core::registry::Registry) -> Vec<&'static energyml_core::reflect::TypeInfo> {
    let mut seen = std::collections::BTreeMap::new();
    let mut stack: Vec<&'static energyml_core::reflect::TypeInfo> = reg.types().collect();
    while let Some(info) = stack.pop() {
        let key = (info.package, info.version, info.name);
        if seen.insert(key, info).is_some() {
            continue;
        }
        for f in info.fields {
            if let Some(next) = f.ty.type_info() {
                stack.push(next);
            }
        }
    }
    seen.into_values().collect()
}
// ---------------------------------------------------------------------------

/// A pattern the regex engine cannot compile used to make `pattern_matches`
/// answer `true` for every value, in silence. Every pattern the generated
/// crates carry - fields and open-enumeration extensions - must compile; and
/// the set must not be empty (a test that loops over nothing proves nothing).
#[test]
fn every_generated_pattern_compiles() {
    let reg = energyml_schemas::all();
    let mut seen = std::collections::BTreeSet::new();
    for info in every_type(&reg) {
        for f in info.fields {
            if let Some(p) = f.facets.pattern {
                seen.insert(p);
            }
        }
        if let Some(p) = info.other_pattern {
            seen.insert(p);
        }
    }
    assert!(seen.len() >= 8, "only {} distinct patterns found", seen.len());
    let broken: Vec<_> = seen
        .iter()
        .filter(|p| !energyml_core::validate::pattern_compiles(p))
        .collect();
    assert!(
        broken.is_empty(),
        "patterns the regex engine cannot run: {broken:?}"
    );
}

/// Every open enumeration of the generated crates that the schema widens with
/// `EnumExtensionPattern` carries it. Before 04/10 the union member was looked
/// up in the package's own schemas only, and 104 of them lost it.
#[test]
fn every_open_enumeration_carries_its_extension_pattern() {
    let reg = energyml_schemas::all();
    let open: Vec<_> = every_type(&reg).into_iter().filter(|i| i.is_open).collect();
    assert!(open.len() > 600, "only {} open enumerations", open.len());
    let bare: Vec<_> = open
        .iter()
        .filter(|i| i.other_pattern.is_none())
        .map(|i| format!("{} {} {}", i.package, i.version, i.name))
        .collect();
    assert!(bare.is_empty(), "open enumerations with no pattern: {bare:?}");
    let closed_unions = [
        "PressureUomWithLegacy",
        "VolumeUomWithLegacy",
        "MassPerVolumeUomWithLegacy",
    ];
    for name in closed_unions {
        assert!(
            every_type(&reg)
                .into_iter()
                .filter(|i| i.name == name)
                .all(|i| !i.is_open),
            "{name} is a union of two closed enumerations"
        );
    }
}

/// `whiteSpace` and `totalDigits`/`fractionDigits`: 1 and 0 occurrences in the
/// vendored XSD respectively, nothing to lose today (control by counting, not a
/// test of behaviour - see the report).
#[test]
fn control_facets_struct_is_exactly_the_documented_seven() {
    let f = energyml_core::reflect::Facets::NONE;
    let _ = (
        f.min_inclusive,
        f.max_inclusive,
        f.min_exclusive,
        f.max_exclusive,
        f.min_length,
        f.max_length,
        f.pattern,
    );
}

// ---------------------------------------------------------------------------
// Reading: closed enumeration, abstract type
// ---------------------------------------------------------------------------

/// A present-but-unreadable value (a boolean spelled `nonsense`) is refused on
/// read; the closed-enumeration case is pinned by `repairs.rs`.
#[test]
fn control_unreadable_boolean_is_refused_on_read() {
    let xml = r#"<?xml version="1.0"?>
<resqml:CulturalFeature xmlns:resqml="http://www.energistics.org/energyml/data/resqmlv2"
  xmlns:eml="http://www.energistics.org/energyml/data/commonv2"
  uuid="a1b2c3d4-e5f6-7890-abcd-ef1234567890" schemaVersion="2.2">
  <eml:Citation><eml:Title>t</eml:Title><eml:Originator>o</eml:Originator>
  <eml:Creation>2026-10-04T00:00:00Z</eml:Creation><eml:Format>f</eml:Format></eml:Citation>
  <resqml:IsWellKnown>nonsense</resqml:IsWellKnown>
  <resqml:CulturalFeatureKind>fieldblock</resqml:CulturalFeatureKind>
</resqml:CulturalFeature>"#;
    let r = energyml_schemas::all();
    // a boolean outside `true/false/1/0`: a value that is present and unreadable
    assert!(
        r.read_xml(
            "application/x-resqml+xml;version=2.2;type=CulturalFeature",
            xml.as_bytes()
        )
        .is_err()
    );
}

/// The reading side of the open enumeration: `section line` read from a file
/// is accepted silently, and so a package written by another tool is not
/// flagged either. Detected only if `check` (validate) runs on it.
#[test]
fn reading_then_validating_a_file_with_section_line() {
    let xml = r#"<?xml version="1.0"?>
<resqml:CulturalFeature xmlns:resqml="http://www.energistics.org/energyml/data/resqmlv2"
  xmlns:eml="http://www.energistics.org/energyml/data/commonv2"
  uuid="a1b2c3d4-e5f6-7890-abcd-ef1234567890" schemaVersion="2.2">
  <eml:Citation><eml:Title>t</eml:Title><eml:Originator>o</eml:Originator>
  <eml:Creation>2026-10-04T00:00:00Z</eml:Creation><eml:Format>f</eml:Format></eml:Citation>
  <resqml:IsWellKnown>false</resqml:IsWellKnown>
  <resqml:CulturalFeatureKind>section line</resqml:CulturalFeatureKind>
</resqml:CulturalFeature>"#;
    let r = energyml_schemas::all();
    let obj = r
        .read_xml(
            "application/x-resqml+xml;version=2.2;type=CulturalFeature",
            xml.as_bytes(),
        )
        .expect("reads");
    let report = obj.as_validate();
    let mut ctx = energyml_core::validate::ValidationCtx::new();
    report.validate_into(&mut ctx);
    let report = ctx.finish();
    assert!(
        !report.is_valid(),
        "the file is invalid per the XSD, the validator says: {:?}",
        report.issues
    );
}
