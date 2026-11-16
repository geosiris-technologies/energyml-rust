//! The table rules of `energyml_domain::integrity`: links between tables, the
//! uris written as values, and a discrete value read by rank (ADR-0030).
//!
//! **The form comes from the specification, the values are the test's own.**
//! Every test builds a table that breaks the rule and a twin that does not, and
//! asserts the first is found **once, by its rule id**, and the twin not at
//! all. Nothing is read from a file: arrays are inline XML, and the citations
//! (common TRG 2.3 §9.1 p. 190 for the key column, §11.11 p. 294 and §11.3 p.
//! 285 for the qualifier facet; Identifier Specification v4.0 §6.3 p. 14 for the
//! uri) are in the comments, where a reader can check them (ADR-0011).
//!
//! The helpers are those of `tests/integrity.rs`, imported rather than copied.

use crate::integrity::{TABLE, boxed, citation, discrete, judge, of, read_of, table};
use energyml_domain::rules::{Gravity, Outcome};
use energyml_domain::table::{DESCRIPTION, IDENTIFIER};
use energyml_eml_2_3 as eml23;

const A: &str = "00000000-0000-0000-0000-00000000f001";
const B: &str = "00000000-0000-0000-0000-00000000f002";
const A_TWIN: &str = "00000000-0000-0000-0000-00000000f003";
const URIS: &str = "00000000-0000-0000-0000-00000000f004";

const RESOLVES: &str = "house/table/link-resolves";
const UNIQUE: &str = "house/table/link-target-key-unique";
const BY_FACET: &str = "house/table/link-declared-by-facet";
const URI_VALUE: &str = "house/table/uri-value-resolves";
const DISCRETE: &str = "house/discrete-value-in-lookup";

fn table_uri(uuid: &str) -> String {
    format!("eml:///eml23.ColumnBasedTable({uuid})")
}

fn strings(values: &[&str]) -> eml23::AbstractValueArray {
    eml23::AbstractValueArray::StringXmlArray(eml23::StringXmlArray {
        values: values.iter().map(|v| v.to_string()).collect(),
        count_per_value: 1,
        ..Default::default()
    })
}

/// A column of text of the given kind, by the property kind's uuid.
fn column(title: &str, values: &[&str], kind: &str) -> eml23::Column {
    eml23::Column {
        title: Some(title.to_string()),
        value_count_per_row: 1,
        property_kind: eml23::DataObjectReference {
            uuid: kind.to_string(),
            qualified_type: "eml23.PropertyKind".to_string(),
            title: "a kind".to_string(),
            ..Default::default()
        },
        values: Some(strings(values)),
        ..Default::default()
    }
}

/// Convention B: one `qualifier` facet holding the uri of the target table
/// (common TRG 2.3 §11.3 p. 285, §11.11 p. 294).
fn declared_to(mut column: eml23::Column, target: &str) -> eml23::Column {
    column.facet.push(eml23::PropertyKindFacet {
        facet: eml23::FacetExt::Other(table_uri(target)),
        kind: eml23::FacetKind::Qualifier,
    });
    column
}

fn linking(uuid: &str, values: &[&str]) -> eml23::ColumnBasedTable {
    eml23::ColumnBasedTable {
        uuid: uuid.to_string(),
        citation: citation("a table of links"),
        column: vec![declared_to(column("index_uid", values, IDENTIFIER), B)],
        ..Default::default()
    }
}

/// The target: one key column, `index_uid`.
fn target(keys: &[&str]) -> eml23::ColumnBasedTable {
    eml23::ColumnBasedTable {
        uuid: B.to_string(),
        citation: citation("the indexes"),
        key_column: vec![column("index_uid", keys, IDENTIFIER)],
        ..Default::default()
    }
}

/// A table of three rows without a `KeyColumn`: its rows are named by rank.
fn keyless() -> eml23::ColumnBasedTable {
    eml23::ColumnBasedTable {
        uuid: TABLE.to_string(),
        citation: citation("lithologies"),
        column: vec![column("name", &["a", "b", "c"], DESCRIPTION)],
        ..Default::default()
    }
}

/// A link column naming a row that is nowhere is found once, with its row and
/// its value; a twin whose values all resolve is not.
#[test]
fn a_dangling_link_is_found_once_with_its_row() {
    let findings = judge(
        vec![
            (A, boxed(linking(A, &["i1", "zz"]))),
            (B, boxed(target(&["i1"]))),
            (A_TWIN, boxed(linking(A_TWIN, &["i1", "i1"]))),
        ],
        true,
    );
    let found = of(&findings, RESOLVES);
    assert_eq!(found.len(), 1, "{findings:?}");
    assert_eq!(found[0].uuid, A);
    let read = read_of(found[0]);
    assert!(read.contains("row(s) 1") && read.contains("zz"), "{read}");
    assert_eq!(found[0].gravity, Gravity::Error);
}

/// Not having looked is not having found nothing: with no array reader, a link
/// is *uncheckable*, never a pass.
#[test]
fn without_an_array_reader_a_link_is_uncheckable_never_a_pass() {
    let findings = judge(
        vec![(A, boxed(linking(A, &["i1", "zz"]))), (B, boxed(target(&["i1"])))],
        false,
    );
    let found = of(&findings, RESOLVES);
    assert_eq!(found.len(), 1, "{findings:?}");
    assert!(
        matches!(&found[0].outcome, Outcome::Unverifiable { .. }),
        "{found:?}"
    );
}

/// Nothing requires keys to be unique (common TRG 2.3 §9.1 p. 190): a link into
/// a duplicated key uses the first row, and says so.
#[test]
fn a_link_into_a_duplicated_key_is_a_warning() {
    let findings = judge(
        vec![
            (A, boxed(linking(A, &["i1", "i1"]))),
            (B, boxed(target(&["i1", "i1"]))),
        ],
        true,
    );
    let found = of(&findings, UNIQUE);
    assert_eq!(found.len(), 1, "{findings:?}");
    assert_eq!(found[0].gravity, Gravity::Warning);
    let read = read_of(found[0]);
    assert!(read.contains("`i1`") && read.contains("rows 0, 1"), "{read}");
    // Ambiguous is not dangling: the first row is used.
    assert!(of(&findings, RESOLVES).is_empty(), "{findings:?}");
}

/// A declaration in the description alone (convention A) is read, followed and
/// reported: the link still resolves, and the table is told to use a facet.
#[test]
fn a_description_only_link_is_reported_and_followed() {
    let mut text_column = column("index_uid", &["i1", "i1"], IDENTIFIER);
    text_column.description = Some(format!(
        "key of the table `the indexes`: {}, KeyColumn `index_uid`",
        table_uri(B)
    ));
    let described = eml23::ColumnBasedTable {
        uuid: A.to_string(),
        citation: citation("a table of links"),
        column: vec![text_column],
        ..Default::default()
    };
    let findings = judge(vec![(A, boxed(described)), (B, boxed(target(&["i1"])))], true);
    let by_facet = of(&findings, BY_FACET);
    assert_eq!(by_facet.len(), 1, "{findings:?}");
    assert!(read_of(by_facet[0]).contains("`index_uid` (description)"));
    assert_eq!(by_facet[0].gravity, Gravity::Warning);
    // Followed: it resolves, so nothing is said by the resolution rule.
    assert!(of(&findings, RESOLVES).is_empty(), "{findings:?}");
    // And the table of the twin, declared by a facet, is silent.
    let clean = judge(
        vec![(A, boxed(linking(A, &["i1", "i1"]))), (B, boxed(target(&["i1"])))],
        true,
    );
    assert!(of(&clean, BY_FACET).is_empty(), "{clean:?}");
}

/// A uri written as a value names an object (Identifier Specification v4.0 §6.3
/// p. 14): one the package holds is silent, one it does not hold is reported
/// with its row.
#[test]
fn a_uri_naming_no_object_is_a_warning() {
    let held = table_uri(B);
    let missing = "eml:///eml23.Activity(00000000-0000-0000-0000-00000000f0ff)";
    let uris = eml23::ColumnBasedTable {
        uuid: URIS.to_string(),
        citation: citation("a table of uris"),
        column: vec![column("epc_uri", &[held.as_str(), missing], DESCRIPTION)],
        ..Default::default()
    };
    let findings = judge(vec![(URIS, boxed(uris)), (B, boxed(target(&["i1"])))], true);
    let found = of(&findings, URI_VALUE);
    assert_eq!(found.len(), 1, "{findings:?}");
    assert_eq!(found[0].gravity, Gravity::Warning);
    let read = read_of(found[0]);
    assert!(read.contains("row(s) 1") && read.contains(missing), "{read}");
    assert!(!read.contains(&held), "{read}");
}

/// A table without a `KeyColumn` names its rows by rank from 0: the value 3 is
/// no row of a table of three rows, and 0 to 2 are (the null `-1` is not a
/// value).
#[test]
fn a_discrete_value_is_checked_by_rank_on_a_keyless_table() {
    let bad = "00000000-0000-0000-0000-00000000f101";
    let twin = "00000000-0000-0000-0000-00000000f102";
    let findings = judge(
        vec![
            (TABLE, boxed(keyless())),
            (bad, boxed(discrete(bad, &[0, 2, 3, -1]))),
            (twin, boxed(discrete(twin, &[0, 1, 2]))),
        ],
        true,
    );
    let found = of(&findings, DISCRETE);
    assert_eq!(found.len(), 1, "{findings:?}");
    assert_eq!(found[0].uuid, bad);
    let read = read_of(found[0]);
    assert!(
        read.contains("3 (1 element(s))") && read.contains("row-rank-without-key"),
        "{read}"
    );
}

/// A keyed table is never read by rank (common TRG 2.3 §9.1 p. 190: a key
/// column makes the table a lookup): `0` is no key of `1, 2, 3`, although the
/// rank 0 exists.
#[test]
fn a_keyed_lookup_is_still_checked_by_its_key() {
    let bad = "00000000-0000-0000-0000-00000000f103";
    let findings = judge(
        vec![(TABLE, boxed(table())), (bad, boxed(discrete(bad, &[0])))],
        true,
    );
    let found = of(&findings, DISCRETE);
    assert_eq!(found.len(), 1, "{findings:?}");
    let read = read_of(found[0]);
    assert!(
        read.contains("0 (1 element(s))") && read.contains("key column"),
        "{read}"
    );
}
