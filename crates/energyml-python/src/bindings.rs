//! The pyo3 dressing - **conversion, and nothing else**.
//!
//! Behind the `python` feature, and so absent from an ordinary workspace
//! build. This file is only reachable after a `maturin develop`: `cargo test`
//! never runs it, which is exactly why it decides nothing. Everything that can
//! be wrong lives in [`crate::session`].

use numpy::{IntoPyArray, PyArray1, PyArrayMethods};
use pyo3::exceptions::{PyIOError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict};

use crate::session::{
    ArrayRef, Block, CheckReport, Copied, Format, Geometry, Header, Link, Package, PropertyValues, Shape,
    Summary, Topology, TypeDescription, TypeSummary,
};

/// A refusal from the library becomes a Python exception, text and all.
fn failed(why: String) -> PyErr {
    PyIOError::new_err(why)
}

/// What is known about an object without opening it.
#[pyclass(name = "Summary", frozen, get_all, skip_from_py_object)]
#[derive(Clone)]
pub struct PySummary {
    /// The ETP URI, the key to everything else.
    pub uri: String,
    /// The uuid alone.
    pub uuid: String,
    /// `resqml22.TriangulatedSetRepresentation`.
    pub qualified_type: String,
    /// The citation's title, empty when there is none.
    pub title: String,
    /// The `objectVersion` the object declares, `None` when it declares none.
    pub object_version: Option<String>,
}

#[pymethods]
impl PySummary {
    fn __repr__(&self) -> String {
        match &self.object_version {
            Some(version) => format!(
                "Summary({} {:?} version {version:?})",
                self.qualified_type, self.title
            ),
            None => format!("Summary({} {:?})", self.qualified_type, self.title),
        }
    }
}

impl From<Summary> for PySummary {
    fn from(one: Summary) -> Self {
        PySummary {
            uri: one.uri,
            uuid: one.uuid,
            qualified_type: one.qualified_type,
            title: one.title,
            object_version: one.object_version,
        }
    }
}

/// Where an array is: a file, and a path inside it.
#[pyclass(name = "ArrayRef", frozen, get_all, skip_from_py_object)]
#[derive(Clone)]
pub struct PyArrayRef {
    /// The file, as the package names it.
    pub file: String,
    /// The dataset's path inside that file.
    pub path_in_file: String,
}

#[pymethods]
impl PyArrayRef {
    #[new]
    fn new(file: String, path_in_file: String) -> Self {
        PyArrayRef { file, path_in_file }
    }

    fn __repr__(&self) -> String {
        format!("ArrayRef({}:{})", self.file, self.path_in_file)
    }
}

impl From<&PyArrayRef> for ArrayRef {
    fn from(one: &PyArrayRef) -> Self {
        ArrayRef {
            file: one.file.clone(),
            path_in_file: one.path_in_file.clone(),
        }
    }
}

/// A block of numbers handed to Python, in the file's own type.
///
/// **Flat**, with the shape travelling beside it: RESQML writes its points
/// flat, and reshaping is the caller's decision. `reshape` is one line in
/// numpy; imposing it here would turn an array of 3N into a matrix nobody
/// asked for - and one day into a matrix of the wrong orientation.
fn block_into_py(py: Python<'_>, block: Block) -> Py<PyAny> {
    match block {
        Block::F64(v) => v.into_pyarray(py).into_any().unbind(),
        Block::F32(v) => v.into_pyarray(py).into_any().unbind(),
        Block::I64(v) => v.into_pyarray(py).into_any().unbind(),
        Block::I32(v) => v.into_pyarray(py).into_any().unbind(),
        Block::I16(v) => v.into_pyarray(py).into_any().unbind(),
        Block::I8(v) => v.into_pyarray(py).into_any().unbind(),
        Block::U64(v) => v.into_pyarray(py).into_any().unbind(),
        Block::U32(v) => v.into_pyarray(py).into_any().unbind(),
        Block::U16(v) => v.into_pyarray(py).into_any().unbind(),
        Block::U8(v) => v.into_pyarray(py).into_any().unbind(),
    }
}

/// The other way: a numpy array becomes a typed block.
///
/// **The array's dtype decides**, and a type the format does not carry is
/// refused with a word on what to do. Converting quietly - a `float16`
/// promoted to a double, a two-dimensional array flattened on its behalf -
/// would put something in the file other than what the caller holds, with no
/// way to notice before reading it back.
fn block_from_py(array: &Bound<'_, PyAny>) -> PyResult<Block> {
    macro_rules! take {
        ($kind:ty, $make:expr) => {
            if let Ok(found) = array.cast::<PyArray1<$kind>>() {
                return Ok($make(found.to_vec()?));
            }
        };
    }
    take!(f64, Block::F64);
    take!(f32, Block::F32);
    take!(i64, Block::I64);
    take!(i32, Block::I32);
    take!(i16, Block::I16);
    take!(i8, Block::I8);
    take!(u64, Block::U64);
    take!(u32, Block::U32);
    take!(u16, Block::U16);
    take!(u8, Block::U8);
    Err(PyValueError::new_err(
        "this build writes float32/64, int8/16/32/64 and uint8/16/32/64, one-dimensional; \
flatten with `.ravel()` and cast with `.astype(...)` before writing",
    ))
}

/// A patch's points and topology, as a dictionary.
///
/// **The indices cross flat**, like the points: a triangle list is 3M numbers
/// and `reshape(-1, 3)` is the caller's line. The variable-length cases have no
/// width to reshape to, and they carry their counts beside them instead.
fn patch_into_py<'py>(py: Python<'py>, patch: &crate::session::Patch) -> PyResult<Bound<'py, PyDict>> {
    let out = PyDict::new(py);
    out.set_item("index", patch.index)?;
    out.set_item("node_offset", patch.node_offset)?;
    out.set_item("points", patch.points.clone().into_pyarray(py))?;
    match &patch.topology {
        Topology::Points => {
            out.set_item("topology", "points")?;
        }
        Topology::Triangles(indices) => {
            out.set_item("topology", "triangles")?;
            out.set_item("triangles", indices.clone().into_pyarray(py))?;
        }
        Topology::Quads(indices) => {
            out.set_item("topology", "quads")?;
            out.set_item("quads", indices.clone().into_pyarray(py))?;
        }
        Topology::Polygons { counts, indices } => {
            out.set_item("topology", "polygons")?;
            out.set_item("counts", counts.clone().into_pyarray(py))?;
            out.set_item("indices", indices.clone().into_pyarray(py))?;
        }
        Topology::Polylines { counts, closed } => {
            out.set_item("topology", "polylines")?;
            out.set_item("counts", counts.clone().into_pyarray(py))?;
            // A list and not an array of flags: **empty means the file said
            // nothing**, which is not the same as all-open, and a numpy array
            // of zeros would say the second.
            out.set_item("closed", closed.clone())?;
        }
        Topology::Polyhedra {
            cell_face_counts,
            face_node_counts,
            face_nodes,
        } => {
            out.set_item("topology", "polyhedra")?;
            out.set_item("cell_face_counts", cell_face_counts.clone().into_pyarray(py))?;
            out.set_item("face_node_counts", face_node_counts.clone().into_pyarray(py))?;
            out.set_item("face_nodes", face_nodes.clone().into_pyarray(py))?;
        }
        Topology::Hexahedra {
            cells,
            dimensions,
            index,
        } => {
            out.set_item("topology", "hexahedra")?;
            out.set_item("cells", cells.clone().into_pyarray(py))?;
            out.set_item("dimensions", *dimensions)?;
            out.set_item("index", index.clone().into_pyarray(py))?;
        }
        Topology::Grid { fastest, slowest } => {
            out.set_item("topology", "grid")?;
            out.set_item("fastest", *fastest)?;
            out.set_item("slowest", *slowest)?;
        }
        Topology::Segments(indices) => {
            out.set_item("topology", "segments")?;
            out.set_item("segments", indices.clone().into_pyarray(py))?;
        }
    }
    Ok(out)
}

/// A representation's shape, as a dictionary.
fn geometry_into_py<'py>(py: Python<'py>, shape: Geometry) -> PyResult<Bound<'py, PyDict>> {
    let out = PyDict::new(py);
    out.set_item("uuid", shape.uuid)?;
    out.set_item("uri", shape.uri)?;
    out.set_item("title", shape.title)?;
    out.set_item("qualified_type", shape.qualified_type)?;
    out.set_item("kind", shape.kind)?;
    out.set_item("epsg", shape.epsg)?;
    out.set_item("placement", shape.placement)?;
    let patches = shape
        .patches
        .iter()
        .map(|one| patch_into_py(py, one))
        .collect::<PyResult<Vec<_>>>()?;
    out.set_item("patches", patches)?;
    Ok(out)
}

/// One property's values, as a dictionary.
fn property_into_py<'py>(py: Python<'py>, one: PropertyValues) -> PyResult<Bound<'py, PyDict>> {
    let out = PyDict::new(py);
    out.set_item("uuid", one.uuid)?;
    out.set_item("name", one.name)?;
    out.set_item("kind", one.kind)?;
    out.set_item("uom", one.uom)?;
    out.set_item("scope", one.scope)?;
    out.set_item("components", one.components)?;
    out.set_item("values", one.values.into_pyarray(py))?;
    // The colour ramp the **package** states, RGBA rows in 0-1, low value
    // first - and **empty when it states none**, which is the answer: the
    // standard defines no default ramp, so inventing one would hand back a
    // picture the file does not describe. Flat, with the width beside it, like
    // every other array crossing here.
    out.set_item("colour_components", 4u64)?;
    out.set_item(
        "colours",
        one.colours
            .into_iter()
            .flat_map(|row| row.into_iter())
            .collect::<Vec<f64>>()
            .into_pyarray(py),
    )?;
    Ok(out)
}

/// One end of a reference, as a dictionary.
fn link_into_py<'py>(py: Python<'py>, one: Link) -> PyResult<Bound<'py, PyDict>> {
    let out = PyDict::new(py);
    out.set_item("uri", one.uri)?;
    out.set_item("uuid", one.uuid)?;
    out.set_item("qualified_type", one.qualified_type)?;
    out.set_item("title", one.title)?;
    out.set_item("object_version", one.object_version)?;
    out.set_item("path", one.path)?;
    out.set_item("present", one.present)?;
    Ok(out)
}

/// One type of the generated model, as a dictionary.
fn type_into_py<'py>(py: Python<'py>, one: &TypeSummary) -> PyResult<Bound<'py, PyDict>> {
    let out = PyDict::new(py);
    out.set_item("qualified_type", one.qualified_type.clone())?;
    out.set_item("type_key", one.type_key.clone())?;
    out.set_item("package", one.package.clone())?;
    out.set_item("version", one.version.clone())?;
    out.set_item("is_abstract", one.is_abstract)?;
    out.set_item("is_top_level", one.is_top_level)?;
    out.set_item("doc", one.doc.clone())?;
    Ok(out)
}

/// A citation, as a dictionary.
fn header_into_py<'py>(py: Python<'py>, one: Header) -> PyResult<Bound<'py, PyDict>> {
    let out = PyDict::new(py);
    out.set_item("uri", one.summary.uri)?;
    out.set_item("uuid", one.summary.uuid)?;
    out.set_item("qualified_type", one.summary.qualified_type)?;
    out.set_item("title", one.summary.title)?;
    out.set_item("originator", one.originator)?;
    out.set_item("creation", one.creation)?;
    out.set_item("last_update", one.last_update)?;
    out.set_item("format", one.format)?;
    out.set_item("object_version", one.object_version)?;
    out.set_item("schema_version", one.schema_version)?;
    Ok(out)
}

/// An array's shape, as a dictionary.
fn shape_into_py<'py>(py: Python<'py>, shape: Shape) -> PyResult<Bound<'py, PyDict>> {
    let out = PyDict::new(py);
    out.set_item("dimensions", shape.dimensions)?;
    out.set_item("element_type", shape.element_type)?;
    Ok(out)
}

/// An open EPC package, with its data files.
#[pyclass(name = "Epc", unsendable)]
pub struct PyEpc {
    inner: Package,
}

#[pymethods]
impl PyEpc {
    /// Open an EPC. Its `.h5` files are looked for beside it.
    #[staticmethod]
    fn open(path: &str) -> PyResult<Self> {
        Package::open(path).map(|inner| PyEpc { inner }).map_err(failed)
    }

    /// An empty package: nothing in it but the two parts the format requires.
    ///
    /// Where it goes is decided at `save_as`. Its data files land where
    /// `set_data_root` says.
    #[staticmethod]
    fn create(name: &str) -> PyResult<Self> {
        Package::create(name).map(|inner| PyEpc { inner }).map_err(failed)
    }

    /// Where the data files are read from and written to.
    fn set_data_root(&mut self, directory: &str) {
        self.inner.set_data_root(directory);
    }

    /// Pin the name the package gives a data file to a path.
    ///
    /// For a caller told where the files are but not what the package calls
    /// them: arrays whose file is `name` are read from and written to `path`,
    /// whatever the package says. Pinning a name again replaces the path.
    fn pin_data_file(&mut self, name: &str, path: &str) {
        self.inner.pin_data_file(name, path);
    }

    /// Name the external file an `EpcExternalPartReference` stands for.
    ///
    /// Only needed by the older arrangement, where an array points at a proxy
    /// object rather than naming its own file.
    fn register_external_file(&mut self, proxy_uuid: &str, file: &str) {
        self.inner.register_external_file(proxy_uuid, file);
    }

    /// What renaming the external data file would touch, without touching it.
    ///
    /// Returns `{from, to, objects, fields, relationships, refusals, allowed}`.
    /// `refusals` is a list of reasons in words; `allowed` is whether it is
    /// empty, so a caller need not decide that for itself.
    fn plan_rename<'py>(&self, py: Python<'py>, from: &str, to: &str) -> PyResult<Bound<'py, PyDict>> {
        let quote = self.inner.plan_rename(from, to);
        let out = PyDict::new(py);
        out.set_item("from", quote.from)?;
        out.set_item("to", quote.to)?;
        out.set_item("objects", quote.objects)?;
        out.set_item("fields", quote.fields)?;
        out.set_item("relationships", quote.relationships)?;
        out.set_item("allowed", quote.refusals.is_empty())?;
        out.set_item("refusals", quote.refusals)?;
        Ok(out)
    }

    /// Rename the external data file: the fields, the relationships, the file.
    ///
    /// Returns `{fields, relationships, file, moved_to}`, where `file` is
    /// `moved`, `absent` or `not-on-disk`. Refuses - raising - rather than
    /// doing part of it: a package naming a file that is not there opens
    /// perfectly and cannot be drawn.
    fn rename_data_file<'py>(
        &mut self,
        py: Python<'py>,
        from: &str,
        to: &str,
    ) -> PyResult<Bound<'py, PyDict>> {
        let done = self.inner.rename_data_file(from, to).map_err(failed)?;
        let out = PyDict::new(py);
        out.set_item("fields", done.fields)?;
        out.set_item("relationships", done.relationships)?;
        out.set_item("file", done.file)?;
        out.set_item("moved_to", done.moved_to)?;
        Ok(out)
    }

    /// Where it came from.
    #[getter]
    fn path(&self) -> Option<String> {
        self.inner.path().map(|one| one.display().to_string())
    }

    /// Everything the package holds.
    fn objects(&self) -> Vec<PySummary> {
        self.inner.objects().into_iter().map(PySummary::from).collect()
    }

    /// The uri of a uuid, or `None`. Raises when the uuid is held under more
    /// than one version: the error names the candidates.
    fn uri_of(&self, uuid: &str) -> PyResult<Option<String>> {
        self.inner.uri_of(uuid).map_err(failed)
    }

    /// An object, as a dictionary.
    ///
    /// The JSON crosses as text and is then parsed by Python's own `json`:
    /// rebuilding it key by key through pyo3 would be a second implementation
    /// of the conversion, for a gain nobody has measured.
    fn get<'py>(&self, py: Python<'py>, uri: &str) -> PyResult<Bound<'py, PyAny>> {
        let text = self.inner.get(uri).map_err(failed)?;
        py.import("json")?.call_method1("loads", (text,))
    }

    /// The same object as JSON text - for a caller that wants it verbatim.
    fn get_json(&self, uri: &str) -> PyResult<String> {
        self.inner.get(uri).map_err(failed)
    }

    /// Put an object, from a dictionary or from JSON text.
    fn put(&mut self, py: Python<'_>, object: &Bound<'_, PyAny>) -> PyResult<String> {
        let text = match object.extract::<String>() {
            Ok(text) => text,
            Err(_) => py
                .import("json")?
                .call_method1("dumps", (object,))?
                .extract::<String>()?,
        };
        self.inner.put(&text).map_err(failed)
    }

    /// Remove an object.
    fn remove(&mut self, uri: &str) -> PyResult<()> {
        self.inner.remove(uri).map_err(failed)
    }

    /// The arrays an object names.
    fn arrays(&self, uri: &str) -> PyResult<Vec<PyArrayRef>> {
        self.inner
            .arrays(uri)
            .map(|found| {
                found
                    .into_iter()
                    .map(|one| PyArrayRef {
                        file: one.file,
                        path_in_file: one.path_in_file,
                    })
                    .collect()
            })
            .map_err(failed)
    }

    /// What an array weighs, without reading it.
    fn array_shape<'py>(&self, py: Python<'py>, array: &PyArrayRef) -> PyResult<Bound<'py, PyDict>> {
        let shape = self.inner.array_shape(&ArrayRef::from(array)).map_err(failed)?;
        shape_into_py(py, shape)
    }

    /// Read an array: the flat numpy array, and its dimensions.
    fn read_array(&self, py: Python<'_>, array: &PyArrayRef) -> PyResult<(Py<PyAny>, Vec<i64>)> {
        let (shape, block) = self.inner.read_array(&ArrayRef::from(array)).map_err(failed)?;
        Ok((block_into_py(py, block), shape.dimensions))
    }

    /// Write an array. With `dimensions` omitted: a vector of its length.
    #[pyo3(signature = (array, values, dimensions = None))]
    fn write_array(
        &self,
        array: &PyArrayRef,
        values: &Bound<'_, PyAny>,
        dimensions: Option<Vec<i64>>,
    ) -> PyResult<()> {
        let block = block_from_py(values)?;
        let dimensions = dimensions.unwrap_or_else(|| vec![block.len() as i64]);
        self.inner
            .write_array(&ArrayRef::from(array), &dimensions, block)
            .map_err(failed)
    }

    /// The objects whose type or title contains what is asked.
    ///
    /// Both filters ignore case, both are optional, and giving both means
    /// *and*. Filtering happens before anything is parsed.
    #[pyo3(signature = (of_type = None, titled = None))]
    fn find(&self, of_type: Option<&str>, titled: Option<&str>) -> Vec<PySummary> {
        self.inner
            .find(of_type, titled)
            .into_iter()
            .map(PySummary::from)
            .collect()
    }

    /// Read a slice of an array rather than all of it.
    ///
    /// `start` and `count` are per dimension - what ETP calls a subarray.
    fn read_array_part(
        &self,
        py: Python<'_>,
        array: &PyArrayRef,
        start: Vec<i64>,
        count: Vec<i64>,
    ) -> PyResult<(Py<PyAny>, Vec<i64>)> {
        let (shape, block) = self
            .inner
            .read_array_part(&ArrayRef::from(array), &start, &count)
            .map_err(failed)?;
        Ok((block_into_py(py, block), shape.dimensions))
    }

    /// What this object points at: one read, and each entry says where the
    /// reference sits and whether the package holds the target.
    fn references<'py>(&self, py: Python<'py>, uri: &str) -> PyResult<Vec<Bound<'py, PyDict>>> {
        self.inner
            .references(uri)
            .map_err(failed)?
            .into_iter()
            .map(|one| link_into_py(py, one))
            .collect()
    }

    /// Who points at this object - the question to ask before deleting it.
    ///
    /// A package has no index for this direction, so every object is read.
    fn referenced_by<'py>(&self, py: Python<'py>, uri: &str) -> PyResult<Vec<Bound<'py, PyDict>>> {
        self.inner
            .referenced_by(uri)
            .map_err(failed)?
            .into_iter()
            .map(|one| link_into_py(py, one))
            .collect()
    }

    /// These objects and everything they need, transitively.
    ///
    /// Returns `(needed, missing)`: a set that can be cut out on its own, and
    /// what is referenced but not held - minus what the standard publishes in a
    /// catalogue, whose absence means nothing.
    fn needs(&self, uris: Vec<String>) -> PyResult<(Vec<String>, Vec<String>)> {
        self.inner.needs(&uris).map_err(failed)
    }

    /// Copy objects, **and everything they need**, out of another package.
    ///
    /// Returns a dictionary: what was copied, what the closure added beyond
    /// what was asked, what is referenced and missing from the source, and how
    /// many datasets travelled.
    #[pyo3(signature = (source, uris, arrays = "copy"))]
    fn copy_from<'py>(
        &mut self,
        py: Python<'py>,
        source: PyRef<'_, PyEpc>,
        uris: Vec<String>,
        arrays: &str,
    ) -> PyResult<Bound<'py, PyDict>> {
        let done: Copied = self
            .inner
            .copy_from(&source.inner, &uris, arrays)
            .map_err(failed)?;
        let out = PyDict::new(py);
        out.set_item("objects", done.objects)?;
        out.set_item("added", done.added)?;
        out.set_item("missing", done.missing)?;
        out.set_item("arrays", done.arrays)?;
        out.set_item("arrays_missed", done.arrays_missed)?;
        out.set_item("unreadable", done.unreadable)?;
        Ok(out)
    }

    /// Judge this package: everything that can be said against it.
    ///
    /// The sections `energyml check` prints, and the same arithmetic - the
    /// report comes from the library, counts included. `schema` is the slowest
    /// section; `rules` are the business constraints the technical guides
    /// state. **Not checked is not the same as clean**, so the report says
    /// which sections ran.
    #[pyo3(signature = (schema = true, rules = true))]
    fn check<'py>(&self, py: Python<'py>, schema: bool, rules: bool) -> PyResult<Bound<'py, PyDict>> {
        let report: CheckReport = self.inner.check(schema, rules);
        let out = PyDict::new(py);
        out.set_item("errors", report.errors)?;
        out.set_item("warnings", report.warnings)?;
        out.set_item("sound", report.errors == 0)?;

        let rows = |items: Vec<Bound<'py, PyDict>>| -> PyResult<Vec<Bound<'py, PyDict>>> { Ok(items) };

        let mut diagnostics = Vec::new();
        for (severity, part, message) in report.diagnostics {
            let row = PyDict::new(py);
            row.set_item("severity", severity)?;
            row.set_item("part", part)?;
            row.set_item("message", message)?;
            diagnostics.push(row);
        }
        out.set_item("diagnostics", rows(diagnostics)?)?;

        let mut unreadable = Vec::new();
        for (uri, why) in report.unreadable {
            let row = PyDict::new(py);
            row.set_item("uri", uri)?;
            row.set_item("why", why)?;
            unreadable.push(row);
        }
        out.set_item("unreadable", rows(unreadable)?)?;

        let mut schema_rows = Vec::new();
        for one in report.schema {
            let row = PyDict::new(py);
            row.set_item("uuid", one.uuid)?;
            row.set_item("severity", format!("{:?}", one.severity).to_lowercase())?;
            row.set_item("path", one.path)?;
            row.set_item("code", one.code)?;
            row.set_item("message", one.message)?;
            schema_rows.push(row);
        }
        let schema_section = PyDict::new(py);
        schema_section.set_item("checked", report.schema_checked)?;
        schema_section.set_item("issues", rows(schema_rows)?)?;
        out.set_item("schema", schema_section)?;

        let mut property_rows = Vec::new();
        for one in report.properties {
            let row = PyDict::new(py);
            row.set_item("uuid", one.uuid)?;
            row.set_item("says", one.says)?;
            row.set_item("is_a_defect", one.is_a_defect)?;
            row.set_item("is_error", one.is_error)?;
            property_rows.push(row);
        }
        let properties = PyDict::new(py);
        properties.set_item("issues", rows(property_rows)?)?;
        properties.set_item("unresolved", report.properties_unresolved)?;
        out.set_item("properties", properties)?;

        let mut count_rows = Vec::new();
        for one in report.counts {
            let row = PyDict::new(py);
            row.set_item("uuid", one.uuid)?;
            row.set_item("says", one.says)?;
            row.set_item("is_error", one.is_error)?;
            count_rows.push(row);
        }
        let counts = PyDict::new(py);
        counts.set_item("issues", rows(count_rows)?)?;
        counts.set_item("unchecked", report.counts_unchecked)?;
        out.set_item("counts", counts)?;

        let mut rule_rows = Vec::new();
        for one in report.rules {
            let row = PyDict::new(py);
            row.set_item("uuid", one.uuid)?;
            row.set_item("rule", one.rule)?;
            row.set_item("error", one.gravity == energyml_domain::rules::Gravity::Error)?;
            row.set_item("says", one.says)?;
            row.set_item("read", one.read)?;
            row.set_item("why", one.why)?;
            rule_rows.push(row);
        }
        let rules_section = PyDict::new(py);
        rules_section.set_item("checked", report.rules_checked)?;
        rules_section.set_item("count", report.rule_count)?;
        rules_section.set_item("findings", rows(rule_rows)?)?;
        out.set_item("rules", rules_section)?;

        let mut reference_rows = Vec::new();
        for (source, path, target, says, is_error, catalogue) in report.references {
            let row = PyDict::new(py);
            row.set_item("source", source)?;
            row.set_item("path", path)?;
            row.set_item("target_uuid", target)?;
            row.set_item("says", says)?;
            row.set_item("is_error", is_error)?;
            // Absent from the package **by design**: the standard publishes it
            // in a catalogue, so not holding it says nothing.
            row.set_item("catalogue", catalogue)?;
            reference_rows.push(row);
        }
        out.set_item("references", rows(reference_rows)?)?;

        let mut relationship_rows = Vec::new();
        for (part, target, says) in report.relationships {
            let row = PyDict::new(py);
            row.set_item("part", part)?;
            row.set_item("target", target)?;
            row.set_item("says", says)?;
            relationship_rows.push(row);
        }
        out.set_item("relationships", rows(relationship_rows)?)?;

        Ok(out)
    }

    /// What kind of store this is: `EPC`.
    #[getter]
    fn store_kind(&self) -> &'static str {
        self.inner.store_kind()
    }

    /// The dataspaces this store holds. An EPC is exactly one.
    fn dataspaces<'py>(&self, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyDict>>> {
        self.inner
            .dataspaces()
            .into_iter()
            .map(|one| {
                let out = PyDict::new(py);
                out.set_item("uri", one.uri)?;
                out.set_item("name", one.name)?;
                out.set_item("created", one.created)?;
                out.set_item("last_changed", one.last_changed)?;
                Ok(out)
            })
            .collect()
    }

    /// The citations of these objects, without reading the objects.
    ///
    /// Plural because the storage abstraction is plural. A uri that cannot be
    /// read is missing from the answer rather than failing it.
    fn headers<'py>(&self, py: Python<'py>, uris: Vec<String>) -> PyResult<Vec<Bound<'py, PyDict>>> {
        self.inner
            .headers(&uris)
            .into_iter()
            .map(|one| header_into_py(py, one))
            .collect()
    }

    /// The types this store can read - the generated model behind it.
    fn supported_types<'py>(&self, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyDict>>> {
        self.inner
            .supported_types()
            .iter()
            .map(|one| type_into_py(py, one))
            .collect()
    }

    /// The parts of the package that are not objects.
    fn attachments(&self) -> Vec<String> {
        self.inner.attachments()
    }

    /// One attachment's bytes.
    fn read_attachment(&self, py: Python<'_>, part: &str) -> PyResult<Py<PyAny>> {
        let bytes = self.inner.read_attachment(part).map_err(failed)?;
        Ok(PyBytes::new(py, &bytes).into_any().unbind())
    }

    /// The relationships recorded for a part - **an index, not the truth**.
    fn relationships<'py>(&self, py: Python<'py>, part: &str) -> PyResult<Vec<Bound<'py, PyDict>>> {
        self.inner
            .relationships(part)
            .map_err(failed)?
            .into_iter()
            .map(|one| {
                let out = PyDict::new(py);
                out.set_item("id", one.id)?;
                out.set_item("kind", one.kind)?;
                out.set_item("target", one.target)?;
                out.set_item("external", one.external)?;
                Ok(out)
            })
            .collect()
    }

    /// Is there anything to save?
    #[getter]
    fn is_dirty(&self) -> bool {
        self.inner.is_dirty()
    }

    /// Write the package elsewhere, and return what the write had to say.
    fn save_as(&self, path: &str) -> PyResult<Vec<String>> {
        self.inner.save_as(path).map_err(failed)
    }

    fn __repr__(&self) -> String {
        match self.inner.path() {
            Some(path) => format!("Epc({})", path.display()),
            None => "Epc(<memory>)".to_string(),
        }
    }
}

/// The interchange formats this build can write.
///
/// From the exporter registry itself: adding a format in Rust makes it appear
/// here with nothing changed on this side.
#[pyfunction]
fn formats<'py>(py: Python<'py>) -> PyResult<Vec<Bound<'py, PyDict>>> {
    crate::session::formats()
        .into_iter()
        .map(|one: Format| {
            let out = PyDict::new(py);
            out.set_item("name", one.name)?;
            out.set_item("extension", one.extension)?;
            out.set_item("mime", one.mime)?;
            out.set_item("describes", one.describes)?;
            Ok(out)
        })
        .collect()
}

/// Write representations out in an interchange format, and hand back the bytes.
///
/// One document for the lot, as the format decides: OBJ concatenates, GeoJSON
/// encloses. A format that cannot write one of these objects **refuses** rather
/// than skipping it in silence.
///
/// `with_properties` carries the values too, for the formats that write them.
/// Off by default, and that is a cost rather than a preference: nothing in a
/// representation lists its properties, so finding them reads every property of
/// the package.
///
/// `metadata` is a spec as JSON - `{"key": "dotted.path"}` - collected on every
/// object and written verbatim by every format, so one spec yields the same
/// keys in an OBJ comment, a GeoJSON property and a VTK field array.
/// `standard_metadata()` gives the one this workspace ships.
#[pyfunction]
#[pyo3(signature = (package, uris, format, placement = None, with_properties = false, metadata = None))]
fn export_of(
    py: Python<'_>,
    package: PyRef<'_, PyEpc>,
    uris: Vec<String>,
    format: &str,
    placement: Option<&str>,
    with_properties: bool,
    metadata: Option<&str>,
) -> PyResult<Py<PyAny>> {
    let bytes = package
        .inner
        .export(&uris, format, placement, with_properties, metadata)
        .map_err(failed)?;
    Ok(PyBytes::new(py, &bytes).into_any().unbind())
}

/// The metadata spec this workspace ships, as JSON.
///
/// A document rather than a keyword: edit it, pass it to `export_of`. What
/// `energyml export --standard-metadata` writes.
#[pyfunction]
fn standard_metadata() -> String {
    crate::session::standard_metadata()
}

/// Every type of the generated model this build carries.
///
/// The Rust model read through reflection, never a second copy of it: a release
/// added to the workspace appears here the day it is registered.
#[pyfunction]
#[pyo3(signature = (package = None, top_level_only = true))]
fn types<'py>(
    py: Python<'py>,
    package: Option<&str>,
    top_level_only: bool,
) -> PyResult<Vec<Bound<'py, PyDict>>> {
    crate::session::model_types(package, top_level_only)
        .iter()
        .map(|one| type_into_py(py, one))
        .collect()
}

/// What one type is: its fields, its values, what extends it.
///
/// Takes a qualified type or a plain type name; a name that matches nothing is
/// refused with both spellings tried.
#[pyfunction]
fn describe<'py>(py: Python<'py>, name: &str) -> PyResult<Bound<'py, PyDict>> {
    let described: TypeDescription = crate::session::describe_type(name).map_err(failed)?;
    let out = type_into_py(py, &described.summary)?;
    out.set_item("kind", described.kind)?;
    let mut fields = Vec::new();
    for field in described.fields {
        let row = PyDict::new(py);
        row.set_item("name", field.name)?;
        row.set_item("doc", field.doc)?;
        row.set_item("type", field.type_name)?;
        row.set_item("allowed", field.allowed)?;
        row.set_item("open", field.is_open)?;
        row.set_item("min_occurs", field.min_occurs)?;
        row.set_item("max_occurs", field.max_occurs)?;
        row.set_item("is_list", field.is_list)?;
        // What a caller actually asks: must I fill this in?
        row.set_item("required", field.min_occurs > 0)?;
        fields.push(row);
    }
    out.set_item("fields", fields)?;
    out.set_item("variants", described.variants)?;
    out.set_item("subtypes", described.subtypes)?;
    out.set_item("base", described.base)?;
    Ok(out)
}

/// An empty object of this type, as a dictionary ready to be filled.
///
/// **This is how an object is created here.** The registry builds the instance
/// and the JSON writer renders it, so what comes back carries its `$type` and
/// its uuid - fill it in, hand it to `put`, and no class per type ever had to
/// exist on this side.
///
/// An abstract type is refused by name, with what extends it.
///
/// `uuid` is optional and has **no default**: an identity is not always a coin
/// toss - `energyml-build` derives its uuids so a second run names the same
/// objects - so a template that stamped a random one would hand such a caller
/// a fresh identity every run. Left out, the field comes back empty and `put`
/// refuses by naming it; passing it here moves the refusal to the call that
/// made the mistake.
#[pyfunction]
#[pyo3(signature = (qualified_type, uuid = None))]
fn template<'py>(py: Python<'py>, qualified_type: &str, uuid: Option<&str>) -> PyResult<Bound<'py, PyAny>> {
    let text = crate::session::template(qualified_type, uuid).map_err(failed)?;
    py.import("json")?.call_method1("loads", (text,))
}

/// The shape of a representation: points and topology, placed.
///
/// `placement` is `projected` (the default), `local` or `metres`. Anything else
/// is refused with the list - a model placed in the wrong system looks exactly
/// like one placed in the right one.
#[pyfunction]
#[pyo3(signature = (package, uri, placement = "projected"))]
fn geometry_of<'py>(
    py: Python<'py>,
    package: PyRef<'_, PyEpc>,
    uri: &str,
    placement: &str,
) -> PyResult<Bound<'py, PyDict>> {
    let shape = package.inner.geometry(uri, placement).map_err(failed)?;
    geometry_into_py(py, shape)
}

/// The same shape as one triangle mesh: flat points, flat triples.
///
/// It **says** that it triangulates: quadrilaterals and cells are cut, which is
/// a decision the caller can see being made rather than discover in the numbers.
#[pyfunction]
#[pyo3(signature = (package, uri, placement = "projected"))]
fn mesh_of(
    py: Python<'_>,
    package: PyRef<'_, PyEpc>,
    uri: &str,
    placement: &str,
) -> PyResult<(Py<PyAny>, Py<PyAny>)> {
    let (points, triangles) = package.inner.mesh(uri, placement).map_err(failed)?;
    Ok((
        points.into_pyarray(py).into_any().unbind(),
        triangles.into_pyarray(py).into_any().unbind(),
    ))
}

/// The properties hanging on a representation, and what did not fit.
///
/// Returns `(values, problems)`. The second list is not decoration: a property
/// dropped in silence is a colour a caller never learns it is missing.
#[pyfunction]
fn properties_of<'py>(
    py: Python<'py>,
    package: PyRef<'_, PyEpc>,
    uri: &str,
) -> PyResult<(Vec<Bound<'py, PyDict>>, Vec<String>)> {
    let (values, problems) = package.inner.properties(uri).map_err(failed)?;
    let values = values
        .into_iter()
        .map(|one| property_into_py(py, one))
        .collect::<PyResult<Vec<_>>>()?;
    Ok((values, problems))
}

/// The compiled core of `energyml.rust`.
///
/// Everything is exported flat from here, and `python/energyml/rust/*.py`
/// arranges it into named modules. The arrangement lives in Python because
/// that is where it can be *read*: registering submodules from Rust would mean
/// writing to `sys.modules` by hand, in a file `cargo test` never executes.
#[pymodule]
fn _core(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<PyEpc>()?;
    module.add_class::<PySummary>()?;
    module.add_class::<PyArrayRef>()?;
    module.add_function(wrap_pyfunction!(geometry_of, module)?)?;
    module.add_function(wrap_pyfunction!(mesh_of, module)?)?;
    module.add_function(wrap_pyfunction!(properties_of, module)?)?;
    module.add_function(wrap_pyfunction!(export_of, module)?)?;
    module.add_function(wrap_pyfunction!(standard_metadata, module)?)?;
    module.add_function(wrap_pyfunction!(formats, module)?)?;
    module.add_function(wrap_pyfunction!(types, module)?)?;
    module.add_function(wrap_pyfunction!(describe, module)?)?;
    module.add_function(wrap_pyfunction!(template, module)?)?;
    Ok(())
}
