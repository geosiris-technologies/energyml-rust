//! `along_well::detect`: what a frame offers a geosteering section, from the sheets of the package index.
//!
//! RESQML TRG 2.2 section 7.1 p. 222: the value count per element, slowest dimension first. The package
//! is built by hand in an empty store; nothing reads a file.

use std::sync::Arc;

use energyml_core::xml::XmlText;
use energyml_domain::package_index::PackageIndex;
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_2 as resqml22;
use energyml_section::along_well::{CandidateNeedAsk, offers};
use energyml_storage::EpcStore;

use super::{FRAME, frame_uri};

const MATRIX: &str = "11111111-1111-4111-8111-111111111111";
const SCALAR: &str = "22222222-2222-4222-8222-222222222222";
const RADIUS: &str = "33333333-3333-4333-8333-333333333333";
const ANGLE: &str = "44444444-4444-4444-8444-444444444444";
const CUBE: &str = "55555555-5555-4555-8555-555555555555";

fn property(uuid: &str, counts: &[i64], uom: &str) -> resqml22::ContinuousProperty {
    resqml22::ContinuousProperty {
        uuid: uuid.to_string(),
        citation: eml23::Citation {
            title: format!("property {}", &uuid[..2]),
            ..Default::default()
        },
        indexable_element: eml23::IndexableElement::Nodes,
        value_count_per_indexable_element: counts.to_vec(),
        property_kind: eml23::DataObjectReference {
            uuid: "dddddddd-dddd-4ddd-8ddd-dddddddddddd".to_string(),
            qualified_type: "eml23.PropertyKind".to_string(),
            title: "a kind".to_string(),
            ..Default::default()
        },
        supporting_representation: eml23::DataObjectReference {
            uuid: FRAME.to_string(),
            qualified_type: "resqml22.WellboreFrameRepresentation".to_string(),
            title: "a frame".to_string(),
            ..Default::default()
        },
        uom: eml23::UnitOfMeasureExt::from_xml_text(uom).expect("a unit"),
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

fn store_of(properties: Vec<resqml22::ContinuousProperty>) -> EpcStore {
    let bytes = energyml_storage::extract::empty_package(None).expect("an empty package is written");
    let mut store = EpcStore::from_reader(
        Box::new(std::io::Cursor::new(bytes)),
        "built.epc",
        Arc::new(energyml_schemas::all()),
    )
    .expect("it opens");
    for property in properties {
        store.put(Box::new(property)).expect("the package takes it");
    }
    store
}

fn uri_in(store: &EpcStore, uuid: &str) -> String {
    store
        .find_by_uuid(uuid)
        .expect("the object is in the store")
        .uri
        .clone()
}

/// A matrix of two dimensions per node is a curtain, and says what a view must still be told (its axes);
/// a radius with its angles is a boundary pair; a scalar, the angles on their own and a matrix of **three**
/// dimensions are neither. No array is read: the function takes no array source.
///
/// To break it: keep the matrix of three dimensions (the cube is offered as a curtain).
#[test]
fn a_matrix_per_node_on_a_frame_is_offered_and_a_dod_pair_too() {
    let store = store_of(vec![
        property(MATRIX, &[81, 81], "ohm.m"),
        property(SCALAR, &[1], "ohm.m"),
        property(RADIUS, &[16], "m"),
        property(ANGLE, &[16], "rad"),
        property(CUBE, &[4, 4, 4], "ohm.m"),
    ]);
    let index = PackageIndex::build(&store, 1);
    let found = offers(&index, &frame_uri());

    assert_eq!(found.frame, frame_uri());
    assert_eq!(found.curtains.len(), 1, "{:?}", found.curtains);
    assert_eq!(found.curtains[0].0, uri_in(&store, MATRIX));
    assert!(found.curtains[0].1.contains(&CandidateNeedAsk::Axes));
    assert_eq!(found.boundaries.len(), 1);
    assert_eq!(found.boundaries[0].radius, uri_in(&store, RADIUS));
    assert_eq!(found.boundaries[0].angle, uri_in(&store, ANGLE));
    assert_eq!(
        found.trajectory, None,
        "this frame is not in the package: no reference to follow"
    );
}

/// A matrix on a grid is not a log along a well: nothing is offered.
///
/// To break it: remove the filter on the support.
#[test]
fn a_frame_the_index_does_not_know_offers_nothing() {
    let store = store_of(vec![property(MATRIX, &[81, 81], "ohm.m")]);
    let index = PackageIndex::build(&store, 1);
    let other = "eml:///resqml22.Grid2dRepresentation(f0000000-0000-4000-8000-0000000000ee)";
    let found = offers(&index, other);
    assert!(found.curtains.is_empty());
    assert!(found.boundaries.is_empty());
}
