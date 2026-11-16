//! `house/property/directional-ratio` (ADR-0036): two sibling properties told
//! apart only by a vertical and a lateral `direction` facet keep, for a
//! resistivity, the vertical value at least 0.95 times the lateral one in 95 %
//! of their pairs.
//!
//! **The form comes from the specification, the values are the test's own.**
//! The directions are named by common TRG 2.3 §11.1 p. 281 (`I`, `J`, `X`,
//! `Y` lateral; `K`, `Z` vertical) and the facet kind by §11.3 p. 285; the
//! specification says **nothing** of the order of their values, which is why
//! the threshold (5 %, 0.95) is the house's and is cited to ADR-0036, not to a
//! document. Every array is inline; nothing is read from a file.

use crate::integrity::{Package, boxed, citation, integers, judge, read_of, reference};
use energyml_core::identity;
use energyml_core::registry::DynObject;
use energyml_core::value::Reflect;
use energyml_core::xml::XmlText;
use energyml_domain::integrity::{self, DeclaredDirection, DirectionRole};
use energyml_domain::rules::{Finding, Gravity, Outcome};
use energyml_domain::source::ArraySource;
use energyml_domain::stats::CellRule;
use energyml_domain::stats::pairs::PairAccumulator;
use energyml_domain::stats::scale::{Scale, ScaleKind};
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_2 as resqml22;

const RULE: &str = "house/property/directional-ratio";

/// A kind of the package, a child of the dictionary's `resistivity`
/// (7c0d1a5d-483e-4d77-81f3-9883527838d2): the resolution goes through the
/// dictionary.
const KIND: &str = "00000000-0000-0000-0000-00000000d001";
const RESISTIVITY: &str = "7c0d1a5d-483e-4d77-81f3-9883527838d2";
/// A kind with no expectation.
const POROSITY_KIND: &str = "00000000-0000-0000-0000-00000000d002";
const FRAME: &str = "00000000-0000-0000-0000-00000000d0f1";
const OTHER_FRAME: &str = "00000000-0000-0000-0000-00000000d0f2";
const RH: &str = "00000000-0000-0000-0000-00000000d0a1";
const RV: &str = "00000000-0000-0000-0000-00000000d0a2";

fn child_of_resistivity() -> eml23::PropertyKind {
    eml23::PropertyKind {
        uuid: KIND.to_string(),
        citation: citation("array resistivity of the package"),
        parent: Some(reference("eml23.PropertyKind", RESISTIVITY)),
        ..Default::default()
    }
}

fn a_kind_with_no_parent() -> eml23::PropertyKind {
    eml23::PropertyKind {
        uuid: POROSITY_KIND.to_string(),
        citation: citation("porosity of the package"),
        ..Default::default()
    }
}

fn facet(value: &str) -> eml23::PropertyKindFacet {
    let facet = match value {
        "I" => eml23::FacetExt::I,
        "J" => eml23::FacetExt::J,
        "K" => eml23::FacetExt::K,
        "X" => eml23::FacetExt::X,
        "Z" => eml23::FacetExt::Z,
        other => eml23::FacetExt::Other(other.to_string()),
    };
    eml23::PropertyKindFacet {
        facet,
        kind: eml23::FacetKind::Direction,
    }
}

/// A resistivity-like property on `support`, its values inline.
fn property(
    uuid: &str,
    kind: &str,
    support: &str,
    direction: &str,
    values: &[i64],
    null: i64,
) -> resqml22::ContinuousProperty {
    resqml22::ContinuousProperty {
        uuid: uuid.to_string(),
        citation: citation(&format!("resistivity {direction}")),
        indexable_element: eml23::IndexableElement::Nodes,
        value_count_per_indexable_element: vec![1],
        property_kind: reference("eml23.PropertyKind", kind),
        supporting_representation: reference("resqml22.WellboreFrameRepresentation", support),
        facet: vec![facet(direction)],
        uom: eml23::UnitOfMeasureExt::from_xml_text("ohm.m").expect("a unit"),
        values_for_patch: vec![resqml22::AbstractValueArray::IntegerXmlArray(integers(
            values, null,
        ))],
        ..Default::default()
    }
}

/// 20 values: `count` of `value`, then `rest` of `other`.
fn twenty(count: usize, value: i64, other: i64) -> Vec<i64> {
    let mut all = vec![value; count];
    all.extend(vec![other; 20 - count]);
    all
}

/// The lateral property of the tests: 20 values of 10.
fn lateral(direction: &str, support: &str) -> Box<dyn DynObject> {
    boxed(property(RH, KIND, support, direction, &[10; 20], -999))
}

fn vertical(direction: &str, support: &str, values: &[i64], null: i64) -> Box<dyn DynObject> {
    boxed(property(RV, KIND, support, direction, values, null))
}

/// Vertical values of the first test: 18 of 12 and 2 of 9, so two pairs of
/// twenty (10 %) are under 0.95 (9 / 10 = 0.9).
fn ten_percent_below() -> Vec<i64> {
    let mut values = vec![12; 18];
    values.extend([9, 9]);
    values
}

fn kinds() -> Vec<(&'static str, Box<dyn DynObject>)> {
    vec![
        (KIND, boxed(child_of_resistivity())),
        (POROSITY_KIND, boxed(a_kind_with_no_parent())),
    ]
}

fn package(mut objects: Vec<(&'static str, Box<dyn DynObject>)>) -> Vec<(&'static str, Box<dyn DynObject>)> {
    let mut all = kinds();
    all.append(&mut objects);
    all
}

/// The rule's findings of a package, nothing declared.
fn found(objects: Vec<(&'static str, Box<dyn DynObject>)>, arrays: bool) -> Vec<Finding> {
    let findings = judge(package(objects), arrays);
    findings
        .into_iter()
        .filter(|finding| finding.rule == RULE)
        .collect()
}

/// The same package, run again with what a person declared.
fn found_declaring(
    objects: Vec<(&'static str, Box<dyn DynObject>)>,
    declared: &[DeclaredDirection],
) -> Vec<Finding> {
    let package = Package(
        package(objects)
            .into_iter()
            .map(|(uuid, object)| (uuid.to_string(), object))
            .collect(),
    );
    let pairs: Vec<(&str, &dyn Reflect)> = package
        .0
        .iter()
        .map(|(uuid, object)| (uuid.as_str(), object.as_reflect()))
        .collect();
    let arrays: &dyn ArraySource = &package;
    integrity::directional_ratio_declaring(&pairs, &package, Some(arrays), declared)
}

fn uri_of(object: &dyn DynObject) -> String {
    identity::uri_of_reflect(object.as_reflect()).expect("an object with an identity")
}

/// The vertical resistivity breaks the order: 2 pairs of 20 under 0.95 times
/// the lateral value is 10 %, over the 5 % of ADR-0036. To break it: invert the
/// `<` of the tally, or the `>` of the share.
#[test]
fn a_vertical_resistivity_mostly_below_the_lateral_one_is_a_warning() {
    let findings = found(
        vec![
            (RH, lateral("I", FRAME)),
            (RV, vertical("K", FRAME, &ten_percent_below(), -999)),
        ],
        true,
    );
    assert_eq!(findings.len(), 1, "{findings:?}");
    let finding = &findings[0];
    assert_eq!(finding.rule, RULE);
    assert_eq!(finding.gravity, Gravity::Warning);
    assert_eq!(finding.uuid, RV);
    let read = read_of(finding);
    assert!(read.contains("10.00 %") && read.contains("20 pairs"), "{read}");
}

/// One pair of 20 under the threshold is 5 %: not more than 5 %, so silent
/// (the share is compared with `>`). To break it: write `>=`.
#[test]
fn a_share_at_the_threshold_is_silent() {
    let findings = found(
        vec![
            (RH, lateral("I", FRAME)),
            (RV, vertical("K", FRAME, &twenty(19, 12, 9), -999)),
        ],
        true,
    );
    assert!(findings.is_empty(), "{findings:?}");
}

/// A direction written as an extension value is the producer's: it is not
/// read (common TRG 2.3 §11.2 p. 284), the rule says it cannot judge, and a
/// caller's declaration makes the comparison. To break it: guess "vertical"
/// from the text of the value.
#[test]
fn an_extension_direction_is_unverifiable_unless_declared() {
    let objects = || {
        vec![
            (RH, lateral("nwdf:horizontal", FRAME)),
            (RV, vertical("nwdf:vertical", FRAME, &ten_percent_below(), -999)),
        ]
    };
    let findings = found(objects(), true);
    assert_eq!(findings.len(), 2, "{findings:?}");
    for uuid in [RH, RV] {
        let finding = findings.iter().find(|f| f.uuid == uuid).expect("one each");
        match &finding.outcome {
            Outcome::Unverifiable { why } => {
                assert!(why.contains("nwdf:"), "{why}");
            }
            other => panic!("{other:?}"),
        }
    }
    assert!(
        findings
            .iter()
            .any(|f| matches!(&f.outcome, Outcome::Unverifiable { why } if why.contains("nwdf:vertical")))
    );

    let (h_uri, v_uri) = {
        let all = package(objects());
        let uri = |uuid: &str| uri_of(all.iter().find(|(held, _)| *held == uuid).unwrap().1.as_ref());
        (uri(RH), uri(RV))
    };
    let declared = [
        DeclaredDirection {
            property: v_uri,
            role: DirectionRole::Vertical,
        },
        DeclaredDirection {
            property: h_uri,
            role: DirectionRole::Lateral,
        },
    ];
    let findings = found_declaring(objects(), &declared);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].uuid, RV);
    assert!(read_of(&findings[0]).contains("10.00 %"));
}

/// The grid's `K` and the coordinates' `X` are not the same family of axes:
/// they are not paired. To break it: pair every family.
#[test]
fn two_families_are_not_paired() {
    let findings = found(
        vec![
            (RH, lateral("X", FRAME)),
            (RV, vertical("K", FRAME, &ten_percent_below(), -999)),
        ],
        true,
    );
    assert!(findings.is_empty(), "{findings:?}");
}

/// Only a kind with an expectation is judged: the same values on a porosity
/// say nothing. To break it: apply the expectation to every kind.
#[test]
fn a_kind_with_no_expectation_is_not_judged() {
    let h = boxed(property(RH, POROSITY_KIND, FRAME, "I", &[10; 20], -999));
    let v = boxed(property(
        RV,
        POROSITY_KIND,
        FRAME,
        "K",
        &ten_percent_below(),
        -999,
    ));
    let findings = found(vec![(RH, h), (RV, v)], true);
    assert!(findings.is_empty(), "{findings:?}");
}

/// Not having looked is not having found nothing: with no array reader the
/// rule is unverifiable, never a pass. To break it: return silence.
#[test]
fn without_arrays_it_is_unverifiable() {
    let findings = found(
        vec![
            (RH, lateral("I", FRAME)),
            (RV, vertical("K", FRAME, &ten_percent_below(), -999)),
        ],
        false,
    );
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(
        matches!(&findings[0].outcome, Outcome::Unverifiable { .. }),
        "{findings:?}"
    );
}

/// The array's own `NullValue` is not a value: two values of -999 are two
/// holes, and the 18 pairs left are all in order. To break it: ignore the
/// `NullValue` (-999 / 10 is under 0.95 and the rule fires).
#[test]
fn the_null_value_is_not_a_pair() {
    let findings = found(
        vec![
            (RH, lateral("I", FRAME)),
            (RV, vertical("K", FRAME, &twenty(18, 12, -999), -999)),
        ],
        true,
    );
    assert!(findings.is_empty(), "{findings:?}");
}

/// Siblings are of one support: the same values on two frames are not
/// compared. To break it: do not group by support.
#[test]
fn siblings_on_two_supports_are_not_compared() {
    let findings = found(
        vec![
            (RH, lateral("I", FRAME)),
            (RV, vertical("K", OTHER_FRAME, &ten_percent_below(), -999)),
        ],
        true,
    );
    assert!(findings.is_empty(), "{findings:?}");
}

/// **Triangulation**: the share the finding states is the share the pair
/// accumulator counts for the same values (two counts of one fact; the
/// accumulator's rule is `y / x` below 0.95, with x the lateral values and y
/// the vertical ones). To break it: divide `h / v`.
#[test]
fn the_share_is_the_pair_accumulators() {
    let vertical_values = ten_percent_below();
    let findings = found(
        vec![
            (RH, lateral("I", FRAME)),
            (RV, vertical("K", FRAME, &vertical_values, -999)),
        ],
        true,
    );
    assert_eq!(findings.len(), 1, "{findings:?}");
    let read = read_of(&findings[0]);
    let stated: f64 = read
        .split(" %")
        .next()
        .and_then(|head| head.split_whitespace().next())
        .and_then(|number| number.parse().ok())
        .unwrap_or_else(|| panic!("a percentage in `{read}`"));

    let scale = Scale::new(ScaleKind::Linear, 0.0, 100.0, false).expect("a scale");
    let mut accumulator =
        PairAccumulator::new(scale, scale, 4, Some(CellRule::Below(integrity::RATIO_TOLERANCE)))
            .expect("an accumulator");
    let x: Vec<f64> = vec![10.0; 20];
    let y: Vec<f64> = vertical_values.iter().map(|v| *v as f64).collect();
    accumulator.push(&x, &y).expect("same lengths");
    let share = accumulator.finish().share_matched.expect("a share");
    assert!(
        (stated / 100.0 - share).abs() < 1e-12,
        "stated {stated} %, counted {share}"
    );
}
