//! The writing traits on an EPC: [`EnergymlWriter`], [`EnergymlDataspaces`],
//! [`EnergymlChanges`], and what [`EnergymlStorage`] gained beside them
//! (`get_raw`, `capabilities`).
//!
//! What is checked is the contract the design notes set
//! (`ecriture-objets-et-tableaux.md` §3.4 and §6): a transaction is an object
//! that only its own holder ends; what a file cannot do is refused **by name**,
//! never answered with an empty success; the dataspace door reaches the one
//! dataspace an EPC is and nothing else.
//!
//! To make them fail: let `commit` accept any transaction, let `put_arrays`
//! write inside a transaction, or let `dataspace` open whatever URI it is given.

use std::io::{Cursor, Write};
use std::sync::Arc;

use energyml_core::registry::{DynObject, Registry};
use energyml_storage::array::{ArrayData, ArrayLocation, ArrayMetadata, ArrayValues};
use energyml_storage::changes::Since;
use energyml_storage::raw::RawFormat;
use energyml_storage::{
    EnergymlChanges, EnergymlDataspaces, EnergymlStorage, EnergymlWriter, EpcStore, Error, Transaction,
    TransactionRequest,
};

const RESQML: &str = "http://www.energistics.org/energyml/data/resqmlv2";
const EML: &str = "http://www.energistics.org/energyml/data/commonv2";

const HELD: &str = "7d000000-0000-4000-8000-00000000000a";
const NEW: &str = "7d000000-0000-4000-8000-00000000000b";

fn run<F: std::future::Future>(future: F) -> F::Output {
    futures_executor::block_on(future)
}

fn registry() -> Arc<Registry> {
    Arc::new(energyml_schemas::all())
}

fn feature(uuid: &str, title: &str) -> String {
    format!(
        "<resqml:BoundaryFeature xmlns:resqml=\"{RESQML}\" xmlns:eml=\"{EML}\" \
         uuid=\"{uuid}\" schemaVersion=\"2.2\">\
         <eml:Citation><eml:Title>{title}</eml:Title>\
         <eml:Originator>test</eml:Originator>\
         <eml:Creation>2026-10-14T00:00:00Z</eml:Creation>\
         <eml:Format>test</eml:Format></eml:Citation>\
         <resqml:IsWellKnown>false</resqml:IsWellKnown>\
         </resqml:BoundaryFeature>"
    )
}

/// A package in memory holding the given features.
fn package(uuids: &[&str]) -> EpcStore {
    let mut bytes = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(Cursor::new(&mut bytes));
        let options: zip::write::SimpleFileOptions = zip::write::FileOptions::default();
        zip.start_file("[Content_Types].xml", options).unwrap();
        zip.write_all(
            b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
              <Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
              <Default Extension=\"rels\" \
              ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
              </Types>",
        )
        .unwrap();
        for uuid in uuids {
            zip.start_file(format!("BoundaryFeature_{uuid}.xml"), options)
                .unwrap();
            zip.write_all(feature(uuid, "a boundary").as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }
    EpcStore::from_reader(Box::new(Cursor::new(bytes)), "writer.epc", registry()).expect("opens")
}

/// An object the store does not hold yet.
fn foreign(uuid: &str) -> Box<dyn DynObject> {
    let other = package(&[uuid]);
    let uri = uri_of(&other, uuid);
    other.get_object(&uri).expect("readable")
}

fn uri_of(store: &EpcStore, uuid: &str) -> String {
    store
        .resources()
        .into_iter()
        .find(|r| r.uuid.eq_ignore_ascii_case(uuid))
        .map(|r| r.uri)
        .unwrap_or_else(|| panic!("{uuid} is in the package"))
}

fn held(store: &EpcStore, uuid: &str) -> bool {
    store
        .resources()
        .iter()
        .any(|r| r.uuid.eq_ignore_ascii_case(uuid))
}

fn unsupported<T: std::fmt::Debug>(result: &energyml_storage::Result<T>) -> bool {
    matches!(result, Err(Error::Unsupported { store: "EPC", .. }))
}

#[test]
fn objects_are_put_and_deleted_through_the_trait_under_their_full_uri() {
    let mut store = package(&[HELD]);
    let put = run(store.put_objects(vec![foreign(NEW)], false));
    let [Ok(put)] = put.as_slice() else {
        panic!("one object, one success: {put:?}")
    };
    assert_eq!(
        put.uri,
        uri_of(&store, NEW),
        "the full uri the object is now known under"
    );
    assert!(put.created_contained.is_empty() && put.deleted_contained.is_empty());

    let held_uri = uri_of(&store, HELD);
    let deleted = run(store.delete_objects(std::slice::from_ref(&held_uri), false));
    let [Ok(deleted)] = deleted.as_slice() else {
        panic!("one uri, one answer: {deleted:?}")
    };
    assert_eq!(deleted, &vec![held_uri]);
    assert!(!held(&store, HELD) && held(&store, NEW));
}

#[test]
fn pruning_contained_objects_is_refused_by_name_and_writes_nothing() {
    let mut store = package(&[HELD]);
    let put = run(store.put_objects(vec![foreign(NEW)], true));
    assert!(unsupported(&put[0]), "{put:?}");
    assert!(!held(&store, NEW), "a refused write writes nothing");

    let uri = uri_of(&store, HELD);
    let deleted = run(store.delete_objects(&[uri], true));
    assert!(unsupported(&deleted[0]), "{deleted:?}");
    assert!(held(&store, HELD));
}

#[test]
fn a_transaction_is_an_object_only_its_holder_ends() {
    let mut store = package(&[HELD]);
    assert_eq!(store.open_transaction(), None);
    let transaction = run(store.begin(TransactionRequest::new().with_message("one"))).expect("opens");
    assert_eq!(store.open_transaction().as_ref(), Some(&transaction));
    assert_eq!(transaction.request.message, "one");

    let second = run(store.begin(TransactionRequest::new()));
    assert!(matches!(second, Err(Error::Transaction { .. })), "{second:?}");

    let stranger = Transaction::new("not-this-one", TransactionRequest::new());
    let refused = run(store.commit(&stranger));
    assert!(matches!(refused, Err(Error::Transaction { .. })), "{refused:?}");
    assert!(
        store.open_transaction().is_some(),
        "a refused commit leaves it open"
    );

    let put = run(store.put_objects(vec![foreign(NEW)], false));
    assert!(put[0].is_ok(), "{put:?}");
    assert!(held(&store, NEW));
    run(store.rollback(&transaction)).expect("its holder ends it");
    assert!(!held(&store, NEW), "a rollback undoes what the transaction wrote");
    assert_eq!(store.open_transaction(), None);

    let again = run(store.commit(&transaction));
    assert!(
        matches!(again, Err(Error::Transaction { .. })),
        "an ended transaction cannot end again"
    );
}

#[test]
fn a_committed_transaction_keeps_what_it_wrote() {
    let mut store = package(&[HELD]);
    let transaction = run(store.begin(TransactionRequest::new())).expect("opens");
    let put = run(store.put_objects(vec![foreign(NEW)], false));
    assert!(put[0].is_ok(), "{put:?}");
    run(store.commit(&transaction)).expect("commits");
    assert!(held(&store, NEW));
    assert_eq!(store.open_transaction(), None);
}

#[test]
fn a_reading_transaction_refuses_every_write() {
    let mut store = package(&[HELD]);
    let transaction = run(store.begin(TransactionRequest::new().read_only())).expect("opens");
    let put = run(store.put_objects(vec![foreign(NEW)], false));
    assert!(matches!(put[0], Err(Error::Transaction { .. })), "{put:?}");
    let uri = uri_of(&store, HELD);
    let deleted = run(store.delete_objects(&[uri], false));
    assert!(
        matches!(deleted[0], Err(Error::Transaction { .. })),
        "{deleted:?}"
    );
    run(store.rollback(&transaction)).expect("ends");
    assert!(held(&store, HELD) && !held(&store, NEW));
}

#[test]
fn a_transaction_over_another_dataspace_is_refused() {
    let mut store = package(&[HELD]);
    let refused = run(store.begin(TransactionRequest::new().over(["eml:///dataspace('elsewhere')"])));
    assert!(matches!(refused, Err(Error::NotFound(_))), "{refused:?}");
    assert_eq!(store.open_transaction(), None);
    let own = run(store.begin(TransactionRequest::new().over(["eml:///"])));
    assert!(own.is_ok(), "its own dataspace: {own:?}");
}

#[test]
fn what_an_epc_cannot_do_with_an_array_is_refused_by_name() {
    let mut store = package(&[HELD]);
    let array = ArrayData {
        metadata: ArrayMetadata::new(vec![2], "double"),
        values: ArrayValues::F64(vec![1.0, 2.0]),
    };
    let whole = ArrayLocation {
        file: "data.h5".into(),
        path_in_file: "/a".into(),
        ..ArrayLocation::default()
    };
    let part = ArrayLocation {
        start: vec![0],
        count: vec![1],
        ..whole.clone()
    };
    let written = run(store.put_arrays(&[(part, array.clone())]));
    assert!(unsupported(&written[0]), "part of an array: {written:?}");

    let declared = run(store.declare_arrays(&[(whole.clone(), array.metadata.clone())]));
    assert!(unsupported(&declared[0]), "declared before written: {declared:?}");

    let transaction = run(store.begin(TransactionRequest::new())).expect("opens");
    let inside = run(store.put_arrays(&[(whole, array)]));
    assert!(
        unsupported(&inside[0]),
        "inside a transaction a rollback would not undo it: {inside:?}"
    );
    run(store.rollback(&transaction)).expect("ends");
}

#[test]
fn the_dataspace_door_reaches_the_one_dataspace_an_epc_is() {
    let mut store = package(&[HELD]);
    let listed = run(store.list_dataspaces()).expect("lists");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].uri, "eml:///");

    let created = run(store.put_dataspaces(vec![energyml_storage::Dataspace::new(
        "eml:///dataspace('x')",
        "x",
    )]));
    assert!(unsupported(&created[0]), "{created:?}");
    let deleted = run(store.delete_dataspaces(&["eml:///".to_string()]));
    assert!(unsupported(&deleted[0]), "{deleted:?}");

    for other in [
        "eml:///dataspace('elsewhere')",
        "eml:///resqml22.BoundaryFeature(7d000000-0000-4000-8000-00000000000a)",
    ] {
        assert!(
            matches!(store.open_dataspace(other), Err(Error::NotFound(_))),
            "{other} is not a dataspace of this store"
        );
    }
    let writer = store.open_dataspace("eml:///").expect("its own");
    let put = run(writer.put_objects(vec![foreign(NEW)], false));
    assert!(put[0].is_ok(), "{put:?}");
    assert!(held(&store, NEW));
}

#[test]
fn an_object_as_bytes_is_the_xml_of_its_part_and_json_is_refused() {
    let store = package(&[HELD]);
    let uri = uri_of(&store, HELD);
    let raw = run(store.get_raw(std::slice::from_ref(&uri), RawFormat::Xml));
    let [Ok(raw)] = raw.as_slice() else {
        panic!("{raw:?}")
    };
    assert_eq!(raw.uri, uri);
    assert_eq!(raw.format, RawFormat::Xml);
    assert_eq!(
        raw.bytes,
        feature(HELD, "a boundary").into_bytes(),
        "the part, byte for byte"
    );

    let json = run(store.get_raw(&[uri], RawFormat::Json));
    assert!(unsupported(&json[0]), "{json:?}");
}

#[test]
fn an_epc_says_what_it_offers() {
    let store = package(&[]);
    let offered = store.capabilities();
    assert!(offered.writes && offered.transactions && offered.changes);
    assert_eq!(offered.raw_formats, vec![RawFormat::Xml]);
    assert!(!offered.several_dataspaces && !offered.subarrays && !offered.uninitialized_arrays);
    assert_eq!(offered.deletions_kept_for, None);
    assert_eq!(offered.store_times, energyml_storage::StoreTimes::NotKept);
    // The EPC states the kind of every edge: ETP 1.2 section 23.34.13 requires
    // one on each `Edge`, and the rules of `kinds` give it (a rule, a schema
    // fact, or primary by default: the maintainer's decision of 23/10/2026).
    assert!(offered.edge_kinds);
    assert_eq!(
        offered.edge_kind_coverage,
        energyml_storage::EdgeKindCoverage::All
    );
    assert!(offered.edge_kind_coverage.covers("eml23") && offered.edge_kind_coverage.covers("prodml23"));
}

#[test]
fn deletions_are_not_kept_by_an_epc_and_it_says_so() {
    let store = package(&[HELD]);
    let deleted = run(store.deleted_resources(&energyml_storage::DeletedFilter::of("eml:///")));
    assert!(unsupported(&deleted), "never `Ok(vec![])`: {deleted:?}");
}

#[test]
fn the_change_feed_names_what_the_writer_wrote() {
    let mut store = package(&[HELD]);
    let before = EnergymlChanges::revision(&store);
    let put = run(store.put_objects(vec![foreign(NEW)], false));
    assert!(put[0].is_ok(), "{put:?}");
    assert!(EnergymlChanges::revision(&store) > before);
    match EnergymlChanges::changes_since(&store, before) {
        Since::Exact(change) => assert!(
            change
                .objects
                .iter()
                .any(|object| object.uuid.eq_ignore_ascii_case(NEW)),
            "{change:?}"
        ),
        Since::Everything => panic!("one put is not beyond what the journal can say"),
    }
}

/// An interpretation of a feature the package does not hold.
fn interpretation(uuid: &str, feature_uuid: &str) -> String {
    format!(
        "<resqml:HorizonInterpretation xmlns:resqml=\"{RESQML}\" xmlns:eml=\"{EML}\" \
         uuid=\"{uuid}\" schemaVersion=\"2.2\">\
         <eml:Citation><eml:Title>a horizon</eml:Title>\
         <eml:Originator>test</eml:Originator>\
         <eml:Creation>2026-10-14T00:00:00Z</eml:Creation>\
         <eml:Format>test</eml:Format></eml:Citation>\
         <resqml:Domain>depth</resqml:Domain>\
         <resqml:InterpretedFeature>\
         <eml:Uuid>{feature_uuid}</eml:Uuid>\
         <eml:QualifiedType>resqml22.BoundaryFeature</eml:QualifiedType>\
         <eml:Title>a boundary</eml:Title>\
         </resqml:InterpretedFeature>\
         </resqml:HorizonInterpretation>"
    )
}

/// `references` is what the objects write, `edges` what the store holds both
/// ends of (design notes, `deux-sources-un-contrat.md` §3): a reference to a
/// feature the package does not hold is in the first and not in the second.
#[test]
fn a_reference_to_an_absent_object_is_written_but_not_an_edge() {
    const HORIZON: &str = "7d000000-0000-4000-8000-0000000000c1";
    let mut store = package(&[HELD]);
    let interpretation = {
        let mut bytes = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(Cursor::new(&mut bytes));
            let options: zip::write::SimpleFileOptions = zip::write::FileOptions::default();
            zip.start_file("[Content_Types].xml", options).unwrap();
            zip.write_all(
                b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
                  <Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
                  <Default Extension=\"rels\" \
                  ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
                  </Types>",
            )
            .unwrap();
            zip.start_file(format!("HorizonInterpretation_{HORIZON}.xml"), options)
                .unwrap();
            zip.write_all(interpretation(HORIZON, NEW).as_bytes()).unwrap();
            zip.finish().unwrap();
        }
        let other =
            EpcStore::from_reader(Box::new(Cursor::new(bytes)), "other.epc", registry()).expect("opens");
        let uri = uri_of(&other, HORIZON);
        other.get_object(&uri).expect("readable")
    };
    let put = run(store.put_objects(vec![interpretation], false));
    assert!(put[0].is_ok(), "{put:?}");

    let written = run(store.references(&[])).expect("references");
    assert!(
        written.iter().any(|edge| edge.to.contains(NEW)),
        "the reference to the absent feature is written: {written:?}"
    );
    let stated = run(store.edges(&[])).expect("edges");
    assert!(
        !stated.iter().any(|edge| edge.to.contains(NEW)),
        "no edge leaves what the store holds: {stated:?}"
    );
}
