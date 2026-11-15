//! What each dimension of a vector property's components stands for
//! (`component_axes`) and the one reading of convention B
//! (`property::qualifier_link`).
//!
//! Sources: RESQML TRG 2.2 §7.1 p. 222 (labels per component, the first given
//! dimension the slowest); EML TRG 2.3 §11.11 p. 294 (*"Don't use several facets
//! of the same kind for the same property."*), §11.3 p. 285 (`qualifier`),
//! §11.2 p. 284 (`FacetExt`, open); WITSML TRG 2.1 §7.12 p. 162
//! (`LogChannelAxis`: the order of several axes is not stated). Every object is
//! built by hand and held by an in-memory package: no file is read.

use std::sync::Arc;

use energyml_core::xml::XmlText;
use energyml_domain::component_axes::{
    AxisLink, AxisOrigin, AxisRefusal, AxisSource, AxisSources, AxisSubject, AxisValues, ComponentAxis,
    candidate_tables,
};
use energyml_domain::package_index::PackageIndex;
use energyml_domain::property::link::{QualifierRefusal, qualifier_link};
use energyml_domain::property::shape::ElementShape;
use energyml_domain::property::{Facet, FacetValue};
use energyml_domain::source::{ArraySource, ObjectSource};
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_2 as resqml22;
use energyml_storage::EpcStore;
use energyml_witsml_2_1 as witsml;

fn unit(code: &str) -> eml23::UnitOfMeasureExt {
    eml23::UnitOfMeasureExt::from_xml_text(code).expect("a unit")
}

fn citation(title: &str) -> eml23::Citation {
    eml23::Citation {
        title: title.to_string(),
        originator: "test".into(),
        creation: "2026-11-14T00:00:00Z".into(),
        format: "test".into(),
        ..Default::default()
    }
}

fn empty_store() -> EpcStore {
    let bytes = energyml_storage::extract::empty_package(None).expect("an empty package is written");
    EpcStore::from_reader(
        Box::new(std::io::Cursor::new(bytes)),
        "built.epc",
        Arc::new(energyml_schemas::all()),
    )
    .expect("it opens")
}

fn put(store: &mut EpcStore, object: impl energyml_core::registry::DynObject + 'static) {
    store.put(Box::new(object)).expect("the package takes it");
}

fn uri_of(store: &EpcStore, uuid: &str) -> String {
    store
        .find_by_uuid(uuid)
        .expect("the object is in the store")
        .uri
        .clone()
}

fn qualifier(value: &str) -> Facet {
    Facet {
        kind: "qualifier".into(),
        value: FacetValue::Extension(value.into()),
    }
}

fn shape(dims: &[u64]) -> ElementShape {
    ElementShape::of(dims, &[]).expect("a shape")
}

fn subject<'a>(
    uri: &'a str,
    shape: &'a ElementShape,
    facets: &'a [Facet],
    links: &'a [AxisLink],
    rows: Option<u64>,
) -> AxisSubject<'a> {
    AxisSubject {
        uri,
        shape,
        facets,
        links,
        rows,
    }
}

fn resolve(
    store: &EpcStore,
    subject: &AxisSubject<'_>,
) -> Result<energyml_domain::component_axes::ResolvedAxes, AxisRefusal> {
    AxisSources::with_builtins().resolve(subject, store as &dyn ObjectSource, store as &dyn ArraySource)
}

// ---------------------------------------------------------------------------
// Convention B, read once (EML TRG 2.3 §11.11 p. 294, §11.3 p. 285, §11.2 p. 284)
// ---------------------------------------------------------------------------

const TABLE_URI: &str = "eml:///eml23.ColumnBasedTable(00000000-0000-0000-0000-0000000000c1)";

/// Break it: make `qualifier_link` return `Ok(None)` for a facet that holds a uri.
#[test]
fn one_qualifier_uri_is_the_link() {
    let facets = [qualifier(TABLE_URI)];
    assert_eq!(qualifier_link(&facets), Ok(Some(TABLE_URI.to_string())));
}

/// Break it: treat every `qualifier` facet as a link.
#[test]
fn an_ordinary_qualifier_is_no_link() {
    let facets = [qualifier("cumulative")];
    assert_eq!(qualifier_link(&facets), Ok(None));
    assert_eq!(qualifier_link(&[]), Ok(None));
}

/// Break it: ignore the ordinary qualifiers when counting.
#[test]
fn a_uri_beside_another_qualifier_is_refused() {
    let facets = [qualifier(TABLE_URI), qualifier("measured")];
    assert_eq!(qualifier_link(&facets), Err(QualifierRefusal::Several { count: 2 }));
}

/// Break it: return `Ok(None)` for a value that starts like a uri and does not parse.
#[test]
fn a_malformed_uri_is_refused_by_name() {
    let facets = [qualifier("eml:///nonsense")];
    assert!(matches!(
        qualifier_link(&facets),
        Err(QualifierRefusal::MalformedUri { .. })
    ));
}

/// Break it: accept a uri that names no object.
#[test]
fn a_dataspace_uri_is_not_an_object() {
    let facets = [qualifier("eml:///dataspace('demo')")];
    assert!(matches!(
        qualifier_link(&facets),
        Err(QualifierRefusal::MalformedUri { .. })
    ));
}

/// A facet of another kind that holds a uri is not a link.
///
/// Break it: look at the value only, not at the kind.
#[test]
fn only_a_qualifier_facet_links() {
    let facets = [Facet {
        kind: "direction".into(),
        value: FacetValue::Extension(TABLE_URI.into()),
    }];
    assert_eq!(qualifier_link(&facets), Ok(None));
}

// ---------------------------------------------------------------------------
// WITSML axes (WITSML TRG 2.1 §7.12 p. 162)
// ---------------------------------------------------------------------------

const CHANNEL: &str = "00000000-0000-0000-0000-0000000000a1";

fn axis(start: f64, spacing: f64, count: i64, uid: &str) -> witsml::LogChannelAxis {
    witsml::LogChannelAxis {
        axis_start: start,
        axis_spacing: spacing,
        axis_count: count,
        axis_name: Some("lateral".into()),
        axis_property_kind: "length".into(),
        axis_uom: unit("m"),
        uid: uid.into(),
    }
}

fn channel_with(axes: Vec<witsml::LogChannelAxis>) -> (EpcStore, String) {
    let mut store = empty_store();
    put(
        &mut store,
        witsml::Channel {
            citation: citation("an array channel"),
            uuid: CHANNEL.into(),
            schema_version: "2.1".into(),
            mnemonic: "UDAR".into(),
            global_mnemonic: "UDAR".into(),
            axis_definition: axes,
            ..Default::default()
        },
    );
    let uri = uri_of(&store, CHANNEL);
    (store, uri)
}

/// WITSML TRG 2.1 §7.12 p. 162. Break it: ignore `AxisStart`.
#[test]
fn a_witsml_axis_is_start_spacing_count() {
    let (store, uri) = channel_with(vec![axis(-40.0, 1.0, 81, "a1")]);
    let shape = shape(&[81]);
    let links = [AxisLink::WitsmlChannel { channel: uri.clone() }];
    let resolved = resolve(&store, &subject(&uri, &shape, &[], &links, None)).expect("resolved");
    assert_eq!(
        resolved.axes[0].values,
        AxisValues::Regular {
            start: -40.0,
            step: 1.0,
            count: 81
        }
    );
    assert_eq!(resolved.axes[0].uom.as_deref(), Some("m"));
    assert_eq!(
        resolved.axes[0].origin,
        AxisOrigin::WitsmlAxisDefinition {
            channel: uri,
            uid: "a1".into()
        }
    );
    assert!(resolved.ranks_only().is_empty());
}

/// WITSML TRG 2.1 §7.12 p. 162 does not say the order; S9. Break it: take the first as the slowest.
#[test]
fn two_witsml_axes_are_refused_by_name() {
    let (store, uri) = channel_with(vec![axis(0.0, 1.0, 3, "a1"), axis(0.0, 1.0, 2, "a2")]);
    let shape = shape(&[3]);
    let links = [AxisLink::WitsmlChannel { channel: uri.clone() }];
    let refusal = resolve(&store, &subject(&uri, &shape, &[], &links, None)).expect_err("refused");
    assert!(matches!(refusal, AxisRefusal::OrderUnstated { axes: 2 }), "{refusal}");
}

// ---------------------------------------------------------------------------
// A paired property, constant along the index
// ---------------------------------------------------------------------------

const PAIRED: &str = "00000000-0000-0000-0000-0000000000b1";

/// A property of `count` angles per row on `rows` rows, in `uom`, the values
/// given row by row.
fn angles(uuid: &str, count: i64, uom: &str, values: Vec<f64>) -> resqml22::ContinuousProperty {
    resqml22::ContinuousProperty {
        uuid: uuid.into(),
        citation: citation("angles"),
        indexable_element: eml23::IndexableElement::Nodes,
        value_count_per_indexable_element: vec![count],
        uom: unit(uom),
        values_for_patch: vec![resqml22::AbstractValueArray::FloatingPointXmlArray(
            eml23::FloatingPointXmlArray {
                count_per_value: 1,
                values,
                ..Default::default()
            },
        )],
        ..Default::default()
    }
}

/// RESQML TRG 2.2 §7.1 p. 222 (the same vector on every element). Break it: compare only row 0 with itself.
#[test]
fn a_paired_axis_must_hold_along_the_index() {
    let mut values: Vec<f64> = (0..16).map(|k| k as f64 * 22.5).collect::<Vec<_>>().repeat(5);
    values[3 * 16] += 0.1;
    let mut store = empty_store();
    put(&mut store, angles(PAIRED, 16, "dega", values));
    let uri = uri_of(&store, PAIRED);
    let shape = shape(&[16]);
    let links = [AxisLink::PairedProperty {
        property: uri.clone(),
        dim: 0,
    }];
    let refusal = resolve(&store, &subject("p", &shape, &[], &links, Some(5))).expect_err("refused");
    assert!(
        matches!(refusal, AxisRefusal::VariesAlongIndex { dim: 0, row: 3 }),
        "{refusal}"
    );
}

/// A paired property with as many rows as the subject, and an `NaN` row skipped.
///
/// Break it: refuse a row that is entirely `NaN`.
#[test]
fn a_paired_axis_skips_an_absent_row() {
    let mut values: Vec<f64> = (0..16).map(|k| k as f64 * 22.5).collect::<Vec<_>>().repeat(3);
    for value in &mut values[16..32] {
        *value = f64::NAN;
    }
    let mut store = empty_store();
    put(&mut store, angles(PAIRED, 16, "dega", values));
    let uri = uri_of(&store, PAIRED);
    let shape = shape(&[16]);
    let links = [AxisLink::PairedProperty {
        property: uri,
        dim: 0,
    }];
    let resolved = resolve(&store, &subject("p", &shape, &[], &links, Some(3))).expect("resolved");
    assert_eq!(resolved.axes[0].values.len(), 16);
}

/// A paired property of 3 rows against a subject of 4: refused by name.
///
/// Break it: ignore `rows`.
#[test]
fn a_paired_axis_must_have_the_subjects_rows() {
    let values: Vec<f64> = (0..16).map(|k| k as f64 * 22.5).collect::<Vec<_>>().repeat(3);
    let mut store = empty_store();
    put(&mut store, angles(PAIRED, 16, "dega", values));
    let uri = uri_of(&store, PAIRED);
    let shape = shape(&[16]);
    let links = [AxisLink::PairedProperty {
        property: uri,
        dim: 0,
    }];
    let refusal = resolve(&store, &subject("p", &shape, &[], &links, Some(4))).expect_err("refused");
    assert!(
        matches!(refusal, AxisRefusal::CountMismatch { axis: 3, shape: 4, .. }),
        "{refusal}"
    );
}

/// Sixteen angles `k x 2 pi / 16` make a full turn; in degrees the period is 360.
///
/// Break it: give the period in radians for degrees.
#[test]
fn sixteen_regular_angles_are_cyclic() {
    let radians: Vec<f64> = (0..16).map(|k| k as f64 * std::f64::consts::TAU / 16.0).collect();
    let degrees: Vec<f64> = (0..16).map(|k| k as f64 * 22.5).collect();
    for (uom, values, period) in [
        ("rad", radians, std::f64::consts::TAU),
        ("dega", degrees, 360.0),
    ] {
        let mut store = empty_store();
        put(&mut store, angles(PAIRED, 16, uom, values));
        let uri = uri_of(&store, PAIRED);
        let shape = shape(&[16]);
        let links = [AxisLink::PairedProperty {
            property: uri,
            dim: 0,
        }];
        let resolved = resolve(&store, &subject("p", &shape, &[], &links, Some(1))).expect("resolved");
        let cyclic = resolved.axes[0].cyclic.expect("a full turn");
        assert!((cyclic - period).abs() < 1e-9, "{uom}: {cyclic}");
    }
}

/// Eight angles over a half turn are regular and not a full turn.
///
/// Break it: call every regular angle axis cyclic.
#[test]
fn a_half_turn_is_not_cyclic() {
    let values: Vec<f64> = (0..8).map(|k| k as f64 * 22.5).collect();
    let mut store = empty_store();
    put(&mut store, angles(PAIRED, 8, "dega", values));
    let uri = uri_of(&store, PAIRED);
    let shape = shape(&[8]);
    let links = [AxisLink::PairedProperty {
        property: uri,
        dim: 0,
    }];
    let resolved = resolve(&store, &subject("p", &shape, &[], &links, Some(1))).expect("resolved");
    assert_eq!(resolved.axes[0].cyclic, None);
}

// ---------------------------------------------------------------------------
// Tables
// ---------------------------------------------------------------------------

const TABLE: &str = "00000000-0000-0000-0000-0000000000c1";

fn column(title: &str, values: Vec<f64>) -> eml23::Column {
    eml23::Column {
        title: Some(title.into()),
        value_count_per_row: 1,
        values: Some(eml23::AbstractValueArray::FloatingPointXmlArray(
            eml23::FloatingPointXmlArray {
                count_per_value: 1,
                values,
                ..Default::default()
            },
        )),
        ..Default::default()
    }
}

fn key_column(rows: usize) -> eml23::Column {
    eml23::Column {
        title: Some("rank".into()),
        value_count_per_row: 1,
        values: Some(eml23::AbstractValueArray::IntegerXmlArray(eml23::IntegerXmlArray {
            count_per_value: 1,
            values: (0..rows as i64).collect(),
            ..Default::default()
        })),
        ..Default::default()
    }
}

fn table(title: &str, rows: usize, value_columns: Vec<eml23::Column>) -> eml23::ColumnBasedTable {
    eml23::ColumnBasedTable {
        uuid: TABLE.into(),
        citation: citation(title),
        schema_version: "2.3".into(),
        key_column: vec![key_column(rows)],
        column: value_columns,
        ..Default::default()
    }
}

fn ramp(rows: usize, from: f64) -> Vec<f64> {
    (0..rows).map(|row| from + row as f64).collect()
}

/// Break it: cut or pad the column to the dimension.
#[test]
fn a_declared_table_of_80_rows_for_81_is_refused() {
    let mut store = empty_store();
    put(&mut store, table("axes", 80, vec![column("Delta", ramp(80, -40.0))]));
    let uri = uri_of(&store, TABLE);
    let shape = shape(&[81]);
    let links = [AxisLink::Table {
        table: uri,
        column: "Delta".into(),
        dim: 0,
    }];
    let refusal = resolve(&store, &subject("p", &shape, &[], &links, None)).expect_err("refused");
    assert!(
        matches!(refusal, AxisRefusal::CountMismatch { dim: 0, axis: 80, shape: 81 }),
        "{refusal}"
    );
}

/// A declared table of the right length gives its column.
///
/// Break it: read the key column instead of the named one.
#[test]
fn a_declared_table_gives_its_named_column() {
    let mut store = empty_store();
    put(&mut store, table("axes", 81, vec![column("Delta", ramp(81, -40.0))]));
    let uri = uri_of(&store, TABLE);
    let shape = shape(&[81]);
    let links = [AxisLink::Table {
        table: uri,
        column: "Delta".into(),
        dim: 0,
    }];
    let resolved = resolve(&store, &subject("p", &shape, &[], &links, None)).expect("resolved");
    let AxisValues::Listed(values) = &resolved.axes[0].values else {
        panic!("a listed axis");
    };
    assert_eq!(values[0], -40.0);
    assert_eq!(values[80], 40.0);
    assert!(matches!(resolved.axes[0].origin, AxisOrigin::DeclaredTable { .. }));
}

/// A column the table does not hold is refused by name.
///
/// Break it: fall back to the first column.
#[test]
fn a_declared_column_that_is_missing_is_refused() {
    let mut store = empty_store();
    put(&mut store, table("axes", 3, vec![column("Delta", ramp(3, 0.0))]));
    let uri = uri_of(&store, TABLE);
    let shape = shape(&[3]);
    let links = [AxisLink::Table {
        table: uri,
        column: "Nothing".into(),
        dim: 0,
    }];
    let refusal = resolve(&store, &subject("p", &shape, &[], &links, None)).expect_err("refused");
    assert!(matches!(refusal, AxisRefusal::NoSuchColumn { .. }), "{refusal}");
}

/// S3 (convention B): the value columns, in document order, are the dimensions, the slowest first.
/// EML TRG 2.3 §11.11 p. 294. Break it: invert the order of the columns.
#[test]
fn convention_b_maps_value_columns_to_dimensions_in_order() {
    let mut store = empty_store();
    put(
        &mut store,
        table(
            "axes",
            81,
            vec![
                column("Delta_N_or_Upside", ramp(81, 0.0)),
                column("Delta_E_or_Horizontal", ramp(81, 1000.0)),
            ],
        ),
    );
    let uri = uri_of(&store, TABLE);
    let shape = shape(&[81, 81]);
    let facets = [qualifier(&uri)];
    let resolved = resolve(&store, &subject("p", &shape, &facets, &[], None)).expect("resolved");
    let AxisValues::Listed(slowest) = &resolved.axes[0].values else {
        panic!("a listed axis");
    };
    let AxisValues::Listed(fastest) = &resolved.axes[1].values else {
        panic!("a listed axis");
    };
    assert_eq!(slowest[0], 0.0);
    assert_eq!(fastest[0], 1000.0);
    assert_eq!(
        resolved.axes[0].origin,
        AxisOrigin::QualifierTable {
            table: uri,
            column: "Delta_N_or_Upside".into()
        }
    );
}

/// A table with two value columns for a shape of one dimension is refused.
///
/// Break it: take the first columns.
#[test]
fn convention_b_counts_the_value_columns() {
    let mut store = empty_store();
    put(
        &mut store,
        table("axes", 3, vec![column("a", ramp(3, 0.0)), column("b", ramp(3, 0.0))]),
    );
    let uri = uri_of(&store, TABLE);
    let shape = shape(&[3]);
    let facets = [qualifier(&uri)];
    let refusal = resolve(&store, &subject("p", &shape, &facets, &[], None)).expect_err("refused");
    assert!(
        matches!(refusal, AxisRefusal::TableColumns { columns: 2, rank: 1, .. }),
        "{refusal}"
    );
}

/// Unequal dimensions do not fit a table whose columns have one row count.
///
/// Break it: pad the shorter dimension.
#[test]
fn convention_b_refuses_unequal_dimensions() {
    let mut store = empty_store();
    put(
        &mut store,
        table("axes", 3, vec![column("a", ramp(3, 0.0)), column("b", ramp(3, 0.0))]),
    );
    let uri = uri_of(&store, TABLE);
    let shape = shape(&[3, 2]);
    let facets = [qualifier(&uri)];
    let refusal = resolve(&store, &subject("p", &shape, &facets, &[], None)).expect_err("refused");
    assert!(
        matches!(refusal, AxisRefusal::CountMismatch { dim: 1, axis: 3, shape: 2 }),
        "{refusal}"
    );
}

/// EML TRG 2.3 §11.11 p. 294. Break it: take the first facet.
#[test]
fn two_qualifier_links_are_refused() {
    let store = empty_store();
    let shape = shape(&[3]);
    let facets = [qualifier(TABLE_URI), qualifier(TABLE_URI)];
    let refusal = resolve(&store, &subject("p", &shape, &facets, &[], None)).expect_err("refused");
    assert!(
        matches!(
            refusal,
            AxisRefusal::Qualifier(QualifierRefusal::Several { count: 2 })
        ),
        "{refusal}"
    );
}

/// Break it: refuse every `qualifier` facet.
#[test]
fn a_plain_qualifier_is_not_a_link() {
    let store = empty_store();
    let shape = shape(&[3, 2]);
    let facets = [qualifier("measured")];
    assert_eq!(qualifier_link(&facets), Ok(None));
    let resolved = resolve(&store, &subject("p", &shape, &facets, &[], None)).expect("resolved");
    assert_eq!(resolved.ranks_only(), vec![0, 1]);
}

/// A link to a table the source does not hold is refused by name, not turned into ranks.
///
/// Break it: fall back to ranks on a missing table.
#[test]
fn a_link_to_a_missing_table_is_refused() {
    let store = empty_store();
    let shape = shape(&[3]);
    let facets = [qualifier(TABLE_URI)];
    let refusal = resolve(&store, &subject("p", &shape, &facets, &[], None)).expect_err("refused");
    assert!(matches!(refusal, AxisRefusal::NotFound { .. }), "{refusal}");
}

// ---------------------------------------------------------------------------
// Ranks, labels, the registry
// ---------------------------------------------------------------------------

/// Break it: return an empty list for a missing axis.
#[test]
fn a_missing_axis_gets_ranks_and_says_so() {
    let store = empty_store();
    let shape = shape(&[3, 2]);
    let resolved = resolve(&store, &subject("p", &shape, &[], &[], None)).expect("resolved");
    assert_eq!(resolved.axes[0].values, AxisValues::Ranks(3));
    assert_eq!(resolved.axes[1].values, AxisValues::Ranks(2));
    assert_eq!(resolved.ranks_only(), vec![0, 1]);
}

/// RESQML TRG 2.2 §7.1 p. 222: labels, on one dimension. Break it: read the labels of a two-dimension shape as an axis.
#[test]
fn labels_are_an_axis_of_one_dimension_only() {
    let store = empty_store();
    let labelled = ElementShape::of(&[3], &["x".into(), "y".into(), "z".into()]).expect("a shape");
    let resolved = resolve(&store, &subject("p", &labelled, &[], &[], None)).expect("resolved");
    assert_eq!(
        resolved.axes[0].values,
        AxisValues::Labels(vec!["x".into(), "y".into(), "z".into()])
    );
    assert_eq!(resolved.axes[0].origin, AxisOrigin::LabelPerComponent);

    let two = ElementShape::of(&[3, 2], &(0..6).map(|n| n.to_string()).collect::<Vec<_>>()).expect("a shape");
    let resolved = resolve(&store, &subject("p", &two, &[], &[], None)).expect("resolved");
    assert_eq!(resolved.ranks_only(), vec![0, 1]);
}

struct Mine;

impl AxisSource for Mine {
    fn id(&self) -> &'static str {
        "mine"
    }
    fn axes(
        &self,
        subject: &AxisSubject<'_>,
        _objects: &dyn ObjectSource,
        _arrays: &dyn ArraySource,
    ) -> Option<Result<Vec<ComponentAxis>, AxisRefusal>> {
        Some(Ok(vec![ComponentAxis {
            dim: 0,
            name: None,
            kind: None,
            uom: None,
            values: AxisValues::Listed((0..subject.shape.dims[0]).map(|n| n as f64 * 10.0).collect()),
            cyclic: None,
            origin: AxisOrigin::PairedProperty { uri: "mine".into() },
        }]))
    }
}

/// The registry rule: the caller's additions are tried first. Break it: `register` at the end of the list.
#[test]
fn the_registry_tries_the_callers_sources_first() {
    let store = empty_store();
    let labelled = ElementShape::of(&[3], &["x".into(), "y".into(), "z".into()]).expect("a shape");
    let mut sources = AxisSources::with_builtins();
    sources.register(Box::new(Mine));
    assert!(sources.find("mine").is_some());
    assert!(sources.find("label-per-component").is_some());
    let resolved = sources
        .resolve(
            &subject("p", &labelled, &[], &[], None),
            &store as &dyn ObjectSource,
            &store as &dyn ArraySource,
        )
        .expect("resolved");
    assert_eq!(
        resolved.axes[0].origin,
        AxisOrigin::PairedProperty { uri: "mine".into() }
    );
}

/// A source whose axis has the wrong length is refused, whoever it is.
///
/// Break it: skip the final count check.
#[test]
fn an_axis_of_the_wrong_length_is_refused_whoever_gave_it() {
    let store = empty_store();
    let shape = shape(&[4]);
    let mut sources = AxisSources::with_builtins();
    struct Short;
    impl AxisSource for Short {
        fn id(&self) -> &'static str {
            "short"
        }
        fn axes(
            &self,
            _subject: &AxisSubject<'_>,
            _objects: &dyn ObjectSource,
            _arrays: &dyn ArraySource,
        ) -> Option<Result<Vec<ComponentAxis>, AxisRefusal>> {
            Some(Ok(vec![ComponentAxis {
                dim: 0,
                name: None,
                kind: None,
                uom: None,
                values: AxisValues::Listed(vec![1.0, 2.0, 3.0]),
                cyclic: None,
                origin: AxisOrigin::Ranks,
            }]))
        }
    }
    sources.register(Box::new(Short));
    let refusal = sources
        .resolve(
            &subject("p", &shape, &[], &[], None),
            &store as &dyn ObjectSource,
            &store as &dyn ArraySource,
        )
        .expect_err("refused");
    assert!(
        matches!(refusal, AxisRefusal::CountMismatch { dim: 0, axis: 3, shape: 4 }),
        "{refusal}"
    );
}

// ---------------------------------------------------------------------------
// A table is offered, never applied
// ---------------------------------------------------------------------------

/// Break it: make `candidate_tables` a source (the resolve below would then give a listed axis).
#[test]
fn a_table_axis_is_offered_never_applied() {
    let mut store = empty_store();
    let mut property = angles(PAIRED, 81, "m", ramp(81, 0.0));
    property.citation = citation("Rh dataset:x:001");
    put(&mut store, property);
    put(
        &mut store,
        table("axes (dataset:x:001)", 81, vec![column("Delta", ramp(81, -40.0))]),
    );
    let index = PackageIndex::build(&store, 1);
    let property_uri = uri_of(&store, PAIRED);
    let shape = shape(&[81]);

    let offered = candidate_tables(&index, &property_uri, &shape, &store, &store);
    assert_eq!(offered.len(), 1);
    assert_eq!(offered[0].tag, "dataset:x:001");
    assert_eq!(offered[0].rows, 81);
    assert_eq!(offered[0].fits, vec![(0, "Delta".to_string())]);

    // Nothing was applied: without a link, the axis is its ranks.
    let resolved = resolve(&store, &subject(&property_uri, &shape, &[], &[], None)).expect("resolved");
    assert_eq!(resolved.ranks_only(), vec![0]);
}

/// A table of the wrong length, or a property without a tag, is not offered.
///
/// Break it: offer every table of the package.
#[test]
fn a_table_that_does_not_fit_is_not_offered() {
    let mut store = empty_store();
    let mut property = angles(PAIRED, 81, "m", ramp(81, 0.0));
    property.citation = citation("Rh dataset:x:001");
    put(&mut store, property);
    put(
        &mut store,
        table("axes (dataset:x:001)", 80, vec![column("Delta", ramp(80, -40.0))]),
    );
    let index = PackageIndex::build(&store, 1);
    let property_uri = uri_of(&store, PAIRED);
    let shape = shape(&[81]);
    assert!(candidate_tables(&index, &property_uri, &shape, &store, &store).is_empty());

    let mut other = empty_store();
    put(&mut other, angles(PAIRED, 81, "m", ramp(81, 0.0)));
    put(
        &mut other,
        table("axes (dataset:x:001)", 81, vec![column("Delta", ramp(81, -40.0))]),
    );
    let index = PackageIndex::build(&other, 1);
    let property_uri = uri_of(&other, PAIRED);
    assert!(candidate_tables(&index, &property_uri, &shape, &other, &other).is_empty());
}
