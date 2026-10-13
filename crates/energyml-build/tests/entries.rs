//! What a caller names, as pieces - `energyml_build::entries`, moved out of
//! `energyml-cli/src/create.rs` on 24/09 so that the command and the recipe
//! runner read an input by one rule.
//!
//! The command's behaviour is what these tests pin: the same pieces under the
//! same names, the same refusals **word for word** - a refusal is what somebody
//! acts on, and a move that changed one would have changed the command.
//!
//! # What would make them fail
//!
//! Testing `#` before the scheme (an address with a fragment sent to open a
//! package); a `.json` read as a table; the piece named after the file when a
//! slot is given, or after the slot for the collection's own name; a slot given
//! twice named alike; a refusal reworded. Each has its assertion below.

use std::collections::BTreeMap;

use serde_json::json;

use energyml_build::entries::{self, Numbering, Reader, Unread};
use energyml_build::pieces::Source;
use energyml_build::source::Source as Where;
use energyml_storage::EpcStore;

/// Texts by source, and what was asked of it - no disk, no network.
#[derive(Default)]
struct Held {
    texts: BTreeMap<String, (String, Option<String>)>,
    asked: Vec<String>,
    packages: Vec<String>,
}

impl Held {
    fn with(mut self, source: &str, text: &str, declared: Option<&str>) -> Self {
        self.texts.insert(
            source.to_string(),
            (text.to_string(), declared.map(str::to_string)),
        );
        self
    }
}

impl Reader for Held {
    fn text(&mut self, source: &Where) -> Result<(String, Option<String>), String> {
        self.asked.push(format!("{source:?}"));
        self.texts
            .get(source.as_str())
            .cloned()
            .ok_or_else(|| format!("reading {}: not held", source.as_str()))
    }

    fn package(&mut self, path: &str) -> Result<&EpcStore, String> {
        self.packages.push(path.to_string());
        Err(format!("opening {path}: no package here"))
    }
}

/// A collection of one point, in WGS 84.
const ONE_POINT: &str = r#"{"type":"FeatureCollection","features":[{"type":"Feature","geometry":{"type":"Point","coordinates":[2.3358,48.8516,35]},"properties":{"name":"a"}}]}"#;

/// A slot is read off the front, and a path - a drive letter, an `=` in a
/// folder name after a separator - is not taken for one. (The command's own
/// test, moved with the function.)
#[test]
fn a_slot_is_words_before_an_equals_sign_and_a_path_is_not_one() {
    use entries::slot_of;
    assert_eq!(slot_of("top=model.epc#abc"), (Some("top"), "model.epc#abc"));
    assert_eq!(
        slot_of("surfaces 2=model.epc#abc"),
        (Some("surfaces 2"), "model.epc#abc")
    );
    assert_eq!(slot_of("model.epc#abc"), (None, "model.epc#abc"));
    assert_eq!(
        slot_of(r"C:\data\a=b\model.epc#abc"),
        (None, r"C:\data\a=b\model.epc#abc")
    );
    assert_eq!(slot_of("=model.epc"), (None, "=model.epc"));
}

/// **A slot named twice is two pieces**, `wells` then `wells 2` - the names the
/// binder ranks. Named alike, every shape was paired with the first companion
/// (23/09).
#[test]
fn a_slot_given_twice_is_numbered_and_another_slot_counts_apart() {
    let mut numbering = Numbering::default();
    assert_eq!(numbering.next("wells"), "wells");
    assert_eq!(numbering.next("wells"), "wells 2");
    assert_eq!(numbering.next("top"), "top");
    assert_eq!(numbering.next("wells"), "wells 3");
}

/// The name decides, then what a server **declared** - never the bytes. And a
/// `.json` is not a table: a template is a `.json` too.
#[test]
fn a_table_and_a_geojson_are_told_apart_by_name_then_by_declared_type() {
    assert!(entries::reads_as_table("passes.csv", None));
    assert!(entries::reads_as_table("PASSES.TSV", None));
    assert!(!entries::reads_as_table("gabarit.json", None));
    assert!(entries::reads_as_table(
        "download",
        Some("text/csv; charset=utf-8")
    ));

    assert!(entries::reads_as_geojson("forages.geojson", None));
    assert!(entries::reads_as_geojson("forages.json", None));
    assert!(entries::reads_as_geojson(
        "features",
        Some("application/geo+json; charset=utf-8")
    ));
    assert!(!entries::reads_as_geojson("passes.csv", None));
    assert!(!entries::reads_as_geojson("features", None));
}

/// A file becomes a collection named after the file; given to a slot, the
/// **piece** takes the slot's name and the **collection** keeps the file's -
/// which is what an operation reads a stage from.
#[test]
fn a_geojson_is_a_collection_named_after_its_file_and_its_piece_after_the_slot() {
    let mut held = Held::default()
        .with("data/forages.geojson", ONE_POINT, None)
        .with("autres.geojson", ONE_POINT, None);
    let mut set_aside = Vec::new();
    let pieces = entries::pieces_of(
        &[
            "features=data/forages.geojson".to_string(),
            "autres.geojson".to_string(),
        ],
        None,
        &mut held,
        &mut set_aside,
    )
    .expect("both read");
    let names: Vec<&str> = pieces.iter().map(|(name, _)| name).collect();
    assert_eq!(names, ["features", "autres.geojson"]);
    let Some(Source::Features { name, features }) = pieces.get("features") else {
        panic!("a collection")
    };
    assert_eq!(name, "forages.geojson");
    assert_eq!(features.len(), 1);
    assert!(set_aside.is_empty(), "{set_aside:?}");
}

/// **The scheme decides before the `#`.** `https://a/x.json#part` is an
/// address with a fragment, not an object of a package called
/// `https://a/x.json` - which is what testing for `#` first did.
#[test]
fn an_address_with_a_fragment_is_fetched_and_never_opened_as_a_package() {
    let url = "https://a.example/x.json#part";
    let mut held = Held::default().with(url, ONE_POINT, Some("application/geo+json"));
    let mut set_aside = Vec::new();
    let pieces = entries::pieces_of(&[url.to_string()], None, &mut held, &mut set_aside)
        .expect("read from the address");
    assert!(
        held.packages.is_empty(),
        "a package was opened: {:?}",
        held.packages
    );
    assert_eq!(held.asked, [format!("{:?}", Where::Url(url.to_string()))]);
    // Named after the **file** the address names, fragment dropped.
    assert_eq!(
        pieces.iter().map(|(name, _)| name).collect::<Vec<_>>(),
        ["x.json"]
    );

    // And a path with a `#` **is** an object of a package: the reader is asked
    // for the package, and its refusal comes back as it was said.
    let mut held = Held::default();
    let why = entries::pieces_of(
        &["top=model.epc#abc".to_string()],
        None,
        &mut held,
        &mut set_aside,
    )
    .expect_err("no package here");
    assert_eq!(held.packages, ["model.epc"]);
    assert_eq!(
        why,
        Unread::Said("opening model.epc: no package here".to_string())
    );
}

/// **The command's refusal, word for word.** A table without a template names
/// the flag that gives one; the recipe names its own way, around the same
/// sentence.
#[test]
fn a_table_without_a_template_is_refused_naming_how_to_give_one() {
    let mut held = Held::default().with("passes.csv", "PROF;NOM\n1;a\n", None);
    let mut set_aside = Vec::new();
    let why = entries::pieces_of(&["passes.csv".to_string()], None, &mut held, &mut set_aside)
        .expect_err("no template");
    assert_eq!(
        why,
        Unread::NoTemplate {
            source: "passes.csv".to_string()
        }
    );
    assert_eq!(
        why.said("`--template <file.json>`"),
        "passes.csv: a table needs `--template <file.json>` - an OSDU ColumnBasedTableTemplate, a File.TabularData, or the bare list of column definitions. What a column means is read from it, never guessed from its heading"
    );
}

/// A table with its template is a table piece, read with the dialect sniffed
/// on its header; the template is read once, through the reader.
#[test]
fn a_table_with_its_template_is_a_table() {
    let template = json!([{
        "ColumnName": "PROF",
        "ValueType": "number",
        "UnitOfMeasureID": "namespace:reference-data--UnitOfMeasure:m:",
        "PropertyType": { "PropertyTypeID": "namespace:reference-data--PropertyType:c48c65d4-9680-4d10-903c-a1b2e30f66b5:", "Name": "measured depth" },
    }]);
    let mut held = Held::default()
        .with("gabarit.json", &template.to_string(), None)
        .with("passes.csv", "PROF;NOM\n1;a\n2;b\n", None);
    let read = entries::template(&mut held, &Where::File("gabarit.json".to_string()))
        .expect("a bare list is a template");
    let mut set_aside = Vec::new();
    let pieces = entries::pieces_of(
        &["table=passes.csv".to_string()],
        Some(&read),
        &mut held,
        &mut set_aside,
    )
    .expect("read");
    let Some(Source::Table {
        name,
        table,
        template,
    }) = pieces.get("table")
    else {
        panic!("a table")
    };
    assert_eq!(name, "passes.csv");
    assert_eq!(table.headers, ["PROF", "NOM"]);
    assert_eq!(table.rows.len(), 2);
    assert_eq!(template.columns[0].name, "PROF");
}

/// What is neither a table nor a GeoJSON is refused naming the file, what it
/// was served as, and what is read - the command's sentence, unchanged.
#[test]
fn a_file_of_another_kind_is_refused_naming_what_is_read() {
    let mut held = Held::default().with("notes.txt", "hello", Some("text/plain"));
    let mut set_aside = Vec::new();
    let why =
        entries::pieces_of(&["notes.txt".to_string()], None, &mut held, &mut set_aside).expect_err("refused");
    let Unread::Said(said) = &why else {
        panic!("said in full: {why:?}")
    };
    assert!(
        said.starts_with("notes.txt: only a GeoJSON can be read here for now"),
        "{said}"
    );
    assert!(
        said.contains("(it is named `notes.txt`, served as `text/plain`"),
        "{said}"
    );
}

/// A feature that does not read is set aside **by name**, and the rest still
/// comes - the command prints the line, `  [refused] <file>: <why>`.
#[test]
fn a_feature_that_does_not_read_is_set_aside_by_name() {
    let mixed = r#"{"type":"FeatureCollection","features":[
        {"type":"Feature","geometry":{"type":"Point","coordinates":[2.3358,48.8516,35]},"properties":{}},
        {"type":"Feature","geometry":{"type":"Point","coordinates":["x"]},"properties":{}}
    ]}"#;
    let mut held = Held::default().with("mixed.geojson", mixed, None);
    let mut set_aside = Vec::new();
    let pieces = entries::pieces_of(&["mixed.geojson".to_string()], None, &mut held, &mut set_aside)
        .expect("half reads");
    let Some(Source::Features { features, .. }) = pieces.get("mixed.geojson") else {
        panic!("a collection")
    };
    assert_eq!(features.len(), 1);
    assert_eq!(set_aside.len(), 1, "{set_aside:?}");
    assert!(set_aside[0].starts_with("mixed.geojson: "), "{set_aside:?}");
}
