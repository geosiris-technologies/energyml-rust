//! What the features say about themselves, said once.
//!
//! A SIGEOL export repeats « Marnes et Caillasses », its referential URL and its
//! two ages on each of 10 374 features. Writing that per point would put the
//! same six strings in a package ten thousand times; writing it only in a title
//! would make it unreadable by anything but a person.
//!
//! So: **one `ColumnBasedTable` for the whole import, one row per group**, and
//! each `PointSetRepresentation` carries a `DiscreteProperty` saying which row
//! it is. That is the shape RESQML 2.2 has.
//!
//! # `CategoricalProperty` is 2.0.1 and is gone
//!
//! It was the obvious answer and it does not exist here: `CategoricalProperty`
//! and `StringTableLookup` are RESQML 2.0.1 types, absent from 2.2. What 2.2
//! offers instead is `DiscreteProperty.CategoryLookup` - a reference to an
//! `eml23.ColumnBasedTable` - which is exactly the join, and the mistake was
//! caught before it was written.
//!
//! # A constant array, and not ten thousand copies of one number
//!
//! Every point of one set is in the same row, so its property is constant. EML
//! 2.3 has `IntegerConstantArray` - a value and a count - so the file says *this
//! is constant* in nine bytes, touches no HDF5, and a reader that expands it
//! gets one index per node. The alternative, writing 1 285 identical integers
//! into a data file, would have been the same fact told expensively and less
//! clearly.
//!
//! # The key column is not the row's position
//!
//! `ColumnBasedTable` has `KeyColumn` beside `Column`, and it is what makes the
//! table a lookup: a stored value is matched against the key, **not counted from
//! the top**. Reading it as a position is a bug this workspace has already paid
//! for - every facies of `h+g_PaleoGTM` came out labelled with its neighbour's
//! name, because that file numbers its keys from 1. So this writes an explicit
//! key column and the property carries the *key*, never the index.
//!
//! # The property kinds are the standard's own
//!
//! Every `Column` requires a `PropertyKind` reference. They are taken from the
//! Energistics dictionary - and four of them are the very ones the BRGM
//! pipeline's own `ColumnBasedTable` references, so a consumer resolves ours the
//! way it already resolves theirs.

use energyml_eml_2_3 as eml;
use energyml_resqml_2_2 as resqml;

/// A well-known `PropertyKind` of the Energistics 2.3 dictionary.
///
/// Uuids taken from `rc/wellKnownData/PropertyKindDictionary_v2.3.xml` and
/// **not invented**: a package referencing a kind that is not in the dictionary
/// has a dangling reference that no store can resolve, which is the very fault
/// the graph now distinguishes (see `energyml_storage::graph`).
pub struct Kind {
    /// The dictionary uuid.
    pub uuid: &'static str,
    /// Its title, carried into the reference so a reader sees a word.
    pub title: &'static str,
}

/// `code` - an identifier without a unit. The key column's kind.
pub const CODE: Kind = Kind {
    uuid: "5f78f66a-ed1b-4827-a868-beb989febb31",
    title: "code",
};
/// `formation name` - the lithostratigraphic unit. Used by the BRGM pipeline for
/// exactly this.
pub const FORMATION_NAME: Kind = Kind {
    uuid: "48a02f88-8cfe-44e5-acb3-45038443d8f7",
    title: "formation name",
};
/// `name` - the stage.
pub const NAME: Kind = Kind {
    uuid: "0eb70eac-c3d2-4448-b617-10260c4abee3",
    title: "name",
};
/// `geologic age` - the bounds, in millions of years.
pub const GEOLOGIC_AGE: Kind = Kind {
    uuid: "e185ec6b-27c2-4086-8e1f-48810130b2f0",
    title: "geologic age",
};
/// `count` - how many source features fed a row.
pub const COUNT: Kind = Kind {
    uuid: "ee1efc57-1990-4c63-af90-974f186d1644",
    title: "count",
};
/// `description` - the referential URL, and anything else that is prose.
pub const DESCRIPTION: Kind = Kind {
    uuid: "89230474-eb6e-417f-8fce-f2f00ee6fafb",
    title: "description",
};
/// `unknown` - what the dictionary has no word for. The BRGM pipeline uses it
/// for its own code column, so it is not a last resort so much as the honest
/// answer.
pub const UNKNOWN: Kind = Kind {
    uuid: "73f28137-03ef-45b4-8e85-31869c7d36fd",
    title: "unknown",
};

impl Kind {
    fn reference(&self) -> eml::DataObjectReference {
        eml::DataObjectReference {
            uuid: self.uuid.to_string(),
            qualified_type: "eml23.PropertyKind".to_string(),
            title: self.title.to_string(),
            ..Default::default()
        }
    }
}

/// One row: everything one group says about itself.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// The key a property refers to this row by. **Not its position.**
    pub key: i64,
    /// The lithostratigraphic unit.
    pub unit: String,
    /// Its entry in the producer's referential, when there is one.
    pub referential: String,
    /// The stage the surface lies in.
    pub stage: String,
    /// `Top` or `Bottom`.
    pub side: String,
    /// The unit's young bound, in Ma.
    pub young_ma: f64,
    /// Its old bound, in Ma.
    pub old_ma: f64,
    /// How many source features fed this row, before deduplication.
    pub features: i64,
}

/// Build the table from the rows, in the order given.
///
/// The caller has already made the rows distinct - that is what *avoiding
/// duplicate entries* means, and it cannot be done here: this module does not
/// know which two groups are the same thing.
pub fn build(uuid: &str, rows: &[Row], citation: eml::Citation) -> eml::ColumnBasedTable {
    let text = |title: &str, kind: &Kind, values: Vec<String>| eml::Column {
        title: Some(title.to_string()),
        value_count_per_row: 1,
        property_kind: kind.reference(),
        values: Some(eml::AbstractValueArray::StringXmlArray(eml::StringXmlArray {
            count_per_value: 1,
            values,
            ..Default::default()
        })),
        ..Default::default()
    };
    let integers = |title: &str, kind: &Kind, values: Vec<i64>| eml::Column {
        title: Some(title.to_string()),
        value_count_per_row: 1,
        property_kind: kind.reference(),
        values: Some(eml::AbstractValueArray::IntegerXmlArray(eml::IntegerXmlArray {
            count_per_value: 1,
            values,
            ..Default::default()
        })),
        ..Default::default()
    };
    let reals = |title: &str, kind: &Kind, values: Vec<f64>| eml::Column {
        title: Some(title.to_string()),
        value_count_per_row: 1,
        property_kind: kind.reference(),
        values: Some(eml::AbstractValueArray::FloatingPointXmlArray(
            eml::FloatingPointXmlArray {
                count_per_value: 1,
                values,
                ..Default::default()
            },
        )),
        ..Default::default()
    };

    eml::ColumnBasedTable {
        uuid: uuid.to_string(),
        schema_version: "2.3".to_string(),
        citation,
        // **The key column, and it is why this is a lookup at all.** Without it
        // a consumer has to count rows from the top, which is the reading that
        // mislabelled every facies of `h+g_PaleoGTM`.
        key_column: vec![integers("index", &CODE, rows.iter().map(|r| r.key).collect())],
        column: vec![
            text(
                "unité",
                &FORMATION_NAME,
                rows.iter().map(|r| r.unit.clone()).collect(),
            ),
            text(
                "référentiel",
                &DESCRIPTION,
                rows.iter().map(|r| r.referential.clone()).collect(),
            ),
            text("étage", &NAME, rows.iter().map(|r| r.stage.clone()).collect()),
            text("côté", &UNKNOWN, rows.iter().map(|r| r.side.clone()).collect()),
            reals(
                "âge sommet (Ma)",
                &GEOLOGIC_AGE,
                rows.iter().map(|r| r.young_ma).collect(),
            ),
            reals(
                "âge base (Ma)",
                &GEOLOGIC_AGE,
                rows.iter().map(|r| r.old_ma).collect(),
            ),
            // What deduplication removed, per row rather than only in the
            // activity: a package read on its own must be able to say that
            // 1 285 places came from 10 354 borehole passes.
            integers("passes", &COUNT, rows.iter().map(|r| r.features).collect()),
        ],
        ..Default::default()
    }
}

/// The property that says which row a point set is.
///
/// Constant over the set, and written as such: one value and a count, no
/// external file. `key` is the table's key and not the row's position.
pub fn property(
    uuid: &str,
    key: i64,
    points: usize,
    representation: &eml::DataObjectReference,
    table: &eml::DataObjectReference,
    citation: eml::Citation,
) -> resqml::DiscreteProperty {
    resqml::DiscreteProperty {
        uuid: uuid.to_string(),
        schema_version: "2.2".to_string(),
        citation,
        // On the nodes: a point set's indexable elements are its points, and
        // there is nothing else to attach to.
        indexable_element: eml::IndexableElement::Nodes,
        value_count_per_indexable_element: vec![1],
        property_kind: CODE.reference(),
        supporting_representation: representation.clone(),
        // The join. This is the 2.2 replacement for the 2.0.1
        // `CategoricalProperty` + `StringTableLookup` pair.
        category_lookup: Some(table.clone()),
        values_for_patch: vec![resqml::AbstractValueArray::IntegerConstantArray(
            eml::IntegerConstantArray {
                value: key,
                count: points as i64,
                ..Default::default()
            },
        )],
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn citation(title: &str) -> eml::Citation {
        eml::Citation {
            title: title.to_string(),
            originator: "BRGM".to_string(),
            creation: "2026-09-08T00:00:00Z".to_string(),
            format: energyml_core::CITATION_FORMAT.to_string(),
            ..Default::default()
        }
    }

    fn rows() -> Vec<Row> {
        vec![
            Row {
                key: 1,
                unit: "Marnes et Caillasses".into(),
                referential: "https://referentiel-lithostrat.brgm-rec.fr/…/67475488".into(),
                stage: "Bartonien".into(),
                side: "Top".into(),
                young_ma: 37.8,
                old_ma: 47.8,
                features: 10_354,
            },
            Row {
                key: 2,
                unit: "Calcaire grossier".into(),
                referential: "https://referentiel-lithostrat.brgm-rec.fr/…/741641e1".into(),
                stage: "Lutétien".into(),
                side: "Top".into(),
                young_ma: 41.2,
                old_ma: 47.8,
                features: 5_634,
            },
        ]
    }

    fn table() -> eml::ColumnBasedTable {
        build(
            "00000000-0000-4000-8000-0000000000t1",
            &rows(),
            citation("imported units"),
        )
    }

    #[test]
    fn the_table_has_a_key_column_and_it_is_not_the_row_position() {
        // What makes it a lookup. Without it a consumer counts from the top,
        // which is the reading that mislabelled every facies of `h+g_PaleoGTM`
        // because that file numbers its keys from 1 - as this one does.
        let made = table();
        assert_eq!(made.key_column.len(), 1);
        let Some(eml::AbstractValueArray::IntegerXmlArray(keys)) = &made.key_column[0].values else {
            panic!("expected integer keys")
        };
        assert_eq!(keys.values, vec![1, 2]);
        // Deliberately not 0-based: a test that used 0..n would pass whether the
        // property carried the key or the position.
        assert_ne!(keys.values[0], 0);
    }

    #[test]
    fn every_column_names_a_kind_of_the_official_dictionary() {
        // A kind that is not in the dictionary is a dangling reference no store
        // can resolve - the very fault the graph now tells apart from a real
        // absence. Four of these are the ones the BRGM pipeline's own table
        // references, so a consumer resolves ours the way it resolves theirs.
        let made = table();
        let known = [
            CODE.uuid,
            FORMATION_NAME.uuid,
            NAME.uuid,
            GEOLOGIC_AGE.uuid,
            COUNT.uuid,
            DESCRIPTION.uuid,
            UNKNOWN.uuid,
        ];
        for column in made.key_column.iter().chain(made.column.iter()) {
            assert_eq!(column.property_kind.qualified_type, "eml23.PropertyKind");
            assert!(
                known.contains(&column.property_kind.uuid.as_str()),
                "{:?} names {} which is not one of ours",
                column.title,
                column.property_kind.uuid
            );
        }
    }

    #[test]
    fn every_column_has_one_value_per_row_and_no_column_is_short() {
        // A column shorter than the others silently shifts every value below it,
        // and nothing validates the alignment: row identity inside a table is
        // positional (common TRG 2.3 §9.1), so a missing cell is a wrong answer
        // rather than a missing one.
        let made = table();
        for column in made.key_column.iter().chain(made.column.iter()) {
            assert_eq!(column.value_count_per_row, 1, "{:?}", column.title);
            let count = match &column.values {
                Some(eml::AbstractValueArray::StringXmlArray(a)) => a.values.len(),
                Some(eml::AbstractValueArray::IntegerXmlArray(a)) => a.values.len(),
                Some(eml::AbstractValueArray::FloatingPointXmlArray(a)) => a.values.len(),
                other => panic!("{:?} carries {other:?}", column.title),
            };
            assert_eq!(count, 2, "{:?} has {count} values for 2 rows", column.title);
        }
    }

    #[test]
    fn the_property_is_constant_and_says_so_rather_than_repeating_itself() {
        // 1 285 identical integers in a data file would be the same fact told
        // expensively. `IntegerConstantArray` is a value and a count.
        let made = property(
            "00000000-0000-4000-8000-0000000000p1",
            2,
            1285,
            &eml::DataObjectReference {
                uuid: "rep".into(),
                qualified_type: "resqml22.PointSetRepresentation".into(),
                ..Default::default()
            },
            &eml::DataObjectReference {
                uuid: "00000000-0000-4000-8000-0000000000t1".into(),
                qualified_type: "eml23.ColumnBasedTable".into(),
                ..Default::default()
            },
            citation("unit"),
        );
        let resqml::AbstractValueArray::IntegerConstantArray(values) = &made.values_for_patch[0] else {
            panic!("expected a constant array")
        };
        // The **key**, not the row index: row 2 of `rows()` has key 2, and a
        // property carrying `1` would resolve to the wrong unit.
        assert_eq!(values.value, 2);
        assert_eq!(values.count, 1285);
        assert_eq!(made.indexable_element, eml::IndexableElement::Nodes);
    }

    #[test]
    fn the_property_joins_the_representation_to_the_table() {
        // `CategoryLookup` is the 2.2 join, and the reason: `CategoricalProperty`
        // and `StringTableLookup` are 2.0.1 types that do not exist here.
        let made = property(
            "p",
            1,
            10,
            &eml::DataObjectReference {
                uuid: "rep".into(),
                ..Default::default()
            },
            &eml::DataObjectReference {
                uuid: "tab".into(),
                ..Default::default()
            },
            citation("x"),
        );
        assert_eq!(made.supporting_representation.uuid, "rep");
        assert_eq!(made.category_lookup.as_ref().expect("a lookup").uuid, "tab");
    }
}
