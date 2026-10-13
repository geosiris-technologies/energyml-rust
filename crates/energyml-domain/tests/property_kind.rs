//! Kind resolution, against the packages and against the shipped dictionary.
//!
//! The acceptance criteria of the design (§11): the local kinds of
//! `80wells` walk up to the root through the dictionary, and Volve's resolve
//! without needing one at all. Plus the two things a resolver must never do —
//! pick between two kinds sharing a title, and answer "not found" without
//! saying where it looked.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use energyml_core::registry::Registry;
use energyml_domain::property::{self, PropertyKindRef};
use energyml_domain::property_kind::{self, Dictionary, Found, Resolver};
use energyml_storage::EpcStore;

/// Every schema version the workspace generates.
///
/// One line rather than a hand-kept subset: a list written out here goes stale
/// the day a release is added, and nothing would notice — the suite would keep
/// passing while quietly never covering the new version. `energyml-schemas`
/// holds the one list, and its own test fails when it disagrees with
/// `codegen.toml`.
fn registry() -> Arc<Registry> {
    Arc::new(energyml_schemas::all())
}

fn corpus() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata");
    let mut found = Vec::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("epc")) {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

fn skip_if_empty(files: &[PathBuf], what: &str) -> bool {
    if files.is_empty() {
        eprintln!("skipped: no EPC under testdata/ ({what})");
        return true;
    }
    false
}

fn properties(store: &EpcStore) -> Vec<(String, Box<dyn energyml_core::registry::DynObject>)> {
    let mut out = Vec::new();
    for summary in store.resources() {
        if !summary.qualified_type.ends_with("Property") {
            continue;
        }
        if let Ok(object) = store.get_object(&summary.uri) {
            out.push((summary.uuid.clone(), object));
        }
    }
    out
}

#[test]
fn every_kind_of_the_corpus_resolves_or_says_precisely_why_not() {
    let files = corpus();
    if skip_if_empty(&files, "every_kind_of_the_corpus_resolves_or_says_precisely_why_not") {
        return;
    }
    let registry = registry();
    let mut from_package = 0usize;
    let mut from_dictionary = 0usize;
    let mut by_name = 0usize;
    let mut refused = 0usize;

    for path in &files {
        let store = EpcStore::open(path, registry.clone()).expect("the package opens");
        let resolver = Resolver::new(&store);
        for (uuid, object) in properties(&store) {
            let read = property::read(object.as_reflect()).expect("the property reads");
            let Some(reference) = read.kind else {
                panic!("{}: {uuid} names no kind", path.display());
            };
            match resolver.resolve(&reference) {
                Ok(resolution) => {
                    assert!(
                        !resolution.chain.is_empty(),
                        "{uuid}: an empty chain is not a resolution"
                    );
                    assert!(
                        !resolution.kind().title.is_empty(),
                        "{uuid}: a kind with no title at all"
                    );
                    match resolution.kind().found_in {
                        Found::Package => from_package += 1,
                        Found::Dictionary => from_dictionary += 1,
                        Found::StandardName => by_name += 1,
                    }
                    // Whatever could not be followed has to be in words.
                    for note in &resolution.notes {
                        assert!(note.len() > 20, "a note that says nothing: {note}");
                    }
                }
                Err(e) => {
                    refused += 1;
                    let message = e.to_string();
                    // The rule this whole module exists for: a refusal names
                    // the thing, both places searched, and the way out.
                    assert!(message.contains("property kind"), "{message}");
                    assert!(
                        message.contains("package") && message.contains("dictionary"),
                        "a refusal that does not say where it looked: {message}"
                    );
                }
            }
        }
    }
    eprintln!(
        "{from_package} kinds from the package, {from_dictionary} from the dictionary, \
         {by_name} left as a standard name, {refused} refused"
    );
    assert!(from_package > 0, "the corpus carries PropertyKind objects; none was used");
}

#[test]
fn a_local_kind_walks_up_to_the_root_of_the_dictionary() {
    // `80wells` carries eight local kinds whose parents are dictionary kinds.
    // Walking from one of them to `property` is the whole point of the lot:
    // it crosses from the package into the dictionary in one chain.
    let files = corpus();
    if skip_if_empty(&files, "a_local_kind_walks_up_to_the_root_of_the_dictionary") {
        return;
    }
    let Some(path) = files.iter().find(|p| p.ends_with("80wells_surf_modified_val_color.epc")) else {
        eprintln!("skipped: 80wells is not in the corpus");
        return;
    };
    let store = EpcStore::open(path, registry()).expect("the package opens");
    let resolver = Resolver::new(&store);

    let (count, _label) = property_kind::builtin();
    assert!(count > 0, "the built-in dictionary is never empty: build.rs refuses to build without data/property_kinds.tsv");

    let mut crossed = 0usize;
    for (_, object) in properties(&store) {
        let read = property::read(object.as_reflect()).expect("it reads");
        let Some(reference) = read.kind else { continue };
        let Ok(resolution) = resolver.resolve(&reference) else {
            continue;
        };
        let from_package = resolution.chain.iter().any(|k| k.found_in == Found::Package);
        let from_dictionary = resolution.chain.iter().any(|k| k.found_in == Found::Dictionary);
        if from_package && from_dictionary {
            crossed += 1;
            assert!(
                resolution.quantity_class().is_some(),
                "a chain that reaches the dictionary reaches a quantity class: {}",
                resolution.path()
            );
        }
    }
    assert!(
        crossed > 0,
        "no chain crossed from the package into the dictionary — the two halves never met"
    );
}

#[test]
fn a_dictionary_can_be_replaced_without_touching_the_default() {
    // The third decision, tested: an application hands over its own dictionary
    // and nothing global changes.
    let mine = Dictionary::from_compact("1111\tmy own kind\t\tunitless\t0\t\n").labelled("my dictionary");
    let store = energyml_domain::source::NoObjects;
    let resolver = Resolver::with_dictionary(&store, &mine);

    let resolution = resolver
        .resolve(&PropertyKindRef::Object {
            uuid: "1111".into(),
            title: None,
        })
        .expect("it is in my dictionary");
    assert_eq!(resolution.kind().title, "my own kind");

    // The default is untouched: the same uuid is unknown there.
    let default_resolver = Resolver::new(&store);
    assert!(
        default_resolver
            .resolve(&PropertyKindRef::Object {
                uuid: "1111".into(),
                title: None,
            })
            .is_err(),
        "a dictionary passed to one resolver must not leak into the default"
    );
}

#[test]
fn the_wrong_file_is_named_rather_than_loaded_as_an_empty_dictionary() {
    let error = Dictionary::from_xml(
        r#"<?xml version="1.0"?><eml:Well xmlns:eml="x" uuid="a"><eml:Citation/></eml:Well>"#,
    )
    .expect_err("a well is not a dictionary");
    let message = error.to_string();
    assert!(message.contains("Well"), "{message}");
    assert!(message.contains("nothing was loaded"), "{message}");
}

#[test]
fn the_shipped_dictionary_holds_what_it_claims() {
    let (count, _label) = property_kind::builtin();
    assert!(count > 0, "the built-in dictionary is never empty: build.rs refuses to build without data/property_kinds.tsv");
    assert_eq!(count, 3662, "the Energistics 2.3 dictionary has 3662 kinds");
    property_kind::with_default(|dictionary| {
        let length = dictionary.by_title("length");
        assert_eq!(length.len(), 1);
        assert_eq!(length[0].quantity_class.as_deref(), Some("length"));
        // The one real ambiguity in the shipped file — the reason `by_title`
        // returns a list rather than an option.
        assert_eq!(dictionary.by_title("spring stiffness").len(), 2);
        // And the root, which is its own parent.
        let root = dictionary.by_title("property");
        assert_eq!(root.len(), 1);
        assert_eq!(root[0].parent.as_deref(), Some(root[0].uuid.as_str()));
    });
}

#[test]
fn a_search_puts_the_word_someone_typed_first() {
    // The whole value of a chooser is its first three rows. `pressure` matches
    // a large part of the dictionary; a list that opens on `absolute pressure`
    // and buries `pressure` itself is a list a person scrolls past.
    let (count, _label) = property_kind::builtin();
    assert!(count > 0, "the built-in dictionary is never empty: build.rs refuses to build without data/property_kinds.tsv");
    property_kind::with_default(|dictionary| {
        let found = dictionary.search("pressure", 20);
        assert!(found.len() > 3, "only {} match(es) for `pressure`", found.len());
        assert_eq!(
            found[0].title.to_ascii_lowercase(),
            "pressure",
            "the exact title is not first: {:?}",
            found.iter().map(|k| &k.title).take(5).collect::<Vec<_>>()
        );

        // Then the ones that *start* with it, before the ones that merely
        // contain it.
        let starts = found
            .iter()
            .position(|k| !k.title.to_ascii_lowercase().starts_with("pressure"))
            .unwrap_or(found.len());
        assert!(
            found[1..starts]
                .iter()
                .all(|k| k.title.to_ascii_lowercase().starts_with("pressure")),
            "the bands are interleaved"
        );
        assert!(
            found[starts..]
                .iter()
                .all(|k| k.title.to_ascii_lowercase().contains("pressure")),
            "something matched nothing"
        );

        // Stable: the same call twice gives the same list, which a dictionary
        // holding two kinds of the same title does not get for free.
        assert_eq!(
            dictionary.search("stiffness", 10).iter().map(|k| &k.uuid).collect::<Vec<_>>(),
            dictionary.search("stiffness", 10).iter().map(|k| &k.uuid).collect::<Vec<_>>()
        );

        // Case is noise, and the limit is honoured.
        assert_eq!(dictionary.search("PRESSURE", 3).len(), 3);
        // An empty query is the head of the list, not everything and not nothing.
        assert_eq!(dictionary.search("", 5).len(), 5);
        assert!(dictionary.search("no kind is called this", 5).is_empty());
    });
}

#[test]
fn an_ancestry_stops_at_the_root_that_is_its_own_parent() {
    let (count, _label) = property_kind::builtin();
    assert!(count > 0, "the built-in dictionary is never empty: build.rs refuses to build without data/property_kinds.tsv");
    property_kind::with_default(|dictionary| {
        let length = dictionary.by_title("length")[0];
        let chain = dictionary.ancestry(&length.uuid);
        assert_eq!(chain[0].uuid, length.uuid, "the chain starts with the kind itself");
        assert!(chain.len() > 1, "`length` has a parent");
        assert_eq!(
            chain.last().unwrap().title.to_ascii_lowercase(),
            "property",
            "the chain does not reach the root: {:?}",
            chain.iter().map(|k| &k.title).collect::<Vec<_>>()
        );

        // The root is its own parent — the walk must stop on the repeat rather
        // than run forever, which is the only reason `ancestry` keeps a set.
        let root = dictionary.by_title("property")[0];
        assert_eq!(dictionary.ancestry(&root.uuid).len(), 1);

        assert!(dictionary.ancestry("not-a-uuid").is_empty());
    });
}
