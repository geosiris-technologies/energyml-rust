//! `trg/geometry-local-crs`: a geometry names a complete local system.
//!
//! The statement is the RESQML Technical Reference Guide 2.2's, §3.1
//! `AbstractGeometry`, p. 22: *"From: AbstractGeometry.LocalCrs To:
//! LocalEngineeringCompoundCrs"* — an association the XSD leaves untyped
//! (`LocalCrs` is a bare `DataObjectReference`), so no schema validation ever
//! caught a geometry naming half a system. RESQML 2.0.1's own target,
//! `AbstractLocal3dCrs`, is `LocalDepth3dCrs` or `LocalTime3dCrs`, which the
//! RESQML 2.2 release notes (p. 3) say the compound replaced.
//!
//! The objects are built here from that statement, never taken from a file:
//! which geometries of the corpus break it is what the rule *reports*, not what
//! it is judged by. And two house decisions (ADR-0018)
//! fix two things the guide does not: the finding is a **warning**,
//! never an error, and a geometry in degrees under a `Geographic2dCrs` — EML 2.3
//! having no compound over a geographic system — is **named**, not silenced.

use energyml_core::registry::DynObject;
use energyml_core::value::Reflect;
use energyml_domain::check::{self, Options, Package, Part};
use energyml_domain::rules::{self, Outcome};
use energyml_domain::source::ObjectSource;
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_2 as resqml22;
use energyml_storage::ObjectSummary;

const RULE: &str = "trg/geometry-local-crs";
const SHAPE: &str = "00000000-0000-4000-8000-0000000000a1";
const CRS: &str = "00000000-0000-4000-8000-0000000000c1";

/// A store that holds nothing: the rule reads the type the reference
/// declares, and needs no target to judge it.
struct Nothing;

impl ObjectSource for Nothing {
    fn object(&self, _uuid: &str) -> Option<Box<dyn DynObject>> {
        None
    }
}

/// A 2.2 point set whose one patch geometry names a system of `crs_type`.
fn point_set(crs_type: &str) -> resqml22::PointSetRepresentation {
    resqml22::PointSetRepresentation {
        uuid: SHAPE.into(),
        schema_version: "2.2".into(),
        node_patch_geometry: vec![resqml22::AnyPointGeometry::PointGeometry(resqml22::PointGeometry {
            local_crs: eml23::DataObjectReference {
                uuid: CRS.into(),
                qualified_type: format!("eml23.{crs_type}"),
                ..Default::default()
            },
            ..Default::default()
        })],
        ..Default::default()
    }
}

/// What the shipped set says about one object, this rule only.
fn judged(object: &dyn Reflect) -> Vec<rules::Finding> {
    let known = energyml_schemas::all();
    let objects: Vec<(&str, &dyn Reflect)> = vec![(SHAPE, object)];
    rules::run(&rules::trg(), objects.into_iter(), &Nothing, Some(&known))
        .into_iter()
        .filter(|finding| finding.rule == RULE)
        .collect()
}

#[test]
fn the_rule_is_shipped_and_looks_inside_the_representations() {
    let shipped = rules::trg();
    let rule = shipped
        .iter()
        .find(|rule| rule.id == RULE)
        .expect("the TRG set carries the geometry rule");
    // A geometry is never a data object of its own: a rule on its concept
    // that only looked at top-level objects would judge nothing, forever.
    assert!(rule.nested, "{rule:?}");
    assert_eq!(rule.source, "AbstractGeometry");
    assert!(rule.says.contains("§3.1"), "the rule cites where it comes from: {}", rule.says);
}

/// A geometry on a `ProjectedCrs` is reported, with the path to the patch and
/// the note that says what is missing.
#[test]
fn a_geometry_on_a_projected_crs_is_reported() {
    let shape = point_set("ProjectedCrs");
    let findings = judged(&shape);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].uuid, SHAPE, "the finding is on the representation that holds the geometry");
    let Outcome::Violation { read } = &findings[0].outcome else {
        panic!("a violation, not {:?}", findings[0].outcome)
    };
    assert!(read.starts_with("NodePatchGeometry[0]: ProjectedCrs"), "{read}");
    assert!(read.contains("horizontal half"), "the note on a projected system: {read}");
}

/// The negative control: the target the guide names draws nothing. Without it
/// the test above would pass on a rule that flags every geometry.
#[test]
fn a_geometry_on_a_compound_is_not() {
    let shape = point_set("LocalEngineeringCompoundCrs");
    assert!(judged(&shape).is_empty());
}

/// RESQML 2.0.1's local systems are complete ones too: the rule must not
/// flag every 2.0.1 package for being written before the compound existed.
#[test]
fn a_2_0_1_geometry_on_a_local_depth_crs_is_not() {
    use energyml_resqml_2_0_1 as resqml20;

    let shape = resqml20::PointSetRepresentation {
        uuid: SHAPE.into(),
        node_patch: vec![resqml20::NodePatch {
            geometry: resqml20::AnyPointGeometry::PointGeometry(resqml20::PointGeometry {
                local_crs: energyml_eml_2_0::DataObjectReference {
                    content_type: "application/x-resqml+xml;version=2.0;type=obj_LocalDepth3dCrs".into(),
                    uuid: CRS.into(),
                    ..Default::default()
                },
                ..Default::default()
            }),
            ..Default::default()
        }],
        ..Default::default()
    };
    assert!(judged(&shape).is_empty());
}

/// Decision (a) of 24/09/2026: a geometry in degrees keeps its
/// `Geographic2dCrs`. Still a finding — the guide's association is not met —
/// and the finding says it is the exception, and why: CRS.xsd types
/// `LocalEngineering2dCrs.OriginProjectedCrs` as a `ProjectedCrs`, so no
/// compound can sit over a geographic system.
#[test]
fn a_geometry_in_degrees_is_the_named_exception() {
    let shape = point_set("Geographic2dCrs");
    let findings = judged(&shape);
    assert_eq!(findings.len(), 1, "{findings:?}");
    let Outcome::Violation { read } = &findings[0].outcome else {
        panic!("a violation, not {:?}", findings[0].outcome)
    };
    assert!(read.contains("Geographic2dCrs"), "{read}");
    assert!(read.contains("named exception"), "the exception is said, not silenced: {read}");
}

fn summary(uuid: &str, qualified_type: &str) -> ObjectSummary {
    ObjectSummary {
        uri: format!("eml:///{qualified_type}({uuid})"),
        uuid: uuid.to_string(),
        qualified_type: qualified_type.to_string(),
        content_type: String::new(),
        locator: String::new(),
    }
}

/// A store over exactly the objects of the package, so every reference the
/// point set holds resolves and the reference section has no error to add.
struct Held(Vec<(String, Box<dyn DynObject>)>);

impl ObjectSource for Held {
    fn object(&self, uuid: &str) -> Option<Box<dyn DynObject>> {
        self.0.iter().find(|(id, _)| id == uuid).map(|(_, object)| object.clone_box())
    }
}

/// The accounting: the finding is a **warning** of `check`, never an error.
///
/// Judged by difference — the same package with and without the rules — so
/// that the other sections (the `.rels` this fixture does not write, say)
/// cannot move the count either way. The two ways this goes wrong both show:
/// counted as an error, `errors()` stops being zero; not counted at all, the
/// warnings stop differing by one.
#[test]
fn check_counts_it_as_a_warning_not_an_error() {
    let shape = point_set("ProjectedCrs");
    let projected = eml23::ProjectedCrs {
        uuid: CRS.into(),
        schema_version: "2.3".into(),
        abstract_projected_crs: Some(eml23::AbstractProjectedCrs::ProjectedEpsgCrs(eml23::ProjectedEpsgCrs {
            epsg_code: 2154,
        })),
        ..Default::default()
    };
    let summaries = [
        summary(SHAPE, "resqml22.PointSetRepresentation"),
        summary(CRS, "eml23.ProjectedCrs"),
    ];
    let objects = [
        Part { summary: &summaries[0], object: &shape, relationships: &[] },
        Part { summary: &summaries[1], object: &projected, relationships: &[] },
    ];
    let source = Held(vec![
        (SHAPE.to_string(), Box::new(shape.clone()) as Box<dyn DynObject>),
        (CRS.to_string(), Box::new(projected.clone()) as Box<dyn DynObject>),
    ]);
    let known = energyml_schemas::all();
    let package = Package {
        objects: &objects,
        summaries: &summaries,
        unreadable: &[],
        diagnostics: &[],
        source: &source,
        known: Some(&known),
    };

    let without = check::check(&package, &Options { schema: false, rules: false, extra_rules: Vec::new() });
    let with = check::check(&package, &Options { schema: false, rules: true, extra_rules: Vec::new() });

    let reported: Vec<_> = with.rules.iter().filter(|finding| finding.rule == RULE).collect();
    assert_eq!(reported.len(), 1, "{:?}", with.rules);
    assert!(reported[0].read.is_some(), "a violation, with what was read: {:?}", reported[0]);

    assert_eq!(without.errors(), 0, "the fixture itself is sound: {:?}", without.references);
    assert_eq!(with.errors(), 0, "the rule must not make the package unsound");
    assert!(with.is_sound());
    assert_eq!(with.warnings(), without.warnings() + 1, "the rule's finding is one warning");
}
