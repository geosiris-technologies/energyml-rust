//! Facets, read the same way in both releases.
//!
//! The two releases swap the element names of `PropertyKindFacet`: RESQML
//! 2.0.1 writes the kind in `Facet` and the value in `Value` (RESQML TRG 2.0.1
//! §6.36 p. 77); EML 2.3, and so RESQML 2.2, write the value in `Facet` and the
//! kind in `Kind` (EML TRG 2.3 §11.11 p. 294). A value outside the schema's list
//! is an extension (EML TRG 2.3 §11.2 p. 284). Every object here is built by
//! hand: nothing reads a file.

use energyml_domain::property::{self, Facet, FacetValue};
use energyml_domain::table::Table;

fn facet(kind: &str, value: FacetValue) -> Facet {
    Facet {
        kind: kind.to_string(),
        value,
    }
}

#[test]
fn a_2_0_1_facet_reads_its_kind_and_its_value() {
    // RESQML TRG 2.0.1 §6.36 p. 77.
    let object = energyml_resqml_2_0_1::ContinuousProperty {
        uuid: "00000000-0000-0000-0000-000000000001".into(),
        facet: vec![
            energyml_resqml_2_0_1::PropertyKindFacet {
                facet: energyml_resqml_2_0_1::Facet::Direction,
                value: "I".into(),
            },
            energyml_resqml_2_0_1::PropertyKindFacet {
                facet: energyml_resqml_2_0_1::Facet::Qualifier,
                value: "nwdf:vertical".into(),
            },
        ],
        ..Default::default()
    };
    let read = property::read(&object).expect("a property that reads");
    assert_eq!(
        read.facets,
        vec![
            facet("direction", FacetValue::Standard("I".into())),
            facet("qualifier", FacetValue::Extension("nwdf:vertical".into())),
        ]
    );
}

#[test]
fn a_2_2_facet_reads_its_kind_from_kind_and_its_value_from_facet() {
    // EML TRG 2.3 §11.11 p. 294, §11.2 p. 284.
    use energyml_eml_2_3::{FacetExt, FacetKind, PropertyKindFacet};
    let object = energyml_resqml_2_2::ContinuousProperty {
        uuid: "00000000-0000-0000-0000-000000000002".into(),
        facet: vec![
            PropertyKindFacet {
                facet: FacetExt::K,
                kind: FacetKind::Direction,
            },
            PropertyKindFacet {
                facet: FacetExt::Other("nwdf:vertical".into()),
                kind: FacetKind::Qualifier,
            },
        ],
        ..Default::default()
    };
    let read = property::read(&object).expect("a property that reads");
    assert_eq!(
        read.facets,
        vec![
            facet("direction", FacetValue::Standard("K".into())),
            facet("qualifier", FacetValue::Extension("nwdf:vertical".into())),
        ]
    );
}

#[test]
fn a_column_reads_its_facets_through_the_same_door() {
    // The form of the convention B of a column that names its link in a
    // `qualifier` facet: the value is a URI, so it is an extension.
    use energyml_eml_2_3::{
        AbstractValueArray, Column, ColumnBasedTable, FacetExt, FacetKind, IntegerXmlArray,
        PropertyKindFacet,
    };
    let uri = "eml:///eml23.ColumnBasedTable(00000000-0000-0000-0000-000000000009)";
    let object = ColumnBasedTable {
        uuid: "00000000-0000-0000-0000-000000000003".into(),
        column: vec![Column {
            title: Some("link".into()),
            value_count_per_row: 1,
            values: Some(AbstractValueArray::IntegerXmlArray(IntegerXmlArray {
                values: vec![1],
                count_per_value: 1,
                ..Default::default()
            })),
            facet: vec![PropertyKindFacet {
                facet: FacetExt::Other(uri.into()),
                kind: FacetKind::Qualifier,
            }],
            ..Default::default()
        }],
        ..Default::default()
    };
    let table = Table::read(&object).expect("a table that reads");
    assert_eq!(
        table.columns[0].facets,
        vec![facet("qualifier", FacetValue::Extension(uri.into()))]
    );
}

#[test]
fn a_facet_value_says_its_text_and_whether_it_is_an_extension() {
    let standard = FacetValue::Standard("net".into());
    let extension = FacetValue::Extension("nwdf:vertical".into());
    assert_eq!(standard.as_str(), "net");
    assert!(!standard.is_extension());
    assert_eq!(extension.to_string(), "nwdf:vertical");
    assert!(extension.is_extension());
}
