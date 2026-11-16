//! The class of a table's columns (`table::class`): by the family of the array
//! and the ancestry of the property kind, never by the title.
//!
//! Every table sheet is built by hand, column by column; kinds are resolved by
//! `Resolver` over the shipped dictionary, which holds the kinds named here
//! (property kind dictionary, `property_kinds.tsv`: `measured depth` line 1286,
//! whose parent is `length`). Nothing is read from a file.

use std::sync::Arc;

use energyml_core::registry::DynObject;
use energyml_domain::package_index::SheetKind;
use energyml_domain::property_kind::{Resolution, Resolver};
use energyml_domain::source::{NoObjects, ObjectSource};
use energyml_domain::table::{
    ColumnClass, ColumnSheet, DepthKind, IntervalColumns, MEASURED_DEPTH, TableSheet, classify_column,
    curve_pairs, md_columns, resolve_kinds,
};
use energyml_domain::values::Family;
use energyml_eml_2_3 as eml23;

/// `length`, the parent of `measured depth` (TSV line 1286).
const LENGTH: &str = "4364b378-899a-403a-8401-b06abd4fc0cf";

fn column(title: &str, kind: Option<&str>, family: Family, uom: Option<&str>) -> ColumnSheet {
    ColumnSheet {
        title: Some(Arc::from(title)),
        description: None,
        key: false,
        kind: kind.map(|uuid| SheetKind::Object {
            uuid: Arc::from(uuid),
            title: None,
        }),
        uom: uom.map(Arc::from),
        values_per_row: 1,
        facets: Box::new([]),
        family: Some(family),
        values: Some(3),
        external: false,
    }
}

fn sheet(columns: Vec<ColumnSheet>) -> TableSheet {
    TableSheet {
        uri: "eml:///eml23.ColumnBasedTable(00000000-0000-0000-0000-00000000c101)".into(),
        title: Some(Arc::from("a table")),
        qualified_type: Arc::from("eml23.ColumnBasedTable"),
        columns: columns.into(),
    }
}

/// The classes of every column of a sheet, kinds resolved through `resolver`.
fn classes(sheet: &TableSheet, resolver: &Resolver<'_>) -> Vec<ColumnClass> {
    let kinds = resolve_kinds(sheet, resolver);
    sheet
        .columns
        .iter()
        .zip(&kinds)
        .map(|(c, k)| classify_column(c, k.as_ref(), None))
        .collect()
}

#[test]
fn a_measured_depth_column_is_a_depth_by_its_kind_not_its_title() {
    let sheet = sheet(vec![
        column("foo", Some(MEASURED_DEPTH), Family::Floating, Some("m")),
        column("depth", Some(LENGTH), Family::Floating, None),
    ]);
    let resolver = Resolver::new(&NoObjects);
    let classes = classes(&sheet, &resolver);
    assert_eq!(
        classes[0],
        ColumnClass::Depth {
            uom: Some("m".to_string()),
            kind: DepthKind::Md
        }
    );
    // `length` is the parent of `measured depth`, not a depth: the title `depth` decides nothing.
    assert_eq!(classes[1], ColumnClass::Real { uom: None });
}

/// A package that holds only property kinds.
struct Package(Vec<(String, Box<dyn DynObject>)>);

impl ObjectSource for Package {
    fn object(&self, uuid: &str) -> Option<Box<dyn DynObject>> {
        self.0
            .iter()
            .find(|(held, _)| held.eq_ignore_ascii_case(uuid))
            .map(|(_, object)| object.clone_box())
    }
}

#[test]
fn a_kind_below_measured_depth_is_a_depth_too() {
    // The package defines its own kind, a child of `measured depth` (parenthood is the
    // `Parent` reference of an eml 2.3 `PropertyKind`, EML TRG 2.3 section 11.9).
    let local = "00000000-0000-0000-0000-00000000c1a1";
    let kind = eml23::PropertyKind {
        uuid: local.to_string(),
        citation: eml23::Citation {
            title: "driller's depth".to_string(),
            ..Default::default()
        },
        parent: Some(eml23::DataObjectReference {
            uuid: MEASURED_DEPTH.to_string(),
            qualified_type: "eml23.PropertyKind".to_string(),
            title: "measured depth".to_string(),
            ..Default::default()
        }),
        ..Default::default()
    };
    let package = Package(vec![(local.to_string(), Box::new(kind) as Box<dyn DynObject>)]);
    let sheet = sheet(vec![column("d", Some(local), Family::Floating, Some("m"))]);
    let resolver = Resolver::new(&package);
    let kinds = resolve_kinds(&sheet, &resolver);
    let resolution: &Resolution = kinds[0].as_ref().expect("the local kind resolves");
    assert_eq!(resolution.kind().uuid, local, "the column's own kind comes first");
    assert_eq!(
        classify_column(&sheet.columns[0], Some(resolution), None),
        ColumnClass::Depth {
            uom: Some("m".to_string()),
            kind: DepthKind::Md
        },
        "the walk up the chain, not only its first element"
    );
}

#[test]
fn a_vector_column_is_a_vector_of_its_class() {
    // Common TRG 2.3 section 9.1 p. 190: `ValueCountPerRow` values per row.
    let mut vector = column("xyz", Some(LENGTH), Family::Floating, None);
    vector.values_per_row = 3;
    let sheet = sheet(vec![vector]);
    let classes = classes(&sheet, &Resolver::new(&NoObjects));
    assert_eq!(
        classes[0],
        ColumnClass::Vector {
            of: Box::new(ColumnClass::Real { uom: None }),
            n: 3
        }
    );
}

#[test]
fn two_md_columns_make_a_pair_three_are_ambiguous() {
    let resolver = Resolver::new(&NoObjects);
    let name = column("name", None, Family::Text, None);
    let md = |title: &str| column(title, Some(MEASURED_DEPTH), Family::Floating, Some("m"));

    let one = sheet(vec![name.clone(), md("a")]);
    assert_eq!(
        md_columns(&one, &resolve_kinds(&one, &resolver)),
        IntervalColumns::Point { md: 1 }
    );

    // Titles that would tell top from base are in the wrong order on purpose: the order of the
    // columns decides.
    let two = sheet(vec![name.clone(), md("base"), md("top")]);
    assert_eq!(
        md_columns(&two, &resolve_kinds(&two, &resolver)),
        IntervalColumns::Pair { top: 1, base: 2 }
    );

    let three = sheet(vec![md("a"), md("b"), name, md("c")]);
    assert_eq!(
        md_columns(&three, &resolve_kinds(&three, &resolver)),
        IntervalColumns::Ambiguous(vec![0, 1, 3])
    );
}

#[test]
fn an_md_kind_in_seconds_is_not_an_md_column() {
    let resolver = Resolver::new(&NoObjects);
    let sheet = sheet(vec![column(
        "t",
        Some(MEASURED_DEPTH),
        Family::Floating,
        Some("s"),
    )]);
    assert_eq!(
        md_columns(&sheet, &resolve_kinds(&sheet, &resolver)),
        IntervalColumns::None,
        "the kind and the unit contradict each other: nothing is guessed"
    );
}

#[test]
fn a_two_number_table_offers_one_curve_and_krpc_offers_three() {
    let resolver = Resolver::new(&NoObjects);

    // (a) a key and a value, both real: one pair.
    let mut key = column("x", None, Family::Floating, None);
    key.key = true;
    let a = sheet(vec![key, column("y", None, Family::Floating, None)]);
    assert_eq!(curve_pairs(&a, &classes(&a, &resolver)), vec![(0, 1)]);

    // (b) a KrPc table: the key `Sw` against three columns.
    let mut sw = column("Sw", None, Family::Floating, None);
    sw.key = true;
    let b = sheet(vec![
        sw,
        column("krw", None, Family::Floating, None),
        column("krow", None, Family::Floating, None),
        column("pc", None, Family::Floating, None),
    ]);
    assert_eq!(
        curve_pairs(&b, &classes(&b, &resolver)),
        vec![(0, 1), (0, 2), (0, 3)]
    );

    // A code is not a curve: an integer key and one real column make no pair.
    let mut code = column("code", None, Family::Integer, None);
    code.key = true;
    let c = sheet(vec![code, column("y", None, Family::Floating, None)]);
    assert!(curve_pairs(&c, &classes(&c, &resolver)).is_empty());
}
