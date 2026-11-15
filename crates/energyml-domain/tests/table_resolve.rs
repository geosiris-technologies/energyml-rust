//! Following a link cell to a row (`table::resolve`).
//!
//! The tables are `eml23.ColumnBasedTable` objects built by hand (common TRG
//! 2.3 section 9.1 p. 190: `KeyColumn 0..*`, `Column 1..*`, ordering matters),
//! read by `Lookup::read` over inline arrays; the specification gives the
//! shape, the values are the test's own. Nothing is read from a file.

use energyml_domain::source::NoObjects;
use energyml_domain::table::{
    Cell, CellState, Convention, DanglingWhy, LinkDeclaration, LinkRefusal, LinkTarget, Lookup, Resolved, TableSet,
    resolve,
};
use energyml_eml_2_3 as eml23;

const OWN: &str = "eml:///eml23.ColumnBasedTable(00000000-0000-0000-0000-00000000f001)";
const TARGET: &str = "eml:///eml23.ColumnBasedTable(00000000-0000-0000-0000-00000000f002)";
const SECOND: &str = "eml:///eml23.ColumnBasedTable(00000000-0000-0000-0000-00000000f003)";

fn strings(values: &[&str]) -> eml23::AbstractValueArray {
    eml23::AbstractValueArray::StringXmlArray(eml23::StringXmlArray {
        values: values.iter().map(|s| s.to_string()).collect(),
        count_per_value: 1,
        ..Default::default()
    })
}

fn column(title: &str, values: &[&str]) -> eml23::Column {
    eml23::Column {
        title: Some(title.to_string()),
        value_count_per_row: 1,
        values: Some(strings(values)),
        ..Default::default()
    }
}

/// A table of text columns; `keys` come first.
fn lookup(keys: Vec<eml23::Column>, columns: Vec<eml23::Column>) -> Lookup {
    let table = eml23::ColumnBasedTable {
        uuid: "00000000-0000-0000-0000-00000000f0ff".to_string(),
        key_column: keys,
        column: columns,
        ..Default::default()
    };
    Lookup::read(&table, &NoObjects).expect("the table reads")
}

fn link(target: LinkTarget, key_named: Option<&str>) -> LinkDeclaration {
    LinkDeclaration {
        column: 0,
        target,
        convention: Convention::KindAndFacet,
        warnings: Vec::new(),
        key_named: key_named.map(str::to_string),
    }
}

fn text(value: &str) -> CellState {
    CellState::Value(Cell::Text(value.to_string()))
}

fn number(value: f64) -> CellState {
    CellState::Value(Cell::Number(value))
}

fn keyed_set() -> TableSet {
    let mut set = TableSet::new();
    set.insert(
        TARGET,
        Some("targets".to_string()),
        lookup(vec![column("index_uid", &["a", "b"])], vec![column("name", &["A", "B"])]),
    );
    set
}

#[test]
fn a_dangling_key_is_named_with_its_row() {
    // House rule `house/table/link-resolves`: a non-empty value of a link column is a key.
    let set = keyed_set();
    let key = link(
        LinkTarget::Key {
            table_uri: TARGET.to_string(),
        },
        None,
    );
    assert_eq!(
        resolve(&key, &text("b"), None, OWN, &set),
        Resolved::Row {
            table_uri: TARGET.to_string(),
            row: 1
        }
    );
    assert_eq!(
        resolve(&key, &text("z"), None, OWN, &set),
        Resolved::Dangling {
            table_uri: TARGET.to_string(),
            why: DanglingWhy::NoSuchKey
        }
    );
}

#[test]
fn a_self_reference_resolves_in_the_same_table() {
    // A tree: a column of the table links to the key of another row of the same table.
    let mut set = TableSet::new();
    set.insert(
        OWN,
        None,
        lookup(vec![column("id", &["a", "b", "c"])], vec![column("parent", &["", "a", "b"])]),
    );
    let own = link(LinkTarget::SameTable, None);
    assert_eq!(
        resolve(&own, &text("c"), None, OWN, &set),
        Resolved::Row {
            table_uri: OWN.to_string(),
            row: 2
        }
    );
}

#[test]
fn an_empty_link_cell_names_no_row() {
    // An empty string is a value, but it is not a key; an absent value is not a value.
    let set = keyed_set();
    let key = link(
        LinkTarget::Key {
            table_uri: TARGET.to_string(),
        },
        None,
    );
    assert_eq!(resolve(&key, &CellState::Empty, None, OWN, &set), Resolved::Empty);
    assert_eq!(resolve(&key, &CellState::Absent, None, OWN, &set), Resolved::Absent);
    assert_eq!(
        resolve(&key, &CellState::Vector(vec![Some(Cell::Number(1.0))]), None, OWN, &set),
        Resolved::Unresolvable(LinkRefusal::VectorValue)
    );
    assert_eq!(
        resolve(&key, &CellState::Unreadable, None, OWN, &set),
        Resolved::Unresolvable(LinkRefusal::CellUnreadable)
    );
}

#[test]
fn a_target_with_two_key_columns_is_ambiguous() {
    // A value names one row by one key: two key columns name nothing.
    let mut set = TableSet::new();
    set.insert(
        TARGET,
        None,
        lookup(
            vec![column("a", &["1", "2"]), column("b", &["x", "y"])],
            vec![column("v", &["p", "q"])],
        ),
    );
    let key = link(
        LinkTarget::Key {
            table_uri: TARGET.to_string(),
        },
        None,
    );
    assert_eq!(
        resolve(&key, &text("1"), None, OWN, &set),
        Resolved::Unresolvable(LinkRefusal::AmbiguousTargetKey { keys: 2 })
    );
}

#[test]
fn a_rank_names_its_row_in_a_keyless_target_and_dangles_outside() {
    let mut set = TableSet::new();
    set.insert(TARGET, None, lookup(Vec::new(), vec![column("v", &["p", "q", "r"])]));
    let rank = link(
        LinkTarget::Rank {
            table_uri: TARGET.to_string(),
        },
        None,
    );
    assert_eq!(
        resolve(&rank, &number(1.0), None, OWN, &set),
        Resolved::Row {
            table_uri: TARGET.to_string(),
            row: 1
        }
    );
    assert_eq!(
        resolve(&rank, &number(3.0), None, OWN, &set),
        Resolved::Dangling {
            table_uri: TARGET.to_string(),
            why: DanglingWhy::NotARank { rows: 3 }
        }
    );
}

#[test]
fn a_target_that_is_not_in_the_set_is_named() {
    let set = keyed_set();
    let key = link(
        LinkTarget::Key {
            table_uri: SECOND.to_string(),
        },
        None,
    );
    assert_eq!(
        resolve(&key, &text("a"), None, OWN, &set),
        Resolved::Unresolvable(LinkRefusal::TargetUnknown {
            uri: SECOND.to_string()
        })
    );
}

#[test]
fn a_title_names_its_table_or_an_external_label() {
    let set = keyed_set();
    let by_title = link(LinkTarget::ByTitleIn { column: 1 }, None);
    assert_eq!(
        resolve(&by_title, &text("a"), Some(&text("targets")), OWN, &set),
        Resolved::Row {
            table_uri: TARGET.to_string(),
            row: 0
        }
    );
    assert_eq!(
        resolve(&by_title, &text("a"), Some(&text("external file")), OWN, &set),
        Resolved::External {
            label: "external file".to_string()
        }
    );

    // A title is not an identity: two tables with it name nothing.
    let mut twice = keyed_set();
    twice.insert(
        SECOND,
        Some("targets".to_string()),
        lookup(vec![column("index_uid", &["a"])], vec![column("name", &["A"])]),
    );
    assert_eq!(
        resolve(&by_title, &text("a"), Some(&text("targets")), OWN, &twice),
        Resolved::Unresolvable(LinkRefusal::AmbiguousTitle {
            title: "targets".to_string(),
            count: 2
        })
    );
}

#[test]
fn a_description_naming_another_key_column_is_refused() {
    let set = keyed_set();
    let target = LinkTarget::Key {
        table_uri: TARGET.to_string(),
    };
    assert_eq!(
        resolve(&link(target.clone(), Some("uid")), &text("a"), None, OWN, &set),
        Resolved::Unresolvable(LinkRefusal::KeyColumnMissing {
            named: "uid".to_string()
        })
    );
    // The key column it names is the target's: followed.
    assert_eq!(
        resolve(&link(target, Some("index_uid")), &text("a"), None, OWN, &set),
        Resolved::Row {
            table_uri: TARGET.to_string(),
            row: 0
        }
    );
}

#[test]
fn a_column_of_object_uris_is_not_a_row_link() {
    let set = keyed_set();
    let objects = link(LinkTarget::Objects, None);
    assert_eq!(
        resolve(&objects, &text("eml:///eml23.Activity(00000000-0000-0000-0000-00000000f009)"), None, OWN, &set),
        Resolved::Unresolvable(LinkRefusal::NotARowLink)
    );
}
