//! Sibling properties and the shape facts of a support along a well, from the
//! sheets of the package index alone.
//!
//! Sources (each test names its own):
//!
//! - EML TRG v2.3 §11.11 `PropertyKindFacet` p. 294 (one facet of a kind per
//!   property), §11.3 `FacetKind` p. 285, §11.2 `FacetExt` p. 284;
//! - RESQML TRG v2.2 §7.1 `AbstractProperty` p. 222 (the value count per
//!   element, slowest dimension first; `LabelPerComponent`).
//!
//! Every package is built by hand in an empty store: nothing reads a file. The
//! supports are not stored: a node the package does not hold is typed by the
//! reference that names it, which is all the detection reads.

use std::sync::Arc;

use energyml_domain::package_index::{CandidateNeed, CandidateRole, PackageIndex, ShapeCandidate};
use energyml_domain::property::kinship::{KinFacts, Kinship, Sibling, TimeKey, kinship};
use energyml_domain::property::{self, Facet, FacetValue, Instant};
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_2 as resqml22;
use energyml_storage::EpcStore;

const FRAME: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const KIND: &str = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";
const FRAME_TYPE: &str = "resqml22.WellboreFrameRepresentation";
const GRID_TYPE: &str = "resqml22.Grid2dRepresentation";

const P1: &str = "11111111-1111-4111-8111-111111111111";
const P2: &str = "22222222-2222-4222-8222-222222222222";
const P3: &str = "33333333-3333-4333-8333-333333333333";
const P4: &str = "44444444-4444-4444-8444-444444444444";

fn dor(uuid: &str, qualified_type: &str, title: &str) -> eml23::DataObjectReference {
    eml23::DataObjectReference {
        uuid: uuid.to_string(),
        qualified_type: qualified_type.to_string(),
        title: title.to_string(),
        ..Default::default()
    }
}

fn unit(code: &str) -> eml23::UnitOfMeasureExt {
    eml23::UnitOfMeasureExt::from_xml_text(code).expect("a unit")
}

/// A facet of a 2.2 property: the value is `FacetExt`, an unknown text is the
/// `Other` hole of the open enumeration.
fn facet(kind: eml23::FacetKind, value: &str) -> eml23::PropertyKindFacet {
    let facet = match value {
        "I" => eml23::FacetExt::I,
        "J" => eml23::FacetExt::J,
        "K" => eml23::FacetExt::K,
        other => eml23::FacetExt::Other(other.to_string()),
    };
    eml23::PropertyKindFacet { facet, kind }
}

fn direction(value: &str) -> eml23::PropertyKindFacet {
    facet(eml23::FacetKind::Direction, value)
}

fn instant(date_time: &str) -> eml23::GeologicTime {
    eml23::GeologicTime {
        date_time: date_time.to_string(),
        age_offset_attribute: None,
    }
}

/// What a test varies on a property; everything else is the same.
struct Spec {
    uuid: &'static str,
    support_type: &'static str,
    counts: Vec<i64>,
    uom: &'static str,
    facets: Vec<eml23::PropertyKindFacet>,
    labels: Vec<String>,
    time: Option<eml23::GeologicTime>,
}

impl Spec {
    fn new(uuid: &'static str, counts: &[i64], uom: &'static str) -> Self {
        Spec {
            uuid,
            support_type: FRAME_TYPE,
            counts: counts.to_vec(),
            uom,
            facets: Vec::new(),
            labels: Vec::new(),
            time: None,
        }
    }

    fn object(&self) -> resqml22::ContinuousProperty {
        resqml22::ContinuousProperty {
            uuid: self.uuid.to_string(),
            citation: eml23::Citation {
                title: format!("property {}", &self.uuid[..2]),
                ..Default::default()
            },
            indexable_element: eml23::IndexableElement::Nodes,
            value_count_per_indexable_element: self.counts.clone(),
            property_kind: dor(KIND, "eml23.PropertyKind", "a kind"),
            supporting_representation: dor(FRAME, self.support_type, "a representation"),
            label_per_component: self.labels.clone(),
            time: self.time.clone(),
            facet: self.facets.clone(),
            uom: unit(self.uom),
            values_for_patch: vec![resqml22::AbstractValueArray::IntegerConstantArray(
                eml23::IntegerConstantArray {
                    value: 1,
                    count: 3,
                    ..Default::default()
                },
            )],
            ..Default::default()
        }
    }
}

fn store_of(specs: &[Spec]) -> EpcStore {
    let bytes = energyml_storage::extract::empty_package(None).expect("an empty package is written");
    let mut store = EpcStore::from_reader(
        Box::new(std::io::Cursor::new(bytes)),
        "built.epc",
        Arc::new(energyml_schemas::all()),
    )
    .expect("it opens");
    for spec in specs {
        store.put(Box::new(spec.object())).expect("the package takes it");
    }
    store
}

fn uri_of(store: &EpcStore, uuid: &str) -> String {
    store
        .find_by_uuid(uuid)
        .expect("the object is in the store")
        .uri
        .clone()
}

fn built(specs: &[Spec]) -> (EpcStore, PackageIndex) {
    let store = store_of(specs);
    let index = PackageIndex::build(&store, 1);
    (store, index)
}

fn direction_facet(value: &str, standard: bool) -> Facet {
    Facet {
        kind: "direction".to_string(),
        value: if standard {
            FacetValue::Standard(value.to_string())
        } else {
            FacetValue::Extension(value.to_string())
        },
    }
}

fn candidate<'a>(candidates: &'a [ShapeCandidate], uri: &str) -> Option<&'a ShapeCandidate> {
    candidates.iter().find(|candidate| candidate.uri == uri)
}

// ---------------------------------------------------------------------------
// F7: the sheet keeps what the property says
// ---------------------------------------------------------------------------

#[test]
fn a_sheet_keeps_facets_labels_and_instant() {
    // RESQML TRG v2.2 §7.1 p. 222 (`LabelPerComponent`, linearised);
    // EML TRG v2.3 §11.11 p. 294 (facets).
    let mut spec = Spec::new(P1, &[3], "ohm.m");
    spec.facets = vec![direction("nwdf:vertical")];
    spec.labels = vec!["a".into(), "b".into(), "c".into()];
    spec.time = Some(instant("2030-01-01T00:00:00Z"));
    let (store, index) = built(&[spec]);

    let sheet = index.sheet(&uri_of(&store, P1)).expect("a sheet");
    assert_eq!(&*sheet.facets, &[direction_facet("nwdf:vertical", false)]);
    let labels: Vec<&str> = sheet.label_per_component.iter().map(|label| &**label).collect();
    assert_eq!(labels, ["a", "b", "c"]);
    assert_eq!(
        sheet.instant.as_deref(),
        Some(&Instant {
            date_time: "2030-01-01T00:00:00Z".to_string(),
            age_offset: None,
        })
    );
    // The shape the sheet gives is the one of its labels.
    let shape = sheet.element_shape().expect("a shape");
    assert_eq!(shape.dims, vec![3]);
    assert_eq!(shape.label_of(&[1]), Some("b"));
}

// ---------------------------------------------------------------------------
// F10: siblings
// ---------------------------------------------------------------------------

#[test]
fn rh_and_rv_are_direction_siblings() {
    // EML TRG v2.3 §11.11 p. 294: the `direction` facet tells two properties of
    // one kind apart.
    let mut rh = Spec::new(P1, &[3], "ohm.m");
    rh.facets = vec![direction("nwdf:horizontal")];
    let mut rv = Spec::new(P2, &[3], "ohm.m");
    rv.facets = vec![direction("nwdf:vertical")];
    let (store, index) = built(&[rh, rv]);
    let (rh_uri, rv_uri) = (uri_of(&store, P1), uri_of(&store, P2));

    // Asked by uri, and by the bare uuid at the entry.
    for asked in [rh_uri.as_str(), P1] {
        assert_eq!(
            index.siblings(asked),
            vec![Sibling {
                uri: rv_uri.clone(),
                kinship: Kinship::Direction {
                    this: direction_facet("nwdf:horizontal", false),
                    other: direction_facet("nwdf:vertical", false),
                },
            }]
        );
    }
    assert_eq!(index.siblings(&rv_uri).len(), 1);
    // Breaking it: compare the facet kinds instead of their values and both
    // directions read as equal, so the first property has `SameKind`.
}

#[test]
fn three_permeabilities_are_three_siblings() {
    // EML TRG v2.3 §11.3 p. 285: `direction` values `I`, `J`, `K`.
    let specs: Vec<Spec> = [(P1, "I"), (P2, "J"), (P3, "K")]
        .into_iter()
        .map(|(uuid, value)| {
            let mut spec = Spec::new(uuid, &[1], "ohm.m");
            spec.facets = vec![direction(value)];
            spec
        })
        .collect();
    let (store, index) = built(&specs);

    for uuid in [P1, P2, P3] {
        let siblings = index.siblings(&uri_of(&store, uuid));
        assert_eq!(siblings.len(), 2, "two siblings of {uuid}");
        assert!(
            siblings
                .iter()
                .all(|sibling| matches!(sibling.kinship, Kinship::Direction { .. }))
        );
    }
    // Breaking it: stop at the first sibling found and the length is 1.
}

#[test]
fn annual_properties_are_time_steps() {
    // RESQML TRG v2.2 §7.1: a 2.2 property dated on its own carries a `Time`.
    let mut first = Spec::new(P1, &[1], "ohm.m");
    first.time = Some(instant("2030-01-01T00:00:00Z"));
    let mut second = Spec::new(P2, &[1], "ohm.m");
    second.time = Some(instant("2031-01-01T00:00:00Z"));
    let (store, index) = built(&[first, second]);

    let siblings = index.siblings(&uri_of(&store, P1));
    assert_eq!(siblings.len(), 1);
    let step = |date_time: &str| {
        TimeKey::Instant(Instant {
            date_time: date_time.to_string(),
            age_offset: None,
        })
    };
    assert_eq!(
        siblings[0].kinship,
        Kinship::TimeStep {
            this: step("2030-01-01T00:00:00Z"),
            other: step("2031-01-01T00:00:00Z"),
        }
    );
    // Breaking it: ignore `instant` in the facts and the answer is `SameKind`.
}

#[test]
fn two_properties_with_nothing_to_tell_them_apart_are_the_same_kind() {
    let (store, index) = built(&[Spec::new(P1, &[1], "ohm.m"), Spec::new(P2, &[1], "ohm.m")]);
    let siblings = index.siblings(&uri_of(&store, P1));
    assert_eq!(siblings.len(), 1);
    assert_eq!(siblings[0].kinship, Kinship::SameKind);
}

#[test]
fn a_radius_and_its_angles_are_a_polar_pair() {
    // `[k]`, `k >= 3`, one length and one angle, on the same support and element.
    let (store, index) = built(&[Spec::new(P1, &[16], "m"), Spec::new(P2, &[16], "rad")]);
    let (radius, angles) = (uri_of(&store, P1), uri_of(&store, P2));

    let from_radius = index.siblings(&radius);
    assert_eq!(
        from_radius[0].kinship,
        Kinship::PolarPair {
            radius: radius.clone(),
            angles: angles.clone(),
        }
    );
    // The pair reads the same from the angles' side.
    let from_angles = index.siblings(&angles);
    assert_eq!(from_angles[0].uri, radius);
    assert_eq!(
        from_angles[0].kinship,
        Kinship::PolarPair {
            radius: radius.clone(),
            angles: angles.clone(),
        }
    );
    // Breaking it: require the same unit (the rule for the other kinships) and
    // the list is empty.
}

#[test]
fn a_sibling_of_another_shape_is_not_one() {
    // RESQML TRG v2.2 §7.1 p. 222: the value count is the same on every element
    // of a property; a `[3]` and a `[4]` are not the same quantity.
    let (store, index) = built(&[Spec::new(P1, &[3], "ohm.m"), Spec::new(P2, &[4], "ohm.m")]);
    assert!(index.siblings(&uri_of(&store, P1)).is_empty());
    // Breaking it: do not compare the shapes and the list holds `SameKind`.
}

#[test]
fn an_unknown_property_has_no_siblings() {
    let (_store, index) = built(&[Spec::new(P1, &[3], "ohm.m")]);
    assert!(index.siblings("eml:///resqml22.ContinuousProperty(99999999-9999-4999-8999-999999999999)").is_empty());
}

#[test]
fn kinship_reads_the_same_on_a_sheet_and_on_a_property() {
    // The index and the read object are two sources of the same fact: the rule
    // must give one answer from each.
    let mut rh = Spec::new(P1, &[3], "ohm.m");
    rh.facets = vec![direction("nwdf:horizontal")];
    let mut rv = Spec::new(P2, &[3], "ohm.m");
    rv.facets = vec![direction("nwdf:vertical")];
    let objects = [rh.object(), rv.object()];
    let store = store_of(&[rh, rv]);
    let index = PackageIndex::build(&store, 1);
    let (rh_uri, rv_uri) = (uri_of(&store, P1), uri_of(&store, P2));
    let support = format!("eml:///{FRAME_TYPE}({FRAME})");

    let on_sheets = kinship(
        &KinFacts::of_sheet(&rh_uri, index.sheet(&rh_uri).expect("a sheet")).expect("facts"),
        &KinFacts::of_sheet(&rv_uri, index.sheet(&rv_uri).expect("a sheet")).expect("facts"),
    );
    let (read_h, read_v) = (
        property::read(&objects[0]).expect("a property"),
        property::read(&objects[1]).expect("a property"),
    );
    let on_properties = kinship(
        &KinFacts::of_property(&rh_uri, &support, &read_h),
        &KinFacts::of_property(&rv_uri, &support, &read_v),
    );
    assert!(matches!(on_sheets, Some(Kinship::Direction { .. })));
    assert_eq!(on_sheets, on_properties);
    // Breaking it: read the kind through a different path on one side (the
    // uuid on the sheet, the title on the property) and the two answers differ.
}

#[test]
fn properties_of_two_supports_are_not_siblings() {
    let a = KinFacts {
        uri: "eml:///resqml22.ContinuousProperty(11111111-1111-4111-8111-111111111111)",
        support: "eml:///resqml22.WellboreFrameRepresentation(aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa)",
        indexable: "nodes",
        kind: Some("k"),
        uom: Some("m"),
        value_count_per_element: &[1],
        facets: &[],
        time: None,
    };
    let other_uri = "eml:///resqml22.ContinuousProperty(22222222-2222-4222-8222-222222222222)";
    let on_another_support = KinFacts {
        uri: other_uri,
        support: "eml:///resqml22.WellboreFrameRepresentation(bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb)",
        ..a.clone()
    };
    assert_eq!(kinship(&a, &on_another_support), None);
    // A bare uuid and the uri of the same object name the same support.
    let by_uuid = KinFacts {
        uri: other_uri,
        support: "AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA",
        ..a.clone()
    };
    assert_eq!(kinship(&a, &by_uuid), Some(Kinship::SameKind));
    // The same property is not its own sibling.
    assert_eq!(kinship(&a, &a), None);
    // Breaking it: skip the support comparison and the first answer is `SameKind`.
}

// ---------------------------------------------------------------------------
// F11: shape candidates
// ---------------------------------------------------------------------------

#[test]
fn detection_reads_no_array() {
    // The function takes no array source: nothing can be read (the signature
    // is the guarantee). A matrix, a scalar, a radius with its angles, sectors.
    let specs = [
        Spec::new(P1, &[81, 81], "ohm.m"),
        Spec::new(P2, &[1], "ohm.m"),
        Spec::new(P3, &[16], "m"),
        Spec::new(P4, &[16], "rad"),
        Spec::new("55555555-5555-4555-8555-555555555555", &[16], "ohm.m"),
    ];
    let (store, index) = built(&specs);
    let found = index.shape_candidates(&format!("eml:///{FRAME_TYPE}({FRAME})"));
    let uri = |uuid: &str| uri_of(&store, uuid);

    assert_eq!(found.len(), 4, "the angles are the axis of a pair, not a layer: {found:?}");
    assert_eq!(
        candidate(&found, &uri(P1)),
        Some(&ShapeCandidate {
            uri: uri(P1),
            role: CandidateRole::Matrix { dims: vec![81, 81] },
            needs: vec![CandidateNeed::Axes],
        })
    );
    assert_eq!(
        candidate(&found, &uri(P2)).map(|candidate| &candidate.role),
        Some(&CandidateRole::Scalar)
    );
    assert_eq!(
        candidate(&found, &uri(P3)),
        Some(&ShapeCandidate {
            uri: uri(P3),
            role: CandidateRole::Polar { angles: Some(uri(P4)) },
            needs: vec![CandidateNeed::AngleConvention],
        })
    );
    assert!(candidate(&found, &uri(P4)).is_none());
    assert_eq!(
        candidate(&found, &uri("55555555-5555-4555-8555-555555555555")).map(|candidate| &candidate.role),
        Some(&CandidateRole::Sectors { count: 16 })
    );
    // Breaking it: classify the `[16]` in radians as `Sectors` and the count
    // of candidates is 5.
}

#[test]
fn several_angle_siblings_are_the_callers_choice() {
    let (store, index) = built(&[
        Spec::new(P1, &[16], "m"),
        Spec::new(P2, &[16], "rad"),
        Spec::new(P3, &[16], "rad"),
    ]);
    let found = index.shape_candidates(&format!("eml:///{FRAME_TYPE}({FRAME})"));
    let radius = candidate(&found, &uri_of(&store, P1)).expect("the radius is a candidate");
    assert_eq!(radius.role, CandidateRole::Polar { angles: None });
    assert!(radius.needs.contains(&CandidateNeed::AngleConvention));
    assert!(
        radius
            .needs
            .iter()
            .any(|need| matches!(need, CandidateNeed::ChooseAngles { candidates } if candidates.len() == 2))
    );
    // Breaking it: take the first angle sibling and `angles` is `Some`.
}

#[test]
fn a_matrix_on_a_grid_is_not_a_candidate() {
    // A `[81, 81]` property of a 2D grid is not a log along a well.
    let mut spec = Spec::new(P1, &[81, 81], "ohm.m");
    spec.support_type = GRID_TYPE;
    let (_store, index) = built(&[spec]);
    assert!(index.shape_candidates(&format!("eml:///{GRID_TYPE}({FRAME})")).is_empty());
    // Breaking it: remove the filter on the support type and the matrix is found.
}

#[test]
fn a_convention_b_matrix_needs_no_axes() {
    // EML TRG v2.3 §11.11 p. 294: the `qualifier` facet holding the uri of a
    // table says where the axes are.
    let mut spec = Spec::new(P1, &[81, 81], "ohm.m");
    spec.facets = vec![facet(
        eml23::FacetKind::Qualifier,
        "eml:///eml23.ColumnBasedTable(00000000-0000-0000-0000-000000000009)",
    )];
    let (store, index) = built(&[spec]);
    let found = index.shape_candidates(&format!("eml:///{FRAME_TYPE}({FRAME})"));
    let matrix = candidate(&found, &uri_of(&store, P1)).expect("a candidate");
    assert_eq!(matrix.role, CandidateRole::Matrix { dims: vec![81, 81] });
    assert!(matrix.needs.is_empty(), "{:?}", matrix.needs);
    // Breaking it: ignore the facet and the need is `Axes`.
}
