//! Finding, and later reading, the arrays that live outside the package.
//!
//! An EnergyML object rarely carries its numbers. It carries a *pointer* to
//! them, and there are two spellings of that pointer, both in use:
//!
//! | | RESQML 2.0.1 / eml 2.0–2.2 | RESQML 2.2 / eml 2.3 |
//! |---|---|---|
//! | in the object | `Hdf5Dataset`: a DOR to an `EpcExternalPartReference`, plus `PathInHdfFile` | `ExternalDataArrayPart`: `URI`, `PathInExternalFile`, `MimeType`, `Count`, `StartIndex` |
//! | file name | in the proxy's `.rels`, as an `externalResource` target | in the object, in `URI` |
//!
//! The newer form is the richer one: it carries the slice as well as the
//! location, which is exactly what ETP's `GetDataSubarrays` needs. Both are
//! read here, into one [`ArrayLocation`].
//!
//! A third pointer is **this library's extension, not the standard's**: a
//! WITSML 2.1 `ChannelData` whose `FileUri` names an HDF5 file and whose `Data`
//! holds the path of a dataset in it (ADR-0013).
//! Located here like the other two, so that everything that walks the arrays
//! of an object — a cut, an import, a rename, a prefetch — carries it along
//! instead of losing it in silence.
//!
//! # The path inside the file is opaque
//!
//! `PathInExternalFile` and `PathInHdfFile` are **strings, and nothing else**.
//! fesapi writes `/resqml22/<uuid>/Points` and the convention is convenient,
//! but no part of the schema requires the uuid to be in there, or requires any
//! shape at all. Nothing here parses, splits or validates the path — it is
//! handed to the backend exactly as the object wrote it, and a package that
//! calls its dataset `/my/points` works. `a_path_need_not_contain_a_uuid` in
//! the tests pins that down, because it is the kind of assumption that gets
//! added later by someone who has only ever seen fesapi files.
//!
//! # And the format is not necessarily HDF5
//!
//! Both spellings carry a MIME type, so both can point at something else — a
//! CSV, a Parquet file, whatever a producer and a consumer agree on. eml 2.3
//! puts it in the part (`MimeType`); eml 2.0–2.2 put it on the
//! `EpcExternalPartReference`, where it is a **mandatory** element whose
//! documentation reads "MIME type for HDF proxy is: application/x-hdf5 (by
//! RESQML convention)" — a convention, therefore, not a rule. The MIME type is
//! what picks the backend; HDF5 is merely the one that ships.
//!
//! [`locate`] is a **free function over an object already in memory**, not a
//! method on a store. That is the rule the whole crate follows: the store does
//! I/O, the logic sits outside it, and nothing has to be written twice for the
//! synchronous and the asynchronous paths.

use energyml_core::identity;
use energyml_core::reflect::TypeKind;
use energyml_core::value::{FieldValue, Reflect, Scalar};

/// Where one array — or one slice of one — actually lives.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ArrayLocation {
    /// The file, as the package names it. A bare file name in practice, though
    /// the schema suggests a URI; both are accepted.
    ///
    /// Empty when the object points at a proxy the caller has not resolved:
    /// see [`locate_with`].
    pub file: String,
    /// Path of the dataset inside the file, e.g. `/resqml22/<uuid>/Points`.
    pub path_in_file: String,
    /// MIME type declared for the data — `application/x-hdf5`, but the field
    /// exists precisely so it need not be.
    ///
    /// eml 2.3 carries it in the part itself; eml 2.0–2.2 carry it on the
    /// `EpcExternalPartReference`, which is why resolving a proxy yields this
    /// as well as the file name (see [`ProxyTarget`]).
    pub mime: Option<String>,
    /// Start index per dimension. Empty means "from the beginning".
    pub start: Vec<i64>,
    /// Element count per dimension. Empty means "the whole dataset".
    pub count: Vec<i64>,
    /// Uuid of the `EpcExternalPartReference` this came through, for the older
    /// form. `None` for the eml 2.3 form, which has no proxy.
    pub proxy_uuid: Option<String>,
    /// Dotted path of the array inside the object, for diagnostics:
    /// `Geometry.Points.Coordinates`.
    pub field_path: String,
}

impl ArrayLocation {
    /// Whether the file is known. It is not, when the object points at a proxy
    /// and no resolver was given.
    ///
    /// # A shape borrowed from the file, and what ETP will do with it
    ///
    /// "Resolved" here means *the store filled in a file name*, and that is a
    /// disk word in a type that is meant to serve a network store too. It is
    /// honest for an EPC: the `EpcExternalPartReference` is a real indirection
    /// and following it is a real step, so an unresolved location is a genuine
    /// state a caller can be handed.
    ///
    /// It does not carry over. An ETP dataspace has no files: a data array is
    /// addressed by the URI of the object holding it plus a `pathInResource`,
    /// and `GetDataArrays` takes exactly that pair. A client filling
    /// [`ArrayLocation::file`] would be inventing a file name so that
    /// `is_resolved` returns true, which is the wrong way round.
    ///
    /// Left as it is, deliberately, rather than generalized on a guess: the
    /// choice is between widening `file` into an opaque "where the store keeps
    /// it" string and giving the ETP client its own addressing with a
    /// conversion at the seam. Which one is right depends on what the client
    /// actually needs to pass to `GetDataArrays`, and that is settled when the
    /// client is written, not before. It is recorded as an open question so
    /// it is a decision taken rather than a leak nobody noticed.
    pub fn is_resolved(&self) -> bool {
        !self.file.is_empty()
    }
}

/// What an `EpcExternalPartReference` stands for.
///
/// Two things, not one: the older EnergyML form keeps the file name in the
/// proxy's `.rels` **and** the MIME type on the proxy object, so resolving a
/// proxy has to yield both or the format is lost and every external file is
/// assumed to be HDF5.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProxyTarget {
    /// The file the proxy names.
    pub file: String,
    /// The MIME type the proxy declares, when it declares a usable one.
    pub mime: Option<String>,
}

impl ProxyTarget {
    /// A target that is only a file name.
    pub fn file(name: impl Into<String>) -> Self {
        Self {
            file: name.into(),
            mime: None,
        }
    }
}

/// Turns the uuid of an `EpcExternalPartReference` into what it stands for.
///
/// In an EPC that means reading the proxy's `.rels` for the file name and the
/// proxy object for the MIME type. Implemented by the store, because only the
/// store can see either.
pub trait ProxyResolver {
    /// What the proxy with this uuid names, if the package knows.
    fn resolve(&self, proxy_uuid: &str) -> Option<ProxyTarget>;
}

impl<F: Fn(&str) -> Option<ProxyTarget>> ProxyResolver for F {
    fn resolve(&self, proxy_uuid: &str) -> Option<ProxyTarget> {
        self(proxy_uuid)
    }
}

/// Nothing to resolve with: proxies stay unresolved, the eml 2.3 form still works.
pub struct NoProxies;

impl ProxyResolver for NoProxies {
    fn resolve(&self, _proxy_uuid: &str) -> Option<ProxyTarget> {
        None
    }
}

/// Every array the object points at, both spellings, without a resolver.
///
/// Locations that go through a proxy come back with an empty `file` and a
/// `proxy_uuid` set — enough to ask the store afterwards.
pub fn locate(object: &dyn Reflect) -> Vec<ArrayLocation> {
    locate_with(object, &NoProxies)
}

/// Every array the object points at, resolving proxies through `resolver`.
pub fn locate_with(object: &dyn Reflect, resolver: &dyn ProxyResolver) -> Vec<ArrayLocation> {
    let mut out = Vec::new();
    walk(object, String::new(), resolver, &mut out, 0);
    out
}

/// The type names that mean "here is where the numbers are".
///
/// Three, not two, and the middle one was missing until 31/08. eml 2.1 and 2.2
/// carry `ExternalDatasetPart` — reached through a field spelled
/// `ExternalFileProxy` — which is the older shape's proxy reference with the
/// newer shape's `PathInExternalFile`. It is neither of its neighbours, and
/// nothing matched it: every array of every eml 2.1/2.2 package came back as
/// "declares no external data at all". That is witsml 2.0, prodml 2.0 and
/// resqml 2.2_dev3, whose 362 polylines refused to draw with their coordinates
/// sitting right there in the document.
const NEW_FORM: &str = "ExternalDataArrayPart";
const MID_FORM: &str = "ExternalDatasetPart";
const OLD_FORM: &str = "Hdf5Dataset";

/// A WITSML `ChannelData` whose rows are a dataset of an HDF5 file — **an
/// extension beyond the standard** (ADR-0013).
///
/// WITSML TRG v2.1 §7.2 p. 146 gives `ChannelData` two forms: `Data`, *"The
/// data blob in JSON form"*, and `FileUri`, *"The URI of a file containing the
/// bulk data. If this field is non-null, then the data field is ignored."*
/// The extension is the pair: `FileUri` names the file as the standard has it
/// name one (a relative URI, the same string an `ExternalDataArrayPart.URI`
/// of the package holds), and `Data` — which the standard ignores once
/// `FileUri` is set — holds the **absolute path of the dataset** inside it.
/// [`is_channel_data_in_file`] is the one definition of the form.
const CHANNEL_DATA: &str = "ChannelData";

/// Whether a `ChannelData`'s `FileUri` and `Data` are the extension of
/// [`CHANNEL_DATA`] — the **one** definition, which the locator here and the
/// series reader of `energyml-domain` both ask.
///
/// Both present, and `Data` an absolute HDF5 path: it begins with `/`, which
/// **no JSON text can** — RFC 8259 §2, a JSON text is optional whitespace then
/// a value, and a value begins with `{`, `[`, `"`, `-`, a digit, `t`, `f` or
/// `n`. So the test is not a guess on content: the two syntaxes are disjoint,
/// and a standard file that writes `FileUri` **beside** an inline blob (the
/// standard then ignores the blob, §7.2 p. 146) stays the standard's form —
/// never a dataset path made of JSON. `FileUri` alone is the standard's form
/// too (a bulk file in the release notes' format, not an HDF5 dataset).
pub fn is_channel_data_in_file(file_uri: &str, data: &str) -> bool {
    !file_uri.trim().is_empty() && data.starts_with('/')
}

/// The MIME type the extension implies: the file `FileUri` names is HDF5 by
/// the extension's own definition — `ChannelData` has no field to say it.
const CHANNEL_DATA_MIME: &str = "application/x-hdf5";

/// The field of a located part that names its **file** — what a rename or an
/// import rewrites. `uri` for the eml 2.3 form; `file_uri` for a WITSML
/// `ChannelData` ([`CHANNEL_DATA`]). The two proxy forms name no file here
/// (their proxy's `.rels` does), and are never asked.
pub fn file_field(part_type: &str) -> &'static str {
    match part_type {
        CHANNEL_DATA => "file_uri",
        _ => "uri",
    }
}

/// The field of a located part that holds the **path inside the file** —
/// `path_in_hdf_file` for the eml 2.0 form, `data` for a WITSML `ChannelData`,
/// `path_in_external_file` otherwise.
pub fn path_field(part_type: &str) -> &'static str {
    match part_type {
        OLD_FORM => "path_in_hdf_file",
        CHANNEL_DATA => "data",
        _ => "path_in_external_file",
    }
}

fn walk(
    object: &dyn Reflect,
    path: String,
    resolver: &dyn ProxyResolver,
    out: &mut Vec<ArrayLocation>,
    depth: u32,
) {
    // Objects reference each other only through DORs, never by containment, so
    // the tree is finite. The bound is a seatbelt, not a design assumption.
    if depth > 64 {
        return;
    }

    let info = object.type_info();
    match info.name {
        NEW_FORM => {
            out.push(from_new_form(object, &path));
            return;
        }
        MID_FORM => {
            out.push(from_mid_form(object, &path, resolver));
            return;
        }
        OLD_FORM => {
            out.push(from_old_form(object, &path, resolver));
            return;
        }
        CHANNEL_DATA => {
            if let Some(found) = from_channel_data(object, &path) {
                out.push(found);
            }
            return;
        }
        _ => {}
    }

    for field in info.fields {
        let Some(value) = object.field(field.name) else {
            continue;
        };
        let child_path = if path.is_empty() {
            field.xml_name.to_string()
        } else {
            format!("{path}.{}", field.xml_name)
        };
        walk_value(&value, child_path, resolver, out, depth);
    }
}

fn walk_value(
    value: &FieldValue<'_>,
    path: String,
    resolver: &dyn ProxyResolver,
    out: &mut Vec<ArrayLocation>,
    depth: u32,
) {
    match value {
        FieldValue::Object(child) => walk(*child, path, resolver, out, depth + 1),
        FieldValue::List(items) => {
            for (i, item) in items.iter().enumerate() {
                walk_value(item, format!("{path}[{i}]"), resolver, out, depth);
            }
        }
        _ => {}
    }
}

/// A `ChannelData` naming a file **and** a dataset — [`is_channel_data_in_file`];
/// `None` for either standard form, which carries no dataset to locate.
///
/// The path is handed over as written, like every path here: never trimmed or
/// split beyond the one character that tells the form.
fn from_channel_data(object: &dyn Reflect, path: &str) -> Option<ArrayLocation> {
    let file = string_field(object, "file_uri")?;
    let dataset = string_field(object, "data")?;
    if !is_channel_data_in_file(&file, &dataset) {
        return None;
    }
    Some(ArrayLocation {
        file,
        path_in_file: dataset,
        mime: Some(CHANNEL_DATA_MIME.to_string()),
        start: Vec::new(),
        count: Vec::new(),
        proxy_uuid: None,
        field_path: path.to_string(),
    })
}

fn from_new_form(object: &dyn Reflect, path: &str) -> ArrayLocation {
    ArrayLocation {
        file: string_field(object, "uri").unwrap_or_default(),
        path_in_file: string_field(object, "path_in_external_file").unwrap_or_default(),
        mime: string_field(object, "mime_type"),
        start: int_list(object, "start_index"),
        count: int_list(object, "count"),
        proxy_uuid: None,
        field_path: path.to_string(),
    }
}

/// eml 2.1 / 2.2: a proxy reference like the old form, a path like the new one.
///
/// The DOR is spelled `EpcExternalPartReference` here rather than `HdfProxy`,
/// and it names the same thing: the part whose `.rels` carries the file name.
/// `Count` and `StartIndex` are single values, not lists — this shape predates
/// the multi-dimensional slicing eml 2.3 added — but they go into the same
/// vectors, so every reader above sees one kind of `ArrayLocation`.
fn from_mid_form(object: &dyn Reflect, path: &str, resolver: &dyn ProxyResolver) -> ArrayLocation {
    let proxy_uuid = object
        .field("epc_external_part_reference")
        .and_then(|value| match value {
            FieldValue::Object(dor) => identity::uuid_of_reflect(dor).map(str::to_string),
            _ => None,
        })
        .filter(|uuid| !uuid.is_empty());

    let target = proxy_uuid
        .as_deref()
        .and_then(|uuid| resolver.resolve(uuid))
        .unwrap_or_default();
    ArrayLocation {
        file: target.file,
        path_in_file: string_field(object, "path_in_external_file").unwrap_or_default(),
        // Declared by the proxy, as in the old form: this part says where in
        // the file, never which file.
        mime: target.mime,
        // **`StartIndex` and `Count` are deliberately not carried over.**
        //
        // They are single values describing a *flat* range — `ExternalFileProxy`
        // repeats, so an array may be split across parts — while
        // `ArrayLocation::start`/`count` are the per-dimension hyperslab eml 2.3
        // introduced. The two are not the same thing, and passing one as the
        // other is not a small error: on this corpus it made every polyline ask
        // for one dimension of a two-dimensional dataset and refuse, with the
        // coordinates sitting right there. Empty means "the whole dataset",
        // which is what the old form has always done and what a single part
        // means anyway.
        //
        // A genuinely *split* array would need flat-range reading, which no
        // backend here offers; it would come back as one location per part,
        // each naming the whole dataset. No package in the corpus splits one,
        // so that is a gap named rather than a guess made.
        start: Vec::new(),
        count: Vec::new(),
        proxy_uuid,
        field_path: path.to_string(),
    }
}

fn from_old_form(object: &dyn Reflect, path: &str, resolver: &dyn ProxyResolver) -> ArrayLocation {
    // `HdfProxy` is a DataObjectReference; its uuid names the
    // `EpcExternalPartReference` whose `.rels` carries the file name.
    let proxy_uuid = object
        .field("hdf_proxy")
        .and_then(|value| match value {
            FieldValue::Object(dor) => identity::uuid_of_reflect(dor).map(str::to_string),
            _ => None,
        })
        .filter(|uuid| !uuid.is_empty());

    let target = proxy_uuid
        .as_deref()
        .and_then(|uuid| resolver.resolve(uuid))
        .unwrap_or_default();
    ArrayLocation {
        file: target.file,
        path_in_file: string_field(object, "path_in_hdf_file").unwrap_or_default(),
        // Declared by the proxy, not by this part — which is the whole reason
        // resolving a proxy returns two things.
        mime: target.mime,
        start: Vec::new(),
        count: Vec::new(),
        proxy_uuid,
        field_path: path.to_string(),
    }
}

fn string_field(object: &dyn Reflect, name: &str) -> Option<String> {
    match object.field(name)? {
        FieldValue::Scalar(Scalar::Str(s)) if !s.is_empty() => Some(s.to_string()),
        _ => None,
    }
}

fn int_list(object: &dyn Reflect, name: &str) -> Vec<i64> {
    let Some(value) = object.field(name) else {
        return Vec::new();
    };
    let one = |v: &FieldValue<'_>| match v {
        FieldValue::Scalar(Scalar::Int(i)) => Some(*i),
        FieldValue::Scalar(Scalar::UInt(u)) => i64::try_from(*u).ok(),
        _ => None,
    };
    match value {
        FieldValue::List(items) => items.iter().filter_map(one).collect(),
        other => one(&other).into_iter().collect(),
    }
}

/// Which end of a multi-byte number the file writes first.
///
/// Not part of [`ArrayMetadata`], and that is deliberate: ETP's
/// `GetDataArrayMetadata` answers a shape and a type and says nothing about
/// byte order, because a transfer defines its own. This is a property of **the
/// file**, and it matters to exactly one kind of caller — one that must write
/// down how the bytes are stored, as an EML 2.3 external array declaration
/// does. So it is asked for separately, by
/// [`crate::backend::ArrayBackend::byte_order`], and never inferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteOrder {
    /// Least significant byte first.
    LittleEndian,
    /// Most significant byte first.
    BigEndian,
}

impl std::fmt::Display for ByteOrder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ByteOrder::LittleEndian => "little-endian",
            ByteOrder::BigEndian => "big-endian",
        })
    }
}

/// Shape and element type of a dataset, without reading it.
///
/// The answer to ETP's `GetDataArrayMetadata`, and what a viewer needs before
/// deciding whether it can afford the data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArrayMetadata {
    /// Number of elements along each dimension.
    pub dimensions: Vec<i64>,
    /// Element type, as the backend names it (`double`, `int64`, …).
    pub element_type: String,
}

/// A block of numbers, in the type the file holds them.
///
/// **Typed, not converted.** A dataset of doubles comes back as `Vec<f64>`,
/// integers as `Vec<i32>`, and so on — because the point of reading a million
/// points is to hand them to something that draws them, and every conversion on
/// the way is a copy of the whole array. See [`ArrayData`] for the counting.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ArrayValues {
    /// `H5T_IEEE_F64` and friends.
    F64(Vec<f64>),
    /// 32-bit floats.
    F32(Vec<f32>),
    /// Signed integers, by width.
    I64(Vec<i64>),
    /// Signed 32-bit.
    I32(Vec<i32>),
    /// Signed 16-bit.
    I16(Vec<i16>),
    /// Signed 8-bit.
    I8(Vec<i8>),
    /// Unsigned 64-bit.
    U64(Vec<u64>),
    /// Unsigned 32-bit.
    U32(Vec<u32>),
    /// Unsigned 16-bit.
    U16(Vec<u16>),
    /// Unsigned 8-bit, and booleans, which HDF5 stores as bytes.
    U8(Vec<u8>),
}

impl ArrayValues {
    /// How many values there are.
    pub fn len(&self) -> usize {
        match self {
            Self::F64(v) => v.len(),
            Self::F32(v) => v.len(),
            Self::I64(v) => v.len(),
            Self::I32(v) => v.len(),
            Self::I16(v) => v.len(),
            Self::I8(v) => v.len(),
            Self::U64(v) => v.len(),
            Self::U32(v) => v.len(),
            Self::U16(v) => v.len(),
            Self::U8(v) => v.len(),
        }
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The element type, named the way [`ArrayMetadata`] names it.
    /// Every element widened to `f64` — see [`ArrayData::to_f64`], which is
    /// this with the shape beside it. **This copies.**
    pub fn to_f64(&self) -> Vec<f64> {
        match self {
            ArrayValues::F64(v) => v.clone(),
            ArrayValues::F32(v) => v.iter().map(|x| *x as f64).collect(),
            ArrayValues::I64(v) => v.iter().map(|x| *x as f64).collect(),
            ArrayValues::I32(v) => v.iter().map(|x| *x as f64).collect(),
            ArrayValues::I16(v) => v.iter().map(|x| *x as f64).collect(),
            ArrayValues::I8(v) => v.iter().map(|x| *x as f64).collect(),
            ArrayValues::U64(v) => v.iter().map(|x| *x as f64).collect(),
            ArrayValues::U32(v) => v.iter().map(|x| *x as f64).collect(),
            ArrayValues::U16(v) => v.iter().map(|x| *x as f64).collect(),
            ArrayValues::U8(v) => v.iter().map(|x| *x as f64).collect(),
        }
    }

    /// The element type, named the way [`ArrayMetadata`] names it.
    pub fn element_type(&self) -> &'static str {
        match self {
            Self::F64(_) => "double",
            Self::F32(_) => "float",
            Self::I64(_) => "int64",
            Self::I32(_) => "int32",
            Self::I16(_) => "int16",
            Self::I8(_) => "int8",
            Self::U64(_) => "uint64",
            Self::U32(_) => "uint32",
            Self::U16(_) => "uint16",
            Self::U8(_) => "uint8",
        }
    }

    /// The same memory seen as bytes. **No copy** — a borrow, for a caller
    /// about to hand the block to a GPU, a socket or a file.
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::F64(v) => bytemuck::cast_slice(v),
            Self::F32(v) => bytemuck::cast_slice(v),
            Self::I64(v) => bytemuck::cast_slice(v),
            Self::I32(v) => bytemuck::cast_slice(v),
            Self::I16(v) => bytemuck::cast_slice(v),
            Self::I8(v) => bytemuck::cast_slice(v),
            Self::U64(v) => bytemuck::cast_slice(v),
            Self::U32(v) => bytemuck::cast_slice(v),
            Self::U16(v) => bytemuck::cast_slice(v),
            Self::U8(v) => v,
        }
    }
}

/// A block of numbers, as read, with the shape it came in.
///
/// # How many copies
///
/// Reading a compressed or chunked dataset cannot be done without the library
/// materialising it somewhere, so **one** copy — from the file into the
/// caller's memory — is the floor. What this type is shaped to avoid is the
/// second and third:
///
/// | | |
/// |---|---|
/// | [`ArrayData::values`] | filled straight by the backend. One copy |
/// | [`ArrayValues::as_bytes`] | a borrow of that same memory. **Zero** |
/// | [`ArrayData::f64`] and friends | a borrow when the type already matches. **Zero** |
/// | [`ArrayData::to_f64`] | widens every element. One more copy, and you asked for it |
///
/// And [`crate::ArrayBackend::read_into`] removes even the first allocation:
/// the caller supplies the buffer — a vertex buffer, a memory-mapped region —
/// and the file is decoded straight into it.
#[derive(Debug, Clone, PartialEq)]
pub struct ArrayData {
    /// Shape of what was read.
    pub metadata: ArrayMetadata,
    /// The values, in the file's own element type.
    pub values: ArrayValues,
}

impl ArrayData {
    /// How many elements the shape promises.
    pub fn element_count(&self) -> usize {
        self.metadata.dimensions.iter().product::<i64>().max(0) as usize
    }

    /// The block as bytes, borrowed. No copy.
    pub fn as_bytes(&self) -> &[u8] {
        self.values.as_bytes()
    }

    /// The values as `f64`, **borrowed**, when that is what the file holds.
    ///
    /// The path a 3D application takes for coordinates: no conversion, no
    /// allocation, just the numbers.
    pub fn f64(&self) -> Option<&[f64]> {
        match &self.values {
            ArrayValues::F64(v) => Some(v),
            _ => None,
        }
    }

    /// The values as `f32`, borrowed, when that is what the file holds.
    pub fn f32(&self) -> Option<&[f32]> {
        match &self.values {
            ArrayValues::F32(v) => Some(v),
            _ => None,
        }
    }

    /// The values as `i64`, borrowed, when that is what the file holds.
    pub fn i64(&self) -> Option<&[i64]> {
        match &self.values {
            ArrayValues::I64(v) => Some(v),
            _ => None,
        }
    }

    /// The values as `u32`, borrowed, when that is what the file holds —
    /// what RESQML uses for indices.
    pub fn u32(&self) -> Option<&[u32]> {
        match &self.values {
            ArrayValues::U32(v) => Some(v),
            _ => None,
        }
    }

    /// Every element widened to `f64`.
    ///
    /// **This copies**, and it is the only method here that does. Use
    /// [`ArrayData::f64`] when the file already holds doubles, which for
    /// coordinates it does.
    pub fn to_f64(&self) -> Vec<f64> {
        self.values.to_f64()
    }
}

/// A buffer the caller owns, for a backend to decode straight into.
///
/// The zero-allocation path: an application that already holds the memory the
/// numbers must end up in — a vertex buffer, a mapped file, a slice of a larger
/// block — passes it here and the file is read directly into it. Nothing is
/// allocated, nothing is copied afterwards.
#[non_exhaustive]
pub enum ArrayBuffer<'a> {
    /// Doubles.
    F64(&'a mut [f64]),
    /// Floats.
    F32(&'a mut [f32]),
    /// Signed 64-bit.
    I64(&'a mut [i64]),
    /// Signed 32-bit.
    I32(&'a mut [i32]),
    /// Unsigned 64-bit.
    U64(&'a mut [u64]),
    /// Unsigned 32-bit.
    U32(&'a mut [u32]),
    /// Bytes.
    U8(&'a mut [u8]),
}

impl ArrayBuffer<'_> {
    /// How many elements it can take.
    pub fn len(&self) -> usize {
        match self {
            Self::F64(b) => b.len(),
            Self::F32(b) => b.len(),
            Self::I64(b) => b.len(),
            Self::I32(b) => b.len(),
            Self::U64(b) => b.len(),
            Self::U32(b) => b.len(),
            Self::U8(b) => b.len(),
        }
    }

    /// Whether it can take none.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The element type it expects, named as [`ArrayMetadata`] names it.
    pub fn element_type(&self) -> &'static str {
        match self {
            Self::F64(_) => "double",
            Self::F32(_) => "float",
            Self::I64(_) => "int64",
            Self::I32(_) => "int32",
            Self::U64(_) => "uint64",
            Self::U32(_) => "uint32",
            Self::U8(_) => "uint8",
        }
    }
}

/// Whether a type is one of the two array pointers.
///
/// Useful to a UI that wants to show "this field holds data elsewhere" without
/// walking into it.
pub fn is_array_pointer(info: &energyml_core::reflect::TypeInfo) -> bool {
    info.kind == TypeKind::Struct && (info.name == NEW_FORM || info.name == OLD_FORM)
}

/// Where an array's numbers were actually found, and why there.
///
/// The distance between `declared` and `path` is the whole point: it is what a
/// person needs told when a package has been renamed, and what the repair
/// utility writes back so the next reader does not have to search at all.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Resolution {
    /// The file name the document wrote. Empty when it wrote none.
    pub declared: String,
    /// The file the numbers came out of.
    pub path: std::path::PathBuf,
    /// Which kind of place that was — see [`crate::backend::Origin`].
    pub origin: crate::backend::Origin,
}

impl Resolution {
    /// The bare file name of the answer, which is what a package should say.
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.display().to_string())
    }

    /// Whether the document already said the right thing.
    pub fn agrees(&self) -> bool {
        !self.origin.is_a_departure()
    }
}
