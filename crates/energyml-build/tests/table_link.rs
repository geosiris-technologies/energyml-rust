//! `table-link`: the link of a column to a table (or of a property to the table
//! of its axes) written in convention B - the kind, and one `qualifier` facet
//! holding the full uri of the target.
//!
//! The form of the assertions comes from the standard and from the readers of
//! the domain, never from what the writer computes: EML TRG 2.3 section 11.11
//! p. 294 (`PropertyKindFacet`), section 11.3 p. 285 (`FacetKind`), and the
//! reader of convention B (`energyml_domain::table::links`,
//! `property::qualifier_link`) which is the **second source** of the
//! round-trip test: what is written must be what that reader reads.
//!
//! What would make them fail: the uuid written alone in the facet, a target
//! with two keys taken for the first, a second qualifier added to a column that
//! has one, a kind changed in silence, a key demanded for a link by rank, the
//! version of the target dropped, a description outside the grammar of
//! convention A.

use energyml_build::operation::{Choices, Identity, Plan, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_core::registry::Registry as Types;
use energyml_domain::property::{Facet, FacetValue, qualifier_link};
use energyml_domain::property_kind::Resolver;
use energyml_domain::source::NoObjects;
use energyml_domain::table::TableSheet;
use energyml_domain::table::class::resolve_kinds;
use energyml_domain::table::links::{Convention, LinkReaders, LinkTarget};
use energyml_eml_2_3 as eml;
use serde_json::{Value, json};

const TABLE: &str = "11111111-0000-4000-8000-000000000001";
const TARGET: &str = "22222222-0000-4000-8000-000000000002";
const PROPERTY: &str = "33333333-0000-4000-8000-000000000003";
/// `identifier`, `explicit index`'s child `classification index`, `name`: the
/// uuids of the standard dictionary (`property_kinds.tsv`).
const IDENTIFIER: &str = "311dcaba-6512-4144-aa7c-79a9de19b4ab";
const CLASSIFICATION_INDEX: &str = "2bcbe7d8-cd41-4758-a88f-242781e7f495";
const NAME: &str = "0eb70eac-c3d2-4448-b617-10260c4abee3";

fn when() -> When {
    When {
        creation: "2026-11-15T00:00:00Z".into(),
        originator: "tests/table_link.rs".into(),
    }
}

fn column(title: &str, kind: &str, description: Option<&str>) -> eml::Column {
    eml::Column {
        title: Some(title.to_string()),
        description: description.map(str::to_string),
        value_count_per_row: 1,
        property_kind: eml::DataObjectReference {
            uuid: kind.to_string(),
            qualified_type: "eml23.PropertyKind".to_string(),
            title: "kind".to_string(),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn table(uuid: &str, title: &str, keys: Vec<eml::Column>, columns: Vec<eml::Column>) -> Value {
    energyml_core::json::write_value(&eml::ColumnBasedTable {
        uuid: uuid.to_string(),
        schema_version: "2.3".to_string(),
        citation: eml::Citation {
            title: title.to_string(),
            ..Default::default()
        },
        key_column: keys,
        column: columns,
        ..Default::default()
    })
}

fn object_piece(uuid: &str, qualified: &str, title: &str, version: Option<&str>, json: Value) -> Source {
    Source::Object(Known {
        reference: Some(Ref::new(uuid, qualified, title).with_version(version.map(str::to_string))),
        json: Some(json),
        ..Default::default()
    })
}

/// The table whose column `station` links: a key, then `station` and `label`.
fn samples(link_kind: &str, description: Option<&str>) -> Value {
    table(
        TABLE,
        "samples",
        vec![column("sample id", IDENTIFIER, None)],
        vec![column("station", link_kind, description), column("label", NAME, None)],
    )
}

fn targets(keys: usize) -> Value {
    let keyed = (0..keys).map(|n| column(&format!("id{n}"), IDENTIFIER, None)).collect();
    table(TARGET, "targets", keyed, vec![column("name", NAME, None)])
}

fn bag(object: Source, target: Source) -> Pieces {
    Pieces::empty().with("object", object).with("target", target)
}

fn tables(link_kind: &str, keys: usize) -> Pieces {
    bag(
        object_piece(TABLE, "eml23.ColumnBasedTable", "samples", None, samples(link_kind, None)),
        object_piece(TARGET, "eml23.ColumnBasedTable", "targets", None, targets(keys)),
    )
}

fn planned(pieces: &Pieces, choices: Choices) -> Result<Plan, String> {
    Registry::new()
        .plan("table-link", pieces, &choices, &Identity::derived(), &when())
        .map_err(|why| why.to_string())
}

fn link_column() -> Choices {
    Choices::new().set("column", "station").set("by", "key")
}

/// The facets of the column `station` of the table written.
fn facets_of(plan: &Plan) -> Vec<Value> {
    plan.objects[0].json["Column"][0]["Facet"].as_array().cloned().unwrap_or_default()
}

fn reread(json: &Value) -> (TableSheet, Vec<Option<energyml_domain::property_kind::Resolution>>) {
    let mut types = Types::new();
    eml::register(&mut types);
    let read = energyml_core::json::read_value(json, &types).expect("the table reads back");
    let sheet = TableSheet::of(read.root.as_reflect()).expect("a sheet");
    let kinds = resolve_kinds(&sheet, &Resolver::new(&NoObjects));
    (sheet, kinds)
}

#[test]
fn a_column_links_by_key_with_one_qualifier_facet() {
    let plan = planned(&tables(IDENTIFIER, 1), link_column()).expect("planned");
    let facets = facets_of(&plan);
    assert_eq!(facets.len(), 1, "one facet");
    assert_eq!(facets[0]["Kind"], json!("qualifier"));
    let uri = facets[0]["Facet"].as_str().expect("a uri");
    // The full uri of the target - never its uuid alone.
    assert!(uri.starts_with("eml:///eml23.ColumnBasedTable("), "{uri}");
    assert!(uri.contains(TARGET), "{uri}");
    let description = plan.objects[0].json["Column"][0]["Description"].as_str().expect("a description");
    assert!(description.starts_with("key of the table `targets`: eml:///"), "{description}");
    assert!(description.ends_with(", KeyColumn `id0`"), "{description}");
}

#[test]
fn a_target_with_two_key_columns_is_refused() {
    let why = planned(&tables(IDENTIFIER, 2), link_column()).expect_err("refused");
    assert!(why.contains("`target`") && why.contains('2'), "{why}");
}

#[test]
fn a_second_qualifier_is_refused() {
    let mut json = samples(IDENTIFIER, None);
    json["Column"][0]["Facet"] = json!([{
        "$type": "eml23.PropertyKindFacet",
        "Kind": "qualifier",
        "Facet": "eml:///eml23.ColumnBasedTable(99999999-0000-4000-8000-000000000009)",
    }]);
    let pieces = bag(
        object_piece(TABLE, "eml23.ColumnBasedTable", "samples", None, json),
        object_piece(TARGET, "eml23.ColumnBasedTable", "targets", None, targets(1)),
    );
    let why = planned(&pieces, link_column()).expect_err("refused");
    assert!(why.contains("qualifier"), "{why}");
}

#[test]
fn a_kind_that_is_no_identifier_is_refused_never_changed() {
    let why = planned(&tables(NAME, 1), link_column()).expect_err("refused");
    assert!(why.contains("`column`") && why.contains("name") && why.contains("edit-object"), "{why}");
}

#[test]
fn a_rank_link_takes_a_classification_index() {
    // By rank, the target needs no key: the rank names the row.
    let plan = planned(&tables(CLASSIFICATION_INDEX, 0), Choices::new().set("column", "station").set("by", "rank"))
        .expect("planned");
    assert_eq!(facets_of(&plan).len(), 1);
    assert!(
        plan.objects[0].json["Column"][0].get("Description").is_none_or(Value::is_null),
        "a link by rank has no form in a description"
    );
    assert!(plan.says.iter().any(|note| note.key == "no-description-for-rank"));
}

#[test]
fn a_property_links_to_the_table_of_its_axes() {
    let property = json!({
        "$type": "resqml22.ContinuousProperty",
        "uuid": PROPERTY,
        "schemaVersion": "2.2",
        "Citation": { "$type": "eml23.Citation", "Title": "cube", "Originator": "test", "Creation": "2026-01-01T00:00:00Z" },
    });
    let pieces = bag(
        object_piece(PROPERTY, "resqml22.ContinuousProperty", "cube", None, property),
        object_piece(TARGET, "eml23.ColumnBasedTable", "targets", None, targets(1)),
    );
    let plan = planned(&pieces, Choices::new()).expect("planned");
    let written = &plan.objects[0].json;
    assert_eq!(written["Facet"].as_array().map(Vec::len), Some(1));
    assert!(written.get("Column").is_none(), "a property has no column");
    // And a column named for a property is refused.
    let why = planned(&pieces, Choices::new().set("column", "station")).expect_err("refused");
    assert!(why.contains("`column`"), "{why}");
}

#[test]
fn the_link_reads_back_as_convention_b() {
    let plan = planned(&tables(IDENTIFIER, 1), link_column()).expect("planned");
    // Second source: the reader of the domain, over what was written.
    let (sheet, kinds) = reread(&plan.objects[0].json);
    let at = sheet.column_titled("station").expect("the column");
    let declaration = LinkReaders::builtin()
        .read_column(&sheet, at, kinds[at].as_ref())
        .expect("a declaration")
        .expect("followed");
    assert_eq!(declaration.convention, Convention::KindAndFacet);
    assert!(declaration.warnings.is_empty(), "{:?}", declaration.warnings);
    match declaration.target {
        LinkTarget::Key { table_uri } => assert!(table_uri.contains(TARGET), "{table_uri}"),
        other => panic!("{other:?}"),
    }
    // A property: read by `property::qualifier_link`, the one door.
    let property = json!({
        "$type": "resqml22.ContinuousProperty", "uuid": PROPERTY, "schemaVersion": "2.2",
        "Citation": { "$type": "eml23.Citation", "Title": "cube", "Originator": "test", "Creation": "2026-01-01T00:00:00Z" },
    });
    let pieces = bag(
        object_piece(PROPERTY, "resqml22.ContinuousProperty", "cube", None, property),
        object_piece(TARGET, "eml23.ColumnBasedTable", "targets", None, targets(1)),
    );
    let written = planned(&pieces, Choices::new()).expect("planned");
    let facets: Vec<Facet> = written.objects[0].json["Facet"]
        .as_array()
        .unwrap()
        .iter()
        .map(|one| Facet {
            kind: one["Kind"].as_str().unwrap().to_string(),
            value: FacetValue::Extension(one["Facet"].as_str().unwrap().to_string()),
        })
        .collect();
    let uri = qualifier_link(&facets).expect("one link").expect("a uri");
    assert!(uri.contains(TARGET), "{uri}");
}

#[test]
fn a_tree_links_to_its_own_table() {
    // The target is the table itself: a column naming its own rows.
    let own = table(
        TABLE,
        "tree",
        vec![column("id", IDENTIFIER, None)],
        vec![column("parent", IDENTIFIER, None)],
    );
    let pieces = bag(
        object_piece(TABLE, "eml23.ColumnBasedTable", "tree", None, own.clone()),
        object_piece(TABLE, "eml23.ColumnBasedTable", "tree", None, own),
    );
    let plan = planned(&pieces, Choices::new().set("column", "parent").set("by", "key")).expect("planned");
    let description = plan.objects[0].json["Column"][0]["Description"].as_str().expect("a description");
    assert!(description.starts_with("key of this table (self reference): eml:///"), "{description}");
    let (sheet, kinds) = reread(&plan.objects[0].json);
    let at = sheet.column_titled("parent").expect("the column");
    let declaration = LinkReaders::builtin()
        .read_column(&sheet, at, kinds[at].as_ref())
        .expect("a declaration")
        .expect("followed");
    assert_eq!(declaration.target, LinkTarget::SameTable);
}

#[test]
fn the_version_is_in_the_uri() {
    let pieces = bag(
        object_piece(TABLE, "eml23.ColumnBasedTable", "samples", None, samples(IDENTIFIER, None)),
        object_piece(TARGET, "eml23.ColumnBasedTable", "targets", Some("2"), targets(1)),
    );
    let plan = planned(&pieces, link_column()).expect("planned");
    let uri = facets_of(&plan)[0]["Facet"].as_str().unwrap().to_string();
    assert!(uri.contains("version='2'"), "{uri}");
}
