//! The HDF5 backend, behind the `hdf5` feature.
//!
//! Everything native lives here and nowhere else: without the feature this
//! module does not exist, and the crate still reads, edits and writes EPC
//! files. That is the arrangement ADR-0007 asks for,
//! and it is what keeps `wasm32` compiling.
//!
//! Built on `hdf5-metno`, the living continuation of the original `hdf5` crate -
//! renamed to `hdf5` in `Cargo.toml`, so the day the original restarts the
//! change is one line and no `use` moves.
//!
//! # Copies
//!
//! A chunked or compressed dataset has to be materialised somewhere, so one
//! copy - file to memory - is the floor. This backend stays at that floor:
//!
//! - [`ArrayBackend::read`] fills a typed `Vec` through HDF5's own
//!   `read_into_raw`, which writes directly into it. **One copy.** Asking for
//!   the bytes afterwards ([`ArrayValues::as_bytes`]) borrows that same memory.
//! - [`ArrayBackend::read_into`] takes the caller's buffer and hands it to the
//!   same call. **One copy and no allocation** - a vertex buffer is filled from
//!   the file with nothing in between.
//!
//! # Open files
//!
//! Held open and reused, bounded, and re-stated on every hit - see `open`, below.
//! Opening is not free and a package reads one file over and over: 218 opens
//! became 1 on the corpus. What makes it safe rather than merely fast is that
//! a cached handle is only trusted while the file's modification time and
//! length are what they were when it was opened.
//!
//! Going below one copy means mapping the file and pointing at it, which HDF5
//! allows only for a contiguous, unfiltered, native-endian dataset - and would
//! need `unsafe`, which this crate forbids. Worth doing the day a profile says
//! so; not worth pretending it is already done.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use hdf5::types::{FloatSize, IntSize, TypeDescriptor};

use crate::array::ByteOrder;

use crate::array::{ArrayBuffer, ArrayData, ArrayMetadata, ArrayValues};
use crate::backend::ArrayBackend;
use crate::error::{Error, ExternalDataReason, Result};

/// Reads arrays from HDF5 files.
pub struct Hdf5Backend;

/// The eight bytes every HDF5 file starts with (HDF5 File Format Specification,
/// section II.A, *Format Signature*): 0x89, `HDF`, CR, LF, 0x1a, LF. A file with a
/// user block carries it at a later offset and is not recognised here: in practice
/// it has a telling extension or a MIME type.
const HDF5_SIGNATURE: [u8; 8] = [0x89, b'H', b'D', b'F', b'\r', b'\n', 0x1a, b'\n'];

fn has_hdf5_signature(file: &Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 8];
    std::fs::File::open(file)
        .and_then(|mut handle| handle.read_exact(&mut head))
        .is_ok()
        && head == HDF5_SIGNATURE
}

impl ArrayBackend for Hdf5Backend {
    fn name(&self) -> &'static str {
        "hdf5"
    }

    fn handles(&self, mime: Option<&str>, file: &Path) -> bool {
        // The MIME type the object declared is the authority (§7); the
        // extension is the fallback for the older form, which carries none.
        if let Some(mime) = mime {
            return mime.contains("hdf5") || mime.contains("hdf");
        }
        if file
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("h5") || e.eq_ignore_ascii_case("hdf5"))
        {
            return true;
        }
        // Neither a MIME type nor a telling extension: a workflow engine hands
        // its tools `dataset_7.dat`. The content decides - the HDF5 signature
        // opens the file - so such a file is read, and a file of another
        // format (CSV, Parquet) still steps aside for the backend that has it.
        has_hdf5_signature(file)
    }

    fn metadata(&self, file: &Path, path_in_file: &str) -> Result<ArrayMetadata> {
        let handle = open(file)?;
        let dataset = dataset(&handle, file, path_in_file)?;
        Ok(ArrayMetadata {
            dimensions: dataset.shape().iter().map(|d| *d as i64).collect(),
            element_type: element_type(&dataset, file, path_in_file)?,
        })
    }

    /// Read from the datatype, through `H5Tget_order`, and never assumed.
    ///
    /// `None` for a one-byte element, where there is nothing to order, and for
    /// the two HDF5 answers that are not an order at all: `None` (the type has
    /// no meaningful one) and `Mixed` (a compound whose members disagree).
    /// Saying nothing is the honest answer there; a caller that must write the
    /// order down then refuses rather than picking the usual one.
    fn byte_order(&self, file: &Path, path_in_file: &str) -> Result<Option<ByteOrder>> {
        let handle = open(file)?;
        let dataset = dataset(&handle, file, path_in_file)?;
        let dtype = dataset
            .dtype()
            .map_err(|e| external(file, path_in_file, ExternalDataReason::Backend(e.to_string())))?;
        if dtype.size() <= 1 {
            return Ok(None);
        }
        Ok(match dtype.byte_order() {
            hdf5::datatype::ByteOrder::LittleEndian => Some(ByteOrder::LittleEndian),
            hdf5::datatype::ByteOrder::BigEndian => Some(ByteOrder::BigEndian),
            _ => None,
        })
    }

    fn read(&self, file: &Path, path_in_file: &str, start: &[i64], count: &[i64]) -> Result<ArrayData> {
        let handle = open(file)?;
        let dataset = dataset(&handle, file, path_in_file)?;
        let shape: Vec<i64> = dataset.shape().iter().map(|d| *d as i64).collect();
        let selection = Selection::of(&shape, start, count, file, path_in_file)?;
        let descriptor = descriptor(&dataset, file, path_in_file)?;
        let wanted: usize = selection.count.iter().product::<i64>().max(0) as usize;

        // One arm per element type: the read is generic, so the type has to be
        // named somewhere, and naming it here keeps every caller free of it.
        // Each arm allocates once and lets HDF5 fill it - no intermediate.
        let values = match &descriptor {
            TypeDescriptor::Float(FloatSize::U8) => {
                ArrayValues::F64(fill(&dataset, &selection, wanted, file, path_in_file)?)
            }
            TypeDescriptor::Float(FloatSize::U4) => {
                ArrayValues::F32(fill(&dataset, &selection, wanted, file, path_in_file)?)
            }
            TypeDescriptor::Integer(IntSize::U8) => {
                ArrayValues::I64(fill(&dataset, &selection, wanted, file, path_in_file)?)
            }
            TypeDescriptor::Integer(IntSize::U4) => {
                ArrayValues::I32(fill(&dataset, &selection, wanted, file, path_in_file)?)
            }
            TypeDescriptor::Integer(IntSize::U2) => {
                ArrayValues::I16(fill(&dataset, &selection, wanted, file, path_in_file)?)
            }
            TypeDescriptor::Integer(IntSize::U1) => {
                ArrayValues::I8(fill(&dataset, &selection, wanted, file, path_in_file)?)
            }
            TypeDescriptor::Unsigned(IntSize::U8) => {
                ArrayValues::U64(fill(&dataset, &selection, wanted, file, path_in_file)?)
            }
            TypeDescriptor::Unsigned(IntSize::U4) => {
                ArrayValues::U32(fill(&dataset, &selection, wanted, file, path_in_file)?)
            }
            TypeDescriptor::Unsigned(IntSize::U2) => {
                ArrayValues::U16(fill(&dataset, &selection, wanted, file, path_in_file)?)
            }
            TypeDescriptor::Unsigned(IntSize::U1) | TypeDescriptor::Boolean => {
                ArrayValues::U8(fill(&dataset, &selection, wanted, file, path_in_file)?)
            }
            // Strings, compounds, references: EnergyML puts numbers in HDF5, so
            // meeting one of these means either a file we did not write or a
            // gap here. Say which, rather than returning bytes nobody can read.
            other => {
                return Err(external(
                    file,
                    path_in_file,
                    ExternalDataReason::Backend(format!(
                        "the dataset holds `{other:?}`, which this backend does not decode"
                    )),
                ));
            }
        };

        Ok(ArrayData {
            metadata: ArrayMetadata {
                dimensions: selection.count.clone(),
                element_type: name_of(&descriptor),
            },
            values,
        })
    }

    fn read_into(
        &self,
        file: &Path,
        path_in_file: &str,
        start: &[i64],
        count: &[i64],
        out: &mut ArrayBuffer<'_>,
    ) -> Result<ArrayMetadata> {
        let handle = open(file)?;
        let dataset = dataset(&handle, file, path_in_file)?;
        let shape: Vec<i64> = dataset.shape().iter().map(|d| *d as i64).collect();
        let selection = Selection::of(&shape, start, count, file, path_in_file)?;
        let descriptor = descriptor(&dataset, file, path_in_file)?;
        let element_type = name_of(&descriptor);
        let wanted: usize = selection.count.iter().product::<i64>().max(0) as usize;

        // A buffer of the wrong size or the wrong type is refused rather than
        // partly filled: half an array that looks whole is the worst outcome.
        if out.len() != wanted {
            return Err(external(
                file,
                path_in_file,
                ExternalDataReason::Backend(format!(
                    "the buffer holds {} element(s), the selection is {wanted}",
                    out.len()
                )),
            ));
        }
        if out.element_type() != element_type {
            return Err(external(
                file,
                path_in_file,
                ExternalDataReason::Backend(format!(
                    "the buffer is `{}`, the dataset is `{element_type}`",
                    out.element_type()
                )),
            ));
        }

        match out {
            ArrayBuffer::F64(b) => into(&dataset, &selection, b, file, path_in_file)?,
            ArrayBuffer::F32(b) => into(&dataset, &selection, b, file, path_in_file)?,
            ArrayBuffer::I64(b) => into(&dataset, &selection, b, file, path_in_file)?,
            ArrayBuffer::I32(b) => into(&dataset, &selection, b, file, path_in_file)?,
            ArrayBuffer::U64(b) => into(&dataset, &selection, b, file, path_in_file)?,
            ArrayBuffer::U32(b) => into(&dataset, &selection, b, file, path_in_file)?,
            ArrayBuffer::U8(b) => into(&dataset, &selection, b, file, path_in_file)?,
        }
        Ok(ArrayMetadata {
            dimensions: selection.count.clone(),
            element_type,
        })
    }

    fn file_uuid(&self, file: &Path) -> Result<Option<String>> {
        let handle = open(file)?;
        let Ok(attribute) = handle.attr("uuid") else {
            return Ok(None);
        };
        // §3.2.3: "a single string of 36 characters […] canonical format (lower
        // case letters only) including the 4 dashes". Fixed-length ASCII is
        // what the specification describes and what the files carry.
        let value = attribute
            .read_scalar::<hdf5::types::FixedAscii<36>>()
            .map(|v| v.to_string())
            .or_else(|_| {
                attribute
                    .read_scalar::<hdf5::types::VarLenUnicode>()
                    .map(|v| v.to_string())
            });
        Ok(value.ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty()))
    }

    fn writes(&self) -> bool {
        true
    }

    fn write(&self, file: &Path, path_in_file: &str, data: &ArrayData) -> Result<()> {
        let handle = open_for_writing(file)?;
        write_into(&handle, file, path_in_file, data)?;
        handle.flush().map_err(|e| {
            external(
                file,
                path_in_file,
                ExternalDataReason::Backend(format!("flushing the file: {e}")),
            )
        })
    }

    /// **The file opened once, flushed once**, whatever the number of
    /// datasets: reopening a growing file for each one was 95 % of what
    /// writing a SIGEOL import cost (see [`ArrayBackend::write_many`]). A
    /// dataset that fails does not stop the others; the file failing to open
    /// or to flush is every dataset's failure, and each says so.
    fn write_many(&self, file: &Path, items: &[(&str, &ArrayData)]) -> Vec<Result<()>> {
        let every = |message: String| -> Vec<Result<()>> {
            items
                .iter()
                .map(|(path, _)| Err(external(file, path, ExternalDataReason::Backend(message.clone()))))
                .collect()
        };
        let handle = match open_for_writing(file) {
            Ok(handle) => handle,
            Err(why) => return every(why.to_string()),
        };
        let written: Vec<Result<()>> = items
            .iter()
            .map(|(path, data)| write_into(&handle, file, path, data))
            .collect();
        if let Err(e) = handle.flush() {
            return every(format!("flushing the file: {e}"));
        }
        written
    }

    fn set_file_uuid(&self, file: &Path, uuid: &str) -> Result<()> {
        let handle = open_for_writing(file)?;
        let fail = |message: String| Error::ExternalData {
            location: file.display().to_string(),
            reason: ExternalDataReason::Backend(message),
        };
        // §3.2.3 asks for "a single string of 36 characters […] canonical
        // format (lower case letters only) including the 4 dashes".
        //
        // Checked against the same rule the reader uses, deliberately:
        // `FixedAscii<36>` is a *maximum*, so it accepts `not-a-uuid` happily
        // and would produce a file that opens fine and fails every conforming
        // cross-validation, including this crate's own.
        let text = uuid.trim().to_ascii_lowercase();
        if !energyml_core::identifiers::is_uuid(&text) {
            return Err(fail(format!(
                "`{uuid}` is not a uuid; §3.2.3 asks for 36 canonical lower-case characters"
            )));
        }
        let value = hdf5::types::FixedAscii::<36>::from_ascii(text.as_bytes())
            .map_err(|_| fail(format!("`{uuid}` is not 36 ASCII characters")))?;
        if handle.attr("uuid").is_ok() {
            handle
                .delete_attr("uuid")
                .map_err(|e| fail(format!("replacing the existing uuid attribute: {e}")))?;
        }
        handle
            .new_attr::<hdf5::types::FixedAscii<36>>()
            .create("uuid")
            .map_err(|e| fail(format!("creating the uuid attribute: {e}")))?
            .write_scalar(&value)
            .map_err(|e| fail(format!("writing the uuid attribute: {e}")))?;
        handle
            .flush()
            .map_err(|e| fail(format!("flushing the file: {e}")))?;
        Ok(())
    }
}

/// The slice to read, in the shape the file actually has.
struct Selection {
    start: Vec<i64>,
    count: Vec<i64>,
    /// Whether it covers everything, in which case the read needs no hyperslab.
    whole: bool,
}

impl Selection {
    fn of(shape: &[i64], start: &[i64], count: &[i64], file: &Path, path_in_file: &str) -> Result<Self> {
        let start: Vec<i64> = if start.is_empty() {
            vec![0; shape.len()]
        } else {
            start.to_vec()
        };
        let count: Vec<i64> = if count.is_empty() {
            shape.to_vec()
        } else {
            count.to_vec()
        };

        // A pointer written for a different shape is a real possibility: the
        // newer EnergyML form carries `Count` and `StartIndex` in the object,
        // and nothing keeps them in step with the file.
        let bad = |message: String| external(file, path_in_file, ExternalDataReason::Backend(message));
        if start.len() != shape.len() || count.len() != shape.len() {
            // One-dimensional bounds over a multi-dimensional dataset: what
            // RESQML writes for a flattened array of points. Read it whole.
            //
            // Two spellings of "the whole thing" are accepted: the count as
            // the flat number of values, and the count as the number of
            // **rows** - a real producer writes `Count=70` for a (70, 3)
            // dataset of points, counting points where the schema counts
            // values. Unambiguous when it equals the leading dimension
            // exactly.
            if start.iter().all(|s| *s == 0)
                && (count.iter().sum::<i64>() >= shape.iter().product::<i64>()
                    || (count.len() == 1 && count[0] == shape[0]))
            {
                return Ok(Self {
                    start: vec![0; shape.len()],
                    count: shape.to_vec(),
                    whole: true,
                });
            }
            return Err(bad(format!(
                "the object asks for {} dimension(s) but the dataset has {}",
                count.len(),
                shape.len()
            )));
        }
        for (axis, ((s, c), extent)) in start.iter().zip(&count).zip(shape).enumerate() {
            if *s < 0 || *c < 0 || s + c > *extent {
                return Err(bad(format!(
                    "dimension {axis}: the object asks for {c} element(s) from {s}, \
                     the dataset holds {extent}"
                )));
            }
        }
        let whole = start.iter().all(|s| *s == 0) && count == shape;
        Ok(Self { start, count, whole })
    }
}

/// How many files stay open at once, unless a caller says otherwise.
///
/// Small on purpose. A package's arrays live in **one** `.h5` in every corpus
/// file and in every producer's output, so one entry already gets the whole
/// win; the rest is headroom for a model split across a handful of files. What
/// the bound is really for is the process file-descriptor limit - an unbounded
/// cache over a directory of a thousand `.h5` files is a crash, not a cache.
const DEFAULT_OPEN_LIMIT: usize = 8;

/// One file held open, and what it looked like when it was.
struct Cached {
    path: PathBuf,
    handle: hdf5::File,
    /// Modification time and length at the moment it was opened.
    ///
    /// The reason the cache is safe. A handle held across somebody else's write
    /// serves the data the file used to hold, which is a correctness bug and
    /// not a performance one - so every hit re-stats the file, and a stamp that
    /// moved throws the handle away. A `stat` costs a microsecond or two
    /// against the 1.5 ms an open costs, so the check keeps essentially all of
    /// the win and none of the risk.
    stamp: (Option<SystemTime>, u64),
}

/// The open files, most recently used last.
struct OpenFiles {
    entries: Vec<Cached>,
    limit: usize,
    /// How many times a file was really opened, cache misses included.
    ///
    /// Exposed as [`real_opens`] so a test can assert the cache *works* rather
    /// than time it. A timing assertion on a shared machine is a flaky test
    /// pretending to be a benchmark; a counter is the thing itself.
    opens: u64,
}

fn cache() -> &'static Mutex<OpenFiles> {
    static FILES: OnceLock<Mutex<OpenFiles>> = OnceLock::new();
    FILES.get_or_init(|| {
        Mutex::new(OpenFiles {
            entries: Vec::new(),
            limit: DEFAULT_OPEN_LIMIT,
            opens: 0,
        })
    })
}

fn stamp_of(file: &Path) -> (Option<SystemTime>, u64) {
    match std::fs::metadata(file) {
        Ok(data) => (data.modified().ok(), data.len()),
        Err(_) => (None, 0),
    }
}

/// How many HDF5 files may stay open at once.
pub fn open_file_limit() -> usize {
    cache()
        .lock()
        .map(|files| files.limit)
        .unwrap_or(DEFAULT_OPEN_LIMIT)
}

/// Change that bound. Zero turns the cache off entirely.
///
/// Lowering it closes the excess immediately, which is the point: a caller that
/// is about to move or delete a file needs the handle gone *now*, and on
/// Windows an open handle makes the rename fail rather than merely go stale.
pub fn set_open_file_limit(limit: usize) {
    if let Ok(mut files) = cache().lock() {
        files.limit = limit;
        while files.entries.len() > limit {
            files.entries.remove(0);
        }
    }
}

/// How many times an HDF5 file has really been opened in this process.
///
/// The cache's own scoreboard: two reads of one file that cost one open is the
/// whole claim, and this is how it is checked.
pub fn real_opens() -> u64 {
    cache().lock().map(|files| files.opens).unwrap_or(0)
}

/// Close every cached file.
///
/// Call it before moving, replacing or deleting an `.h5` - see
/// [`set_open_file_limit`] for why that is not merely tidy on Windows.
pub fn close_cached_files() {
    if let Ok(mut files) = cache().lock() {
        files.entries.clear();
    }
}

/// Forget one file, whatever the cache holds for it.
fn evict(file: &Path) {
    if let Ok(mut files) = cache().lock() {
        files.entries.retain(|entry| entry.path != file);
    }
}

/// Open a file for reading, or hand back the handle already open on it.
///
/// Measured before it was written, because a cache nobody needs is a bug with
/// good intentions. One pass over `testingPackageCpp.epc` touches **218
/// arrays**, all in one `.h5`: without a cache that is 218 opens, with one it
/// is **1**, and the metadata pass goes from 18 ms to 5 ms. The ratio is what
/// travels - a reservoir model carries arrays by the ten thousand, and the
/// requirement's own observation is right that they nearly always live in a
/// single file.
fn open(file: &Path) -> Result<hdf5::File> {
    let stamp = stamp_of(file);
    if let Ok(mut files) = cache().lock()
        && let Some(at) = files.entries.iter().position(|entry| entry.path == file)
    {
        if files.entries[at].stamp == stamp {
            // Most recently used goes last, so the eviction below drops the
            // one nobody has wanted for longest.
            let entry = files.entries.remove(at);
            let handle = entry.handle.clone();
            files.entries.push(entry);
            return Ok(handle);
        }
        // Somebody wrote to it. The handle is of the file that was.
        files.entries.remove(at);
    }

    if let Ok(mut files) = cache().lock() {
        files.opens += 1;
    }
    let handle = hdf5::File::open(file).map_err(|e| Error::ExternalData {
        location: file.display().to_string(),
        reason: ExternalDataReason::Backend(e.to_string()),
    })?;

    if let Ok(mut files) = cache().lock()
        && files.limit > 0
    {
        files.entries.push(Cached {
            path: file.to_path_buf(),
            handle: handle.clone(),
            stamp,
        });
        while files.entries.len() > files.limit {
            files.entries.remove(0);
        }
    }
    Ok(handle)
}

/// Open for writing, creating the file if it is not there.
///
/// Deliberately **not** `File::create`, which truncates: a package's arrays all
/// live in one `.h5`, so writing the second one must not erase the first.
/// One dataset into a file already open for writing - the groups leading to
/// it, the dataset replaced when it exists. Neither opens nor flushes: that is
/// the caller's, once for one dataset or for many.
fn write_into(handle: &hdf5::File, file: &Path, path_in_file: &str, data: &ArrayData) -> Result<()> {
    let bad = |message: String| external(file, path_in_file, ExternalDataReason::Backend(message));

    // The shape is the object's, not the buffer's, and the two disagreeing
    // is the caller's bug - writing the values under a shape that does not
    // describe them produces a file that reads back as something else.
    let shape: Vec<usize> = data
        .metadata
        .dimensions
        .iter()
        .map(|d| (*d).max(0) as usize)
        .collect();
    let declared: usize = shape.iter().product();
    if shape.is_empty() {
        return Err(bad("the array declares no dimensions".to_string()));
    }
    if declared != data.values.len() {
        return Err(bad(format!(
            "the shape {shape:?} describes {declared} element(s), the values hold {}",
            data.values.len()
        )));
    }

    // Groups first: `/resqml20/<uuid>/points_patch0` needs two of them, and
    // HDF5 will not create them on the way past.
    if let Some((groups, _)) = path_in_file.rsplit_once('/') {
        let mut current = String::new();
        for name in groups.split('/').filter(|s| !s.is_empty()) {
            current.push('/');
            current.push_str(name);
            if handle.group(&current).is_err() {
                handle
                    .create_group_builder()
                    // No modification time in the header: see `open_for_writing`.
                    .obj_track_times(false)
                    .create(current.as_str())
                    .map_err(|e| bad(format!("creating the group `{current}`: {e}")))?;
            }
        }
    }

    // HDF5 cannot resize a dataset in place, so replacing one means
    // unlinking it first. The space is not reclaimed - see the trait doc.
    if handle.dataset(path_in_file).is_ok() {
        handle
            .unlink(path_in_file)
            .map_err(|e| bad(format!("replacing the existing dataset: {e}")))?;
    }

    macro_rules! put {
        ($values:expr, $ty:ty) => {{
            let builder = handle.new_dataset::<$ty>().shape(shape.clone());
            let dataset = builder
                .create(path_in_file)
                .map_err(|e| bad(format!("creating the dataset: {e}")))?;
            dataset
                .write_raw($values)
                .map_err(|e| bad(format!("writing the values: {e}")))?;
        }};
    }
    match &data.values {
        ArrayValues::F64(v) => put!(v, f64),
        ArrayValues::F32(v) => put!(v, f32),
        ArrayValues::I64(v) => put!(v, i64),
        ArrayValues::I32(v) => put!(v, i32),
        ArrayValues::I16(v) => put!(v, i16),
        ArrayValues::I8(v) => put!(v, i8),
        ArrayValues::U64(v) => put!(v, u64),
        ArrayValues::U32(v) => put!(v, u32),
        ArrayValues::U16(v) => put!(v, u16),
        ArrayValues::U8(v) => put!(v, u8),
    }
    Ok(())
}

fn open_for_writing(file: &Path) -> Result<hdf5::File> {
    // A read handle held on this file would go stale the moment the write
    // lands, and HDF5 will not open a file read-write while it is open
    // read-only in the same process. Dropped before, not after.
    evict(file);
    let fail = |e: hdf5::Error| Error::ExternalData {
        location: file.display().to_string(),
        reason: ExternalDataReason::Backend(e.to_string()),
    };
    if file.exists() {
        return hdf5::File::open_rw(file).map_err(fail);
    }
    if let Some(folder) = file.parent()
        && !folder.as_os_str().is_empty()
        && !folder.exists()
    {
        std::fs::create_dir_all(folder).map_err(|e| Error::ExternalData {
            location: folder.display().to_string(),
            reason: ExternalDataReason::Backend(e.to_string()),
        })?;
    }
    // **No clock in the file.** HDF5 writes a modification time into the
    // header of every group by default (the root included; datasets are left
    // untimed by `hdf5-metno` already), so two runs of the same recipe wrote
    // two `.h5` differing in 946 bytes and nothing else - measured on
    // Lutetia, 28/09. A file that is a function of its inputs is what lets a
    // package be replayed and **compared**, the way `generate --check` holds
    // the generated crates: the Lutetia example the front end opens is versioned
    // and checked that way.
    hdf5::File::with_options()
        .with_fcpl(|plist| plist.obj_track_times(false))
        .create(file)
        .map_err(fail)
}

fn dataset(handle: &hdf5::File, file: &Path, path_in_file: &str) -> Result<hdf5::Dataset> {
    handle.dataset(path_in_file).map_err(|_| Error::ExternalData {
        location: file.display().to_string(),
        reason: ExternalDataReason::PathMissing(path_in_file.to_string()),
    })
}

fn descriptor(dataset: &hdf5::Dataset, file: &Path, path_in_file: &str) -> Result<TypeDescriptor> {
    dataset
        .dtype()
        .and_then(|dtype| dtype.to_descriptor())
        .map_err(|e| external(file, path_in_file, ExternalDataReason::Backend(e.to_string())))
}

fn element_type(dataset: &hdf5::Dataset, file: &Path, path_in_file: &str) -> Result<String> {
    Ok(name_of(&descriptor(dataset, file, path_in_file)?))
}

/// Allocate once, and let HDF5 write into it.
fn fill<T: hdf5::H5Type + Copy + Default>(
    dataset: &hdf5::Dataset,
    selection: &Selection,
    wanted: usize,
    file: &Path,
    path_in_file: &str,
) -> Result<Vec<T>> {
    let mut buffer = vec![T::default(); wanted];
    into(dataset, selection, &mut buffer, file, path_in_file)?;
    Ok(buffer)
}

/// The one read: straight into the caller's memory, whoever the caller is.
fn into<T: hdf5::H5Type + Copy>(
    dataset: &hdf5::Dataset,
    selection: &Selection,
    buffer: &mut [T],
    file: &Path,
    path_in_file: &str,
) -> Result<()> {
    let fail = |e: hdf5::Error| external(file, path_in_file, ExternalDataReason::Backend(e.to_string()));

    if selection.whole {
        // `read_into_raw` hands HDF5 the caller's slice and it decodes there.
        let read = dataset.read_into_raw(buffer).map_err(fail)?;
        if read != buffer.len() {
            return Err(external(
                file,
                path_in_file,
                ExternalDataReason::Backend(format!(
                    "read {read} element(s) into a buffer of {}",
                    buffer.len()
                )),
            ));
        }
        return Ok(());
    }

    // A hyperslab still goes through the crate's `ndarray` return type; the
    // copy out of it is the price of a partial read until `hdf5-metno` grows a
    // `read_slice_into`. The whole-dataset path - every array in the corpus -
    // does not pay it.
    let hyperslab: Vec<hdf5::SliceOrIndex> = selection
        .start
        .iter()
        .zip(&selection.count)
        .map(|(start, count)| hdf5::SliceOrIndex::SliceCount {
            start: *start as usize,
            step: 1,
            block: 1,
            count: *count as usize,
        })
        .collect();
    let array = dataset
        .read_slice::<T, _, ndarray::IxDyn>(hdf5::Hyperslab::from(hyperslab))
        .map_err(fail)?;
    for (slot, value) in buffer.iter_mut().zip(array.iter()) {
        *slot = *value;
    }
    Ok(())
}

/// The element type, named the way a caller can act on.
fn name_of(descriptor: &TypeDescriptor) -> String {
    match descriptor {
        TypeDescriptor::Float(FloatSize::U8) => "double".to_string(),
        TypeDescriptor::Float(FloatSize::U4) => "float".to_string(),
        TypeDescriptor::Integer(size) => format!("int{}", *size as usize * 8),
        TypeDescriptor::Unsigned(size) => format!("uint{}", *size as usize * 8),
        TypeDescriptor::Boolean => "bool".to_string(),
        other => format!("{other:?}"),
    }
}

fn external(file: &Path, path_in_file: &str, reason: ExternalDataReason) -> Error {
    Error::ExternalData {
        location: format!("{}:{path_in_file}", file.display()),
        reason,
    }
}
