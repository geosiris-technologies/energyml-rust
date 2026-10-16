//! An EPC opened **as a named dataspace** (`EpcStore::open_in_dataspace`,
//! `EpcStore::from_reader_in_dataspace`).
//!
//! A server holding a folder of EPCs serves each as the dataspace named after
//! its file: `eml:///dataspace('Lutetia')/resqml22.X(uuid)`. Without the name,
//! two packages served together would give two distinct objects the same uri,
//! and the identity of an object is its **full uri** (ETP v1.2 §25.3; Identifier
//! Specification v5.0 §3.4). A reference (a DOR) names no dataspace: it resolves
//! in the dataspace of the object that writes it, so the target of a reference
//! is spelled in its source's dataspace.
//!
//! The packages are fabricated here, from the contract's own model. The same
//! bytes are opened twice - under `eml:///` and under `dataspace('alpha')` - and
//! the two stores are held against each other.
//!
//! To make these fail: spell one of the uris an EPC builds (`reconcile_uris`,
//! `classify`, `summary_of`, the absent node of `graph::build`) without the
//! dataspace, or let `EpcStore::locate` skip `in_this_dataspace`.

use std::collections::BTreeSet;
use std::io::{Cursor, Write};
use std::sync::Arc;

use energyml_core::registry::Registry;
use energyml_storage::contract::{self, ABSENT, COLUMN, DANGLING, FEATURE, HORIZON_1, ModelObject};
use energyml_storage::contract_witsml::{self as witsml, CHANNEL_SHARED};
use energyml_storage::{Context, EnergymlStorage, EpcStore, Error, Scope};

const ALPHA: &str = "alpha";
const ALPHA_URI: &str = "eml:///dataspace('alpha')";
const PREFIX: &str = "eml:///dataspace('alpha')/";

fn run<F: std::future::Future>(future: F) -> F::Output {
    futures_executor::block_on(future)
}

fn registry() -> Arc<Registry> {
    Arc::new(energyml_schemas::all())
}

/// The bytes of a package holding exactly these parts.
fn bytes_of(objects: &[ModelObject]) -> Vec<u8> {
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
        for object in objects {
            zip.start_file(object.part_name(), options).unwrap();
            zip.write_all(object.xml.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }
    bytes
}

fn default_of(bytes: &[u8]) -> EpcStore {
    EpcStore::from_reader(Box::new(Cursor::new(bytes.to_vec())), "model.epc", registry()).expect("opens")
}

fn named_of(bytes: &[u8], dataspace: &str) -> EpcStore {
    EpcStore::from_reader_in_dataspace(
        Box::new(Cursor::new(bytes.to_vec())),
        "model.epc",
        registry(),
        dataspace,
    )
    .expect("opens under a named dataspace")
}

fn model_bytes() -> Vec<u8> {
    bytes_of(&contract::model().objects)
}

fn named() -> EpcStore {
    named_of(&model_bytes(), ALPHA)
}

fn uris(store: &EpcStore) -> Vec<String> {
    store.resources().into_iter().map(|s| s.uri).collect()
}

fn uuids(store: &EpcStore) -> BTreeSet<String> {
    store.resources().into_iter().map(|s| s.uuid).collect()
}

#[test]
fn the_same_bytes_have_the_same_uuids_and_different_uris() {
    let bytes = model_bytes();
    let (plain, alpha) = (default_of(&bytes), named_of(&bytes, ALPHA));

    assert_eq!(uuids(&plain), uuids(&alpha));
    assert_eq!(uris(&alpha).len(), 5);
    for (plain, alpha) in uris(&plain).iter().zip(uris(&alpha).iter()) {
        assert!(plain.starts_with("eml:///resqml22."), "{plain}");
        // The named uri is the default one with the dataspace in front.
        assert_eq!(*alpha, plain.replacen("eml:///", PREFIX, 1));
    }
}

#[test]
fn a_uri_of_one_dataspace_is_not_found_in_the_other() {
    let bytes = model_bytes();
    let (plain, alpha) = (default_of(&bytes), named_of(&bytes, ALPHA));
    let elsewhere = named_of(&bytes, "beta");

    let refused = |error: &Error| matches!(error, Error::NotFound(_));
    for (store, other) in [(&plain, &alpha), (&alpha, &plain), (&alpha, &elsewhere)] {
        let theirs = uris(other);
        // The plural reads, one answer per uri, every one a refusal.
        for object in store.get_objects(&theirs) {
            assert!(
                matches!(object, Err(Error::NotFound(_))),
                "a foreign uri reads nothing"
            );
        }
        for header in store.headers(&theirs) {
            assert!(refused(&header.expect_err("a foreign uri has no header")));
        }
        for uri in &theirs {
            assert!(
                refused(&store.resolve(uri).expect_err("a foreign uri resolves to nothing")),
                "{uri}"
            );
            assert!(
                refused(&store.resource(uri).expect_err("nor as a resource")),
                "{uri}"
            );
        }
    }
    // Their own uris read, the named one included.
    let own = uris(&alpha);
    assert!(alpha.get_objects(&own).iter().all(|object| object.is_ok()));
    assert!(alpha.headers(&own).iter().all(|header| header.is_ok()));
    // And the dataspace-less form of a named store's uri is a foreign uri.
    let bare = format!("eml:///resqml22.BoundaryFeature({FEATURE})");
    assert!(refused(
        &alpha.resolve(&bare).expect_err("no dataspace on a named store")
    ));
    assert!(plain.resolve(&bare).is_ok());
}

#[test]
fn a_bare_uuid_is_accepted_at_the_entrance_and_answers_the_full_uri() {
    let alpha = named();
    let summary = alpha.resolve(FEATURE).expect("a bare uuid is accepted");
    assert_eq!(
        summary.uri,
        format!("eml:///dataspace('alpha')/resqml22.BoundaryFeature({FEATURE})")
    );
    assert_eq!(alpha.lookup_uuid(FEATURE).expect("looked up").uri, summary.uri);
}

#[test]
fn the_dataspace_is_the_named_one_and_keeps_its_title() {
    let bytes = model_bytes();
    let alpha = named_of(&bytes, ALPHA);
    assert_eq!(alpha.dataspace().uri, ALPHA_URI);
    // The title is the file name, apart from the dataspace's path.
    assert_eq!(alpha.dataspace().name, "model.epc");
    let listed = run(alpha.dataspaces()).expect("lists");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].uri, ALPHA_URI);
    assert_eq!(listed[0].name, "model.epc");

    // The default is unchanged.
    assert_eq!(default_of(&bytes).dataspace().uri, "eml:///");
    // A path of several segments is a dataspace path (ETP v1.2 §25.3.2).
    assert_eq!(
        named_of(&bytes, "projets/demo").dataspace().uri,
        "eml:///dataspace('projets/demo')"
    );
}

#[test]
fn a_reference_targets_the_dataspace_of_its_source() {
    let alpha = named();
    let ends = |edges: &[energyml_storage::Edge]| -> Vec<(String, String)> {
        edges.iter().map(|e| (e.from.clone(), e.to.clone())).collect()
    };
    let uri = |uuid: &str| {
        alpha
            .resources()
            .into_iter()
            .find(|s| s.uuid == uuid)
            .map(|s| s.uri)
            .expect("held")
    };

    // references(): the four DORs of the model, the one to the absent object too.
    let written = run(alpha.references(&[])).expect("references");
    assert_eq!(written.len(), 4);
    for edge in &written {
        assert!(edge.from.starts_with(PREFIX), "{}", edge.from);
        assert!(
            edge.to.starts_with(PREFIX),
            "a reference's target is spelled in its source's dataspace: {}",
            edge.to
        );
    }
    let absent = written
        .iter()
        .find(|e| e.from == uri(DANGLING))
        .expect("the dangling reference");
    assert_eq!(
        absent.to,
        format!("eml:///dataspace('alpha')/resqml22.BoundaryFeature({ABSENT})")
    );
    assert!(ends(&written).contains(&(uri(COLUMN), uri(HORIZON_1))));

    // edges(): the stated ones, whose target is held.
    let stated = run(alpha.edges(&[])).expect("edges");
    assert_eq!(stated.len(), 3);
    assert!(
        ends(&stated)
            .iter()
            .all(|(from, to)| from.starts_with(PREFIX) && to.starts_with(PREFIX))
    );
    assert!(ends(&stated).contains(&(uri(HORIZON_1), uri(FEATURE))));

    // graph(): a node for the absent target, in the dataspace, marked absent.
    let graph = run(alpha.graph(&[])).expect("graph");
    assert!(
        graph.nodes.iter().all(|node| node.uri.starts_with(PREFIX)),
        "{:?}",
        graph.nodes
    );
    let gone = graph
        .nodes
        .iter()
        .find(|node| node.uuid == ABSENT)
        .expect("the absent object has a node");
    assert!(!gone.present);
    assert_eq!(gone.uri, absent.to);
    for edge in &graph.edges {
        assert!(
            graph.nodes.iter().any(|node| node.uri == edge.to),
            "an edge into nothing: {}",
            edge.to
        );
    }

    // The default dataspace is unchanged by it.
    let plain = default_of(&model_bytes());
    let written = run(plain.references(&[])).expect("references");
    assert!(
        written
            .iter()
            .all(|e| e.to.starts_with("eml:///resqml22.") && e.from.starts_with("eml:///resqml22."))
    );
}

#[test]
fn discovery_answers_the_dataspace_and_refuses_another() {
    let alpha = named();
    let found = run(alpha.resources_in(&Context::dataspace(ALPHA_URI))).expect("the store's own dataspace");
    assert_eq!(found.resources.len(), 5);
    assert!(found.resources.iter().all(|s| s.uri.starts_with(PREFIX)));

    for other in ["eml:///", "eml:///dataspace('beta')"] {
        let refused = run(alpha.resources_in(&Context::dataspace(other)));
        assert!(matches!(refused, Err(Error::NotFound(_))), "{other}: {refused:?}");
    }
    // From an object: the walk starts at an object of this dataspace.
    let column = alpha.resolve(COLUMN).expect("held").uri.clone();
    let reached = run(alpha.resources_in(&Context::of(column, Scope::Targets))).expect("walks");
    assert!(reached.resources.iter().any(|s| s.uuid == HORIZON_1));
    // And the default dataspace refuses the named one.
    let plain = default_of(&model_bytes());
    let refused = run(plain.resources_in(&Context::dataspace(ALPHA_URI)));
    assert!(matches!(refused, Err(Error::NotFound(_))), "{refused:?}");
}

#[test]
fn an_object_put_in_a_named_dataspace_is_in_it() {
    use energyml_storage::EnergymlWriter;
    let mut alpha = named();
    let object = contract::feature(contract::NEW_1, "new").read(&registry());
    let put = run(alpha.put_objects(vec![object], false));
    assert!(put.iter().all(|one| one.is_ok()), "{put:?}");
    let created = alpha.resolve(contract::NEW_1).expect("held now").uri.clone();
    assert_eq!(
        created,
        format!(
            "eml:///dataspace('alpha')/resqml22.BoundaryFeature({})",
            contract::NEW_1
        )
    );
    // The writer's door takes the dataspace's own uri and refuses the others.
    use energyml_storage::EnergymlDataspaces;
    assert!(alpha.open_dataspace(ALPHA_URI).is_ok());
    assert!(matches!(alpha.open_dataspace("eml:///"), Err(Error::NotFound(_))));
}

#[test]
fn a_name_the_uri_refuses_is_refused_by_name() {
    let bytes = model_bytes();
    for name in ["", "it's", "a')/b", "x'"] {
        let opened = EpcStore::from_reader_in_dataspace(
            Box::new(Cursor::new(bytes.clone())),
            "model.epc",
            registry(),
            name,
        );
        match opened.err() {
            Some(Error::BadUri { reason, .. }) => {
                assert!(
                    reason.contains("not a usable dataspace name"),
                    "{name:?}: {reason}"
                )
            }
            other => panic!("{name:?} should be a BadUri, got {other:?}"),
        }
    }
    let dir = std::env::temp_dir().join(format!("energyml-named-dataspace-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a temporary folder");
    let path = dir.join("model.epc");
    std::fs::write(&path, &bytes).expect("writes");
    assert!(matches!(
        EpcStore::open_in_dataspace(&path, registry(), "").err(),
        Some(Error::BadUri { .. })
    ));
    let opened =
        EpcStore::open_in_dataspace(&path, registry(), "model").expect("a file as its own dataspace");
    assert_eq!(opened.dataspace().uri, "eml:///dataspace('model')");
    assert!(
        uris(&opened)
            .iter()
            .all(|uri| uri.starts_with("eml:///dataspace('model')/"))
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// The contract, played on a named dataspace
// ---------------------------------------------------------------------------

#[test]
fn every_group_of_the_contract_on_a_named_dataspace() {
    let bytes = model_bytes();
    let make = || named_of(&bytes, "projets/demo");
    run(contract::all(&make, &registry()));
    // The groups that name another dataspace refuse it by its own rule; the
    // transaction's and the door's own-dataspace rules read the store's.
    run(contract::dataspace_door(&make, &registry()));
    run(contract::deletions(&make));
}

#[test]
fn the_containment_groups_on_a_named_dataspace() {
    let bytes = bytes_of(&witsml::model_witsml().parts);
    let make = || named_of(&bytes, ALPHA);
    run(witsml::contents(&make));
    run(witsml::containment_edges(&make));
    run(witsml::contained_reads(&make));
    run(witsml::contained_references(&make));
    run(witsml::contained_writes(&make, &registry()));

    // A contained object lives in its container's dataspace.
    let store = make();
    let contents = store.contents().expect("contents");
    let channel = contents
        .objects
        .iter()
        .find(|s| s.uuid == CHANNEL_SHARED)
        .expect("the shared channel");
    assert!(channel.uri.starts_with(PREFIX), "{}", channel.uri);
    assert!(
        store
            .get_objects(std::slice::from_ref(&channel.uri))
            .remove(0)
            .is_ok()
    );
    let foreign = channel.uri.replacen(PREFIX, "eml:///", 1);
    assert!(matches!(
        store.get_objects(&[foreign]).remove(0).err(),
        Some(Error::NotFound(_))
    ));
}
