//! How a column declares a link to a table (`table::links`): conventions B, A
//! and D, the registry, and the columns of uris (E).
//!
//! **The form comes from the specification.** B rests on EML TRG 2.3 section
//! 11.3 p. 285 (`qualifier`: any other context) and section 11.11 p. 294
//! ("Don't use several facets of the same kind for the same property"). A and D
//! are producers' conventions, not the specification's: their tests say what is
//! read and what is warned about. Every sheet is built by hand; nothing is read
//! from a file.

use std::borrow::Cow;
use std::sync::Arc;

use energyml_domain::package_index::SheetKind;
use energyml_domain::property::{Facet, FacetValue, PropertyKindRef};
use energyml_domain::property_kind::{Resolution, Resolver};
use energyml_domain::source::NoObjects;
use energyml_domain::table::{
    ColumnCells, ColumnSheet, Convention, EXPLICIT_INDEX, IDENTIFIER, LinkDeclaration, LinkReader, LinkReaders,
    LinkRefusal, LinkTarget, LinkWarning, TableSheet, UriColumn, uri_column,
};
use energyml_domain::values::{Family, Values};

const OWN: &str = "eml:///eml23.ColumnBasedTable(00000000-0000-0000-0000-00000000d001)";
const TARGET: &str = "eml:///eml23.ColumnBasedTable(7179e925-fba7-50be-9032-5462a75484da)";
const OTHER: &str = "eml:///eml23.ColumnBasedTable(00000000-0000-0000-0000-00000000d002)";
/// `classification index`, a child of `explicit index` (TSV lines 1762 and 1764).
const CLASSIFICATION_INDEX: &str = "2bcbe7d8-cd41-4758-a88f-242781e7f495";
/// `length`: neither an identifier nor an index.
const LENGTH: &str = "4364b378-899a-403a-8401-b06abd4fc0cf";

fn kind(uuid: &str) -> Resolution {
    Resolver::new(&NoObjects)
        .resolve(&PropertyKindRef::Object {
            uuid: uuid.to_string(),
            title: None,
        })
        .expect("the dictionary holds the kind")
}

fn qualifier(value: &str) -> Facet {
    Facet {
        kind: "qualifier".to_string(),
        value: FacetValue::Extension(value.to_string()),
    }
}

fn column(title: &str, description: Option<&str>, facets: Vec<Facet>) -> ColumnSheet {
    ColumnSheet {
        title: Some(Arc::from(title)),
        description: description.map(Arc::from),
        key: false,
        kind: None,
        uom: None,
        values_per_row: 1,
        facets: facets.into(),
        family: Some(Family::Text),
        values: Some(3),
        external: false,
    }
}

fn sheet(columns: Vec<ColumnSheet>) -> TableSheet {
    TableSheet {
        uri: OWN.into(),
        title: Some(Arc::from("the table")),
        qualified_type: Arc::from("eml23.ColumnBasedTable"),
        columns: columns.into(),
    }
}

/// The declaration of column 0, which must exist.
fn declared(table: &TableSheet, kind: Option<&Resolution>) -> Result<LinkDeclaration, LinkRefusal> {
    LinkReaders::builtin()
        .read_column(table, 0, kind)
        .expect("the column declares something")
}

#[test]
fn a_qualifier_facet_with_a_table_uri_declares_a_key_link() {
    let table = sheet(vec![column("code", None, vec![qualifier(TARGET)])]);
    let declaration = declared(&table, Some(&kind(IDENTIFIER))).expect("followed");
    assert_eq!(
        declaration.target,
        LinkTarget::Key {
            table_uri: TARGET.to_string()
        }
    );
    assert_eq!(declaration.convention, Convention::KindAndFacet);
    assert!(declaration.warnings.is_empty(), "{:?}", declaration.warnings);
    assert_eq!(declaration.key_named, None);
}

#[test]
fn an_explicit_index_column_links_by_rank() {
    // `classification index` is a child of `explicit index`: the walk up the chain decides.
    let table = sheet(vec![column("rank", None, vec![qualifier(TARGET)])]);
    for uuid in [EXPLICIT_INDEX, CLASSIFICATION_INDEX] {
        let declaration = declared(&table, Some(&kind(uuid))).expect("followed");
        assert_eq!(
            declaration.target,
            LinkTarget::Rank {
                table_uri: TARGET.to_string()
            },
            "{uuid}"
        );
        assert!(declaration.warnings.is_empty());
    }
}

#[test]
fn two_qualifier_facets_are_refused() {
    // EML TRG 2.3 section 11.11 p. 294: one facet of a kind per property.
    let table = sheet(vec![column(
        "code",
        None,
        vec![qualifier(TARGET), qualifier("cumulative")],
    )]);
    assert_eq!(
        declared(&table, Some(&kind(IDENTIFIER))),
        Err(LinkRefusal::SeveralQualifierFacets { column: 0, count: 2 })
    );
}

#[test]
fn a_qualifier_that_is_not_a_uri_is_no_link() {
    // EML TRG 2.3 section 11.3 p. 285: `qualifier` is any other context.
    let table = sheet(vec![column("volume", None, vec![qualifier("cumulative")])]);
    assert!(
        LinkReaders::builtin()
            .read_column(&table, 0, Some(&kind(IDENTIFIER)))
            .is_none()
    );
}

#[test]
fn a_facet_naming_a_wellbore_is_not_a_table() {
    let wellbore = "eml:///witsml21.Wellbore(00000000-0000-0000-0000-00000000d003)";
    let table = sheet(vec![column("code", None, vec![qualifier(wellbore)])]);
    assert_eq!(
        declared(&table, Some(&kind(IDENTIFIER))),
        Err(LinkRefusal::NotATable {
            uri: wellbore.to_string(),
            qualified_type: "witsml21.Wellbore".to_string()
        })
    );
}

#[test]
fn a_kind_that_is_no_identifier_is_followed_and_warned() {
    // Detect and signal, do not forbid: the link is followed, the oddity is said.
    let table = sheet(vec![column("code", None, vec![qualifier(TARGET)])]);
    let declaration = declared(&table, Some(&kind(LENGTH))).expect("followed all the same");
    assert!(matches!(declaration.target, LinkTarget::Key { .. }));
    assert_eq!(declaration.warnings, vec![LinkWarning::KindNotAnIdentifier]);
}

#[test]
fn a_description_link_is_read_and_marked_by_convention() {
    // Convention A, the UDAR form. A producer's convention, not the specification's.
    let text = format!("key of the table `NWDF indexes`: {TARGET}, KeyColumn `index_uid`");
    let table = sheet(vec![column("index", Some(&text), Vec::new())]);
    let declaration = declared(&table, None).expect("followed");
    assert_eq!(
        declaration.target,
        LinkTarget::Key {
            table_uri: TARGET.to_string()
        }
    );
    assert_eq!(declaration.convention, Convention::DescriptionText);
    assert_eq!(
        declaration.warnings,
        vec![LinkWarning::ByConvention, LinkWarning::KindNotAnIdentifier]
    );
    assert_eq!(declaration.key_named.as_deref(), Some("index_uid"));
}

#[test]
fn a_self_reference_is_a_same_table_link() {
    let text = format!("key of this table (self reference): {OWN}, KeyColumn `id`");
    let table = sheet(vec![column("parent", Some(&text), Vec::new())]);
    let declaration = declared(&table, Some(&kind(IDENTIFIER))).expect("followed");
    assert_eq!(declaration.target, LinkTarget::SameTable);
    assert_eq!(declaration.key_named.as_deref(), Some("id"));

    // "This table" with the uri of another one is not a self reference: refused, not followed.
    let wrong = format!("key of this table (self reference): {TARGET}, KeyColumn `id`");
    let table = sheet(vec![column("parent", Some(&wrong), Vec::new())]);
    assert!(matches!(
        declared(&table, Some(&kind(IDENTIFIER))),
        Err(LinkRefusal::UnreadableDeclaration { .. })
    ));
}

#[test]
fn a_description_that_almost_matches_is_refused_not_guessed() {
    // No backticks around the title: it starts like a declaration and is not one.
    let text = format!("key of the table NWDF indexes: {TARGET}, KeyColumn `index_uid`");
    let table = sheet(vec![column("index", Some(&text), Vec::new())]);
    assert!(matches!(
        declared(&table, Some(&kind(IDENTIFIER))),
        Err(LinkRefusal::UnreadableDeclaration { .. })
    ));
    // A description that does not start like one says nothing.
    let table = sheet(vec![column("index", Some("the index of the rows"), Vec::new())]);
    assert!(
        LinkReaders::builtin()
            .read_column(&table, 0, Some(&kind(IDENTIFIER)))
            .is_none()
    );
}

#[test]
fn facet_and_description_that_disagree_are_refused_by_name() {
    let text = format!("key of the table `other`: {OTHER}, KeyColumn `id`");
    let table = sheet(vec![column("code", Some(&text), vec![qualifier(TARGET)])]);
    assert_eq!(
        declared(&table, Some(&kind(IDENTIFIER))),
        Err(LinkRefusal::ConflictingDeclarations {
            facet: TARGET.to_string(),
            description: OTHER.to_string()
        })
    );

    // The description repeats the facet: B alone, with no warning.
    let text = format!("key of the table `NWDF indexes`: {TARGET}, KeyColumn `id`");
    let table = sheet(vec![column("code", Some(&text), vec![qualifier(TARGET)])]);
    let declaration = declared(&table, Some(&kind(IDENTIFIER))).expect("followed");
    assert_eq!(declaration.convention, Convention::KindAndFacet);
    assert!(declaration.warnings.is_empty());
}

#[test]
fn a_link_by_table_title_warns() {
    let table = sheet(vec![
        column("source_uid", None, Vec::new()),
        column("source_table", None, Vec::new()),
    ]);
    let readers = LinkReaders::builtin();
    let declaration = readers
        .read_column(&table, 0, None)
        .expect("declares")
        .expect("followed");
    assert_eq!(declaration.target, LinkTarget::ByTitleIn { column: 1 });
    assert_eq!(declaration.convention, Convention::SiblingColumn);
    assert_eq!(declaration.warnings, vec![LinkWarning::TargetByTitle]);
    // The column that names the table declares nothing itself.
    assert!(readers.read_column(&table, 1, None).is_none());

    // `source` and `source_uid` both beside `source_table`: which one is it?
    let table = sheet(vec![
        column("source", None, Vec::new()),
        column("source_uid", None, Vec::new()),
        column("source_table", None, Vec::new()),
    ]);
    assert!(matches!(
        readers.read_column(&table, 0, None),
        Some(Err(LinkRefusal::AmbiguousSibling { .. }))
    ));
}

/// A reader of the caller's: every column links by key to `OTHER`.
struct Always;

impl LinkReader for Always {
    fn id(&self) -> &'static str {
        "test"
    }

    fn read(
        &self,
        _table: &TableSheet,
        column: usize,
        _kind: Option<&Resolution>,
    ) -> Option<Result<LinkDeclaration, LinkRefusal>> {
        Some(Ok(LinkDeclaration {
            column,
            target: LinkTarget::Key {
                table_uri: OTHER.to_string(),
            },
            convention: Convention::Other("test"),
            warnings: Vec::new(),
            key_named: None,
        }))
    }
}

#[test]
fn a_callers_link_reader_is_tried_first() {
    let table = sheet(vec![column("code", None, vec![qualifier(TARGET)])]);
    let mut readers = LinkReaders::builtin();
    readers.push(Box::new(Always));
    assert_eq!(
        readers.ids(),
        vec!["test", "kind-and-facet", "description-text", "sibling-column"]
    );
    let declaration = readers
        .read_column(&table, 0, Some(&kind(IDENTIFIER)))
        .expect("declares")
        .expect("followed");
    assert_eq!(declaration.convention, Convention::Other("test"));

    readers.without("test");
    let declaration = readers
        .read_column(&table, 0, Some(&kind(IDENTIFIER)))
        .expect("declares")
        .expect("followed");
    assert_eq!(declaration.convention, Convention::KindAndFacet);
}

fn text_cells(values: &[&str]) -> ColumnCells<'static> {
    ColumnCells {
        values: Cow::Owned(Values::Text(values.iter().map(|s| s.to_string()).collect())),
        per_row: 1,
        null: None,
    }
}

#[test]
fn a_column_of_uris_is_a_uri_column_and_an_empty_cell_does_not_spoil_it() {
    let activity = "eml:///eml23.Activity(00000000-0000-0000-0000-00000000d101)";
    let wellbore = "eml:///witsml21.Wellbore(00000000-0000-0000-0000-00000000d102)";
    assert_eq!(
        uri_column(&text_cells(&[activity, "", wellbore])),
        Some(UriColumn {
            uris: 2,
            empty: 1,
            qualified_types: vec!["eml23.Activity".to_string(), "witsml21.Wellbore".to_string()]
        })
    );
    // One value that is not a uri and the column is not a column of uris.
    assert_eq!(uri_column(&text_cells(&[activity, "not a uri"])), None);
    // A dataspace uri names no object.
    assert_eq!(uri_column(&text_cells(&["eml:///dataspace('x')"])), None);
    // Nothing but empty cells: no uri at all.
    assert_eq!(uri_column(&text_cells(&["", ""])), None);
}
