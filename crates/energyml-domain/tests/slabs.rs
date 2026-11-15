//! An in-memory source of datasets that reads by block, for the tests of
//! `values::read_block` and the readers built on it. No file is read: the
//! datasets are given as numbers, row-major (RESQML Usage Guide 2.2 §4.2.2
//! p. 39: the HDF5 order and the RESQML index order are the same).
//!
//! It answers like `EpcStore`: `metadata` gives the shape of the **whole**
//! dataset whatever the window of the part, and `read` applies `start` and
//! `count` (EML TRG 2.3, `ExternalDataArrayPart`). With `whole_only` it
//! ignores them, like a source that does not read by block.
#![allow(dead_code)]

use std::cell::Cell;
use std::collections::BTreeMap;

use energyml_core::registry::DynObject;
use energyml_core::value::Reflect;
use energyml_domain::source::{ArraySource, ObjectSource};
use energyml_storage::array::{ArrayBuffer, ArrayData, ArrayLocation, ArrayMetadata, ArrayValues};
use energyml_storage::{ExternalDataReason, Error as StorageError};

/// Objects by uri, datasets by path in the file, and a count of what was read.
#[derive(Default)]
pub struct Slabs {
    objects: Vec<(String, Box<dyn DynObject>)>,
    datasets: BTreeMap<String, (Vec<i64>, Vec<f64>)>,
    /// Ignore `start` and `count`: answer the whole dataset.
    pub whole_only: bool,
    /// How many times `read` was called.
    pub reads: Cell<usize>,
    /// How many values `read` returned, all calls together.
    pub values_returned: Cell<usize>,
}

impl Slabs {
    /// A source with no object and no dataset.
    pub fn new() -> Slabs {
        Slabs::default()
    }

    /// Add a dataset: its path in the file, its dimensions, its values (row-major).
    pub fn with_dataset(mut self, path: &str, dims: Vec<i64>, values: Vec<f64>) -> Slabs {
        assert_eq!(
            dims.iter().product::<i64>() as usize,
            values.len(),
            "a dataset holds the product of its dimensions"
        );
        self.datasets.insert(path.to_string(), (dims, values));
        self
    }

    /// Add an object, held under its uri.
    pub fn with_object(mut self, uri: &str, object: Box<dyn DynObject>) -> Slabs {
        self.objects.push((uri.to_string(), object));
        self
    }

    /// The same source, reading whole datasets whatever the block.
    pub fn whole_only(mut self) -> Slabs {
        self.whole_only = true;
        self
    }
}

impl ObjectSource for Slabs {
    fn object(&self, uuid: &str) -> Option<Box<dyn DynObject>> {
        let wanted = uuid.to_ascii_lowercase();
        self.objects
            .iter()
            .find(|(uri, _)| uri.to_ascii_lowercase().contains(&wanted))
            .map(|(_, object)| object.clone_box())
    }

    fn object_at(&self, uri: &str) -> Option<Box<dyn DynObject>> {
        self.objects
            .iter()
            .find(|(held, _)| held.eq_ignore_ascii_case(uri))
            .map(|(_, object)| object.clone_box())
    }
}

impl ArraySource for Slabs {
    fn locate(&self, object: &dyn Reflect) -> Vec<ArrayLocation> {
        energyml_storage::array::locate(object)
    }

    fn metadata(&self, location: &ArrayLocation) -> energyml_domain::Result<ArrayMetadata> {
        let (dims, _) = self.dataset(location)?;
        Ok(ArrayMetadata::new(dims.clone(), "double"))
    }

    fn read(&self, location: &ArrayLocation) -> energyml_domain::Result<ArrayData> {
        let (dims, values) = self.dataset(location)?;
        self.reads.set(self.reads.get() + 1);
        let (counts, picked) = if self.whole_only || location.count.is_empty() {
            (dims.clone(), values.clone())
        } else {
            let start: Vec<i64> = if location.start.is_empty() {
                vec![0; dims.len()]
            } else {
                location.start.clone()
            };
            if start.len() != dims.len() || location.count.len() != dims.len() {
                return Err(self.refuse(location, "the window does not have the rank of the dataset"));
            }
            for (k, dim) in dims.iter().enumerate() {
                if start[k] < 0 || location.count[k] < 0 || start[k] + location.count[k] > *dim {
                    return Err(self.refuse(location, &format!("the window leaves dimension {k}")));
                }
            }
            (location.count.clone(), cut(values, dims, &start, &location.count))
        };
        self.values_returned.set(self.values_returned.get() + picked.len());
        Ok(ArrayData {
            metadata: ArrayMetadata::new(counts, "double"),
            values: ArrayValues::F64(picked),
        })
    }

    fn read_into(
        &self,
        _location: &ArrayLocation,
        _out: &mut ArrayBuffer<'_>,
    ) -> energyml_domain::Result<ArrayMetadata> {
        Err(energyml_domain::Error::unsupported(
            "reading into a buffer",
            "this test's source reads into owned vectors",
        ))
    }
}

impl Slabs {
    fn dataset(&self, location: &ArrayLocation) -> energyml_domain::Result<&(Vec<i64>, Vec<f64>)> {
        self.datasets.get(&location.path_in_file).ok_or_else(|| {
            StorageError::ExternalData {
                location: format!("{}:{}", location.file, location.path_in_file),
                reason: ExternalDataReason::PathMissing(location.path_in_file.clone()),
            }
            .into()
        })
    }

    fn refuse(&self, location: &ArrayLocation, why: &str) -> energyml_domain::Error {
        StorageError::ExternalData {
            location: format!("{}:{}", location.file, location.path_in_file),
            reason: ExternalDataReason::Backend(why.to_string()),
        }
        .into()
    }
}

/// The block `start .. start + count` of a row-major dataset, row-major.
fn cut(values: &[f64], dims: &[i64], start: &[i64], count: &[i64]) -> Vec<f64> {
    let rank = dims.len();
    let mut strides = vec![1i64; rank];
    for k in (0..rank.saturating_sub(1)).rev() {
        strides[k] = strides[k + 1] * dims[k + 1];
    }
    let total: i64 = count.iter().product();
    let mut out = Vec::with_capacity(total.max(0) as usize);
    for n in 0..total {
        // The multi-index of the n-th value of the block.
        let mut rest = n;
        let mut linear = 0i64;
        for k in (0..rank).rev() {
            let at = rest % count[k];
            rest /= count[k];
            linear += (start[k] + at) * strides[k];
        }
        out.push(values[linear as usize]);
    }
    out
}
