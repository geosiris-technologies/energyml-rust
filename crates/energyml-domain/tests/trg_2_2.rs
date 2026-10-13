//! Three TRG rules that were wrong in RESQML 2.2, each held by an object that
//! breaks it and one that keeps it (findings 3 to 5 of a review
//! of the business rules).
//!
//! The objects are built from the RESQML Technical Reference Guide 2.2, never
//! taken from a file:
//!
//! - §7.7 `DiscreteProperty`, p. 230: *"From: DiscreteProperty.CategoryLookup
//!   0..1 To: ColumnBasedTable"*. The rule was written on `CategoricalProperty`,
//!   a class 2.2 no longer has, so it never applied;
//! - §8.10 `SubRepresentationPatch`, p. 247: *"From:
//!   SubRepresentationPatch.SupportingRepresentation 1 To:
//!   AbstractRepresentation"*. The rule read `SupportingRepresentation` on the
//!   sub-representation itself, where 2.2 has none: every 2.2 sub-representation
//!   came back uncheckable;
//! - §12.13 `WellboreFrameRepresentation`, p. 310, and §12.17
//!   `WellboreTrajectoryRepresentation`, p. 314: *"a WITSML v2 Wellbore can be
//!   used as the represented data object for this representation"*. The rule
//!   flagged exactly that as a violation.

use energyml_core::registry::DynObject;
use energyml_core::value::Reflect;
use energyml_domain::rules::{self, Outcome};
use energyml_domain::source::ObjectSource;
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_2 as resqml22;

const SUBJECT: &str = "00000000-0000-4000-8000-0000000000a1";
const TARGET: &str = "00000000-0000-4000-8000-0000000000b1";

/// A store that holds nothing: every rule here reads the type the reference
/// declares, and the registry says what it descends from.
struct Nothing;

impl ObjectSource for Nothing {
    fn object(&self, _uuid: &str) -> Option<Box<dyn DynObject>> {
        None
    }
}

fn aiming_at(qualified_type: &str) -> eml23::DataObjectReference {
    eml23::DataObjectReference {
        uuid: TARGET.into(),
        qualified_type: qualified_type.into(),
        title: "the target".into(),
        ..Default::default()
    }
}

/// Every finding the shipped set has on one object.
fn judged(object: &dyn Reflect) -> Vec<rules::Finding> {
    let known = energyml_schemas::all();
    let objects: Vec<(&str, &dyn Reflect)> = vec![(SUBJECT, object)];
    rules::run(&rules::trg(), objects.into_iter(), &Nothing, Some(&known))
}

fn ids(findings: &[rules::Finding]) -> Vec<&str> {
    findings.iter().map(|finding| finding.rule.as_str()).collect()
}

// --- DiscreteProperty.CategoryLookup ----------------------------------------

fn discrete(lookup: Option<&str>) -> resqml22::DiscreteProperty {
    resqml22::DiscreteProperty {
        uuid: SUBJECT.into(),
        schema_version: "2.2".into(),
        category_lookup: lookup.map(aiming_at),
        // A support of its own, so that only the lookup is in question.
        supporting_representation: eml23::DataObjectReference {
            uuid: "00000000-0000-4000-8000-0000000000c1".into(),
            qualified_type: "resqml22.TriangulatedSetRepresentation".into(),
            title: "the support".into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

#[test]
fn a_2_2_category_lookup_on_a_column_based_table_is_kept() {
    assert!(judged(&discrete(Some("eml23.ColumnBasedTable"))).is_empty());
}

#[test]
fn a_2_2_category_lookup_on_anything_else_is_reported() {
    // The case the rule never saw: it was written on `CategoricalProperty`.
    let findings = judged(&discrete(Some("resqml22.HorizonInterpretation")));
    assert_eq!(
        ids(&findings),
        ["trg/discrete-property-category-lookup"],
        "{findings:?}"
    );
    assert_eq!(
        findings[0].outcome,
        Outcome::Violation {
            read: "HorizonInterpretation".into()
        }
    );
}

#[test]
fn a_2_2_discrete_property_without_categories_is_not_judged() {
    // `0..1`: a discrete property need not have categories, and a typing rule
    // says what a reference may aim at, not that there must be one.
    assert!(judged(&discrete(None)).is_empty());
}

// --- SubRepresentationPatch.SupportingRepresentation -----------------------

fn sub_representation(support: &str) -> resqml22::SubRepresentation {
    resqml22::SubRepresentation {
        uuid: SUBJECT.into(),
        schema_version: "2.2".into(),
        sub_representation_patch: vec![resqml22::SubRepresentationPatch {
            supporting_representation: aiming_at(support),
            ..Default::default()
        }],
        ..Default::default()
    }
}

#[test]
fn a_2_2_sub_representation_on_a_representation_is_kept() {
    // Nothing at all - the uncheckable on the object is what the review found.
    let findings = judged(&sub_representation("resqml22.TriangulatedSetRepresentation"));
    assert!(findings.is_empty(), "{findings:?}");
}

#[test]
fn a_2_2_sub_representation_patch_on_an_interpretation_is_reported() {
    let findings = judged(&sub_representation("resqml22.HorizonInterpretation"));
    assert_eq!(
        ids(&findings),
        ["trg/subrepresentation-patch-support"],
        "{findings:?}"
    );
    let Outcome::Violation { read } = &findings[0].outcome else {
        panic!("a violation, not {:?}", findings[0].outcome)
    };
    assert_eq!(
        read, "SubRepresentationPatch[0]: HorizonInterpretation",
        "the path to the patch leads"
    );
}

// --- A WITSML wellbore as what a well representation represents -------------

fn frame(represented: &str) -> resqml22::WellboreFrameRepresentation {
    resqml22::WellboreFrameRepresentation {
        uuid: SUBJECT.into(),
        schema_version: "2.2".into(),
        represented_object: Some(aiming_at(represented)),
        ..Default::default()
    }
}

#[test]
fn a_wellbore_frame_may_represent_a_witsml_wellbore() {
    let findings = judged(&frame("witsml21.Wellbore"));
    assert!(findings.is_empty(), "{findings:?}");
}

#[test]
fn a_wellbore_trajectory_may_represent_a_witsml_wellbore() {
    let trajectory = resqml22::WellboreTrajectoryRepresentation {
        uuid: SUBJECT.into(),
        schema_version: "2.2".into(),
        represented_object: Some(aiming_at("witsml21.Wellbore")),
        ..Default::default()
    };
    let findings = judged(&trajectory);
    assert!(
        !findings
            .iter()
            .any(|finding| finding.rule.ends_with("-represents")),
        "{findings:?}"
    );
}

#[test]
fn a_subclass_of_the_frame_may_too() {
    // The note is on the class, and a subclass inherits its associations.
    let seismic = resqml22::SeismicWellboreFrameRepresentation {
        uuid: SUBJECT.into(),
        schema_version: "2.2".into(),
        represented_object: Some(aiming_at("witsml21.Wellbore")),
        ..Default::default()
    };
    assert!(judged(&seismic).is_empty());
}

#[test]
fn a_frame_representing_a_representation_is_reported_once() {
    // By the frame's own rule, and **not** by the general one too: two findings
    // for one reference would read as two defects.
    let findings = judged(&frame("resqml22.TriangulatedSetRepresentation"));
    assert_eq!(ids(&findings), ["trg/wellbore-frame-represents"], "{findings:?}");
}

#[test]
fn any_other_representation_may_not_represent_a_witsml_wellbore() {
    // The note is §12.13's and §12.17's, not a licence for every representation:
    // the negative control of the three tests above.
    let surface = resqml22::TriangulatedSetRepresentation {
        uuid: SUBJECT.into(),
        schema_version: "2.2".into(),
        represented_object: Some(aiming_at("witsml21.Wellbore")),
        ..Default::default()
    };
    let findings = judged(&surface);
    assert_eq!(ids(&findings), ["trg/representation-represents"], "{findings:?}");
    assert_eq!(
        findings[0].outcome,
        Outcome::Violation {
            read: "Wellbore".into()
        }
    );
}

#[test]
fn an_except_that_is_not_a_concept_name_is_refused() {
    let refused = rules::parse(
        r#"[{ "id": "x", "source": "AbstractRepresentation", "except": [3],
              "check": { "path": "RepresentedObject", "targets": "Wellbore" }, "says": "x" }]"#,
    )
    .expect_err("a number is not a concept");
    assert!(refused.contains("except"), "{refused}");
}
