//! What `energyml check` says about a document that breaks its **schema**
//! (see `crates/energyml-tests/tests/validation_audit.rs`).
//!
//! The values are the test's own; the form comes from the XSD, cited at each
//! test. Nothing is read from `rc/`.

use energyml_core::registry::DynObject;
use energyml_core::validate::{Severity, Validate};
use energyml_domain::check::{self, Options, Package, Part};
use energyml_domain::source::NoObjects;
use energyml_eml_2_3 as eml23;
use energyml_prodml_2_3 as prodml23;
use energyml_resqml_2_2 as resqml22;
use energyml_storage::graph::RefIssueKind;
use energyml_storage::resource::ObjectSummary;

const UUID_A: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";
const UUID_B: &str = "b1b2c3d4-e5f6-7890-abcd-ef1234567891";

fn citation() -> eml23::Citation {
    eml23::Citation {
        title: "t".into(),
        originator: "o".into(),
        creation: "2026-10-04T00:00:00Z".into(),
        format: "f".into(),
        ..Default::default()
    }
}

fn summary(uuid: &str, ty: &str) -> ObjectSummary {
    let name = ty.split('.').nth(1).unwrap();
    ObjectSummary {
        uri: format!("eml:///{ty}({uuid})"),
        uuid: uuid.to_string(),
        qualified_type: ty.to_string(),
        content_type: format!("application/x-resqml+xml;version=2.2;type={name}"),
        locator: format!("{name}_{uuid}.xml"),
    }
}

/// Run the whole check on a package of the given objects.
fn judge(objects: Vec<(ObjectSummary, Box<dyn DynObject>)>) -> check::Report {
    let parts: Vec<Part<'_>> = objects
        .iter()
        .map(|(summary, object)| Part {
            summary,
            object: object.as_ref(),
            relationships: &[],
        })
        .collect();
    let summaries: Vec<ObjectSummary> = objects.iter().map(|(s, _)| s.clone()).collect();
    let registry = energyml_schemas::all();
    let package = Package {
        objects: &parts,
        summaries: &summaries,
        unreadable: &[],
        diagnostics: &[],
        source: &NoObjects,
        known: Some(&registry),
    };
    check::check(&package, &Options::everything())
}

fn feature(kind: resqml22::CulturalFeatureKindExt) -> (ObjectSummary, Box<dyn DynObject>) {
    let object = resqml22::CulturalFeature {
        uuid: UUID_A.into(),
        schema_version: "2.2".into(),
        citation: citation(),
        cultural_feature_kind: kind,
        ..Default::default()
    };
    (summary(UUID_A, "resqml22.CulturalFeature"), Box::new(object))
}

/// resqml 2.2 Features.xsd l. 38-40: `CulturalFeatureKindExt` is the union of
/// `CulturalFeatureKind` and `eml:EnumExtensionPattern` (`.*:.*`). The report
/// of `check` must carry the finding, as an **error**.
#[test]
fn check_reports_an_open_enumeration_value_without_the_colon() {
    let bad = judge(vec![feature(resqml22::CulturalFeatureKindExt::Other(
        "section line".into(),
    ))]);
    assert!(
        bad.schema.iter().any(|i| i.code == "pattern"
            && i.severity == Severity::Error
            && i.path.contains("CulturalFeatureKind")),
        "{:?}",
        bad.schema
    );
    assert!(bad.errors() >= 1);

    let ok = judge(vec![feature(resqml22::CulturalFeatureKindExt::Other(
        "energyml-rust:section line".into(),
    ))]);
    assert!(ok.schema.is_empty(), "{:?}", ok.schema);
}

/// XML Schema Part 2 §3.3.7: month 13 and hour 99 are not a dateTime, though
/// the schema's own pattern `.+T.+[Z+\-].*` (eml 2.3 BaseTypes.xsd, `TimeStamp`)
/// lets them through.
#[test]
fn check_reports_a_datetime_that_is_not_one() {
    let (s, _) = feature(resqml22::CulturalFeatureKindExt::Fieldblock);
    let mut f = resqml22::CulturalFeature {
        uuid: UUID_A.into(),
        citation: citation(),
        ..Default::default()
    };
    f.citation.creation = "2024-13-45T99:99:99Z".into();
    let report = judge(vec![(s, Box::new(f))]);
    assert!(
        report.schema.iter().any(|i| i.code == "lexical-dateTime"),
        "{:?}",
        report.schema
    );
}

/// prodml 2.3 PtaParameters.xsd l. 121: `Abbreviation` is `fixed="Pbar"` on
/// `AveragePressure`.
#[test]
fn a_fixed_value_is_enforced() {
    let mut p = prodml23::AveragePressure {
        abbreviation: "Pbar".into(),
        ..Default::default()
    };
    assert!(!p.validate().errors().any(|i| i.code == "fixed"));
    p.abbreviation = "Zzz".into();
    let r = p.validate();
    assert!(
        r.errors().any(|i| i.code == "fixed" && i.path == "Abbreviation"),
        "{:?}",
        r.issues
    );
}

/// Identifier Specification v5.0 §2.3.3: `QualifiedType` is `resqml22.Foo`. A
/// pattern-conforming name that no schema defines is reported — as a warning,
/// because the pattern accepts any two-digit version.
#[test]
fn check_reports_a_reference_to_a_type_nobody_defines() {
    let object = resqml22::GenericFeatureInterpretation {
        uuid: UUID_A.into(),
        schema_version: "2.2".into(),
        citation: citation(),
        interpreted_feature: eml23::DataObjectReference {
            uuid: UUID_B.into(),
            qualified_type: "resqml22.NoSuchThing".into(),
            title: "x".into(),
            ..Default::default()
        },
        ..Default::default()
    };
    let report = judge(vec![(
        summary(UUID_A, "resqml22.GenericFeatureInterpretation"),
        Box::new(object),
    )]);
    assert!(
        report
            .references
            .iter()
            .any(|r| r.kind == RefIssueKind::UnknownType),
        "{:?}",
        report.references
    );
    let known = judge(vec![(
        summary(UUID_A, "resqml22.GenericFeatureInterpretation"),
        Box::new(resqml22::GenericFeatureInterpretation {
            uuid: UUID_A.into(),
            citation: citation(),
            interpreted_feature: eml23::DataObjectReference {
                uuid: UUID_B.into(),
                qualified_type: "resqml22.CulturalFeature".into(),
                title: "x".into(),
                ..Default::default()
            },
            ..Default::default()
        }),
    )]);
    assert!(
        !known
            .references
            .iter()
            .any(|r| r.kind == RefIssueKind::UnknownType)
    );
}

/// eml 2.3 Abstract.xsd l. 393–399: `Citation.Format` *"Must be human and
/// machine readable and unambiguously identify the software by including the
/// organization name, software application name and software version"*. The
/// element is `minOccurs="1"` but `String2000` has no `minLength`, so the
/// schema passes an empty one: `check` warns, citing the documentation.
#[test]
fn an_empty_citation_format_is_a_warning_the_schema_alone_would_miss() {
    let mut empty = resqml22::CulturalFeature {
        uuid: UUID_A.into(),
        citation: citation(),
        ..Default::default()
    };
    empty.citation.format = "  ".into();
    let report = judge(vec![(
        summary(UUID_A, "resqml22.CulturalFeature"),
        Box::new(empty),
    )]);
    let found: Vec<_> = report
        .schema
        .iter()
        .filter(|i| i.code == "citation-format-empty")
        .collect();
    assert_eq!(found.len(), 1, "{:?}", report.schema);
    assert_eq!(found[0].severity, Severity::Warning);
    assert_eq!(found[0].path, "Citation.Format");
    assert_eq!(
        report.errors(),
        0,
        "the schema admits it: a warning, not an error"
    );

    let named = resqml22::CulturalFeature {
        uuid: UUID_A.into(),
        citation: citation(),
        ..Default::default()
    };
    let report = judge(vec![(
        summary(UUID_A, "resqml22.CulturalFeature"),
        Box::new(named),
    )]);
    assert!(
        !report.schema.iter().any(|i| i.code == "citation-format-empty"),
        "{:?}",
        report.schema
    );
}
