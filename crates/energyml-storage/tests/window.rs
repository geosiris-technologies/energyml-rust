//! `window::read_window` and `window::stats_of`, over an array held in memory.
//!
//! The rules under test: the metadata is read first and the data second; the
//! window is clipped along the first axis only and says so; a request that
//! cannot be honoured is refused **before** any data is read; the statistics
//! describe what was read, count the NaN apart and are absent for values that
//! are not quantities. No HDF5 is involved: the fake store serves a row-major
//! block of doubles, honouring `start` and `count`.
//!
//! To make them fail: clip the last axis instead of the first, compute the
//! statistics over the whole array, fold NaN into the mean, or let a short
//! answer through.

use std::sync::Mutex;

use energyml_core::registry::DynObject;
use energyml_core::value::Reflect;
use energyml_storage::array::{ArrayData, ArrayLocation, ArrayMetadata, ArrayValues};
use energyml_storage::error::{Diagnostic, Error, ExternalDataReason, Result};
use energyml_storage::resource::{Dataspace, ObjectHeader, ObjectSummary};
use energyml_storage::storage::EnergymlStorage;
use energyml_storage::window::{WindowLimits, read_window, stats_of};

/// An array of doubles of rank 1 or 2, and a log of what was read from it.
struct Block {
    dims: Vec<i64>,
    values: Vec<f64>,
    logical_type: Option<String>,
    /// Answer one value short, as a store that lies would.
    short: bool,
    reads: Mutex<Vec<ArrayLocation>>,
    metadata_reads: Mutex<usize>,
}

impl Block {
    /// `rows x cols`, the value at `(r, c)` being `r * cols + c`.
    fn grid(rows: i64, cols: i64) -> Self {
        Self::of(vec![rows, cols], (0..rows * cols).map(|v| v as f64).collect())
    }

    fn of(dims: Vec<i64>, values: Vec<f64>) -> Self {
        Self {
            dims,
            values,
            logical_type: None,
            short: false,
            reads: Mutex::new(Vec::new()),
            metadata_reads: Mutex::new(0),
        }
    }

    fn metadata(&self) -> ArrayMetadata {
        let mut metadata = ArrayMetadata::new(self.dims.clone(), "double");
        metadata.logical_type = self.logical_type.clone();
        metadata
    }

    fn reads(&self) -> Vec<ArrayLocation> {
        self.reads.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl EnergymlStorage for Block {
    fn store_kind(&self) -> &'static str {
        "block"
    }

    async fn dataspaces(&self) -> Result<Vec<Dataspace>> {
        Ok(vec![Dataspace::new("eml:///", "block")])
    }

    async fn resources(&self) -> Result<Vec<ObjectSummary>> {
        Ok(Vec::new())
    }

    async fn headers(&self, uris: &[String]) -> Vec<Result<ObjectHeader>> {
        uris.iter().map(|uri| Err(Error::NotFound(uri.clone()))).collect()
    }

    async fn get_objects(&self, uris: &[String]) -> Vec<Result<Box<dyn DynObject>>> {
        uris.iter().map(|uri| Err(Error::NotFound(uri.clone()))).collect()
    }

    async fn supported_types(&self) -> Vec<&'static energyml_core::reflect::TypeInfo> {
        Vec::new()
    }

    fn arrays_of(&self, _object: &dyn Reflect) -> Vec<ArrayLocation> {
        Vec::new()
    }

    async fn array_metadata(&self, _location: &ArrayLocation) -> Result<ArrayMetadata> {
        *self.metadata_reads.lock().unwrap() += 1;
        Ok(self.metadata())
    }

    async fn read_array(&self, location: &ArrayLocation) -> Result<ArrayData> {
        self.reads.lock().unwrap().push(location.clone());
        let pick = |axis: usize| {
            let start = location.start.get(axis).copied().unwrap_or(0);
            let count = location
                .count
                .get(axis)
                .copied()
                .unwrap_or(self.dims[axis] - start);
            (start as usize, count as usize)
        };
        let mut values = Vec::new();
        let mut counts = Vec::new();
        match self.dims.len() {
            1 => {
                let (start, count) = pick(0);
                values.extend_from_slice(&self.values[start..start + count]);
                counts.push(count as i64);
            }
            2 => {
                let (row0, rows) = pick(0);
                let (col0, cols) = pick(1);
                let width = self.dims[1] as usize;
                for row in row0..row0 + rows {
                    values.extend_from_slice(&self.values[row * width + col0..row * width + col0 + cols]);
                }
                counts.extend([rows as i64, cols as i64]);
            }
            rank => panic!("the fake serves rank 1 and 2, not {rank}"),
        }
        if self.short {
            values.pop();
        }
        let mut metadata = ArrayMetadata::new(counts, "double");
        metadata.logical_type = self.logical_type.clone();
        Ok(ArrayData {
            metadata,
            values: ArrayValues::F64(values),
        })
    }

    fn diagnostics(&self) -> Vec<Diagnostic> {
        Vec::new()
    }
}

fn block<F: std::future::Future>(future: F) -> F::Output {
    futures_executor::block_on(future)
}

fn at(start: &[i64], count: &[i64]) -> ArrayLocation {
    ArrayLocation {
        file: "data.h5".into(),
        path_in_file: "/resqml22/x/Points".into(),
        start: start.to_vec(),
        count: count.to_vec(),
        ..ArrayLocation::default()
    }
}

fn values(window: &energyml_storage::window::Window) -> Vec<f64> {
    window.data.f64().expect("doubles").to_vec()
}

#[test]
fn a_small_array_is_read_whole() {
    let store = Block::grid(3, 2);
    let window = block(read_window(&store, &at(&[], &[]), &WindowLimits::new(100))).unwrap();
    assert!(!window.clipped);
    assert_eq!(window.whole.dimensions, vec![3, 2]);
    assert_eq!(window.read.start, vec![0, 0], "every axis spelled out");
    assert_eq!(window.read.count, vec![3, 2]);
    assert_eq!(values(&window), vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0]);
    let stats = window.stats.expect("doubles are numeric");
    assert_eq!((stats.count, stats.nan), (6, 0));
    assert_eq!(
        (stats.min, stats.max, stats.mean),
        (Some(0.0), Some(5.0), Some(2.5))
    );
    assert_eq!(*store.metadata_reads.lock().unwrap(), 1);
}

#[test]
fn a_large_array_is_clipped_on_the_first_axis_and_the_stats_are_of_the_window() {
    let store = Block::grid(10, 3);
    let window = block(read_window(&store, &at(&[], &[]), &WindowLimits::new(12))).unwrap();
    assert!(window.clipped);
    assert_eq!(
        window.read.count,
        vec![4, 3],
        "4 whole rows of 3: the other axis is kept whole"
    );
    assert_eq!(window.data.values.len(), 12);
    let stats = window.stats.unwrap();
    assert_eq!(
        (stats.min, stats.max, stats.mean),
        (Some(0.0), Some(11.0), Some(5.5))
    );
    // Not of the whole array, whose maximum is 29.
    assert_ne!(stats.max, Some(29.0));
    let reads = store.reads();
    assert_eq!(reads.len(), 1, "one read, of the window only");
    assert_eq!(reads[0].count, vec![4, 3]);

    // Negative: a bound that exactly fits does not clip.
    let store = Block::grid(10, 3);
    let window = block(read_window(&store, &at(&[], &[]), &WindowLimits::new(30))).unwrap();
    assert!(!window.clipped);
    assert_eq!(window.read.count, vec![10, 3]);
}

#[test]
fn a_window_in_the_middle_keeps_its_start_and_defaults_its_count() {
    let store = Block::grid(10, 3);
    let window = block(read_window(
        &store,
        &at(&[2, 0], &[2, 3]),
        &WindowLimits::new(100),
    ))
    .unwrap();
    assert_eq!(values(&window), vec![6.0, 7.0, 8.0, 9.0, 10.0, 11.0]);

    // No count: from the start to the end of every axis.
    let window = block(read_window(&store, &at(&[8, 1], &[]), &WindowLimits::new(100))).unwrap();
    assert_eq!(window.read.count, vec![2, 2]);
    assert_eq!(values(&window), vec![25.0, 26.0, 28.0, 29.0]);

    // Clipping starts from the start: the rows after row 5, not the first.
    let window = block(read_window(&store, &at(&[5, 0], &[]), &WindowLimits::new(6))).unwrap();
    assert!(window.clipped);
    assert_eq!(values(&window), vec![15.0, 16.0, 17.0, 18.0, 19.0, 20.0]);
}

#[test]
fn a_request_that_cannot_be_honoured_is_refused_before_any_read() {
    let store = Block::grid(10, 3);
    let limits = WindowLimits::new(100);
    let cases: Vec<(&str, ArrayLocation, WindowLimits)> = vec![
        ("start at the end of axis 0", at(&[10, 0], &[]), limits),
        ("start beyond axis 1", at(&[0, 7], &[]), limits),
        ("negative start", at(&[-1, 0], &[]), limits),
        ("count past the end", at(&[8, 0], &[5, 3]), limits),
        ("count past the end of axis 1", at(&[0, 0], &[2, 4]), limits),
        ("zero count", at(&[0, 0], &[0, 3]), limits),
        ("start of the wrong rank", at(&[1], &[]), limits),
        ("count of the wrong rank", at(&[], &[1, 1, 1]), limits),
    ];
    for (name, location, limits) in cases {
        let error = block(read_window(&store, &location, &limits)).unwrap_err();
        assert!(matches!(error, Error::OutOfBounds { .. }), "{name}: {error}");
        assert!(store.reads().is_empty(), "{name}: nothing may be read");
    }
    // Too large is not out of bounds: the caller narrows, it does not move.
    let too_large = [
        ("a zero bound", at(&[], &[]), WindowLimits::new(0)),
        // One row holds 3 elements.
        ("a row larger than the bound", at(&[], &[]), WindowLimits::new(2)),
    ];
    for (name, location, limits) in too_large {
        let error = block(read_window(&store, &location, &limits)).unwrap_err();
        assert!(matches!(error, Error::TooLarge { .. }), "{name}: {error}");
        assert!(store.reads().is_empty(), "{name}: nothing may be read");
    }

    // The too-wide row says its size, so the caller knows what to narrow.
    let wide = Block::grid(4, 100);
    let error = block(read_window(&wide, &at(&[], &[]), &WindowLimits::new(50))).unwrap_err();
    assert!(error.to_string().contains("100 elements"), "{error}");
    assert!(wide.reads().is_empty());
    // Negative: narrowing the trailing axis makes the same array readable.
    let window = block(read_window(&wide, &at(&[0, 0], &[4, 10]), &WindowLimits::new(50))).unwrap();
    assert!(!window.clipped, "5 rows of 10 fit, 4 were asked: no clip");
    assert_eq!(window.data.values.len(), 40);
}

#[test]
fn a_store_that_answers_short_is_refused_by_name() {
    let mut store = Block::grid(3, 2);
    store.short = true;
    let error = block(read_window(&store, &at(&[], &[]), &WindowLimits::new(100))).unwrap_err();
    match error {
        Error::ExternalData {
            reason: ExternalDataReason::ShapeDisagrees { promised, held },
            ..
        } => assert_eq!((promised, held), (6, 5)),
        other => panic!("expected ShapeDisagrees, got {other}"),
    }
}

fn doubles(values: Vec<f64>) -> ArrayData {
    ArrayData {
        metadata: ArrayMetadata::new(vec![values.len() as i64], "double"),
        values: ArrayValues::F64(values),
    }
}

#[test]
fn nan_are_counted_and_left_out_of_the_other_figures() {
    let stats = stats_of(&doubles(vec![1.0, f64::NAN, 3.0, f64::NAN])).unwrap();
    assert_eq!((stats.count, stats.nan), (4, 2));
    assert_eq!(
        (stats.min, stats.max, stats.mean),
        (Some(1.0), Some(3.0), Some(2.0))
    );

    // Only NaN: nothing to say about min, max or mean, and not zero.
    let stats = stats_of(&doubles(vec![f64::NAN, f64::NAN])).unwrap();
    assert_eq!((stats.count, stats.nan), (2, 2));
    assert_eq!((stats.min, stats.max, stats.mean), (None, None, None));

    // Empty: no figure invented.
    let stats = stats_of(&doubles(Vec::new())).unwrap();
    assert_eq!((stats.count, stats.min, stats.mean), (0, None, None));
}

#[test]
fn every_integer_width_has_stats() {
    let ints = |values: ArrayValues| {
        let mut metadata = ArrayMetadata::new(vec![values.len() as i64], values.element_type());
        metadata.logical_type = None;
        ArrayData { metadata, values }
    };
    let stats = stats_of(&ints(ArrayValues::I32(vec![-2, 4]))).unwrap();
    assert_eq!(
        (stats.min, stats.max, stats.mean),
        (Some(-2.0), Some(4.0), Some(1.0))
    );
    let stats = stats_of(&ints(ArrayValues::U32(vec![0, 6, 6]))).unwrap();
    assert_eq!((stats.count, stats.max, stats.mean), (3, Some(6.0), Some(4.0)));
    let stats = stats_of(&ints(ArrayValues::F32(vec![0.5, 1.5]))).unwrap();
    assert_eq!(stats.mean, Some(1.0));
    let stats = stats_of(&ints(ArrayValues::U8(vec![1, 2, 3]))).unwrap();
    assert_eq!(stats.mean, Some(2.0));
}

#[test]
fn booleans_and_strings_have_no_stats() {
    let bytes = ArrayValues::U8(vec![0, 1, 1, 0]);
    for logical in ["arrayOfBoolean", "arrayOfString", "arrayOfCustom"] {
        let mut metadata = ArrayMetadata::new(vec![4], "uint8");
        metadata.logical_type = Some(logical.to_string());
        let data = ArrayData {
            metadata,
            values: bytes.clone(),
        };
        assert_eq!(stats_of(&data), None, "{logical}");
    }
    // Negative: the same bytes under a numeric logical type, or none, are numbers.
    let mut metadata = ArrayMetadata::new(vec![4], "uint8");
    metadata.logical_type = Some("arrayOfUInt8".to_string());
    let numeric = ArrayData {
        metadata,
        values: bytes,
    };
    assert!(stats_of(&numeric).is_some());

    // Through the window: the logical type of the whole array decides.
    let mut store = Block::grid(2, 2);
    store.logical_type = Some("arrayOfBoolean".into());
    let window = block(read_window(&store, &at(&[], &[]), &WindowLimits::new(10))).unwrap();
    assert!(window.stats.is_none());
    assert_eq!(window.data.values.len(), 4, "the values are still returned");
}

#[test]
fn nan_in_a_window_are_counted_over_what_was_read() {
    let store = Block::of(vec![4], vec![1.0, f64::NAN, 5.0, 100.0]);
    let window = block(read_window(&store, &at(&[], &[]), &WindowLimits::new(3))).unwrap();
    assert!(window.clipped);
    let stats = window.stats.unwrap();
    assert_eq!((stats.count, stats.nan), (3, 1));
    assert_eq!(
        (stats.max, stats.mean),
        (Some(5.0), Some(3.0)),
        "the 100 was not read"
    );
}
