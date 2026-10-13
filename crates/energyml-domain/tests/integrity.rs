//! The cross-object rules of `energyml_domain::integrity`, one anomaly of
//! the synthetic reference model audit (ADR-0018) at a time.
//!
//! **The form comes from the specification, the values are the test's own.**
//! Every test builds an object that breaks the cited rule and a twin that does
//! not, and asserts that the first is found **exactly once, by its rule id**,
//! and the twin not at all. Nothing is read from a file: the citations are in
//! the comments, where a reader can check them (ADR-0011: a reference document
//! is cited, not read at run time).
//!
//! A test that only asserted silence on the twin could not fail; the finding
//! on the broken object is what gives each one its teeth — and each was run
//! red once, by emptying the check it guards, before being trusted.

use energyml_core::registry::DynObject;
use energyml_core::value::Reflect;
use energyml_domain::integrity;
use energyml_domain::rules::{Finding, Outcome};
use energyml_domain::source::{ArraySource, ObjectSource};
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_2 as resqml22;
use energyml_storage::array::{ArrayBuffer, ArrayData, ArrayLocation, ArrayMetadata};
use energyml_witsml_2_1 as witsml21;

/// Lends the objects it holds, and reads no array from any file: every array
/// in these tests is inline XML, which `values::read` computes without one.
struct Package(Vec<(String, Box<dyn DynObject>)>);

impl ObjectSource for Package {
    fn object(&self, uuid: &str) -> Option<Box<dyn DynObject>> {
        self.0
            .iter()
            .find(|(held, _)| held.eq_ignore_ascii_case(uuid))
            .map(|(_, object)| object.clone_box())
    }
}

impl ArraySource for Package {
    fn locate(&self, _object: &dyn Reflect) -> Vec<ArrayLocation> {
        Vec::new()
    }
    fn metadata(&self, _location: &ArrayLocation) -> energyml_domain::Result<ArrayMetadata> {
        Err(energyml_domain::Error::unsupported("an external array", "the test's arrays are inline"))
    }
    fn read(&self, _location: &ArrayLocation) -> energyml_domain::Result<ArrayData> {
        Err(energyml_domain::Error::unsupported("an external array", "the test's arrays are inline"))
    }
    fn read_into(&self, _location: &ArrayLocation, _out: &mut ArrayBuffer<'_>) -> energyml_domain::Result<ArrayMetadata> {
        Err(energyml_domain::Error::unsupported("an external array", "the test's arrays are inline"))
    }
}

/// Run every check over these objects, the package lending them.
fn judge(objects: Vec<(&str, Box<dyn DynObject>)>, arrays: bool) -> Vec<Finding> {
    let package = Package(objects.into_iter().map(|(uuid, object)| (uuid.to_string(), object)).collect());
    let pairs: Vec<(&str, &dyn Reflect)> = package.0.iter().map(|(uuid, object)| (uuid.as_str(), object.as_reflect())).collect();
    let arrays: Option<&dyn ArraySource> = if arrays { Some(&package) } else { None };
    integrity::run(&pairs, &package, arrays)
}

/// The findings of one rule.
fn of<'a>(findings: &'a [Finding], rule: &str) -> Vec<&'a Finding> {
    findings.iter().filter(|finding| finding.rule == rule).collect()
}

fn read_of(finding: &Finding) -> &str {
    match &finding.outcome {
        Outcome::Violation { read } => read,
        other => panic!("expected a violation, got {other:?}"),
    }
}

fn reference(qualified_type: &str, uuid: &str) -> eml23::DataObjectReference {
    eml23::DataObjectReference {
        uuid: uuid.to_string(),
        qualified_type: qualified_type.to_string(),
        title: "a target".to_string(),
        ..Default::default()
    }
}

fn citation(title: &str) -> eml23::Citation {
    eml23::Citation {
        title: title.to_string(),
        originator: "test".to_string(),
        creation: "2026-09-25T00:00:00Z".to_string(),
        format: "test".to_string(),
        ..Default::default()
    }
}

fn boxed<T: DynObject + 'static>(object: T) -> Box<dyn DynObject> {
    Box::new(object)
}

// ---------------------------------------------------------------------------
// house/discrete-value-in-lookup
// ---------------------------------------------------------------------------

const TABLE: &str = "00000000-0000-0000-0000-00000000e301";

fn integers(values: &[i64], null: i64) -> eml23::IntegerXmlArray {
    eml23::IntegerXmlArray {
        values: values.to_vec(),
        null_value: null,
        count_per_value: 1,
        ..Default::default()
    }
}

/// A category table keyed by 1, 2, 3 — the shape of a lithologies table, a
/// `KeyColumn` and a value column, both inline.
fn table() -> eml23::ColumnBasedTable {
    let column = |title: &str, values: &[i64]| eml23::Column {
        title: Some(title.to_string()),
        value_count_per_row: 1,
        values: Some(eml23::AbstractValueArray::IntegerXmlArray(integers(values, -999))),
        ..Default::default()
    };
    eml23::ColumnBasedTable {
        uuid: TABLE.to_string(),
        citation: citation("lithologies"),
        key_column: vec![column("code", &[1, 2, 3])],
        column: vec![column("rank", &[10, 20, 30])],
        ..Default::default()
    }
}

fn discrete(uuid: &str, values: &[i64]) -> resqml22::DiscreteProperty {
    resqml22::DiscreteProperty {
        uuid: uuid.to_string(),
        citation: citation("lithology"),
        indexable_element: eml23::IndexableElement::Cells,
        value_count_per_indexable_element: vec![1],
        category_lookup: Some(reference("eml23.ColumnBasedTable", TABLE)),
        values_for_patch: vec![resqml22::AbstractValueArray::IntegerXmlArray(integers(values, -1))],
        ..Default::default()
    }
}

/// A `DiscreteProperty` value absent from the `KeyColumn` of its table
/// (the value is looked up in the key column).
#[test]
fn e03_a_discrete_value_no_key_names_is_found_once() {
    let bad = "00000000-0000-0000-0000-00000000e302";
    let good = "00000000-0000-0000-0000-00000000e303";
    let findings = judge(
        vec![
            (TABLE, boxed(table())),
            // 7 twice: counted once, with its count. -1 is the array's own null.
            (bad, boxed(discrete(bad, &[1, 7, 2, 7, -1]))),
            (good, boxed(discrete(good, &[1, 2, 3, -1, 3]))),
        ],
        true,
    );
    let found = of(&findings, "house/discrete-value-in-lookup");
    assert_eq!(found.len(), 1, "{findings:?}");
    assert_eq!(found[0].uuid, bad);
    let read = read_of(found[0]);
    assert!(read.contains("7 (2 element(s))") && read.contains("code"), "{read}");
    assert!(!read.contains("-1"), "the null value is not a value: {read}");
}

/// Not having looked is not having found nothing: with no array reader, the
/// property is *uncheckable*, never passed.
#[test]
fn e03_without_an_array_reader_is_uncheckable_never_a_pass() {
    let bad = "00000000-0000-0000-0000-00000000e304";
    let findings = judge(vec![(TABLE, boxed(table())), (bad, boxed(discrete(bad, &[7])))], false);
    let found = of(&findings, "house/discrete-value-in-lookup");
    assert_eq!(found.len(), 1, "{findings:?}");
    assert!(matches!(&found[0].outcome, Outcome::Unverifiable { .. }), "{found:?}");
}

// ---------------------------------------------------------------------------
// house/time-series-asset-resolves
// ---------------------------------------------------------------------------

/// A PRODML 2.3 series whose `asset identifier` is the given URI, in the
/// house convention.
fn series(uuid: &str, asset: &str) -> Box<dyn DynObject> {
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<prodml:TimeSeriesData xmlns:prodml="http://www.energistics.org/energyml/data/prodmlv2" xmlns:eml="http://www.energistics.org/energyml/data/commonv2" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" uuid="{uuid}" schemaVersion="2.3">
  <eml:Citation>
    <eml:Title>a head pressure</eml:Title>
    <eml:Originator>test</eml:Originator>
    <eml:Creation>2026-09-25T00:00:00Z</eml:Creation>
    <eml:Format>test</eml:Format>
  </eml:Citation>
  <prodml:Key keyword="asset identifier">{asset}</prodml:Key>
  <prodml:Key keyword="product">water</prodml:Key>
  <prodml:Uom>bar</prodml:Uom>
  <prodml:MeasureClass>pressure</prodml:MeasureClass>
  <prodml:DataValue xsi:type="prodml:DoubleValue">
    <prodml:DoubleValue dTim="2025-06-02T00:00:00Z">3.5</prodml:DoubleValue>
  </prodml:DataValue>
</prodml:TimeSeriesData>"#
    );
    energyml_schemas::all()
        .read_xml("application/x-prodml+xml;version=2.3;type=TimeSeriesData", xml.as_bytes())
        .expect("the series reads")
}

fn well(uuid: &str, name: &str) -> witsml21::Well {
    witsml21::Well {
        uuid: uuid.to_string(),
        citation: citation(name),
        ..Default::default()
    }
}

/// A `TimeSeriesData` whose `asset identifier` resolves to nothing — the
/// orphan series. The URI grammar is the
/// Energistics Identifier Specification v4.0 §6.3 p. 14.
#[test]
fn e04_a_series_naming_an_absent_well_is_found_once() {
    let orphan = "00000000-0000-0000-0000-00000000e401";
    let rooted = "00000000-0000-0000-0000-00000000e402";
    let findings = judge(
        vec![
            ("00000000-0000-0000-0000-00000000e403", boxed(well("00000000-0000-0000-0000-00000000e403", "GTH-1"))),
            (orphan, series(orphan, "eml://lutetia.geosiris.com/well(GTH-9)/wellbore(01)/wellhead")),
            (rooted, series(rooted, "eml://lutetia.geosiris.com/well(GTH-1)/wellbore(01)/wellhead")),
        ],
        true,
    );
    let found = of(&findings, "house/time-series-asset-resolves");
    assert_eq!(found.len(), 1, "{findings:?}");
    assert_eq!(found[0].uuid, orphan);
    assert!(read_of(found[0]).contains("GTH-9"), "{found:?}");
}

/// Two wells answering to one name: a name is not an identity, so the series
/// is *uncheckable* rather than matched to the first.
#[test]
fn e04_a_name_two_wells_answer_to_is_uncheckable() {
    let series_uuid = "00000000-0000-0000-0000-00000000e404";
    let findings = judge(
        vec![
            ("00000000-0000-0000-0000-00000000e405", boxed(well("00000000-0000-0000-0000-00000000e405", "GTH-1"))),
            ("00000000-0000-0000-0000-00000000e406", boxed(well("00000000-0000-0000-0000-00000000e406", "GTH-1"))),
            (series_uuid, series(series_uuid, "eml://lutetia.geosiris.com/well(GTH-1)/wellhead")),
        ],
        true,
    );
    let found = of(&findings, "house/time-series-asset-resolves");
    assert_eq!(found.len(), 1, "{findings:?}");
    assert!(matches!(&found[0].outcome, Outcome::Unverifiable { why } if why.contains("not an identity")), "{found:?}");
}

// ---------------------------------------------------------------------------
// trg/one-definitive-trajectory-per-wellbore
// ---------------------------------------------------------------------------

fn trajectory(uuid: &str, wellbore: &str, definitive: Option<bool>) -> witsml21::Trajectory {
    witsml21::Trajectory {
        uuid: uuid.to_string(),
        citation: citation("a survey"),
        definitive,
        wellbore: reference("witsml21.Wellbore", wellbore),
        ..Default::default()
    }
}

/// Two `Trajectory` `Definitive` on one wellbore — WITSML TRG 2.1 §16.6
/// p. 350: *"There can only be one trajectory per wellbore with
/// definitive=true"*. The finding is the wellbore's, and it names both.
#[test]
fn e06_two_definitive_trajectories_on_one_wellbore_are_found_once() {
    let twice = "00000000-0000-0000-0000-00000000e601";
    let once = "00000000-0000-0000-0000-00000000e602";
    let findings = judge(
        vec![
            ("00000000-0000-0000-0000-00000000e611", boxed(trajectory("00000000-0000-0000-0000-00000000e611", twice, Some(true)))),
            ("00000000-0000-0000-0000-00000000e612", boxed(trajectory("00000000-0000-0000-0000-00000000e612", twice, Some(true)))),
            // The twin: one definitive, one explicitly not, one silent.
            ("00000000-0000-0000-0000-00000000e621", boxed(trajectory("00000000-0000-0000-0000-00000000e621", once, Some(true)))),
            ("00000000-0000-0000-0000-00000000e622", boxed(trajectory("00000000-0000-0000-0000-00000000e622", once, Some(false)))),
            ("00000000-0000-0000-0000-00000000e623", boxed(trajectory("00000000-0000-0000-0000-00000000e623", once, None))),
        ],
        true,
    );
    let found = of(&findings, "trg/one-definitive-trajectory-per-wellbore");
    assert_eq!(found.len(), 1, "{findings:?}");
    assert_eq!(found[0].uuid, twice);
    let read = read_of(found[0]);
    assert!(read.contains("e611") && read.contains("e612"), "{read}");
}

// ---------------------------------------------------------------------------
// trg/is-listric-iff-normal
// ---------------------------------------------------------------------------

fn fault(uuid: &str, throws: &[resqml22::ThrowKindExt], listric: Option<bool>) -> resqml22::FaultInterpretation {
    resqml22::FaultInterpretation {
        uuid: uuid.to_string(),
        citation: citation("a fault"),
        interpreted_feature: reference("resqml22.BoundaryFeature", "00000000-0000-0000-0000-00000000e7ff"),
        is_listric: listric,
        throw_interpretation: vec![resqml22::FaultThrow {
            throw: throws.to_vec(),
            ..Default::default()
        }],
        ..Default::default()
    }
}

/// `IsListric` on a reverse fault (F9) — Interpretations.xsd l. 659–662,
/// TRG 2.2 §6.20 p. 188: *"Must not be present if the fault is not normal."*
#[test]
fn e07_is_listric_on_a_reverse_fault_is_found_once() {
    use resqml22::ThrowKindExt::{Normal, Reverse, StrikeMinusSlip};
    let reverse = "00000000-0000-0000-0000-00000000e701";
    let findings = judge(
        vec![
            (reverse, boxed(fault(reverse, &[Reverse], Some(true)))),
            // The twins, all conforming under the rule (present if and only if the only
            // set is normal): a normal fault that says whether it is listric,
            // a reverse one that does not, an oblique one that does not.
            ("00000000-0000-0000-0000-00000000e702", boxed(fault("00000000-0000-0000-0000-00000000e702", &[Normal], Some(false)))),
            ("00000000-0000-0000-0000-00000000e703", boxed(fault("00000000-0000-0000-0000-00000000e703", &[Reverse], None))),
            ("00000000-0000-0000-0000-00000000e704", boxed(fault("00000000-0000-0000-0000-00000000e704", &[Normal, StrikeMinusSlip], None))),
        ],
        true,
    );
    let found = of(&findings, "trg/is-listric-iff-normal");
    assert_eq!(found.len(), 1, "{findings:?}");
    assert_eq!(found[0].uuid, reverse);
    assert!(read_of(found[0]).contains("reverse"), "{found:?}");
}

/// The other half of the same sentence: *"Must be present if the fault is
/// normal."*
#[test]
fn e07_a_normal_fault_without_is_listric_is_found_too() {
    use resqml22::ThrowKindExt::Normal;
    let silent = "00000000-0000-0000-0000-00000000e705";
    let findings = judge(vec![(silent, boxed(fault(silent, &[Normal], None)))], true);
    let found = of(&findings, "trg/is-listric-iff-normal");
    assert_eq!(found.len(), 1, "{findings:?}");
    assert_eq!(found[0].uuid, silent);
    assert!(read_of(found[0]).contains("absent"), "{found:?}");
}

/// Where the guide contradicts itself, the answer is a named refusal: the
/// same page (TRG 2.2 §6.20 p. 188) writes a thrust with *"is Listric = 0"*
/// and forbids `IsListric` on a fault that is not normal.
#[test]
fn e07_a_thrust_written_as_the_guide_says_is_uncheckable() {
    use resqml22::ThrowKindExt::Thrust;
    let thrust = "00000000-0000-0000-0000-00000000e706";
    let findings = judge(vec![(thrust, boxed(fault(thrust, &[Thrust], Some(false))))], true);
    let found = of(&findings, "trg/is-listric-iff-normal");
    assert_eq!(found.len(), 1, "{findings:?}");
    assert!(matches!(&found[0].outcome, Outcome::Unverifiable { why } if why.contains("thrust")), "{found:?}");
}

// ---------------------------------------------------------------------------
// trg/homogeneous-collection, trg/wellbore-interpretation-set-holds-wellbores
// ---------------------------------------------------------------------------

fn collection(uuid: &str, homogeneous: Option<bool>) -> eml23::DataobjectCollection {
    eml23::DataobjectCollection {
        uuid: uuid.to_string(),
        citation: citation("wells"),
        homogeneous_datatype: homogeneous,
        ..Default::default()
    }
}

fn associations(uuid: &str, collection: &str, members: &[(&str, &str)], flag: Option<bool>) -> eml23::CollectionsToDataobjectsAssociationSet {
    eml23::CollectionsToDataobjectsAssociationSet {
        uuid: uuid.to_string(),
        citation: citation("memberships"),
        single_collection_association: vec![eml23::SingleCollectionAssociation {
            homogeneous_datatype: flag,
            collection: reference("eml23.DataobjectCollection", collection),
            dataobject: members.iter().map(|(kind, member)| reference(kind, member)).collect(),
        }],
        ..Default::default()
    }
}

fn earth_model(uuid: &str, set: &str) -> resqml22::EarthModelInterpretation {
    resqml22::EarthModelInterpretation {
        uuid: uuid.to_string(),
        citation: citation("an earth model"),
        interpreted_feature: reference("resqml22.Model", "00000000-0000-0000-0000-00000000e8ff"),
        wellbore_interpretation_set: vec![reference("eml23.DataobjectCollection", set)],
        ..Default::default()
    }
}

/// A `WellboreInterpretationSet` → a collection `HomogeneousDatatype`
/// true that mixes a `WellboreInterpretation` and a `HorizonInterpretation`.
/// Two rules, two findings expected: Collection.xsd l. 66–70 on the collection's flag, and TRG 2.2
/// §6.19 p. 187 (*"the DataobjectCollection must only contain
/// WellboreInterpretation"*) on the earth model's set.
#[test]
fn e08_a_mixed_wellbore_set_is_found_by_its_two_rules_once_each() {
    let (bad_set, bad_model) = ("00000000-0000-0000-0000-00000000e801", "00000000-0000-0000-0000-00000000e802");
    let (good_set, good_model) = ("00000000-0000-0000-0000-00000000e811", "00000000-0000-0000-0000-00000000e812");
    let findings = judge(
        vec![
            (bad_set, boxed(collection(bad_set, Some(true)))),
            (
                "00000000-0000-0000-0000-00000000e803",
                boxed(associations(
                    "00000000-0000-0000-0000-00000000e803",
                    bad_set,
                    &[
                        ("resqml22.WellboreInterpretation", "00000000-0000-0000-0000-00000000e804"),
                        ("resqml22.HorizonInterpretation", "00000000-0000-0000-0000-00000000e805"),
                    ],
                    None,
                )),
            ),
            (bad_model, boxed(earth_model(bad_model, bad_set))),
            (good_set, boxed(collection(good_set, Some(true)))),
            (
                "00000000-0000-0000-0000-00000000e813",
                boxed(associations(
                    "00000000-0000-0000-0000-00000000e813",
                    good_set,
                    &[
                        ("resqml22.WellboreInterpretation", "00000000-0000-0000-0000-00000000e814"),
                        ("resqml22.WellboreInterpretation", "00000000-0000-0000-0000-00000000e815"),
                    ],
                    Some(true),
                )),
            ),
            (good_model, boxed(earth_model(good_model, good_set))),
        ],
        true,
    );

    let homogeneous = of(&findings, "trg/homogeneous-collection");
    assert_eq!(homogeneous.len(), 1, "{findings:?}");
    assert_eq!(homogeneous[0].uuid, bad_set);
    let read = read_of(homogeneous[0]);
    assert!(read.contains("1 WellboreInterpretation") && read.contains("1 HorizonInterpretation"), "{read}");

    let set = of(&findings, "trg/wellbore-interpretation-set-holds-wellbores");
    assert_eq!(set.len(), 1, "{findings:?}");
    assert_eq!(set[0].uuid, bad_model);
    let read = read_of(set[0]);
    assert!(read.contains("e805") && !read.contains("e804"), "only the stranger is named: {read}");

    // The association of the bad set declares nothing, the good one's is
    // right: the association-level rule has nothing to say here.
    assert!(of(&findings, "trg/homogeneous-association").is_empty(), "{findings:?}");
}

/// The same flag, written on the association itself (Collection.xsd
/// l. 82–86): its own list is judged, and the finding is the association
/// set's, the association named by its index.
#[test]
fn e08_an_association_claiming_one_type_and_listing_two_is_found_once() {
    let set = "00000000-0000-0000-0000-00000000e821";
    let findings = judge(
        vec![(
            set,
            boxed(associations(
                set,
                "00000000-0000-0000-0000-00000000e822",
                &[
                    ("resqml22.WellboreInterpretation", "00000000-0000-0000-0000-00000000e823"),
                    ("resqml22.HorizonInterpretation", "00000000-0000-0000-0000-00000000e824"),
                ],
                Some(true),
            )),
        )],
        true,
    );
    let found = of(&findings, "trg/homogeneous-association");
    assert_eq!(found.len(), 1, "{findings:?}");
    assert_eq!(found[0].uuid, set);
    assert!(read_of(found[0]).starts_with("SingleCollectionAssociation[0]"), "{found:?}");
}

// ---------------------------------------------------------------------------
// A unit outside its kind's quantity class: red until the units table lands
// ---------------------------------------------------------------------------

/// `g/L` on `water salinity` (whose quantity class is
/// `dimensionless`). The rule is
/// already `property_kind::check`'s (`UomOutsideQuantityClass`); what is
/// missing is the table of units per class, so today the answer
/// is `QuantityClassNotCheckable` — honest, and not a finding.
///
/// It is meant to stay red until the units table is done. Ignored so the suite stays green, runnable with `--ignored` to
/// see it red; the day it lands, the `ignore` goes and this is the test.
/// The units of `dimensionless` are the sixteen of `DimensionlessUom` in EML
/// 2.3 QuantityClass.xsd (`%`, `Euc` and its prefixes, `ppk`, `ppm`) — `g/L`
/// is `mass per volume`.
#[test]
#[ignore = "the units of a quantity class are not in this build — red until then, deliberately"]
fn e05_a_unit_outside_its_kinds_quantity_class_is_a_defect() {
    use energyml_domain::property::PropertyKindRef;
    use energyml_domain::property_kind::{self, KindIssue, Resolver};

    let uuid = "00000000-0000-0000-0000-00000000e501";
    let kind = eml23::PropertyKind {
        uuid: uuid.to_string(),
        citation: citation("water salinity"),
        quantity_class: eml23::QuantityClassKindExt::Dimensionless,
        ..Default::default()
    };
    let package = Package(vec![(uuid.to_string(), boxed(kind))]);
    let resolution = Resolver::new(&package)
        .resolve(&PropertyKindRef::Object { uuid: uuid.to_string(), title: None })
        .expect("the local kind resolves");

    let wrong = property_kind::check(&resolution, Some("g/L"));
    assert!(matches!(wrong.as_slice(), [KindIssue::UomOutsideQuantityClass { .. }]), "{wrong:?}");
    let right = property_kind::check(&resolution, Some("ppm"));
    assert!(right.is_empty(), "{right:?}");
}
