//! The business-constraint engine, against real generated objects -
//! increment 1 of the constraints design (ADR-0014; BRT1, BRT2, BRT9).

use energyml_core::registry::DynObject;
use energyml_domain::rules::{self, Gravity, Outcome};
use energyml_domain::source::ObjectSource;
use energyml_resqml_2_0_1 as resqml20;

/// An object lender as a function - the shape the tests handle.
struct Lender<F: Fn(&str) -> Option<Box<dyn DynObject>>>(F);

impl<F: Fn(&str) -> Option<Box<dyn DynObject>>> ObjectSource for Lender<F> {
    fn object(&self, uuid: &str) -> Option<Box<dyn DynObject>> {
        (self.0)(uuid)
    }
}

const RULES: &str = r#"[
  {
    "id": "brt1/fault-interprets-tectonic",
    "source": "FaultInterpretation",
    "check": { "path": "InterpretedFeature", "targets": "TectonicBoundaryFeature" },
    "says": "a fault interpretation interprets a tectonic boundary"
  },
  {
    "id": "brt2/earth-model-kind",
    "source": "EarthModelInterpretation",
    "checks": [
      { "path": "InterpretedFeature", "targets": "OrganizationFeature" },
      { "path": "InterpretedFeature", "field": "OrganizationKind", "equals": "earth model" }
    ],
    "says": "an earth model interprets an organisation of kind earth model"
  }
]"#;

fn reference(content_type: &str, uuid: &str) -> energyml_eml_2_0::DataObjectReference {
    energyml_eml_2_0::DataObjectReference {
        content_type: format!("application/x-resqml+xml;version=2.0;type={content_type}"),
        title: "cible".into(),
        uuid: uuid.into(),
        ..Default::default()
    }
}

fn fault(uuid: &str, target_type: &str) -> resqml20::FaultInterpretation {
    resqml20::FaultInterpretation {
        uuid: uuid.into(),
        interpreted_feature: reference(target_type, "00000000-0000-0000-0000-00000000fea1"),
        ..Default::default()
    }
}

#[test]
fn a_reference_written_with_version_2_0_1_still_yields_a_key() {
    // The 2016 BRT1 package writes `version=2.0.1` in the ContentType -
    // the spelling the real corpus uses too - and identification must read
    // it as `2.0`.
    let reference = energyml_eml_2_0::DataObjectReference {
        content_type: "application/x-resqml+xml;version=2.0.1;type=obj_GeneticBoundaryFeature".into(),
        title: "Horizon1".into(),
        uuid: "b8fefcff-3e58-488e-95e9-67a973b7aaec".into(),
        ..Default::default()
    };
    let key = energyml_core::identity::key_of_reflect(&reference);
    assert!(key.is_some(), "no key from a 2.0.1-spelled ContentType");
    assert_eq!(key.unwrap().type_key(), "GeneticBoundaryFeature");
}

#[test]
fn brt1_a_fault_must_interpret_a_tectonic_boundary() {
    let rules = rules::parse(RULES).expect("the set parses");
    let good = fault(
        "00000000-0000-0000-0000-000000000001",
        "obj_TectonicBoundaryFeature",
    );
    let bad = fault(
        "00000000-0000-0000-0000-000000000002",
        "obj_GeneticBoundaryFeature",
    );
    let objects: Vec<(&str, &dyn energyml_core::value::Reflect)> = vec![
        ("00000000-0000-0000-0000-000000000001", &good),
        ("00000000-0000-0000-0000-000000000002", &bad),
    ];
    let findings = rules::run(&rules, objects.into_iter(), &Lender(|_| None), None);
    // The right object produces nothing; the wrong one produces a violation
    // showing what was read - the type aimed at, not a boolean.
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].uuid, "00000000-0000-0000-0000-000000000002");
    assert_eq!(
        findings[0].outcome,
        Outcome::Violation {
            read: "GeneticBoundaryFeature".into()
        }
    );
}

#[test]
fn brt2_reads_the_target_through_the_lender() {
    let rules = rules::parse(RULES).expect("the set parses");
    let model = resqml20::EarthModelInterpretation {
        uuid: "00000000-0000-0000-0000-000000000003".into(),
        interpreted_feature: reference("obj_OrganizationFeature", "00000000-0000-0000-0000-00000000fea2"),
        ..Default::default()
    };
    let objects: Vec<(&str, &dyn energyml_core::value::Reflect)> =
        vec![("00000000-0000-0000-0000-000000000003", &model)];

    // The target with the wrong kind: a violation, the value read shown.
    let wrong = rules::run(
        &rules,
        objects.clone().into_iter(),
        &Lender(|uuid| {
            assert_eq!(uuid, "00000000-0000-0000-0000-00000000fea2");
            let feature = resqml20::OrganizationFeature {
                uuid: uuid.into(),
                organization_kind: resqml20::OrganizationKind::Structural,
                ..Default::default()
            };
            Some(Box::new(feature) as Box<dyn DynObject>)
        }),
        None,
    );
    assert_eq!(wrong.len(), 1, "{wrong:?}");
    assert_eq!(
        wrong[0].outcome,
        Outcome::Violation {
            read: "structural".into()
        }
    );

    // The right target: silence.
    let right = rules::run(
        &rules,
        objects.clone().into_iter(),
        &Lender(|uuid| {
            let feature = resqml20::OrganizationFeature {
                uuid: uuid.into(),
                organization_kind: resqml20::OrganizationKind::EarthModel,
                ..Default::default()
            };
            Some(Box::new(feature) as Box<dyn DynObject>)
        }),
        None,
    );
    assert_eq!(right, vec![], "{right:?}");

    // The target outside the package: uncheckable, never a silent pass.
    let absent = rules::run(&rules, objects.into_iter(), &Lender(|_| None), None);
    assert_eq!(absent.len(), 1);
    assert!(matches!(&absent[0].outcome, Outcome::Unverifiable { why } if why.contains("fea2")));
}

#[test]
fn a_wrong_path_is_unverifiable_never_a_silent_pass() {
    let rules = rules::parse(
        r#"[{
          "id": "typo",
          "source": "FaultInterpretation",
          "check": { "path": "InterpretedFaeture", "targets": "TectonicBoundaryFeature" },
          "says": "chemin fautif"
        }]"#,
    )
    .expect("parses");
    let object = fault(
        "00000000-0000-0000-0000-000000000004",
        "obj_TectonicBoundaryFeature",
    );
    let objects: Vec<(&str, &dyn energyml_core::value::Reflect)> =
        vec![("00000000-0000-0000-0000-000000000004", &object)];
    let findings = rules::run(&rules, objects.into_iter(), &Lender(|_| None), None);
    assert_eq!(findings.len(), 1);
    assert!(
        matches!(&findings[0].outcome, Outcome::Unverifiable { .. }),
        "{findings:?}"
    );
}

#[test]
fn a_when_that_does_not_hold_is_silent() {
    let rules = rules::parse(
        r#"[{
          "id": "conditional",
          "source": "FaultInterpretation",
          "when": { "path": "Domain", "equals": "depth" },
          "check": { "path": "InterpretedFeature", "targets": "TectonicBoundaryFeature" },
          "says": "seulement en depth"
        }]"#,
    )
    .expect("parses");
    // Domain is "time": the rule is inapplicable - silent.
    let mut object = fault(
        "00000000-0000-0000-0000-000000000005",
        "obj_GeneticBoundaryFeature",
    );
    object.domain = resqml20::Domain::Time;
    let objects: Vec<(&str, &dyn energyml_core::value::Reflect)> =
        vec![("00000000-0000-0000-0000-000000000005", &object)];
    let findings = rules::run(&rules, objects.into_iter(), &Lender(|_| None), None);
    assert_eq!(findings, vec![], "{findings:?}");
}

#[test]
fn a_compare_judges_numbers_with_product_and_offset() {
    // The shape of BRT4, on numbers we hold: a count judged against
    // `reference x factors + offset`. Here on a bare `Grid2dPatch` - the
    // engine judges `Reflect`s, top-level or not.
    let rules = rules::parse(
        r#"[
          { "id": "eq", "source": "Grid2dPatch", "says": "slowest = fastest − 1",
            "check": { "compare": "SlowestAxisCount", "op": "=", "to": "FastestAxisCount", "plus": -1 } },
          { "id": "cap", "source": "Grid2dPatch", "says": "slowest ≤ fastest × fastest",
            "check": { "compare": "SlowestAxisCount", "op": "<=", "to": "FastestAxisCount", "times": ["FastestAxisCount"] } }
        ]"#,
    )
    .expect("the set parses");

    let good = resqml20::Grid2dPatch {
        slowest_axis_count: 3,
        fastest_axis_count: 4,
        ..Default::default()
    };
    let bad = resqml20::Grid2dPatch {
        slowest_axis_count: 30,
        fastest_axis_count: 5,
        ..Default::default()
    };
    let objects: Vec<(&str, &dyn energyml_core::value::Reflect)> = vec![("good", &good), ("bad", &bad)];
    let findings = rules::run(&rules, objects.into_iter(), &Lender(|_| None), None);

    // The right patch: 3 = 4 - 1 and 3 <= 16 - silence. The wrong one breaks
    // both, and each violation shows **both sides**, not a boolean.
    assert_eq!(findings.len(), 2, "{findings:?}");
    assert!(findings.iter().all(|finding| finding.uuid == "bad"));
    let eq = findings.iter().find(|f| f.rule == "eq").expect("eq violated");
    match &eq.outcome {
        Outcome::Violation { read } => {
            assert!(read.contains("30") && read.contains('4'), "{read}");
        }
        other => panic!("expected a violation, got {other:?}"),
    }
    let cap = findings.iter().find(|f| f.rule == "cap").expect("cap violated");
    assert!(
        matches!(&cap.outcome, Outcome::Violation { read } if read.contains("25")),
        "{cap:?}"
    );
}

#[test]
fn a_compare_that_cannot_read_is_unverifiable_never_a_silent_pass() {
    let rules = rules::parse(
        r#"[{ "id": "typo", "source": "Grid2dPatch", "says": "chemin fautif",
             "check": { "compare": "SlowestAxisCuont", "op": "=", "to": "FastestAxisCount" } }]"#,
    )
    .expect("parses");
    let patch = resqml20::Grid2dPatch {
        slowest_axis_count: 3,
        fastest_axis_count: 4,
        ..Default::default()
    };
    let objects: Vec<(&str, &dyn energyml_core::value::Reflect)> = vec![("p", &patch)];
    let findings = rules::run(&rules, objects.into_iter(), &Lender(|_| None), None);
    assert_eq!(findings.len(), 1);
    assert!(
        matches!(&findings[0].outcome, Outcome::Unverifiable { why } if why.contains("SlowestAxisCuont")),
        "{findings:?}"
    );
}

#[test]
fn a_when_targets_condition_gates_like_the_2016_implies() {
    // The `implies` of BRT4/BRT5: the rule only applies if the feature aimed
    // at is of the right concept - otherwise, silence.
    let rules = rules::parse(
        r#"[{
          "id": "gated", "source": "FaultInterpretation", "says": "domain depth required",
          "when": { "path": "InterpretedFeature", "targets": "TectonicBoundaryFeature" },
          "check": { "path": "Domain", "equals": "depth" }
        }]"#,
    )
    .expect("parses");
    let mut tectonic = fault(
        "00000000-0000-0000-0000-000000000006",
        "obj_TectonicBoundaryFeature",
    );
    tectonic.domain = resqml20::Domain::Time;
    let mut genetic = fault(
        "00000000-0000-0000-0000-000000000007",
        "obj_GeneticBoundaryFeature",
    );
    genetic.domain = resqml20::Domain::Time;
    let objects: Vec<(&str, &dyn energyml_core::value::Reflect)> = vec![
        ("00000000-0000-0000-0000-000000000006", &tectonic),
        ("00000000-0000-0000-0000-000000000007", &genetic),
    ];
    let findings = rules::run(&rules, objects.into_iter(), &Lender(|_| None), None);
    // The tectonic one is judged (and breaks - time), the genetic is out of scope.
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].uuid, "00000000-0000-0000-0000-000000000006");
    assert!(matches!(&findings[0].outcome, Outcome::Violation { read } if read == "time"));
}

#[test]
fn brt3_the_domain_propagates_to_the_listed_neighbours() {
    // The 2016 propagation: a structural organisation's domain holds for its
    // faults (direct references) and its horizons (**wrapped** references -
    // `Horizons[*].Horizon`). Both are written with the same `forEach`: the
    // field crosses the reference.
    let rules = rules::parse(
        r#"[{
          "id": "brt3/structural-domain-propagates",
          "source": "StructuralOrganizationInterpretation",
          "checks": [
            { "each": "Faults", "field": "Domain", "sameAs": "Domain" },
            { "each": "Horizons", "field": "Horizon.Domain", "sameAs": "Domain" }
          ],
          "says": "an organisation's domain holds for its associated interpretations"
        }]"#,
    )
    .expect("parses");

    let organisation = resqml20::StructuralOrganizationInterpretation {
        uuid: "00000000-0000-0000-0000-000000000010".into(),
        domain: resqml20::Domain::Depth,
        faults: vec![
            reference("obj_FaultInterpretation", "00000000-0000-0000-0000-00000000f001"),
            reference("obj_FaultInterpretation", "00000000-0000-0000-0000-00000000f002"),
        ],
        ..Default::default()
    };
    let objects: Vec<(&str, &dyn energyml_core::value::Reflect)> =
        vec![("00000000-0000-0000-0000-000000000010", &organisation)];

    let lender = |domain_of_f002: resqml20::Domain| {
        move |uuid: &str| {
            let domain = if uuid.ends_with("f002") {
                domain_of_f002.clone()
            } else {
                resqml20::Domain::Depth
            };
            Some(Box::new(resqml20::FaultInterpretation {
                uuid: uuid.into(),
                domain,
                ..Default::default()
            }) as Box<dyn DynObject>)
        }
    };

    // A fault in time inside an organisation in depth: a violation, and the
    // report names **the index** of the faulty neighbour and both values.
    let caught = rules::run(
        &rules,
        objects.clone().into_iter(),
        &Lender(lender(resqml20::Domain::Time)),
        None,
    );
    assert_eq!(caught.len(), 1, "{caught:?}");
    match &caught[0].outcome {
        Outcome::Violation { read } => {
            assert!(
                read.contains("Faults[1]") && read.contains("time") && read.contains("depth"),
                "{read}"
            );
            assert!(!read.contains("Faults[0]"), "{read}");
        }
        other => panic!("expected a violation, got {other:?}"),
    }

    // Everyone in depth: silence - and the empty lists (the horizons)
    // fabricate nothing, a forAll over nothing is true.
    let sane = rules::run(
        &rules,
        objects.clone().into_iter(),
        &Lender(lender(resqml20::Domain::Depth)),
        None,
    );
    assert_eq!(sane, vec![], "{sane:?}");

    // A neighbour outside the package: uncheckable, never a silent pass.
    let absent = rules::run(&rules, objects.into_iter(), &Lender(|_| None), None);
    assert_eq!(absent.len(), 1);
    assert!(
        matches!(&absent[0].outcome, Outcome::Unverifiable { why } if why.contains("Faults[0]")),
        "{absent:?}"
    );
}

/// A representation represents an interpretation **or a technical feature**.
///
/// The business rule is the 2.2 schema's own, carried in the annotation on
/// `RepresentedObject`: *"The data object represented by the representation is
/// either an interpretation or a technical feature."* Two allowed targets and
/// not one, which is why the pre-existing 2.0.1 rule
/// (`trg/representation-interprets`, one target) could not simply be respelled.
///
/// This was requested as an example of a business rule to add to the check.
#[test]
fn trg_a_representation_represents_an_interpretation_or_a_technical_feature() {
    use energyml_resqml_2_2 as resqml22;

    fn representing(uuid: &str, target_type: &str) -> resqml22::TriangulatedSetRepresentation {
        resqml22::TriangulatedSetRepresentation {
            uuid: uuid.to_string(),
            represented_object: Some(energyml_eml_2_3::DataObjectReference {
                uuid: "00000000-0000-0000-0000-0000000000ff".to_string(),
                qualified_type: format!("resqml22.{target_type}"),
                title: "the target".to_string(),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    let rules = rules::trg();
    let interpretation = representing("00000000-0000-0000-0000-000000000011", "HorizonInterpretation");
    let technical = representing("00000000-0000-0000-0000-000000000012", "SeismicLatticeFeature");
    let neither = representing(
        "00000000-0000-0000-0000-000000000013",
        "TriangulatedSetRepresentation",
    );
    let objects: Vec<(&str, &dyn energyml_core::value::Reflect)> = vec![
        ("00000000-0000-0000-0000-000000000011", &interpretation),
        ("00000000-0000-0000-0000-000000000012", &technical),
        ("00000000-0000-0000-0000-000000000013", &neither),
    ];
    // The registry is what makes `subtypes` work: a type's ancestry is static,
    // so `HorizonInterpretation` is known to descend from
    // `AbstractFeatureInterpretation` without opening anything. Without it every
    // one of the three comes back *unverifiable* — which is honest, and useless.
    let known = energyml_schemas::all();
    let findings: Vec<_> = rules::run(&rules, objects.into_iter(), &Lender(|_| None), Some(&known))
        .into_iter()
        .filter(|finding| finding.rule == "trg/representation-represents")
        .collect();

    // Only the third is judged. The **second** is the one that matters: a rule
    // written with one allowed target would have flagged it, and a package that
    // seismic-lattices its way through would come back full of violations that
    // are not violations.
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].uuid, "00000000-0000-0000-0000-000000000013");
    assert_eq!(
        findings[0].outcome,
        Outcome::Violation {
            read: "TriangulatedSetRepresentation".into()
        }
    );
}

/// The two releases disagree, so each rule speaks for one spelling only.
///
/// **The trap this exists for.** `metadata` treats `RepresentedInterpretation`
/// and `RepresentedObject` as one field on purpose — a path is written once and
/// run against every release. But here the releases do not merely respell, they
/// *disagree*: RESQML 2.2 says the target is *"either an interpretation or a
/// technical feature"*, and 2.0.1 has no such clause. Read through the synonym,
/// the 2.0.1 rule therefore judged 2.2 packages and reported as a violation
/// exactly what 2.2 added — a false alarm on a conforming file, which is worse
/// than a missing check because someone will go and "fix" the file.
///
/// `exact` is the escape hatch, and this test is what holds it: the 2.2 object
/// naming a technical feature must produce **nothing**, while the 2.0.1 object
/// doing the same must still be caught.
#[test]
fn a_release_specific_rule_does_not_judge_the_other_release() {
    let rules = rules::trg();
    let known = energyml_schemas::all();

    let new = energyml_resqml_2_2::TriangulatedSetRepresentation {
        uuid: "00000000-0000-0000-0000-000000000031".to_string(),
        represented_object: Some(energyml_eml_2_3::DataObjectReference {
            uuid: "00000000-0000-0000-0000-0000000000ff".to_string(),
            qualified_type: "resqml22.SeismicLatticeFeature".to_string(),
            title: "a technical feature".to_string(),
            ..Default::default()
        }),
        ..Default::default()
    };
    let objects: Vec<(&str, &dyn energyml_core::value::Reflect)> =
        vec![("00000000-0000-0000-0000-000000000031", &new)];
    let findings = rules::run(&rules, objects.into_iter(), &Lender(|_| None), Some(&known));
    assert!(
        findings.is_empty(),
        "2.2 allows a representation to name a technical feature: {findings:?}"
    );

    // And the 2.0.1 statement keeps its teeth: the same shape, in the release
    // that does not allow it, is still a violation.
    let old = resqml20::TriangulatedSetRepresentation {
        uuid: "00000000-0000-0000-0000-000000000032".to_string(),
        represented_interpretation: Some(reference(
            "obj_SeismicLatticeFeature",
            "00000000-0000-0000-0000-0000000000ff",
        )),
        ..Default::default()
    };
    let objects: Vec<(&str, &dyn energyml_core::value::Reflect)> =
        vec![("00000000-0000-0000-0000-000000000032", &old)];
    let findings = rules::run(&rules, objects.into_iter(), &Lender(|_| None), Some(&known));
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].rule, "trg/representation-interprets");
}

/// An interpretation of a boundary interprets a boundary feature.
///
/// The schema's own words, on `BoundaryFeatureInterpretation` in both releases:
/// *"The data-object reference (of type \"interprets\") must reference only a
/// boundary feature."* It is a tightening of the shipped
/// `trg/interpretation-interprets-feature`, which only asks for *a* feature —
/// so a horizon interpreting a wellbore passed until now.
///
/// The releases differ in shape and the rule does not have to: 2.0.1 derives
/// `GeneticBoundaryFeature`, `TectonicBoundaryFeature` and
/// `FluidBoundaryFeature` from `BoundaryFeature`, while 2.2 folded them into one
/// class with a `Kind`. `subtypes` covers the first and exact equality the
/// second, which is why one rule says it for both.
#[test]
fn trg_a_boundary_interpretation_interprets_a_boundary_feature() {
    let rules = rules::trg();
    let known = energyml_schemas::all();

    let good = fault(
        "00000000-0000-0000-0000-000000000041",
        "obj_TectonicBoundaryFeature",
    );
    let bad = fault("00000000-0000-0000-0000-000000000042", "obj_WellboreFeature");
    let objects: Vec<(&str, &dyn energyml_core::value::Reflect)> = vec![
        ("00000000-0000-0000-0000-000000000041", &good),
        ("00000000-0000-0000-0000-000000000042", &bad),
    ];
    let findings: Vec<_> = rules::run(&rules, objects.into_iter(), &Lender(|_| None), Some(&known))
        .into_iter()
        .filter(|finding| finding.rule == "trg/boundary-interpretation-interprets-boundary")
        .collect();

    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].uuid, "00000000-0000-0000-0000-000000000042");
    assert_eq!(
        findings[0].outcome,
        Outcome::Violation {
            read: "WellboreFeature".into()
        }
    );
}

/// An earth model interprets a model, and nothing else.
///
/// RESQML 2.2, on `EarthModelInterpretation`: *"BUSINESS RULE: An earth model
/// Interpretation interprets only a model feature"* (Interpretations.xsd
/// l. 632; TRG 2.2 §6.19 p. 187). The shipped
/// `trg/interpretation-interprets-feature` asks only for *a* feature, so an
/// earth model interpreting a horizon passed. RESQML 2.0.1 has no `Model`: its
/// earth model interprets an `OrganizationFeature` (whose kind `brt2` checks),
/// hence two accepted targets and one rule for both releases.
#[test]
fn trg_an_earth_model_interprets_a_model() {
    let rules = rules::trg();
    let known = energyml_schemas::all();
    let one = |uuid: &str, target: &str| energyml_resqml_2_2::EarthModelInterpretation {
        uuid: uuid.to_string(),
        interpreted_feature: energyml_eml_2_3::DataObjectReference {
            uuid: "00000000-0000-0000-0000-0000000000fe".to_string(),
            qualified_type: format!("resqml22.{target}"),
            title: "a feature".to_string(),
            ..Default::default()
        },
        ..Default::default()
    };
    let good = one("00000000-0000-0000-0000-000000000051", "Model");
    let bad = one("00000000-0000-0000-0000-000000000052", "BoundaryFeature");
    let old = resqml20::EarthModelInterpretation {
        uuid: "00000000-0000-0000-0000-000000000053".into(),
        interpreted_feature: reference("obj_OrganizationFeature", "00000000-0000-0000-0000-0000000000fd"),
        ..Default::default()
    };
    let objects: Vec<(&str, &dyn energyml_core::value::Reflect)> = vec![
        ("00000000-0000-0000-0000-000000000051", &good),
        ("00000000-0000-0000-0000-000000000052", &bad),
        ("00000000-0000-0000-0000-000000000053", &old),
    ];
    let findings: Vec<_> = rules::run(&rules, objects.into_iter(), &Lender(|_| None), Some(&known))
        .into_iter()
        .filter(|finding| finding.rule == "trg/earth-model-interprets-model")
        .collect();

    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].uuid, "00000000-0000-0000-0000-000000000052");
    assert_eq!(
        findings[0].outcome,
        Outcome::Violation {
            read: "BoundaryFeature".into()
        }
    );
}

/// A trajectory runs forward, and *"always less than"* is not *"at most"*.
///
/// RESQML 2.0.1 states it on both fields: *"BUSINESS RULE: Start MD is always
/// less than the Finish MD."* Writing that as `<=` would let the degenerate
/// trajectory through — start and finish equal, no length at all — which is
/// exactly the file someone would be hunting for. So the vocabulary gained `<`
/// rather than the rule being softened to fit it.
///
/// 2.2 says it in another place and in the same terms — the annotation on
/// `MdInterval` reads *"BUSINESS RULE: The MdMin must be less than the MdMax
/// within the MdInterval"* — so both rules are strict, each citing its own
/// release.
#[test]
fn trg_a_trajectory_starts_before_it_finishes() {
    let rules = rules::trg();
    let known = energyml_schemas::all();
    let one = |uuid: &str, start: f64, finish: f64| resqml20::WellboreTrajectoryRepresentation {
        uuid: uuid.to_string(),
        start_md: start,
        finish_md: finish,
        ..Default::default()
    };

    let forward = one("00000000-0000-0000-0000-000000000051", 100.0, 2500.0);
    let flat = one("00000000-0000-0000-0000-000000000052", 900.0, 900.0);
    let backward = one("00000000-0000-0000-0000-000000000053", 2500.0, 100.0);
    let objects: Vec<(&str, &dyn energyml_core::value::Reflect)> = vec![
        ("00000000-0000-0000-0000-000000000051", &forward),
        ("00000000-0000-0000-0000-000000000052", &flat),
        ("00000000-0000-0000-0000-000000000053", &backward),
    ];
    let findings: Vec<_> = rules::run(&rules, objects.into_iter(), &Lender(|_| None), Some(&known))
        .into_iter()
        .filter(|finding| finding.rule == "trg/trajectory-md-increases")
        .collect();

    // Two, not one: the equal pair is the case `<=` would have missed.
    let judged: Vec<&str> = findings.iter().map(|finding| finding.uuid.as_str()).collect();
    assert_eq!(
        judged,
        vec![
            "00000000-0000-0000-0000-000000000052",
            "00000000-0000-0000-0000-000000000053"
        ],
        "{findings:?}"
    );
    // The report shows both numbers, so a reader sees the disagreement rather
    // than a boolean.
    let Outcome::Violation { read } = &findings[1].outcome else {
        panic!("{:?}", findings[1].outcome)
    };
    assert!(read.contains("2500") && read.contains("100"), "{read}");
}

/// And the 2.2 spelling is judged by its own rule, not by the 2.0.1 one.
///
/// The same trap as the represented object: `StartMd` names nothing on a 2.2
/// trajectory, so without `exact` the rule would report *uncheckable* on every
/// 2.2 well in every package — noise that reads as a broken validator.
#[test]
fn a_2_2_trajectory_is_judged_on_its_md_interval_and_nothing_else() {
    let rules = rules::trg();
    let known = energyml_schemas::all();
    let one = |uuid: &str, low: f64, high: f64| energyml_resqml_2_2::WellboreTrajectoryRepresentation {
        uuid: uuid.to_string(),
        md_interval: energyml_eml_2_3::MdInterval {
            md_min: low,
            md_max: high,
            ..Default::default()
        },
        ..Default::default()
    };

    let forward = one("00000000-0000-0000-0000-000000000061", 100.0, 2500.0);
    let backward = one("00000000-0000-0000-0000-000000000062", 2500.0, 100.0);
    let objects: Vec<(&str, &dyn energyml_core::value::Reflect)> = vec![
        ("00000000-0000-0000-0000-000000000061", &forward),
        ("00000000-0000-0000-0000-000000000062", &backward),
    ];
    let findings = rules::run(&rules, objects.into_iter(), &Lender(|_| None), Some(&known));

    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].rule, "trg/trajectory-md-interval-increases");
    assert_eq!(findings[0].uuid, "00000000-0000-0000-0000-000000000062");
}

/// An anomaly of the synthetic reference model audit (ADR-0018): a contact whose `Subject` is a
/// **representation**.
///
/// RESQML TRG 2.2 §6.9 p. 173, on `ContactElement`: *"BUSINESS RULE: The
/// content type of the corresponding data-object reference must be a
/// geological feature-interpretation or a frontier feature."* A contact part is
/// never an object of its own — it sits in an organisation's
/// `ContactInterpretation` list — so the rule is `nested`, and the finding is
/// the organisation's, with the path to the part at the head of what was read.
///
/// The twin relates a horizon and a fault, which is what a contact is for; the
/// broken one puts a triangulated surface where the horizon was, and is found
/// exactly once — its `DirectObject` is right, so only one of the two checks
/// speaks.
#[test]
fn e09_a_contact_whose_subject_is_a_representation_is_found_once() {
    use energyml_resqml_2_2 as resqml22;

    fn element(target: &str, uuid: &str) -> resqml22::ContactElement {
        resqml22::ContactElement {
            uuid: uuid.to_string(),
            qualified_type: format!("resqml22.{target}"),
            title: "an element".to_string(),
            ..Default::default()
        }
    }
    fn organisation(uuid: &str, subject: &str) -> resqml22::StructuralOrganizationInterpretation {
        resqml22::StructuralOrganizationInterpretation {
            uuid: uuid.to_string(),
            contact_interpretation: vec![
                resqml22::AbstractContactInterpretationPart::BinaryContactInterpretationPart(
                    resqml22::BinaryContactInterpretationPart {
                        subject: element(subject, "00000000-0000-0000-0000-0000000009a1"),
                        direct_object: element("FaultInterpretation", "00000000-0000-0000-0000-0000000009a2"),
                        verb: resqml22::ContactVerb::Stops,
                        ..Default::default()
                    },
                ),
            ],
            ..Default::default()
        }
    }

    let rules = rules::trg();
    let known = energyml_schemas::all();
    let good = organisation("00000000-0000-0000-0000-000000000901", "HorizonInterpretation");
    let bad = organisation(
        "00000000-0000-0000-0000-000000000902",
        "TriangulatedSetRepresentation",
    );
    let objects: Vec<(&str, &dyn energyml_core::value::Reflect)> = vec![
        ("00000000-0000-0000-0000-000000000901", &good),
        ("00000000-0000-0000-0000-000000000902", &bad),
    ];
    let findings: Vec<_> = rules::run(&rules, objects.into_iter(), &Lender(|_| None), Some(&known))
        .into_iter()
        .filter(|finding| finding.rule == "trg/contact-element-is-geological")
        .collect();

    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].uuid, "00000000-0000-0000-0000-000000000902");
    let Outcome::Violation { read } = &findings[0].outcome else {
        panic!("{:?}", findings[0].outcome)
    };
    assert!(
        read.starts_with("ContactInterpretation[0]") && read.ends_with("TriangulatedSetRepresentation"),
        "{read}"
    );
}

/// The finding (about `testingPackageCpp22.epc`, written by fesapi): a contact
/// relates geological feature interpretations or a frontier feature, never a
/// representation (RESQML TRG 2.2 §6.9 p. 173: the content type of a
/// ContactElement must be a geological feature-interpretation or a frontier
/// feature); read: ContactInterpretation[4]: GenericFeatureInterpretation, in
/// StructuralOrg1 Interp1 resqml22.StructuralOrganizationInterpretation.
///
/// fesapi's contact reaches the model frontier **through** a
/// `GenericFeatureInterpretation` rather than naming the `CulturalFeature`
/// itself. The decision (option C) softens exactly that shape to a
/// named warning (`Check::Targets::through`) and nothing else: every other
/// wrong target, representation included, stays the error RESQML TRG 2.2
/// §6.9 p. 173 states.
///
/// Four objects, fabricated (never `testdata/`, per the house rule): (a) the
/// frontier reached through its interpretation — a warning, never an error;
/// (b) the same shape but the interpretation is of something else entirely (a
/// seismic line) — still an error, `through` must not blanket-accept every
/// `GenericFeatureInterpretation`; (c) the `CulturalFeature` named directly,
/// with no interpretation in between — nothing at all, and a lender that
/// panics on any call proves no resolution was even attempted; (d) a
/// representation, as before `through` existed — still an error.
#[test]
fn trg_a_contact_reached_through_a_frontier_interpretation_is_a_named_warning() {
    use energyml_resqml_2_2 as resqml22;

    fn element(target: &str, uuid: &str) -> resqml22::ContactElement {
        resqml22::ContactElement {
            uuid: uuid.to_string(),
            qualified_type: format!("resqml22.{target}"),
            title: "an element".to_string(),
            ..Default::default()
        }
    }
    fn dor(target: &str, uuid: &str) -> energyml_eml_2_3::DataObjectReference {
        energyml_eml_2_3::DataObjectReference {
            uuid: uuid.to_string(),
            qualified_type: format!("resqml22.{target}"),
            title: "the interpreted feature".to_string(),
            ..Default::default()
        }
    }
    fn organisation(
        uuid: &str,
        subject_type: &str,
        subject_uuid: &str,
    ) -> resqml22::StructuralOrganizationInterpretation {
        resqml22::StructuralOrganizationInterpretation {
            uuid: uuid.to_string(),
            contact_interpretation: vec![
                resqml22::AbstractContactInterpretationPart::BinaryContactInterpretationPart(
                    resqml22::BinaryContactInterpretationPart {
                        subject: element(subject_type, subject_uuid),
                        direct_object: element("FaultInterpretation", "00000000-0000-0000-0000-0000000009b2"),
                        verb: resqml22::ContactVerb::Stops,
                        ..Default::default()
                    },
                ),
            ],
            ..Default::default()
        }
    }
    fn only_rule(
        store: &dyn ObjectSource,
        object: &dyn energyml_core::value::Reflect,
        known: &energyml_core::registry::Registry,
    ) -> Vec<rules::Finding> {
        let objects: Vec<(&str, &dyn energyml_core::value::Reflect)> = vec![("subject-of-this-test", object)];
        rules::run(&rules::trg(), objects.into_iter(), store, Some(known))
            .into_iter()
            .filter(|finding| finding.rule == "trg/contact-element-is-geological")
            .collect()
    }

    let known = energyml_schemas::all();

    // (a) fesapi's own shape: Subject -> GenericFeatureInterpretation ->
    // InterpretedFeature -> CulturalFeature. A warning, and it carries the
    // new wording — never the rule's own `says`. Breaking it: delete the
    // `through` clause from `trg/contact-element-is-geological` in
    // `rules.rs` and this comes back an error with the old sentence.
    let generic_uuid = "00000000-0000-0000-0000-0000000009c1";
    let frontier_via_interpretation = organisation(
        "00000000-0000-0000-0000-000000000921",
        "GenericFeatureInterpretation",
        generic_uuid,
    );
    let lender = Lender(|uuid: &str| {
        assert_eq!(uuid, generic_uuid, "only the interpretation should be resolved");
        Some(Box::new(resqml22::GenericFeatureInterpretation {
            uuid: uuid.to_string(),
            interpreted_feature: dor("CulturalFeature", "00000000-0000-0000-0000-0000000009c2"),
            ..Default::default()
        }) as Box<dyn DynObject>)
    });
    let findings = only_rule(&lender, &frontier_via_interpretation, &known);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].gravity, Gravity::Warning, "{findings:?}");
    assert!(
        matches!(&findings[0].outcome, Outcome::Violation { .. }),
        "{findings:?}"
    );
    assert_ne!(
        findings[0].says,
        rules::trg()
            .into_iter()
            .find(|r| r.id == "trg/contact-element-is-geological")
            .unwrap()
            .says
    );
    assert!(
        findings[0].says.contains("through its interpretation"),
        "{}",
        findings[0].says
    );

    // (b) the same shape, but the interpretation interprets something else
    // entirely — a seismic line, not a frontier. `through` must not turn
    // every `GenericFeatureInterpretation` into a pass: still an error, with
    // the rule's own wording. Breaking it: widen `through.targets` to accept
    // anything and this stays a warning instead of reddening.
    let generic_uuid_b = "00000000-0000-0000-0000-0000000009d1";
    let via_something_else = organisation(
        "00000000-0000-0000-0000-000000000922",
        "GenericFeatureInterpretation",
        generic_uuid_b,
    );
    let lender_b = Lender(|uuid: &str| {
        assert_eq!(uuid, generic_uuid_b);
        Some(Box::new(resqml22::GenericFeatureInterpretation {
            uuid: uuid.to_string(),
            interpreted_feature: dor("SeismicLatticeFeature", "00000000-0000-0000-0000-0000000009d2"),
            ..Default::default()
        }) as Box<dyn DynObject>)
    });
    let findings = only_rule(&lender_b, &via_something_else, &known);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].gravity, Gravity::Error, "{findings:?}");
    assert!(
        matches!(&findings[0].outcome, Outcome::Violation { read } if read.contains("GenericFeatureInterpretation")),
        "{findings:?}"
    );

    // (c) the `CulturalFeature` named directly — exactly what the letter of
    // the guide asks for. Nothing at all, and the lender panics on any call:
    // a direct match must not even try to resolve anything. Breaking it:
    // drop `CulturalFeature` from the main `targets` list and this rougit as
    // an error instead of staying silent.
    let straight_to_the_frontier = organisation(
        "00000000-0000-0000-0000-000000000923",
        "CulturalFeature",
        "00000000-0000-0000-0000-0000000009e1",
    );
    let panics_if_resolved = Lender(|uuid: &str| panic!("a direct target must not be resolved: {uuid}"));
    let findings = only_rule(&panics_if_resolved, &straight_to_the_frontier, &known);
    assert_eq!(findings, vec![], "{findings:?}");

    // (d) a representation, as before `through` existed — still an error,
    // unsoftened: `through` only reaches through an *interpretation*, and a
    // representation is not one.
    let a_representation = organisation(
        "00000000-0000-0000-0000-000000000924",
        "TriangulatedSetRepresentation",
        "00000000-0000-0000-0000-0000000009f1",
    );
    let findings = only_rule(&Lender(|_| None), &a_representation, &known);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].gravity, Gravity::Error, "{findings:?}");
    assert!(
        matches!(&findings[0].outcome, Outcome::Violation { read } if read.ends_with("TriangulatedSetRepresentation")),
        "{findings:?}"
    );
}

/// A rule's weight is declared with it: `"severity"` absent is a warning,
/// `"error"` an error, and any other word a refusal by name — a typo must not
/// quietly demote an error to a warning.
#[test]
fn a_rule_declares_its_weight_and_an_unknown_one_is_refused() {
    let one = |severity: &str| {
        format!(
            r#"[{{ "id": "x/y", "source": "FaultInterpretation", {severity} "check": {{ "path": "InterpretedFeature", "targets": "TectonicBoundaryFeature" }}, "says": "s" }}]"#
        )
    };
    assert_eq!(rules::parse(&one("")).unwrap()[0].gravity, Gravity::Warning);
    assert_eq!(
        rules::parse(&one(r#""severity": "warning","#)).unwrap()[0].gravity,
        Gravity::Warning
    );
    assert_eq!(
        rules::parse(&one(r#""severity": "error","#)).unwrap()[0].gravity,
        Gravity::Error
    );
    let refused = rules::parse(&one(r#""severity": "fatal","#)).unwrap_err();
    assert!(
        refused.contains("x/y") && refused.contains("severity"),
        "{refused}"
    );
}

/// The weights decided in the review of the synthetic reference model (ADR-0018):
/// All are errors but the series asset rule, a warning.
#[test]
fn the_weights_of_27_09_are_those_the_checks_and_rules_declare() {
    let errors = [
        "house/discrete-value-in-lookup",
        "trg/one-definitive-trajectory-per-wellbore",
        "trg/is-listric-iff-normal",
        "trg/homogeneous-collection",
        "trg/homogeneous-association",
        "trg/wellbore-interpretation-set-holds-wellbores",
    ];
    for check in energyml_domain::integrity::checks() {
        let expected = if errors.contains(&check.id) {
            Gravity::Error
        } else {
            Gravity::Warning
        };
        assert_eq!(check.gravity, expected, "{}", check.id);
    }
    for id in errors.iter().chain(&["house/time-series-asset-resolves"]) {
        assert!(
            energyml_domain::integrity::checks()
                .iter()
                .any(|check| check.id == *id),
            "{id} is a check"
        );
    }
    let contact = rules::trg()
        .into_iter()
        .find(|rule| rule.id == "trg/contact-element-is-geological")
        .expect("the contact rule exists");
    assert_eq!(contact.gravity, Gravity::Error);
    assert!(
        rules::trg()
            .iter()
            .filter(|rule| rule.id != contact.id)
            .all(|rule| rule.gravity == Gravity::Warning)
    );
}
