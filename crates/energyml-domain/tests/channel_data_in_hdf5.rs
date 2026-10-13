//! A WITSML `ChannelSet` whose rows are a dataset of an HDF5 file — **an
//! extension beyond WITSML 2.1** (ADR-0013),
//! read by `channel_set::read_with` through an [`ArraySource`].
//!
//! What is held, each by a test that fails when it is broken:
//!
//! - **the two forms read the same series** — the rows the test writes inline
//!   and the same rows as a `[rows, indexes + channels]` dataset, `NaN` where
//!   inline writes `null`, compared to each other **and** to the test's own
//!   values (a reader agreeing with itself proves nothing);
//! - **the form is told by syntax**: `FileUri` beside an inline blob stays the
//!   standard's form — WITSML TRG v2.1 §7.2 p. 146, *"If this field is
//!   non-null, then the data field is ignored"* — and only an absolute path
//!   (`/…`, which no JSON text begins with, RFC 8259 §2) is a dataset;
//! - **the location is the storage's**: `energyml_storage::array::locate`
//!   finds the dataset form and nothing else, so a cut or an import carries it;
//! - **each refusal by its variant**: no source, no dataset, no backend (a
//!   missing capability, never an empty series), a shape that is not the rows,
//!   a column a number cannot hold — the last one before anything is read.
//!
//! The sources here are the test's: a map of datasets, and one that answers
//! as a build without HDF5 does. The same reading through a real `.h5` is in
//! `crates/energyml-build/tests/witsml_logs_in_hdf5.rs`.

use std::cell::Cell;
use std::collections::BTreeMap;

use energyml_core::registry::DynObject;
use energyml_core::value::Reflect;
use energyml_core::xml::XmlText;
use energyml_domain::series::{self, Quality, Readout, Refused, Series, channel_set};
use energyml_domain::source::{ArraySource, NoObjects, ObjectSource};
use energyml_domain::values::Values;
use energyml_eml_2_3 as eml;
use energyml_storage::{
    ArrayBuffer, ArrayData, ArrayLocation, ArrayMetadata, ArrayValues, ExternalDataReason,
};
use energyml_witsml_2_1 as witsml;

// ---------------------------------------------------------------------------
// Building by hand — the form from the schema, the values the test's own
// ---------------------------------------------------------------------------

fn citation(title: &str) -> eml::Citation {
    eml::Citation {
        title: title.into(),
        originator: "test".into(),
        creation: "2026-09-28T00:00:00Z".into(),
        format: "test".into(),
        ..Default::default()
    }
}

fn unit(code: &str) -> eml::UnitOfMeasureExt {
    eml::UnitOfMeasureExt::from_xml_text(code).expect("a unit")
}

fn index(kind: eml::DataIndexKind, mnemonic: &str, uom: &str) -> witsml::ChannelIndex {
    witsml::ChannelIndex {
        index_kind: kind,
        uom: unit(uom),
        direction: eml::IndexDirection::Increasing,
        mnemonic: mnemonic.into(),
        ..Default::default()
    }
}

fn md() -> witsml::ChannelIndex {
    index(eml::DataIndexKind::MeasuredDepth, "MD", "m")
}

fn channel(
    uuid: &str,
    mnemonic: &str,
    kind: witsml::ChannelDataKind,
    uom: &str,
    own: witsml::ChannelIndex,
) -> witsml::AnyChannel {
    witsml::AnyChannel::Channel(witsml::Channel {
        citation: citation(mnemonic),
        uuid: uuid.into(),
        schema_version: "2.1".into(),
        mnemonic: mnemonic.into(),
        global_mnemonic: mnemonic.into(),
        data_kind: kind,
        uom: unit(uom),
        index: vec![own],
        ..Default::default()
    })
}

const SET: &str = "60000000-0000-4000-8000-000000000001";
const LOG: &str = "60000000-0000-4000-8000-000000000002";
const T_CHANNEL: &str = "60000000-0000-4000-8000-000000000011";
const P_CHANNEL: &str = "60000000-0000-4000-8000-000000000012";
const N_CHANNEL: &str = "60000000-0000-4000-8000-000000000013";

const FILE: &str = "model.h5";
const PATH: &str = "/witsml21/60000000-0000-4000-8000-000000000001/ChannelData";

fn set_with(
    indexes: Vec<witsml::ChannelIndex>,
    channels: Vec<witsml::AnyChannel>,
    data: witsml::ChannelData,
) -> witsml::ChannelSet {
    witsml::ChannelSet {
        citation: citation("a set"),
        uuid: SET.into(),
        schema_version: "2.1".into(),
        index: indexes,
        channel: channels,
        data: Some(data),
        ..Default::default()
    }
}

/// MD, then T (double, degC), P (double, bar), N (long) — in that order.
fn columns() -> Vec<witsml::AnyChannel> {
    vec![
        channel(T_CHANNEL, "T", witsml::ChannelDataKind::Double, "degC", md()),
        channel(P_CHANNEL, "P", witsml::ChannelDataKind::Double, "bar", md()),
        channel(N_CHANNEL, "N", witsml::ChannelDataKind::Long, "Euc", md()),
    ]
}

fn inline(rows: &str) -> witsml::ChannelSet {
    set_with(
        vec![md()],
        columns(),
        witsml::ChannelData {
            data: Some(rows.into()),
            file_uri: None,
        },
    )
}

/// The extension: `FileUri` names the file, `Data` the dataset.
fn in_file(file: &str, path: &str) -> witsml::ChannelSet {
    set_with(
        vec![md()],
        columns(),
        witsml::ChannelData {
            data: Some(path.into()),
            file_uri: Some(file.into()),
        },
    )
}

/// The test's own rows: a gap in T at 10 m, an infinity in P at 20 m.
const ROWS: &str = r#"[[0,10.5,1.0,7],[10,null,2.0,8],[20,11.5,"+Infinity",9]]"#;

/// The same rows as the dataset of the extension, row-major.
fn dataset() -> ArrayData {
    let values = vec![
        0.0,
        10.5,
        1.0,
        7.0,
        10.0,
        f64::NAN,
        2.0,
        8.0,
        20.0,
        11.5,
        f64::INFINITY,
        9.0,
    ];
    ArrayData {
        metadata: ArrayMetadata {
            dimensions: vec![3, 4],
            element_type: "float64".into(),
        },
        values: ArrayValues::F64(values),
    }
}

// ---------------------------------------------------------------------------
// Two sources: a map of datasets, and a build without HDF5
// ---------------------------------------------------------------------------

/// Datasets by `(file, path)`. It locates with the **storage's** locator — the
/// one a store uses — and counts its reads.
#[derive(Default)]
struct Datasets {
    held: BTreeMap<(String, String), ArrayData>,
    reads: Cell<usize>,
}

impl Datasets {
    fn with(file: &str, path: &str, data: ArrayData) -> Datasets {
        let mut held = BTreeMap::new();
        held.insert((file.to_string(), path.to_string()), data);
        Datasets {
            held,
            reads: Cell::new(0),
        }
    }
}

impl ObjectSource for Datasets {
    fn object(&self, _uuid: &str) -> Option<Box<dyn DynObject>> {
        None
    }
}

impl ArraySource for Datasets {
    fn locate(&self, object: &dyn Reflect) -> Vec<ArrayLocation> {
        energyml_storage::array::locate(object)
    }
    fn metadata(&self, location: &ArrayLocation) -> energyml_domain::Result<ArrayMetadata> {
        self.read(location).map(|data| data.metadata)
    }
    fn read(&self, location: &ArrayLocation) -> energyml_domain::Result<ArrayData> {
        self.reads.set(self.reads.get() + 1);
        self.held
            .get(&(location.file.clone(), location.path_in_file.clone()))
            .cloned()
            .ok_or_else(|| {
                energyml_storage::Error::ExternalData {
                    location: format!("{}:{}", location.file, location.path_in_file),
                    reason: ExternalDataReason::PathMissing(location.path_in_file.clone()),
                }
                .into()
            })
    }
    fn read_into(
        &self,
        _location: &ArrayLocation,
        _out: &mut ArrayBuffer<'_>,
    ) -> energyml_domain::Result<ArrayMetadata> {
        Err(energyml_domain::Error::unsupported(
            "reading into a buffer",
            "this test's source reads whole",
        ))
    }
}

/// What a store answers in a build compiled without the `hdf5` feature.
struct NoBackend;

impl ObjectSource for NoBackend {
    fn object(&self, _uuid: &str) -> Option<Box<dyn DynObject>> {
        None
    }
}

impl ArraySource for NoBackend {
    fn locate(&self, object: &dyn Reflect) -> Vec<ArrayLocation> {
        energyml_storage::array::locate(object)
    }
    fn metadata(&self, location: &ArrayLocation) -> energyml_domain::Result<ArrayMetadata> {
        self.read(location).map(|data| data.metadata)
    }
    fn read(&self, location: &ArrayLocation) -> energyml_domain::Result<ArrayData> {
        Err(energyml_storage::Error::ExternalData {
            location: format!("{}:{}", location.file, location.path_in_file),
            reason: ExternalDataReason::FeatureNotEnabled {
                feature: "hdf5",
                mime: location.mime.clone(),
            },
        }
        .into())
    }
    fn read_into(
        &self,
        _location: &ArrayLocation,
        _out: &mut ArrayBuffer<'_>,
    ) -> energyml_domain::Result<ArrayMetadata> {
        Err(energyml_domain::Error::unsupported(
            "reading into a buffer",
            "no backend",
        ))
    }
}

fn refused(result: Result<Series, Refused>) -> Refused {
    result.expect_err("refused")
}

fn floats(series: &Series, mnemonic: &str) -> Vec<f64> {
    match &series
        .channel(mnemonic)
        .unwrap_or_else(|| panic!("a channel `{mnemonic}`"))
        .values
    {
        Values::Floating(values) => values.clone(),
        other => panic!("`{mnemonic}` holds {:?}", other.family()),
    }
}

// ---------------------------------------------------------------------------
// The two forms, one reading
// ---------------------------------------------------------------------------

#[test]
fn the_rows_inline_and_in_a_dataset_read_as_the_same_series() {
    // To break: a `NaN` cell read as a number (T at 10 m becomes good), the
    // columns taken column-major, or the integer channel widened on the way.
    let written = channel_set::read(&inline(ROWS)).expect("the inline rows read");
    let source = Datasets::with(FILE, PATH, dataset());
    let referenced = channel_set::read_with(&in_file(FILE, PATH), &source).expect("the dataset reads");
    assert_eq!(source.reads.get(), 1, "one read of the dataset");

    // Against each other — through the JSON a front end receives, where `NaN`
    // compares as `null`…
    assert_eq!(written.to_json(), referenced.to_json());
    // …and against the test's own rows, which neither reader wrote.
    for series in [&written, &referenced] {
        assert_eq!(series.index.values, vec![0.0, 10.0, 20.0]);
        let t = floats(series, "T");
        assert_eq!((t[0], t[2]), (10.5, 11.5));
        assert!(t[1].is_nan());
        assert_eq!(
            series.channel("T").unwrap().quality,
            vec![Quality::Good, Quality::Absent, Quality::Good]
        );
        assert_eq!(floats(series, "P"), vec![1.0, 2.0, f64::INFINITY]);
        assert_eq!(
            series.channel("P").unwrap().quality[2],
            Quality::Flagged("+Infinity".into())
        );
        assert_eq!(
            series.channel("N").unwrap().values,
            Values::Integer(vec![7, 8, 9])
        );
    }
}

#[test]
fn an_integer_dataset_keeps_its_integers_exact() {
    // A `long` channel past 2^53 cannot survive a trip through `f64`; the
    // dataset's integers are read as integers.
    let big = 9_007_199_254_740_993_i64; // 2^53 + 1
    let data = ArrayData {
        metadata: ArrayMetadata {
            dimensions: vec![2, 4],
            element_type: "int64".into(),
        },
        values: ArrayValues::I64(vec![0, 1, 2, big, 10, 3, 4, 5]),
    };
    let series =
        channel_set::read_with(&in_file(FILE, PATH), &Datasets::with(FILE, PATH, data)).expect("reads");
    assert_eq!(series.channel("N").unwrap().values, Values::Integer(vec![big, 5]));
}

#[test]
fn the_same_checks_run_on_a_dataset_as_on_inline_rows() {
    // One row loop: an index that goes back is refused in both forms.
    let back = ArrayData {
        metadata: ArrayMetadata {
            dimensions: vec![2, 4],
            element_type: "float64".into(),
        },
        values: ArrayValues::F64(vec![10.0, 1.0, 1.0, 1.0, 0.0, 1.0, 1.0, 1.0]),
    };
    let read = channel_set::read_with(&in_file(FILE, PATH), &Datasets::with(FILE, PATH, back));
    assert!(matches!(refused(read), Refused::NotMonotone { at: 2, .. }));
}

#[test]
fn a_log_and_the_registry_read_a_dataset_through_the_source_they_are_given() {
    let log = witsml::Log {
        citation: citation("a log"),
        uuid: LOG.into(),
        schema_version: "2.1".into(),
        channel_set: vec![witsml::AnyChannelSet::ChannelSet(in_file(FILE, PATH))],
        ..Default::default()
    };
    let source = Datasets::with(FILE, PATH, dataset());
    let sets = channel_set::read_log_with(&log, &source).expect("the log reads");
    assert_eq!(sets[0].index.values, vec![0.0, 10.0, 20.0]);

    let objects: Vec<(&str, &dyn Reflect)> = vec![(LOG, &log)];
    let Ok(Readout::Sets(read)) = series::read(LOG, &objects, &source) else {
        panic!("the registry reads the log")
    };
    assert_eq!(read[0].to_json(), sets[0].to_json());
    // The nested set by its own uuid, through the part that holds it.
    let Ok(Readout::Sets(nested)) = series::read(SET, &objects, &source) else {
        panic!("the nested set reads")
    };
    assert_eq!(nested[0].to_json(), sets[0].to_json());
}

// ---------------------------------------------------------------------------
// The form, by syntax
// ---------------------------------------------------------------------------

#[test]
fn a_file_uri_beside_an_inline_blob_is_the_standard_form_not_a_dataset() {
    // §7.2 p. 146: "If this field is non-null, then the data field is
    // ignored." A blob beside `FileUri` is never read as rows, and never taken
    // for a dataset path either: JSON cannot begin with `/`.
    let source = Datasets::with(FILE, PATH, dataset());
    let beside = set_with(
        vec![md()],
        columns(),
        witsml::ChannelData {
            data: Some(ROWS.into()),
            file_uri: Some("./abc.avro".into()),
        },
    );
    assert!(
        matches!(refused(channel_set::read_with(&beside, &source)), Refused::FileUri { uri } if uri == "./abc.avro")
    );
    let alone = set_with(
        vec![md()],
        columns(),
        witsml::ChannelData {
            data: None,
            file_uri: Some("./abc.avro".into()),
        },
    );
    assert!(matches!(
        refused(channel_set::read_with(&alone, &source)),
        Refused::FileUri { .. }
    ));
    assert_eq!(source.reads.get(), 0, "the standard's form reads no dataset");

    // A path with no file is no dataset: it is inline data that is not JSON.
    let orphan = set_with(
        vec![md()],
        columns(),
        witsml::ChannelData {
            data: Some(PATH.into()),
            file_uri: None,
        },
    );
    assert!(matches!(
        refused(channel_set::read_with(&orphan, &source)),
        Refused::NotRows { .. }
    ));
}

#[test]
fn the_storage_locates_the_dataset_form_and_nothing_else() {
    // What a cut, an import, a rename and a prefetch walk: the extension is an
    // array location there, the standard's forms are not.
    let found = energyml_storage::array::locate(&in_file(FILE, PATH));
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(
        (found[0].file.as_str(), found[0].path_in_file.as_str()),
        (FILE, PATH)
    );
    assert_eq!(found[0].mime.as_deref(), Some("application/x-hdf5"));
    assert_eq!(found[0].field_path, "Data", "the field of the set that holds it");
    assert!(energyml_storage::array::locate(&inline(ROWS)).is_empty());
    let beside = set_with(
        vec![md()],
        columns(),
        witsml::ChannelData {
            data: Some(ROWS.into()),
            file_uri: Some("./abc.avro".into()),
        },
    );
    assert!(energyml_storage::array::locate(&beside).is_empty());
    // And what a rename or an import rewrites, by the part's type.
    assert_eq!(energyml_storage::array::file_field("ChannelData"), "file_uri");
    assert_eq!(energyml_storage::array::path_field("ChannelData"), "data");
}

// ---------------------------------------------------------------------------
// Refusals, each by its variant
// ---------------------------------------------------------------------------

#[test]
fn with_no_array_source_a_dataset_is_refused_by_name() {
    let read = channel_set::read(&in_file(FILE, PATH));
    assert!(
        matches!(refused(read), Refused::DatasetNotLocated { file, path } if file == FILE && path == PATH)
    );
    let read = channel_set::read_with(&in_file(FILE, PATH), &NoObjects);
    assert!(matches!(refused(read), Refused::DatasetNotLocated { .. }));
}

#[test]
fn a_dataset_the_file_does_not_hold_is_refused_by_name() {
    let source = Datasets::with(FILE, "/elsewhere", dataset());
    let read = channel_set::read_with(&in_file(FILE, PATH), &source);
    assert!(
        matches!(refused(read), Refused::DatasetNotRead { file, path, .. } if file == FILE && path == PATH)
    );
}

#[test]
fn a_build_without_hdf5_says_so_and_never_reads_an_empty_series() {
    // A missing capability is not missing data (ADR-0005).
    let read = channel_set::read_with(&in_file(FILE, PATH), &NoBackend);
    assert!(matches!(refused(read), Refused::DatasetNeedsFeature { feature, .. } if feature == "hdf5"));
}

#[test]
fn a_dataset_that_is_not_the_rows_is_refused_by_its_shape() {
    let shaped = |dimensions: Vec<i64>, count: usize| ArrayData {
        metadata: ArrayMetadata {
            dimensions,
            element_type: "float64".into(),
        },
        values: ArrayValues::F64(vec![1.0; count]),
    };
    // One dimension: the rows flattened, which says nothing of their width.
    let flat = channel_set::read_with(
        &in_file(FILE, PATH),
        &Datasets::with(FILE, PATH, shaped(vec![12], 12)),
    );
    assert!(matches!(
        refused(flat),
        Refused::DatasetShape {
            expected: 4,
            indexes: 1,
            channels: 3,
            ..
        }
    ));
    // Three columns where MD and three channels make four.
    let narrow = channel_set::read_with(
        &in_file(FILE, PATH),
        &Datasets::with(FILE, PATH, shaped(vec![4, 3], 12)),
    );
    assert!(matches!(refused(narrow), Refused::DatasetShape { dimensions, .. } if dimensions == vec![4, 3]));
}

#[test]
fn a_column_no_number_holds_is_refused_before_anything_is_read() {
    let source = Datasets::with(FILE, PATH, dataset());
    let dated = set_with(
        vec![index(eml::DataIndexKind::DateTime, "TIME", "s")],
        vec![channel(
            T_CHANNEL,
            "T",
            witsml::ChannelDataKind::Double,
            "degC",
            index(eml::DataIndexKind::DateTime, "TIME", "s"),
        )],
        witsml::ChannelData {
            data: Some(PATH.into()),
            file_uri: Some(FILE.into()),
        },
    );
    assert!(
        matches!(refused(channel_set::read_with(&dated, &source)), Refused::DatasetColumnKind { column, kind } if column == "TIME" && kind == "date time")
    );
    let text = set_with(
        vec![md()],
        vec![channel(
            T_CHANNEL,
            "LITHO",
            witsml::ChannelDataKind::String,
            "Euc",
            md(),
        )],
        witsml::ChannelData {
            data: Some(PATH.into()),
            file_uri: Some(FILE.into()),
        },
    );
    assert!(
        matches!(refused(channel_set::read_with(&text, &source)), Refused::DatasetColumnKind { column, .. } if column == "LITHO")
    );
    assert_eq!(source.reads.get(), 0, "nothing read for a column no number holds");
}
