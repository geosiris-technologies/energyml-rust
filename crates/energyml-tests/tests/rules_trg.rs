//! The TRG rule set, against the real corpus.
//!
//! # What this file asserts, and what it does not
//!
//! **The corpus is not authoritative.** No assertion says "this package is
//! conformant": a producer may write something non-conformant, and pinning
//! what a file contains would turn its defect into a rule the suite would
//! then defend. What is asserted is about **our rules**:
//!
//! - they apply (real objects fall under their `source`), without which a
//!   rule set that judges nothing would pass for one that judges everything
//!   and finds nothing;
//! - they are **decidable**: nothing comes back *uncheckable*, because the
//!   uncheckable here is the symptom of a path misspelt in the rule - the
//!   only defect this test can really catch;
//! - and the **negative control**: the same rule with a faulty path must
//!   return the uncheckable, or this test could not fail.
//!
//! The violations found are *displayed*, never counted in an assertion: they
//! speak about the files, not about us.

use energyml_core::registry::DynObject;
use energyml_domain::rules::{self, Outcome};
use energyml_storage::EpcStore;
use std::sync::Arc;

fn open(name: &str) -> Option<EpcStore> {
    let path = crate::fixtures::find(name)?;
    let bytes = std::fs::read(&path).ok()?;
    let registry = Arc::new(energyml_schemas::all());
    let source = Box::new(std::io::Cursor::new(bytes));
    let mut store = EpcStore::from_reader(source, path.to_string_lossy().as_ref(), registry).ok()?;
    store.set_lenient(true);
    Some(store)
}

fn findings_of(store: &EpcStore, rules: &[rules::Rule]) -> Vec<rules::Finding> {
    let resources = store.resources();
    let objects: Vec<(String, Box<dyn DynObject>)> = resources
        .iter()
        .filter_map(|summary| {
            store
                .get_object(&summary.uri)
                .ok()
                .map(|object| (summary.uuid.clone(), object))
        })
        .collect();
    let pairs = objects
        .iter()
        .map(|(uuid, object)| (uuid.as_str(), object.as_reflect()));
    rules::run(rules, pairs, store, Some(store.registry()))
}

#[test]
fn the_shipped_trg_set_parses_and_names_its_rules() {
    // A shipped set that does not parse would be a library unable to read
    // its own format; `trg()` assumes it with an `expect`, so somebody has
    // to check it.
    let rules = rules::trg();
    assert!(rules.len() >= 5, "{} rule(s) shipped", rules.len());
    for rule in &rules {
        assert!(
            rule.id.starts_with("trg/"),
            "`{}` does not say it comes from the technical reference guide",
            rule.id
        );
        assert!(!rule.says.trim().is_empty(), "`{}` says nothing", rule.id);
    }
}

#[test]
fn the_trg_set_applies_to_the_corpus_and_stays_decidable() {
    // Two packages, two releases: the same rule must judge both, which is
    // the whole point of a `source` given as a `type_key`.
    let mut judged = 0usize;
    let mut applied_rules = std::collections::BTreeSet::new();
    for package in ["testingPackageCpp.epc", "testingPackageCpp22.epc"] {
        let Some(store) = open(package) else {
            eprintln!("skipped: {package} is not in testdata");
            continue;
        };
        let rules = rules::trg();
        let findings = findings_of(&store, &rules);
        judged += 1;

        let unverifiable: Vec<_> = findings
            .iter()
            .filter(|finding| matches!(finding.outcome, Outcome::Unverifiable { .. }))
            .collect();
        assert!(
            unverifiable.is_empty(),
            "{package}: {} check(s) could not be decided — a path in our own rules is wrong: {:?}",
            unverifiable.len(),
            &unverifiable[..unverifiable.len().min(3)]
        );

        // What the rules say about the files is **displayed**, not pinned.
        for finding in &findings {
            if let Outcome::Violation { read } = &finding.outcome {
                eprintln!("{package}: {} on {} — {read}", finding.rule, finding.uuid);
            }
        }
        for rule in &rules {
            if applies_to_any(&store, rule) {
                applied_rules.insert(rule.id.clone());
            }
        }
    }
    if judged == 0 {
        eprintln!("skipped: the corpus is not present");
        return;
    }
    // A rule set that meets no object proves nothing - it is exactly the
    // test that cannot fail.
    assert!(
        applied_rules.len() >= 4,
        "only {} rule(s) met an object of the corpus: {applied_rules:?}",
        applied_rules.len()
    );
}

/// Does this package hold at least one object of the rule's `source`?
fn applies_to_any(store: &EpcStore, rule: &rules::Rule) -> bool {
    store.resources().iter().any(|summary| {
        store.get_object(&summary.uri).is_ok_and(|object| {
            let info = object.as_reflect().type_info();
            let mut chain = vec![info.type_key];
            let mut base = info.base;
            while let Some(next) = base {
                let parent = next();
                chain.push(parent.type_key);
                base = parent.base;
            }
            chain.contains(&rule.source.as_str())
        })
    })
}

#[test]
fn a_misspelled_path_would_be_caught_the_negative_control() {
    // What the test above is worth depends entirely on this: if a faulty
    // path passed *also* for decidable, the assertion "nothing uncheckable"
    // would say nothing at all. So a rule is broken on purpose to check
    // that it turns red.
    let Some(store) = open("testingPackageCpp.epc") else {
        eprintln!("skipped: the corpus is not present");
        return;
    };
    let broken = rules::parse(
        r#"[{
          "id": "trg/deliberately-misspelled",
          "source": "AbstractProperty",
          "check": { "path": "SupportingRepresentaiton", "targets": "AbstractRepresentation", "subtypes": true },
          "says": "a faulty path must show"
        }]"#,
    )
    .expect("parses");
    let findings = findings_of(&store, &broken);
    assert!(
        findings
            .iter()
            .any(|finding| matches!(finding.outcome, Outcome::Unverifiable { .. })),
        "a misspelled path went unnoticed — the decidability assertion above proves nothing"
    );
}

#[test]
fn a_property_pointing_at_an_interpretation_is_caught() {
    // The shape of the assertion comes from the **specification** (the TRG
    // types `SupportingRepresentation` by `AbstractRepresentation`), and the
    // faulty object is built here - not hunted for in a file.
    let property = energyml_resqml_2_0_1::ContinuousProperty {
        uuid: "00000000-0000-0000-0000-0000000000aa".into(),
        supporting_representation: energyml_eml_2_0::DataObjectReference {
            content_type: "application/x-resqml+xml;version=2.0;type=obj_FaultInterpretation".into(),
            title: "une interprétation".into(),
            uuid: "00000000-0000-0000-0000-0000000000bb".into(),
            ..Default::default()
        },
        ..Default::default()
    };
    let objects: Vec<(&str, &dyn energyml_core::value::Reflect)> =
        vec![("00000000-0000-0000-0000-0000000000aa", &property)];

    // The target is not in the package: what it descends from cannot be
    // known, hence *uncheckable* - never a violation pronounced blind.
    //
    struct Nothing;
    impl energyml_domain::source::ObjectSource for Nothing {
        fn object(&self, _uuid: &str) -> Option<Box<dyn DynObject>> {
            None
        }
    }
    let blind = rules::run(&rules::trg(), objects.clone().into_iter(), &Nothing, None);
    assert!(
        blind.iter().any(|finding| {
            finding.rule == "trg/property-supporting-representation"
                && matches!(finding.outcome, Outcome::Unverifiable { .. })
        }),
        "{blind:?}"
    );

    // A readable target: the verdict lands, and it names what was read.
    struct Lender;
    impl energyml_domain::source::ObjectSource for Lender {
        fn object(&self, uuid: &str) -> Option<Box<dyn DynObject>> {
            Some(Box::new(energyml_resqml_2_0_1::FaultInterpretation {
                uuid: uuid.into(),
                ..Default::default()
            }))
        }
    }
    let seen = rules::run(&rules::trg(), objects.into_iter(), &Lender, None);
    let caught = seen
        .iter()
        .find(|finding| finding.rule == "trg/property-supporting-representation")
        .expect("the rule ran");
    assert_eq!(
        caught.outcome,
        Outcome::Violation {
            read: "FaultInterpretation".into()
        }
    );

    // And a real representation passes - including by **subtype**: that is
    // what `subtypes` cost.
    struct Right;
    impl energyml_domain::source::ObjectSource for Right {
        fn object(&self, uuid: &str) -> Option<Box<dyn DynObject>> {
            Some(Box::new(energyml_resqml_2_0_1::TriangulatedSetRepresentation {
                uuid: uuid.into(),
                ..Default::default()
            }))
        }
    }
    let good = energyml_resqml_2_0_1::ContinuousProperty {
        uuid: "00000000-0000-0000-0000-0000000000cc".into(),
        supporting_representation: energyml_eml_2_0::DataObjectReference {
            content_type: "application/x-resqml+xml;version=2.0;type=obj_TriangulatedSetRepresentation"
                .into(),
            title: "une surface".into(),
            uuid: "00000000-0000-0000-0000-0000000000dd".into(),
            ..Default::default()
        },
        ..Default::default()
    };
    let objects: Vec<(&str, &dyn energyml_core::value::Reflect)> =
        vec![("00000000-0000-0000-0000-0000000000cc", &good)];
    let quiet = rules::run(&rules::trg(), objects.into_iter(), &Right, None);
    assert!(
        !quiet
            .iter()
            .any(|finding| finding.rule == "trg/property-supporting-representation"),
        "a triangulated set is a representation: {quiet:?}"
    );
}
