//! The four BRT rules of the Energistics 2016 delivery, **shipped** in
//! `rules::trg()` (they were only a test constant until 04/10/2026), judged on
//! objects built here: one that breaks the rule, one that does not.
//!
//! The form comes from the schema texts and guides cited in each rule's `says`;
//! the values are the test's own. Nothing is read from a file. Each test was
//! run red once by emptying the shipped rule it guards.

use energyml_core::registry::DynObject;
use energyml_core::value::Reflect;
use energyml_domain::rules::{self, Finding, Outcome};
use energyml_domain::source::ObjectSource;
use energyml_eml_2_0 as eml20;
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_0_1 as resqml20;
use energyml_resqml_2_2 as resqml22;

struct Lender(Vec<(String, Box<dyn DynObject>)>);

impl ObjectSource for Lender {
    fn object(&self, uuid: &str) -> Option<Box<dyn DynObject>> {
        self.0
            .iter()
            .find(|(held, _)| held == uuid)
            .map(|(_, one)| one.clone_box())
    }
}

fn judge(objects: &[(&str, &dyn Reflect)], source: &Lender) -> Vec<Finding> {
    let shipped = rules::trg();
    rules::run(&shipped, objects.iter().copied(), source, None)
}

fn of<'a>(findings: &'a [Finding], prefix: &str) -> Vec<&'a Finding> {
    findings.iter().filter(|f| f.rule.starts_with(prefix)).collect()
}

fn dor20(content_type: &str, uuid: &str) -> eml20::DataObjectReference {
    eml20::DataObjectReference {
        content_type: format!("application/x-resqml+xml;version=2.0;type={content_type}"),
        title: "t".into(),
        uuid: uuid.into(),
        ..Default::default()
    }
}

fn dor23(qualified: &str, uuid: &str) -> eml23::DataObjectReference {
    eml23::DataObjectReference {
        uuid: uuid.into(),
        qualified_type: qualified.into(),
        title: "t".into(),
        ..Default::default()
    }
}

// --- BRT1 ------------------------------------------------------------------

fn fault(uuid: &str, target: &str) -> resqml20::FaultInterpretation {
    resqml20::FaultInterpretation {
        uuid: uuid.into(),
        interpreted_feature: dor20(target, "00000000-0000-0000-0000-00000000fea1"),
        ..Default::default()
    }
}

#[test]
fn brt1_is_shipped_and_speaks_for_2_0_1_only() {
    let bad = fault(
        "00000000-0000-0000-0000-000000000002",
        "obj_GeneticBoundaryFeature",
    );
    let good = fault(
        "00000000-0000-0000-0000-000000000001",
        "obj_TectonicBoundaryFeature",
    );
    let found = judge(&[("bad", &bad), ("good", &good)], &Lender(Vec::new()));
    let brt1 = of(&found, "trg/brt1-");
    assert_eq!(brt1.len(), 1, "{found:?}");
    assert_eq!(brt1[0].uuid, "bad");
    assert_eq!(
        brt1[0].outcome,
        Outcome::Violation {
            read: "GeneticBoundaryFeature".into()
        }
    );

    // A 2.2 fault aims at a `BoundaryFeature`; BRT1 has nothing to say about it.
    let fault22 = resqml22::FaultInterpretation {
        uuid: "00000000-0000-0000-0000-000000000003".into(),
        interpreted_feature: dor23("resqml22.BoundaryFeature", "00000000-0000-0000-0000-00000000fea1"),
        ..Default::default()
    };
    let found = judge(&[("f22", &fault22)], &Lender(Vec::new()));
    assert!(of(&found, "trg/brt1-").is_empty(), "{found:?}");
}

// --- BRT2 ------------------------------------------------------------------

#[test]
fn brt2_an_earth_model_interprets_an_organization_of_kind_earth_model() {
    let model = resqml20::EarthModelInterpretation {
        uuid: "00000000-0000-0000-0000-000000000003".into(),
        interpreted_feature: dor20("obj_OrganizationFeature", "00000000-0000-0000-0000-00000000fea2"),
        ..Default::default()
    };
    let lender = |kind| {
        Lender(vec![(
            "00000000-0000-0000-0000-00000000fea2".to_string(),
            Box::new(resqml20::OrganizationFeature {
                uuid: "00000000-0000-0000-0000-00000000fea2".into(),
                organization_kind: kind,
                ..Default::default()
            }) as Box<dyn DynObject>,
        )])
    };
    let wrong = judge(&[("m", &model)], &lender(resqml20::OrganizationKind::Structural));
    let wrong = of(&wrong, "trg/brt2-");
    assert_eq!(wrong.len(), 1);
    assert_eq!(
        wrong[0].outcome,
        Outcome::Violation {
            read: "structural".into()
        }
    );
    assert!(
        judge(&[("m", &model)], &lender(resqml20::OrganizationKind::EarthModel))
            .iter()
            .all(|f| !f.rule.starts_with("trg/brt2-"))
    );
}

// --- BRT4, BRT5 in 2.0.1 -----------------------------------------------------

const GRID: &str = "00000000-0000-0000-0000-0000000000a1";
const INTERPRETATION: &str = "00000000-0000-0000-0000-0000000000a2";
const LATTICE: &str = "00000000-0000-0000-0000-0000000000a3";

fn spacing20(count: u64) -> Option<resqml20::AbstractDoubleArray> {
    Some(resqml20::AbstractDoubleArray::DoubleConstantArray(
        resqml20::DoubleConstantArray { value: 1.0, count },
    ))
}

fn grid20(
    slowest: u64,
    fastest: u64,
    slow_spacing: u64,
    fast_spacing: u64,
) -> resqml20::Grid2dRepresentation {
    let lattice = resqml20::Point3dLatticeArray {
        offset: vec![
            resqml20::Point3dOffset {
                spacing: spacing20(slow_spacing),
                ..Default::default()
            },
            resqml20::Point3dOffset {
                spacing: spacing20(fast_spacing),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    resqml20::Grid2dRepresentation {
        uuid: GRID.into(),
        represented_interpretation: Some(dor20("obj_GenericFeatureInterpretation", INTERPRETATION)),
        grid2d_patch: resqml20::Grid2dPatch {
            slowest_axis_count: slowest,
            fastest_axis_count: fastest,
            geometry: resqml20::AnyPointGeometry::PointGeometry(resqml20::PointGeometry {
                points: Some(resqml20::AbstractPoint3dArray::Point3dLatticeArray(lattice)),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn seismic_lender(inlines: u64, crosslines: u64) -> Lender {
    Lender(vec![
        (
            INTERPRETATION.to_string(),
            Box::new(resqml20::GenericFeatureInterpretation {
                uuid: INTERPRETATION.into(),
                interpreted_feature: dor20("obj_SeismicLatticeFeature", LATTICE),
                ..Default::default()
            }) as Box<dyn DynObject>,
        ),
        (
            LATTICE.to_string(),
            Box::new(resqml20::SeismicLatticeFeature {
                uuid: LATTICE.into(),
                inline_count: inlines,
                crossline_count: crosslines,
                ..Default::default()
            }) as Box<dyn DynObject>,
        ),
    ])
}

/// RESQML 2.0.1 Geometry.xsd l. 22: a lattice of N points has N-1 spacings.
#[test]
fn brt4_the_spacings_of_a_seismic_grid_count_one_fewer_than_its_axes() {
    let good = grid20(3, 4, 2, 3);
    let found = judge(&[(GRID, &good)], &seismic_lender(4, 3));
    assert!(of(&found, "trg/brt4-").is_empty(), "{found:?}");

    let bad = grid20(3, 4, 2, 5);
    let found = judge(&[(GRID, &bad)], &seismic_lender(4, 3));
    let brt4 = of(&found, "trg/brt4-");
    assert_eq!(brt4.len(), 1, "{found:?}");
    assert!(
        matches!(&brt4[0].outcome, Outcome::Violation { read } if read.contains('5') && read.contains('3')),
        "{brt4:?}"
    );
}

/// The 2016 BRT5: the grid does not exceed the lattice.
#[test]
fn brt5_a_seismic_grid_fits_inside_its_lattice() {
    let good = grid20(3, 4, 2, 3);
    assert!(of(&judge(&[(GRID, &good)], &seismic_lender(4, 3)), "trg/brt5-").is_empty());
    // The same grid on a lattice of 4 inlines and only 2 crosslines.
    let found = judge(&[(GRID, &good)], &seismic_lender(4, 2));
    let brt5 = of(&found, "trg/brt5-");
    assert_eq!(brt5.len(), 1, "{found:?}");
    assert!(matches!(&brt5[0].outcome, Outcome::Violation { .. }));
}

/// A grid that does not represent a seismic lattice is not judged by either.
#[test]
fn brt4_and_brt5_are_silent_on_a_grid_that_is_not_seismic() {
    let grid = grid20(3, 4, 99, 99);
    let lender = Lender(vec![(
        INTERPRETATION.to_string(),
        Box::new(resqml20::GenericFeatureInterpretation {
            uuid: INTERPRETATION.into(),
            interpreted_feature: dor20("obj_GeneticBoundaryFeature", LATTICE),
            ..Default::default()
        }) as Box<dyn DynObject>,
    )]);
    let found = judge(&[(GRID, &grid)], &lender);
    assert!(
        of(&found, "trg/brt4-").is_empty() && of(&found, "trg/brt5-").is_empty(),
        "{found:?}"
    );
}

// --- BRT4, BRT5 in 2.2 -------------------------------------------------------

fn spacing22(count: i64) -> Option<eml23::AbstractFloatingPointArray> {
    Some(eml23::AbstractFloatingPointArray::FloatingPointConstantArray(
        eml23::FloatingPointConstantArray {
            value: 1.0,
            count,
            ..Default::default()
        },
    ))
}

fn grid22(
    slowest: i64,
    fastest: i64,
    slow_spacing: i64,
    fast_spacing: i64,
) -> resqml22::Grid2dRepresentation {
    let lattice = resqml22::Point3dLatticeArray {
        dimension: vec![
            resqml22::Point3dLatticeDimension {
                spacing: spacing22(slow_spacing),
                ..Default::default()
            },
            resqml22::Point3dLatticeDimension {
                spacing: spacing22(fast_spacing),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    resqml22::Grid2dRepresentation {
        uuid: GRID.into(),
        represented_object: Some(dor23("resqml22.SeismicLatticeFeature", LATTICE)),
        slowest_axis_count: slowest,
        fastest_axis_count: fastest,
        geometry: resqml22::AnyPointGeometry::PointGeometry(resqml22::PointGeometry {
            points: Some(resqml22::AbstractPoint3dArray::Point3dLatticeArray(lattice)),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn labels(count: i64) -> Option<eml23::IntegerLatticeArray> {
    Some(eml23::IntegerLatticeArray {
        start_value: 1,
        offset: vec![eml23::IntegerConstantArray {
            value: 1,
            count,
            ..Default::default()
        }],
        ..Default::default()
    })
}

fn lattice_lender(inline_labels: i64, crossline_labels: i64) -> Lender {
    Lender(vec![(
        LATTICE.to_string(),
        Box::new(resqml22::SeismicLatticeFeature {
            uuid: LATTICE.into(),
            inline_labels: labels(inline_labels),
            crossline_labels: labels(crossline_labels),
            ..Default::default()
        }) as Box<dyn DynObject>,
    )])
}

/// RESQML 2.2 Geometry.xsd l. 415 and TRG 2.2 §3.17 p. 40.
#[test]
fn brt4_in_2_2_the_slowest_axis_comes_first() {
    let good = grid22(3, 4, 2, 3);
    let found = judge(&[(GRID, &good)], &lattice_lender(3, 2));
    assert!(of(&found, "trg/brt4-").is_empty(), "{found:?}");
    // The two spacings written the other way round: both axes are wrong.
    let swapped = grid22(3, 4, 3, 2);
    let found = judge(&[(GRID, &swapped)], &lattice_lender(3, 2));
    assert_eq!(of(&found, "trg/brt4-").len(), 2, "{found:?}");
}

/// RESQML 2.2 Seismic.xsd l. 244 and l. 251, and common TRG 2.3 §6.42 p. 128
/// (`Count` is n-1 for n values).
#[test]
fn brt5_in_2_2_the_labels_are_as_many_as_the_axes_nodes() {
    let good = grid22(3, 4, 2, 3);
    // 4 inlines on the fastest axis: a label array of Count 3; 3 crosslines: Count 2.
    let found = judge(&[(GRID, &good)], &lattice_lender(3, 2));
    assert!(of(&found, "trg/brt5-").is_empty(), "{found:?}");
    let found = judge(&[(GRID, &good)], &lattice_lender(7, 2));
    let brt5 = of(&found, "trg/brt5-");
    assert_eq!(brt5.len(), 1, "{found:?}");
    assert!(matches!(&brt5[0].outcome, Outcome::Violation { .. }));
}

// --- the parser -----------------------------------------------------------------

#[test]
fn an_unknown_key_is_refused_by_name() {
    let refusal = rules::parse(r#"[{ "id": "x", "source": "Y", "severty": "error", "says": "z", "check": { "path": "P", "equals": "v" } }]"#)
        .unwrap_err();
    assert!(refusal.contains("severty") && refusal.contains('x'), "{refusal}");
    let refusal = rules::parse(r#"[{ "id": "x", "source": "Y", "says": "z", "check": { "path": "P", "equals": "v", "subtype": true } }]"#)
        .unwrap_err();
    assert!(refusal.contains("subtype"), "{refusal}");
}

#[test]
fn every_source_and_target_of_the_shipped_rules_is_a_concept_the_schemas_define() {
    let registry = energyml_schemas::all();
    // A concept is a registered type **or an ancestor of one**: the abstract
    // classes (`AbstractProperty`…) are not in the registry on their own, they
    // are the bases the registered types carry.
    let mut known: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for info in registry.types() {
        known.insert(info.type_key);
        let mut base = info.base;
        while let Some(next) = base {
            let one = next();
            known.insert(one.type_key);
            base = one.base;
        }
    }
    let mut missing = Vec::new();
    for rule in rules::trg() {
        if !known.contains(rule.source.as_str()) {
            missing.push(format!("{}: source {}", rule.id, rule.source));
        }
        for check in rule.checks.iter().chain(rule.when.iter()) {
            if let rules::Check::Targets { targets, .. } = check {
                for target in targets {
                    if !known.contains(target.as_str()) {
                        missing.push(format!("{}: target {target}", rule.id));
                    }
                }
            }
        }
    }
    assert!(known.len() > 500, "the registry lists the concepts");
    assert!(
        missing.is_empty(),
        "a typo makes a rule that never applies: {missing:?}"
    );
}
