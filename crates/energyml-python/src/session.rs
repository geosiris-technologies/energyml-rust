//! What an EPC package can do, in plain Rust.
//!
//! # Why this file is separate from the binding
//!
//! It is the WebAssembly front end's rule, and it holds here for the same reason:
//! **everything that can be wrong lives in Rust the test suite exercises; the
//! binding only converts.** A pyo3 binding can only be tested with a Python
//! interpreter able to load a `cdylib` — which happens after `maturin
//! develop`, and so never under `cargo test`. Logic written up there would be
//! code nobody can exercise.
//!
//! This file therefore mentions **neither pyo3 nor numpy**. It returns
//! standard-library types and `String` errors; `bindings.rs` dresses them.
//!
//! # Two layers, and the second one is thin
//!
//! The catalogue — objects as JSON, arrays as numbers — is what a package
//! *is*. Above it sits what the libraries make of a package: the geometry of a
//! representation and the values hanging on it, which `energyml-geometry` and
//! `energyml-domain` already compute. Nothing of that is reimplemented here;
//! this file only hands it across, because a caller who receives a raw array
//! and has to recompose the points is a caller doing again what the workspace
//! does already.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use energyml_geometry::geometry as geom;
use energyml_geometry::scene::{self, AttributeScope, Placement};
use energyml_storage::array::{ArrayData, ArrayLocation, ArrayMetadata, ArrayValues};
use energyml_storage::{EpcStore, SaveOptions};

/// What a caller learns about an object without reading it.
///
/// The four fields every view in this repository shows — the front end's tree, the
/// `list` command, the wasm module's inventory. The same ones here, for the
/// same reason: it is what a catalogue has to say for a caller to choose what
/// to open.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Summary {
    /// The ETP URI, which is the key to everything else.
    pub uri: String,
    /// The uuid alone, because a user reads it in some other tool.
    pub uuid: String,
    /// The qualified type: `resqml22.TriangulatedSetRepresentation`.
    pub qualified_type: String,
    /// The citation's title, empty when there is none.
    pub title: String,
    /// The `objectVersion` the object declares — `None` when it declares none
    /// or a blank one. With the title, what tells two versions of one uuid apart.
    pub object_version: Option<String>,
}

/// Where an array is.
///
/// **The file name and the path inside it travel together**, because a package
/// may name several: one `.h5` per proxy is allowed, and two objects of the
/// same EPC can read from two different files. Splitting the pair would leave
/// a caller to recombine two strings, and one day it would recombine them
/// wrongly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArrayRef {
    /// The file, as the package names it.
    pub file: String,
    /// The dataset's path inside that file.
    pub path_in_file: String,
}

impl ArrayRef {
    fn location(&self) -> ArrayLocation {
        ArrayLocation {
            file: self.file.clone(),
            path_in_file: self.path_in_file.clone(),
            ..Default::default()
        }
    }
}

/// What an array weighs, before reading it.
///
/// Available without decompressing anything, which is what lets a caller
/// decide whether it really wants the eighty million numbers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shape {
    /// How many elements per dimension.
    pub dimensions: Vec<i64>,
    /// The element type, as the backend names it — `double`, `int64`.
    pub element_type: String,
}

/// An array's values, **in the file's own type**.
///
/// Typed and not converted, which is `energyml-storage`'s rule and the reason
/// it exists: the point of reading a million points is to hand them to
/// something that draws them, and every conversion on the way is one more copy
/// of the whole array. Doubles come back as doubles, integers as integers, and
/// wanting anything else is the caller's decision.
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    F64(Vec<f64>),
    F32(Vec<f32>),
    I64(Vec<i64>),
    I32(Vec<i32>),
    I16(Vec<i16>),
    I8(Vec<i8>),
    U64(Vec<u64>),
    U32(Vec<u32>),
    U16(Vec<u16>),
    U8(Vec<u8>),
}

impl Block {
    /// How many numbers — regardless of the shape, which is said elsewhere.
    pub fn len(&self) -> usize {
        match self {
            Block::F64(v) => v.len(),
            Block::F32(v) => v.len(),
            Block::I64(v) => v.len(),
            Block::I32(v) => v.len(),
            Block::I16(v) => v.len(),
            Block::I8(v) => v.len(),
            Block::U64(v) => v.len(),
            Block::U32(v) => v.len(),
            Block::U16(v) => v.len(),
            Block::U8(v) => v.len(),
        }
    }

    /// An empty array is an answer, not an absence.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn from(values: ArrayValues) -> Self {
        match values {
            ArrayValues::F64(v) => Block::F64(v),
            ArrayValues::F32(v) => Block::F32(v),
            ArrayValues::I64(v) => Block::I64(v),
            ArrayValues::I32(v) => Block::I32(v),
            ArrayValues::I16(v) => Block::I16(v),
            ArrayValues::I8(v) => Block::I8(v),
            ArrayValues::U64(v) => Block::U64(v),
            ArrayValues::U32(v) => Block::U32(v),
            ArrayValues::U16(v) => Block::U16(v),
            ArrayValues::U8(v) => Block::U8(v),
            // `ArrayValues` is `non_exhaustive`: a variant added there must
            // not break the build here, but it must not pass for an empty
            // array either. It gets named.
            other => unreachable!("an array type this bridge does not know: {other:?}"),
        }
    }

    fn into_values(self) -> ArrayValues {
        match self {
            Block::F64(v) => ArrayValues::F64(v),
            Block::F32(v) => ArrayValues::F32(v),
            Block::I64(v) => ArrayValues::I64(v),
            Block::I32(v) => ArrayValues::I32(v),
            Block::I16(v) => ArrayValues::I16(v),
            Block::I8(v) => ArrayValues::I8(v),
            Block::U64(v) => ArrayValues::U64(v),
            Block::U32(v) => ArrayValues::U32(v),
            Block::U16(v) => ArrayValues::U16(v),
            Block::U8(v) => ArrayValues::U8(v),
        }
    }

    /// The type's name, so a refusal can say which one.
    pub fn element_type(&self) -> &'static str {
        match self {
            Block::F64(_) => "double",
            Block::F32(_) => "float",
            Block::I64(_) => "int64",
            Block::I32(_) => "int32",
            Block::I16(_) => "int16",
            Block::I8(_) => "int8",
            Block::U64(_) => "uint64",
            Block::U32(_) => "uint32",
            Block::U16(_) => "uint16",
            Block::U8(_) => "uint8",
        }
    }
}

/// How a patch's points connect.
///
/// **Flat runs, because that is what numpy takes.** A triangle list crosses as
/// 3M indices and not as M triples: the shape is the caller's `reshape`, and
/// the variable-length cases (polygons, polylines, cells) have no fixed width
/// to reshape to anyway — they carry their counts beside them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Topology {
    /// Points that mean nothing together.
    Points,
    /// 3M indices into the patch's points.
    Triangles(Vec<u32>),
    /// 4M indices, in ring order.
    Quads(Vec<u32>),
    /// Rings of any length: how many nodes each has, then the indices.
    Polygons { counts: Vec<u32>, indices: Vec<u32> },
    /// Runs of consecutive points: how many nodes each polyline has, and which
    /// of them close back on their first node.
    ///
    /// `closed` is **empty when the file says nothing**, which is not the same
    /// as all-open: a producer that omits `ClosedPolylines` has stated nothing.
    Polylines { counts: Vec<u32>, closed: Vec<bool> },
    /// Cells bounded by faces: faces per cell, nodes per face, then the nodes.
    ///
    /// Every face is already wound so its right-hand normal points **out of
    /// its cell**, which is the one thing a caller cannot recover on its own.
    Polyhedra {
        cell_face_counts: Vec<u32>,
        face_node_counts: Vec<u32>,
        face_nodes: Vec<u32>,
    },
    /// Hexahedra: 8M indices in VTK's corner order, and the `Ni, Nj, Nk` they
    /// were built from.
    ///
    /// `index` holds **the RESQML cell number of each entry**, and it is empty
    /// when they coincide. It is not decoration: a reader drops a cell the file
    /// marks undefined, so on a faulted or eroded grid an entry sits before the
    /// cell it belongs to — and anything that addresses a cell by the number
    /// the file uses (a connection set, for one) would be quietly wrong.
    Hexahedra {
        cells: Vec<u32>,
        dimensions: [usize; 3],
        index: Vec<u32>,
    },
    /// A regular lattice of points, `fastest` × `slowest`, row by row.
    Grid { fastest: usize, slowest: usize },
    /// Indexed segments: 2M indices, each pair one edge.
    ///
    /// What polylines cannot say — they address *consecutive* points, and the
    /// edges of a wireframe join nodes that are nowhere near each other.
    Segments(Vec<u32>),
}

/// One patch of a representation, placed.
#[derive(Debug, Clone, PartialEq)]
pub struct Patch {
    /// Its index inside the representation, as RESQML numbers them.
    pub index: usize,
    /// Where its points start in the representation's own node numbering.
    ///
    /// A property or a subrepresentation that indexes *nodes of the
    /// representation* counts from here; the topology's indices do not.
    pub node_offset: usize,
    /// The points, flat: 3N numbers.
    pub points: Vec<f64>,
    /// How they connect.
    pub topology: Topology,
}

/// A representation's shape, read and placed.
#[derive(Debug, Clone, PartialEq)]
pub struct Geometry {
    /// The representation's uuid.
    pub uuid: String,
    /// Its ETP uri.
    pub uri: String,
    /// Its citation title, or empty.
    pub title: String,
    /// Its qualified type.
    pub qualified_type: String,
    /// The kind of shape the reader made of it.
    pub kind: String,
    /// Its projected EPSG code, when the CRS states one.
    pub epsg: Option<u64>,
    /// Which coordinates these are.
    pub placement: String,
    /// One entry per patch, in order.
    pub patches: Vec<Patch>,
}

/// One interchange format this build can write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Format {
    /// What a caller asks for: `obj`, `geojson`, `vtu`.
    pub name: String,
    /// The file extension, without the dot.
    pub extension: String,
    /// The MIME type, for a caller serving this over HTTP.
    pub mime: String,
    /// One line on what it is.
    pub describes: String,
}

/// Every type of the generated model this build carries.
///
/// **The model is the Rust one, read through reflection** — not a second copy
/// of it. A release added to `crates/generated/` appears here the day it is
/// registered, with its fields and its documentation, and nothing on this side
/// changes.
///
/// `package` filters on `eml`, `resqml`, `witsml` or `prodml`; `top_level_only`
/// keeps the types that can be a part of an EPC on their own, which is what a
/// caller about to create an object wants — and so it is
/// [`energyml_schemas::may_create`], the one list every face offers: a
/// development drop (`resqml 2.2_dev3`) is read, never offered. Without the
/// flag the whole registry comes back, the drop included, as an inventory.
pub fn model_types(package: Option<&str>, top_level_only: bool) -> Vec<TypeSummary> {
    let registry = energyml_schemas::all();
    let mut out: Vec<TypeSummary> = registry
        .types()
        .filter(|info| !top_level_only || (info.is_top_level && energyml_schemas::may_create(info)))
        .filter(|info| package.is_none_or(|want| info.package.eq_ignore_ascii_case(want)))
        .map(summarise_type)
        .collect();
    out.sort_by(|a, b| (&a.package, &a.version, &a.type_key).cmp(&(&b.package, &b.version, &b.type_key)));
    out
}

/// What one type is: its fields, its values, what extends it.
///
/// Takes a qualified type (`resqml22.TriangulatedSetRepresentation`) or a plain
/// type name. A name that matches nothing is refused **with what was tried**,
/// because the two spellings are easy to confuse and "not found" alone sends a
/// caller reading source.
pub fn describe_type(name: &str) -> Result<TypeDescription, String> {
    use energyml_core::reflect::TypeKind;

    let registry = energyml_schemas::all();
    let info = registry
        .by_qualified_type(name)
        .map(|entry| entry.info)
        .or_else(|| registry.find_type(name))
        .ok_or_else(|| {
            format!(
                "no type `{name}`: tried it as a qualified type \
(`resqml22.TriangulatedSetRepresentation`) and as a type name \
(`TriangulatedSetRepresentation`)"
            )
        })?;

    Ok(TypeDescription {
        summary: summarise_type(info),
        kind: match info.kind {
            TypeKind::Struct => "struct",
            TypeKind::Enumeration => "enumeration",
            TypeKind::Choice => "choice",
        }
        .to_string(),
        // The key a JSON document uses for each field — `_` for the text of a
        // simple content — from the writer's own mapping, not `json_name`.
        fields: info
            .fields
            .iter()
            .zip(energyml_core::json::json_keys(info))
            .map(|(field, json_key)| FieldSummary {
                name: json_key,
                doc: field.doc.to_string(),
                type_name: type_name_of(&field.ty),
                allowed: field
                    .ty
                    .allowed_values()
                    .iter()
                    .map(|one| one.value.to_string())
                    .collect(),
                is_open: field.ty.type_info().is_some_and(|info| info.is_open),
                min_occurs: field.occurs.min as i64,
                max_occurs: match field.occurs.max {
                    energyml_core::reflect::MaxOccurs::Bounded(max) => Some(max as i64),
                    energyml_core::reflect::MaxOccurs::Unbounded => None,
                },
                is_list: field.is_list,
            })
            .collect(),
        variants: info.variants.iter().map(|one| one.name.to_string()).collect(),
        subtypes: info
            .subtypes
            .iter()
            .filter_map(|one| one().qualified_type.map(str::to_string))
            .collect(),
        base: info.base.and_then(|one| one().qualified_type.map(str::to_string)),
    })
}

/// An empty object of this type, as a dictionary ready to be filled.
///
/// **This is how an object is created here**, and it is the generated model
/// that makes it: the registry builds the instance, the JSON writer renders it,
/// and what comes back carries its `$type`. Filling it in and handing it to
/// `put` is the whole of creating an object — no class per type on this side,
/// and no second model to keep in step.
///
/// # The uuid is given, never invented
///
/// `uuid` is optional and there is **no default**, because an identity is not
/// always a coin toss: `energyml-build` derives its uuids (v5) so that a second
/// run over the same source names the same objects, and a template that quietly
/// stamped a random one would give such a caller a fresh identity every run —
/// visible only at the second, when the package doubles its objects.
///
/// Left out, the field comes back empty and `put` refuses by naming it. That
/// refusal happens at the write; passing the uuid here moves it to the call
/// that made the mistake, which is the better place for it.
///
/// An abstract type is refused by name, with what extends it: the schema says
/// it cannot be instantiated, and a caller who asked for one wants one of its
/// subtypes. **No build of this workspace registers one today** — the registry
/// carries what can be a part — so that branch is a guard for the day a release
/// does, and `tests/model.rs` says so out loud rather than pretending to cover
/// it.
pub fn template(qualified_type: &str, uuid: Option<&str>) -> Result<String, String> {
    let registry = energyml_schemas::all();
    if let Some(entry) = registry.by_qualified_type(qualified_type)
        && entry.info.is_abstract
    {
        let subtypes: Vec<&str> = entry
            .info
            .subtypes
            .iter()
            .filter_map(|one| one().qualified_type)
            .collect();
        return Err(format!(
            "`{qualified_type}` is abstract, so nothing can be an instance of it; \
one of these can: {}",
            if subtypes.is_empty() {
                "none in this build".to_string()
            } else {
                subtypes.join(", ")
            }
        ));
    }
    let mut object = registry.create(qualified_type).map_err(|why| format!("{why}"))?;
    if let Some(uuid) = uuid {
        // Where the type keeps it — `uuid`, `UUID` or `uid` — resolved from the
        // roles rather than by name, and refused here if it is not one.
        energyml_core::identity::set_uuid_reflect(object.as_reflect_mut(), uuid)
            .map_err(|why| format!("{why}"))?;
    }
    energyml_core::json::write_string(object.as_reflect(), false).map_err(|why| format!("{why}"))
}

/// The name of a field's type, as someone filling a dictionary needs it.
///
/// `{:?}` on a `FieldType` prints a function pointer — true and useless. What a
/// caller wants is the type to put in `$type`, or the name of the scalar.
fn type_name_of(ty: &energyml_core::reflect::FieldType) -> String {
    use energyml_core::reflect::FieldType;
    match ty {
        FieldType::Scalar(kind) => format!("{kind:?}").to_lowercase(),
        FieldType::Object(one) | FieldType::Enumeration(one) => {
            let info = one();
            info.qualified_type
                .map(str::to_string)
                .unwrap_or_else(|| info.name.to_string())
        }
    }
}

fn summarise_type(info: &'static energyml_core::reflect::TypeInfo) -> TypeSummary {
    TypeSummary {
        qualified_type: info.qualified_type.map(str::to_string),
        type_key: info.type_key.to_string(),
        package: info.package.to_string(),
        version: info.version.to_string(),
        is_abstract: info.is_abstract,
        is_top_level: info.is_top_level,
        doc: info.doc.to_string(),
    }
}

/// The interchange formats this build offers.
///
/// From the exporter registry itself, never a list written twice: adding a
/// format to `energyml-export` makes it appear here, in `energyml export
/// --list`, and in the front end, without a line changing in any of them.
pub fn formats() -> Vec<Format> {
    energyml_export::Exporters::builtin()
        .all()
        .into_iter()
        .map(|one| Format {
            name: one.name().to_string(),
            extension: one.extension().to_string(),
            mime: one.mime().to_string(),
            describes: one.describe().to_string(),
        })
        .collect()
}

/// What renaming the external data file would touch.
///
/// `energyml_storage::rename::Rename`, converted. The **quote**: the caller
/// shows it and decides, which is the answer to the question the request itself
/// asked — update automatically, or ask.
#[derive(Debug, Clone, Default)]
pub struct RenamePlan {
    /// The file as the package names it today.
    pub from: String,
    /// The bare name it would take.
    pub to: String,
    /// The uris of the objects carrying the name in a field of their own.
    pub objects: Vec<String>,
    /// Fields that would be rewritten.
    pub fields: usize,
    /// `.rels` entries that would be rewritten.
    pub relationships: usize,
    /// Why it cannot be carried out. Empty means it can.
    pub refusals: Vec<String>,
}

/// What a rename did.
#[derive(Debug, Clone, Default)]
pub struct Renamed {
    /// Fields rewritten.
    pub fields: usize,
    /// `.rels` entries rewritten.
    pub relationships: usize,
    /// `moved`, `absent` or `not-on-disk` — three answers and not two, because
    /// *there was nothing to move* and *this store has no disk* are different
    /// news and a caller conflating them would report a missing file that is
    /// not missing.
    pub file: String,
    /// Where the file went, when it went anywhere.
    pub moved_to: Option<String>,
}

/// Everything that can be said against a package.
///
/// The report is `energyml-domain::check`'s, **counts included**: which finding
/// is an error is a decision, and two faces counting differently would disagree
/// about whether a file is valid.
#[derive(Debug, Clone, Default)]
pub struct CheckReport {
    /// How many findings are faults of the file.
    pub errors: usize,
    /// How many are worth knowing without condemning it.
    pub warnings: usize,
    /// What the reader noticed: severity, part, message.
    pub diagnostics: Vec<(String, String, String)>,
    /// Parts that could not be read: uri, and why.
    pub unreadable: Vec<(String, String)>,
    /// Whether the schema section ran — not the same as finding nothing.
    pub schema_checked: bool,
    /// Objects against their schema.
    pub schema: Vec<energyml_domain::check::SchemaIssue>,
    /// Properties against the technical guides.
    pub properties: Vec<energyml_domain::check::PropertyIssue>,
    /// Properties whose kind could not be resolved, so were not checked.
    pub properties_unresolved: usize,
    /// Properties whose values do not add up with what they hang on.
    pub counts: Vec<energyml_domain::check::CountIssue>,
    /// Properties whose count could not be judged — not checked, not fine.
    pub counts_unchecked: usize,
    /// Whether the business rules ran.
    pub rules_checked: bool,
    /// How many rules were evaluated.
    pub rule_count: usize,
    /// The business rules, violated or uncheckable.
    pub rules: Vec<energyml_domain::check::RuleFinding>,
    /// References that point somewhere wrong, or nowhere. Each carries whether
    /// it is an error and whether the target is a catalogue concept — absent by
    /// design, so neither an error nor a warning.
    pub references: Vec<(String, String, String, String, bool, bool)>,
    /// Where the `.rels` index and the objects disagree.
    pub relationships: Vec<(String, String, String)>,
}

/// A dataspace: what a store holds packages in.
///
/// An EPC is one dataspace and always will be — the concept comes from ETP,
/// where a server holds several. It is here rather than left out because the
/// **shape of the answer** must not change the day the other store arrives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dataspace {
    /// Its uri: `eml:///` for the default one.
    pub uri: String,
    /// Its name. For an EPC, the file name.
    pub name: String,
    /// When it was created, when the store knows.
    pub created: Option<String>,
    /// When it last changed, when the store knows.
    pub last_changed: Option<String>,
}

/// An object's citation, without reading the object.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Header {
    /// The four fields of a summary.
    pub summary: Summary,
    /// Who or what produced it.
    pub originator: Option<String>,
    /// When, in its lexical form.
    pub creation: Option<String>,
    /// When it last changed.
    pub last_update: Option<String>,
    /// The producing software, as it named itself.
    pub format: Option<String>,
    /// `objectVersion`, when it carries one.
    pub object_version: Option<String>,
    /// Schema version as the document writes it.
    pub schema_version: Option<String>,
}

/// One relationship recorded in a `.rels` part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relationship {
    /// Its id, unique within its `.rels`.
    pub id: String,
    /// The full type uri.
    pub kind: String,
    /// What it points at: a part name, or a file name when external.
    pub target: String,
    /// Whether the target lies outside the package.
    pub external: bool,
}

/// One type of the generated model, as a catalogue entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeSummary {
    /// `resqml22.TriangulatedSetRepresentation`, when the type is top-level.
    pub qualified_type: Option<String>,
    /// The concept, identical across releases: `TriangulatedSetRepresentation`.
    pub type_key: String,
    /// `eml`, `resqml`, `witsml` or `prodml`.
    pub package: String,
    /// The release, as the directory names it: `2.0.1`, `2.2`.
    pub version: String,
    /// Whether the XSD type is abstract — **it cannot be created**.
    pub is_abstract: bool,
    /// Whether it may be a part of an EPC on its own.
    pub is_top_level: bool,
    /// The schema's own documentation for it.
    pub doc: String,
}

/// One field of a type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldSummary {
    /// The name a JSON document uses — the one to put in a dictionary.
    pub name: String,
    /// The schema's own documentation.
    pub doc: String,
    /// The type of one value, as the schema names it.
    pub type_name: String,
    /// The values an enumeration field accepts.
    ///
    /// Empty for anything else — and **not exhaustive** when `is_open` is set:
    /// the Energistics `…Ext` enumerations take any other string too, which is
    /// how a producer names a concept the standard does not.
    pub allowed: Vec<String>,
    /// Whether that list is open.
    pub is_open: bool,
    /// How few values are allowed: 0 means optional.
    pub min_occurs: i64,
    /// How many are allowed; `None` for unbounded.
    pub max_occurs: Option<i64>,
    /// Whether the field takes a list of values rather than one.
    pub is_list: bool,
}

/// What a type is, field by field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeDescription {
    /// The catalogue entry.
    pub summary: TypeSummary,
    /// `struct`, `enumeration` or `choice`.
    pub kind: String,
    /// Its fields, for a struct.
    pub fields: Vec<FieldSummary>,
    /// Its values, for an enumeration; its cases, for a choice.
    pub variants: Vec<String>,
    /// The types that extend it — **where to look when this one is abstract**.
    pub subtypes: Vec<String>,
    /// The type it extends, when it extends one.
    pub base: Option<String>,
}

/// What a copy from another package did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Copied {
    /// Every object written, in the source's order.
    pub objects: Vec<String>,
    /// Of those, the ones nobody asked for: what the closure pulled in.
    pub added: Vec<String>,
    /// Referenced by what was copied, and the source does not hold it.
    pub missing: Vec<String>,
    /// How many datasets travelled.
    pub arrays: usize,
    /// Datasets a copied object points at that could not be copied.
    pub arrays_missed: Vec<String>,
    /// Objects the source could not hand over.
    pub unreadable: Vec<String>,
}

/// The other end of a reference, and where it sits.
///
/// One struct for both directions on purpose: an edge is an edge, and which end
/// is described is said by the call that returned it — [`Package::references`]
/// describes the target, [`Package::referenced_by`] describes the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// The other object's uri.
    pub uri: String,
    /// Its uuid.
    pub uuid: String,
    /// Its qualified type — **as the reference declares it** when the target is
    /// the far end, which is not always what the package holds.
    pub qualified_type: String,
    /// Its title, when one is known.
    pub title: Option<String>,
    /// Its `objectVersion`, when it declares one (and the package holds it, or
    /// the reference names it).
    pub object_version: Option<String>,
    /// The dotted path where the reference sits inside the *source* object.
    pub path: String,
    /// Whether the package holds the object described.
    ///
    /// Always true looking backwards — a package cannot hold a reference from
    /// an object it does not have. Looking forwards it is the news: `false`
    /// with a type the standard publishes in a catalogue is normal, `false`
    /// with anything else is a dangling reference.
    pub present: bool,
}

/// A property's values, lined up with a representation's points or cells.
#[derive(Debug, Clone, PartialEq)]
pub struct PropertyValues {
    /// Uuid of the property object.
    pub uuid: String,
    /// Its title, or its uuid when it has none.
    pub name: String,
    /// What it measures, when the kind resolved.
    pub kind: Option<String>,
    /// Its unit, when it states one.
    pub uom: Option<String>,
    /// `points` or `cells` — **checked against the geometry**, not trusted.
    pub scope: String,
    /// Values per element: 1 for a scalar, more for a vector.
    pub components: u64,
    /// The values, widened to `f64`, in element order.
    pub values: Vec<f64>,
    /// The colour ramp the **package** states for this property: RGBA rows in
    /// 0–1, low value first.
    ///
    /// Empty when the package states none, and empty is the right answer — the
    /// standard defines no default ramp, and inventing one would hand back a
    /// picture the file does not describe.
    ///
    /// It could not be reached from here until 20/09: resolving it needs the
    /// whole package (the cascade walks the `GraphicalInformationSet`), so
    /// neither an exporter, handed one object, nor a reader going shape by
    /// shape could do it. `scene::of_package` does.
    pub colours: Vec<[f64; 4]>,
}

/// An open EPC package, with its data files.
///
/// # One door, and it is synchronous
///
/// `EnergymlStorage` is an `async` trait, because an ETP store talks to a
/// server. `EpcStore` carries the same operations synchronously — it is what
/// the command line uses — and that is the one bound here: Python is
/// synchronous, and spinning a runtime up to await a file read would be paying
/// for machinery nothing needs. The day ETP arrives, it arrives through its
/// own door.
pub struct Package {
    store: EpcStore,
}

impl Package {
    /// Open an EPC.
    ///
    /// Data files are looked for **beside it**, which is the format's own
    /// convention and what the command line does: an EPC names its `.h5` files
    /// by a bare file name, not by a path.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        let registry = Arc::new(energyml_schemas::all());
        // **The refusal names the path.** What comes up from the I/O layer is
        // `The system cannot find the file specified. (os error 2)` — the operating
        // system's own sentence, in the operating system's own language, and
        // it says which file about nothing. A caller who passed three paths
        // would have no way to tell which one failed.
        let mut store =
            EpcStore::open(path, registry).map_err(|why| format!("cannot open {}: {why}", path.display()))?;
        if let Some(beside) = path.parent() {
            store.external_files_mut().set_root(beside);
        }
        // **The native backend, when this build has one.** Without it the
        // package still opens, lists and returns its objects; only an array
        // that is asked for refuses, and the refusal comes from the storage
        // layer and names the missing feature. A missing capability is not
        // missing data, so it does not turn into an empty list here.
        #[cfg(feature = "hdf5")]
        store.add_array_backend(Box::new(energyml_storage::hdf5::Hdf5Backend));
        Ok(Package { store })
    }

    /// An empty package, holding nothing but the two parts the specification
    /// requires.
    ///
    /// It has no path — where it goes is decided at [`Package::save_as`], and a
    /// function that both created and placed a package would give a caller no
    /// way to look before writing. Its data files land where
    /// [`Package::set_data_root`] says, and until then in the working
    /// directory.
    pub fn create(name: &str) -> Result<Self, String> {
        let registry = Arc::new(energyml_schemas::all());
        let bytes = energyml_storage::extract::empty_package(None).map_err(|why| format!("{why}"))?;
        #[allow(unused_mut)]
        let mut store = EpcStore::from_reader(Box::new(std::io::Cursor::new(bytes)), name, registry)
            .map_err(|why| format!("cannot create {name}: {why}"))?;
        #[cfg(feature = "hdf5")]
        store.add_array_backend(Box::new(energyml_storage::hdf5::Hdf5Backend));
        Ok(Package { store })
    }

    /// Where the data files are read from and written to.
    ///
    /// An EPC names its `.h5` by a bare file name (EPC §3.2.3), so something
    /// has to say which folder that name is relative to. Opening a package
    /// answers it — beside the `.epc` — and creating one cannot, which is why
    /// this exists and why it is not hidden in a constructor.
    pub fn set_data_root(&mut self, dir: impl AsRef<Path>) {
        self.store.external_files_mut().set_root(dir.as_ref());
    }

    /// Pin the name a package gives a data file to a path of its own.
    ///
    /// **For the caller who is told where the files are but not what the
    /// package calls them** — a Galaxy tool gets `dataset_7.dat` while the
    /// objects say `result.h5`. Reads and writes of an array whose file is
    /// `name` go to `path` whatever the package says, and the pin wins over
    /// everything else the storage layer would try (`ExternalFiles::set`). Pin
    /// the same name again and the last path wins, which is how one run reads
    /// an input file and then writes into its copy.
    pub fn pin_data_file(&mut self, name: &str, path: impl AsRef<Path>) {
        self.store.external_files_mut().set(name, path.as_ref());
    }

    /// Name the external file an `EpcExternalPartReference` stands for.
    ///
    /// Only needed for the older arrangement, where an array points at a proxy
    /// object rather than naming its file: saving regenerates every `.rels`
    /// from what the store knows, and a proxy created in this session would
    /// otherwise be written out naming no file at all. An EML 2.3 external
    /// array carries its own `URI` and needs none of this.
    pub fn register_external_file(&mut self, proxy_uuid: &str, file: &str) {
        self.store.register_external_file(proxy_uuid, file);
    }

    /// What renaming the external data file would touch.
    ///
    /// Reads every object, changes nothing. A refusal is a **list of reasons**
    /// rather than an exception: the caller draws them next to its button, and
    /// [`Package::rename_data_file`] declines to act while there is one.
    pub fn plan_rename(&self, from: &str, to: &str) -> RenamePlan {
        let quote = energyml_storage::rename::plan(&self.store, from, to);
        RenamePlan {
            from: quote.from,
            to: quote.to,
            objects: quote.objects,
            fields: quote.fields,
            relationships: quote.relationships,
            refusals: quote.refusals,
        }
    }

    /// Rename the external data file: the fields, the relationships, the file.
    ///
    /// Plans first, every time. Handing this a name and having it do less than
    /// all three would leave a package that opens and whose arrays cannot be
    /// found — the fault `energyml check` reports, made by the tool meant to
    /// prevent it.
    pub fn rename_data_file(&mut self, from: &str, to: &str) -> Result<Renamed, String> {
        use energyml_storage::rename::{self, FileMove};

        let quote = rename::plan(&self.store, from, to);
        let done = rename::apply(&mut self.store, &quote).map_err(|why| format!("{why}"))?;
        Ok(Renamed {
            fields: done.fields,
            relationships: done.relationships,
            file: match &done.file {
                FileMove::Moved(_) => "moved".to_string(),
                FileMove::Absent => "absent".to_string(),
                FileMove::NotOnDisk => "not-on-disk".to_string(),
            },
            moved_to: match &done.file {
                FileMove::Moved(path) => Some(path.display().to_string()),
                _ => None,
            },
        })
    }

    /// Where it came from, when it came from a file.
    pub fn path(&self) -> Option<PathBuf> {
        self.store.path().map(Path::to_path_buf)
    }

    /// Everything the package holds, reading no more than the headers.
    ///
    /// **The title comes from the headers, the list comes from the
    /// resources**, and the two are not the same thing: a summary is read off
    /// the zip part's name, a title requires parsing that part. An unreadable
    /// part therefore loses its title and **keeps its row** — dropping it from
    /// the list would say the package does not contain it, which is the
    /// opposite news.
    pub fn objects(&self) -> Vec<Summary> {
        let headers: std::collections::HashMap<String, (Option<String>, Option<String>)> = self
            .store
            .all_headers()
            .into_iter()
            .filter_map(|found| found.ok())
            .map(|head| (head.summary.uri, (head.title, version_of(head.object_version))))
            .collect();
        self.store
            .resources()
            .into_iter()
            .map(|one| Summary {
                title: headers
                    .get(&one.uri)
                    .and_then(|(title, _)| title.clone())
                    .unwrap_or_default(),
                object_version: headers.get(&one.uri).and_then(|(_, version)| version.clone()),
                uri: one.uri,
                uuid: one.uuid,
                qualified_type: one.qualified_type,
            })
            .collect()
    }

    /// The uri of a uuid, or `None` — which lets a caller work with the
    /// identifiers it reads in its other tools rather than with URIs.
    ///
    /// Through the store's gate: a uuid the package holds under **two**
    /// versions (or releases) is an error naming the candidates, never one of
    /// them chosen for the caller.
    pub fn uri_of(&self, uuid: &str) -> Result<Option<String>, String> {
        match self.store.lookup_uuid(uuid) {
            Ok(one) => Ok(Some(one.uri.clone())),
            Err(energyml_storage::Error::NotFound(_)) => Ok(None),
            Err(other) => Err(format!("{other}")),
        }
    }

    /// An object, as JSON.
    ///
    /// **JSON is the exchange form, and that is a choice.** A RESQML object has
    /// hundreds of fields and a type hierarchy; exposing it as Python classes
    /// would mean generating one module per release — redoing in Python what
    /// `crates/generated/` does in Rust, and redoing it at every new release. A
    /// dictionary is read, edited and written back without a single line
    /// knowing a type.
    pub fn get(&self, uri: &str) -> Result<String, String> {
        let object = self.store.get_object(uri).map_err(|why| format!("{why}"))?;
        energyml_core::json::write_string(object.as_reflect(), false).map_err(|why| format!("{why}"))
    }

    /// Put an object — a new one, or a replacement for the same uuid.
    ///
    /// Returns the uri written, which is not necessarily the expected one: it
    /// is derived from the type and the uuid the JSON carries, so a caller who
    /// changed either has written somewhere else.
    pub fn put(&mut self, json: &str) -> Result<String, String> {
        let document =
            energyml_core::json::read_string(json, self.store.registry()).map_err(|why| format!("{why}"))?;
        self.store.put(document.root).map_err(|why| format!("{why}"))
    }

    /// Remove an object.
    pub fn remove(&mut self, uri: &str) -> Result<(), String> {
        self.store.remove(uri).map_err(|why| format!("{why}"))
    }

    /// The arrays an object names.
    ///
    /// An object does not *contain* its numbers: it names a file and a path
    /// inside it. That list is what this returns, and it is what lets a
    /// multi-gigabyte package be read without decompressing a byte until a
    /// choice is made.
    pub fn arrays(&self, uri: &str) -> Result<Vec<ArrayRef>, String> {
        let object = self.store.get_object(uri).map_err(|why| format!("{why}"))?;
        Ok(self
            .store
            .arrays_of(object.as_reflect())
            .into_iter()
            .map(|one| ArrayRef {
                file: one.file,
                path_in_file: one.path_in_file,
            })
            .collect())
    }

    /// What an array weighs, without reading it.
    pub fn array_shape(&self, array: &ArrayRef) -> Result<Shape, String> {
        let ArrayMetadata {
            dimensions,
            element_type,
        } = self
            .store
            .array_metadata(&array.location())
            .map_err(|why| format!("{why}"))?;
        Ok(Shape {
            dimensions,
            element_type,
        })
    }

    /// Read an array, in the file's own type.
    pub fn read_array(&self, array: &ArrayRef) -> Result<(Shape, Block), String> {
        let ArrayData { metadata, values } = self
            .store
            .read_array(&array.location())
            .map_err(|why| format!("{why}"))?;
        let shape = Shape {
            dimensions: metadata.dimensions,
            element_type: metadata.element_type,
        };
        Ok((shape, Block::from(values)))
    }

    /// Write an array.
    ///
    /// **The shape and the values must agree**, and the store refuses when they
    /// do not, naming both counts — an array whose shape promises more numbers
    /// than it carries writes without error in some formats and reads back
    /// silently with zeros at the end. The check is `EpcStore`'s and not this
    /// bridge's, so every writer gets it.
    pub fn write_array(&self, array: &ArrayRef, dimensions: &[i64], block: Block) -> Result<(), String> {
        let data = ArrayData {
            metadata: ArrayMetadata {
                dimensions: dimensions.to_vec(),
                element_type: block.element_type().to_string(),
            },
            values: block.into_values(),
        };
        self.store
            .write_array(&array.location(), &data)
            .map_err(|why| format!("{why}"))
    }

    /// Read a slice of an array rather than all of it.
    ///
    /// `start` and `count` are per dimension, and they are what ETP calls a
    /// subarray. The point is a grid of eighty million numbers whose caller
    /// wants one layer: the backend reads the slice from the file, so nothing
    /// bigger than the answer is ever allocated.
    pub fn read_array_part(
        &self,
        array: &ArrayRef,
        start: &[i64],
        count: &[i64],
    ) -> Result<(Shape, Block), String> {
        if start.len() != count.len() {
            return Err(format!(
                "start has {} dimension(s) and count has {}: a slice states both for each",
                start.len(),
                count.len()
            ));
        }
        let location = ArrayLocation {
            start: start.to_vec(),
            count: count.to_vec(),
            ..array.location()
        };
        let ArrayData { metadata, values } =
            self.store.read_array(&location).map_err(|why| format!("{why}"))?;
        let shape = Shape {
            dimensions: metadata.dimensions,
            element_type: metadata.element_type,
        };
        Ok((shape, Block::from(values)))
    }

    /// The objects whose type or title contains what is asked.
    ///
    /// The loop a caller writes over [`Package::objects`], done here where it
    /// costs nothing: a summary already carries the type, so the type filter
    /// runs **before any header is read** — a package of seventy thousand
    /// objects asked for its five point sets inflates five citations, not
    /// seventy thousand (08/10/2026: `objects()` first, then the filter, cost
    /// 20 s of a run that needed 5 headers). The title filter, which does need
    /// the citation, runs on what the type kept. Both filters are
    /// case-insensitive, and asking for neither returns everything.
    pub fn find(&self, of_type: Option<&str>, titled: Option<&str>) -> Vec<Summary> {
        use energyml_storage::resource::{title_matches, type_matches};
        self.store
            .resources()
            .into_iter()
            .filter(|one| of_type.is_none_or(|want| type_matches(&one.qualified_type, want)))
            .map(|one| {
                // A part whose header cannot be read **keeps its row** with an
                // empty title, as in `objects()`: dropping it would say the
                // package does not hold it.
                let head = self.store.header(&one.uri).ok();
                Summary {
                    title: head
                        .as_ref()
                        .and_then(|head| head.title.clone())
                        .unwrap_or_default(),
                    object_version: version_of(head.and_then(|head| head.object_version)),
                    uri: one.uri,
                    uuid: one.uuid,
                    qualified_type: one.qualified_type,
                }
            })
            .filter(|one| titled.is_none_or(|want| title_matches(&one.title, want)))
            .collect()
    }

    /// The shape of a representation: points and topology, placed.
    ///
    /// **This is the answer an array cannot give.** A `TriangulatedSetRepresentation`
    /// names two arrays and a CRS; turning them into points in a projected
    /// system is `energyml-geometry`'s work, and it covers grids, parametric
    /// pillars, polylines, subrepresentations and borrowed geometry alike. A
    /// caller that receives the raw arrays instead is a caller redoing it.
    pub fn geometry(&self, uri: &str, placement: &str) -> Result<Geometry, String> {
        let placed = Placement::parse(placement)?;
        let object = self.store.get_object(uri).map_err(|why| format!("{why}"))?;
        let built =
            scene::object_of(object.as_reflect(), &self.store, uri).map_err(|why| format!("{why}"))?;
        let shape = built.geometry_in(placed).map_err(|why| format!("{why}"))?;
        Ok(Geometry {
            uuid: built.uuid.clone(),
            uri: built.uri.clone(),
            title: built.title.clone(),
            qualified_type: built.qualified_type.clone(),
            kind: shape.kind.to_string(),
            epsg: built.epsg(),
            placement: placement.to_string(),
            patches: shape.patches.iter().map(patch_of).collect(),
        })
    }

    /// The same shape as one triangle mesh: points and triples, patches merged.
    ///
    /// What a viewer wants, and it **says** that it triangulates — quadrilaterals
    /// and cells are cut, which is a decision the caller can now see being made
    /// rather than discover in the numbers.
    pub fn mesh(&self, uri: &str, placement: &str) -> Result<(Vec<f64>, Vec<u32>), String> {
        let placed = Placement::parse(placement)?;
        let object = self.store.get_object(uri).map_err(|why| format!("{why}"))?;
        let built =
            scene::object_of(object.as_reflect(), &self.store, uri).map_err(|why| format!("{why}"))?;
        let (points, triangles) = built.mesh_in(placed).map_err(|why| format!("{why}"))?;
        Ok((
            points.iter().flat_map(|p| p.iter().copied()).collect(),
            triangles.iter().flat_map(|t| t.iter().copied()).collect(),
        ))
    }

    /// The properties hanging on a representation, with their values.
    ///
    /// Reads **every** property of the package, because nothing in a
    /// representation lists its own: every link points the other way. What
    /// comes back is what fits this shape; the second list holds what did not,
    /// rather than disappearing — a property dropped in silence is a colour a
    /// caller will never know it is missing.
    pub fn properties(&self, uri: &str) -> Result<(Vec<PropertyValues>, Vec<String>), String> {
        // Named before anything is read, because *this package holds no such
        // object* is a different answer from *this shape carries nothing*, and
        // a walk that simply found nothing would give the second for the first.
        let resources = self.store.resources();
        let wanted = resources
            .iter()
            .find(|summary| summary.uri == uri)
            .ok_or_else(|| format!("this package holds no object at {uri}"))?;

        // **The one door** (`scene::of_package`), which is what brings the
        // colour ramps with the values: they need the whole package.
        let only = [wanted.uuid.clone()];
        let read = scene::of_package(
            &resources,
            &self.store,
            &scene::Request {
                only: &only,
                properties: scene::Properties::All,
                ..Default::default()
            },
        );
        let mut problems = read.unreadable;
        problems.extend(read.refused);
        problems.extend(read.unattached);
        let attributes = read
            .objects
            .into_iter()
            .next()
            .map(|one| one.attributes)
            .unwrap_or_default();

        Ok((
            attributes
                .into_iter()
                .map(|one| PropertyValues {
                    uuid: one.uuid,
                    name: one.name,
                    kind: one.kind,
                    uom: one.uom,
                    scope: match one.scope {
                        AttributeScope::Points => "points".to_string(),
                        AttributeScope::Cells => "cells".to_string(),
                    },
                    components: one.components,
                    values: one.values,
                    colours: one.lut,
                })
                .collect(),
            problems,
        ))
    }

    /// What this object points at.
    ///
    /// The cheap direction: one object read. `present` says whether the package
    /// holds the target — the question to ask before following it.
    pub fn references(&self, uri: &str) -> Result<Vec<Link>, String> {
        let object = self.store.get_object(uri).map_err(|why| format!("{why}"))?;
        Ok(energyml_storage::graph::references(object.as_reflect())
            .into_iter()
            .map(|one| {
                // Resolved as a reference — uuid, type, and the version the
                // reference names, read from its uri by the one parser — and
                // answered with **the uri of the object found**: a reference
                // written without a version to an object that declares one
                // names it by the full uri the store knows it by, never by the
                // reference's own spelling.
                let named = energyml_core::identity::ObjectId::parse(&one.uri)
                    .ok()
                    .and_then(|(_, id)| id.object_version);
                let held = self
                    .store
                    .resolve_reference(&one.uuid, Some(&one.qualified_type), named.as_deref())
                    .ok();
                Link {
                    object_version: version_of(
                        held.and_then(|found| {
                            self.store.header(&found.uri).ok().and_then(|h| h.object_version)
                        })
                        .or(named),
                    ),
                    // The package's own title beats the hint the reference
                    // carries: a DOR's title is what the writer believed, and
                    // the two disagree in real files.
                    title: held
                        .and_then(|found| self.store.header(&found.uri).ok().and_then(|h| h.title))
                        .or(one.title),
                    present: held.is_some(),
                    uri: held.map(|found| found.uri.clone()).unwrap_or(one.uri),
                    uuid: one.uuid,
                    qualified_type: one.qualified_type,
                    path: one.path,
                }
            })
            .collect())
    }

    /// Who points at this object.
    ///
    /// **The question to answer before deleting anything**, and the one
    /// direction a package has no index for: every object has to be read. On a
    /// large package that is the cost of building the graph, and it is a call
    /// rather than a field so a caller can see when they pay it.
    pub fn referenced_by(&self, uri: &str) -> Result<Vec<Link>, String> {
        let uuid = self
            .store
            .resource(uri)
            .map_err(|why| format!("{why}"))?
            .uuid
            .clone();
        let (held, _) = self.store.read_objects(None);
        Ok(energyml_storage::graph::dependents(
            held.iter()
                .map(|(summary, object)| (summary, object.as_reflect())),
            &uuid,
        )
        .into_iter()
        .map(|one| Link {
            object_version: version_of(self.store.header(&one.uri).ok().and_then(|h| h.object_version)),
            uri: one.uri,
            uuid: one.uuid,
            qualified_type: one.qualified_type,
            title: one.title,
            path: one.path,
            present: true,
        })
        .collect())
    }

    /// These objects and everything they need, following references as far as
    /// they go.
    ///
    /// Returns `(needed, missing)`. `needed` is a set nothing in the package
    /// would break if it were cut out on its own — what an extraction or a copy
    /// into another package takes. `missing` is what is referenced and not
    /// held, minus what the standard publishes in a catalogue: a closure that
    /// dropped those in silence would call a set closed when the package it
    /// came from was already broken.
    pub fn needs(&self, uris: &[String]) -> Result<(Vec<String>, Vec<String>), String> {
        let (held, _) = self.store.read_objects(None);
        let graph = energyml_storage::graph::build(
            held.iter()
                .map(|(summary, object)| (summary, object.as_reflect())),
        );
        let closed = energyml_storage::graph::closure(&graph, uris);
        Ok((closed.needed, closed.missing))
    }

    /// Write representations out in an interchange format.
    ///
    /// The bytes rather than a file: a caller that wants a file writes one, and
    /// a caller serving this over HTTP has what it needs without going through
    /// the disk. One document for the lot — which is what the format decides,
    /// not this: OBJ concatenates, GeoJSON encloses.
    ///
    /// `placement` overrides what the exporter would have chosen, and choosing
    /// nothing is the ordinary case: every exporter states the coordinates it
    /// wants, and overriding that is for a caller who knows better.
    ///
    /// `with_properties` carries the values too, for the formats that write
    /// them — a VTK field array, a GeoJSON property. It is off by default and
    /// that is a cost, not a preference: nothing in a representation lists its
    /// properties, so finding them means reading every property of the package.
    ///
    /// `metadata` is a **metadata spec as JSON** — `{"key": "dotted.path"}` —
    /// collected on every object and written verbatim by every format, so the
    /// same spec yields the same keys in an OBJ comment, a GeoJSON property and
    /// a VTK field array. [`standard_metadata`] gives the one this workspace
    /// ships. It is JSON rather than a keyword because a keyword and a document
    /// in one argument are two meanings for one thing.
    ///
    /// **A format that cannot write one of these objects is a refusal, not a
    /// silence.** An export that quietly skipped what it could not draw would
    /// hand back a file that looks complete.
    pub fn export(
        &self,
        uris: &[String],
        format: &str,
        placement: Option<&str>,
        with_properties: bool,
        metadata: Option<&str>,
    ) -> Result<Vec<u8>, String> {
        let exporters = energyml_export::Exporters::builtin();
        let exporter = exporters.get(format).map_err(|why| format!("{why}"))?;
        // **The geodesy, built whether or not anybody asks.** RFC 7946 makes
        // GeoJSON WGS 84 by definition, so the exporter refuses a model in
        // projected metres without a reprojector — and without this line
        // `export(…, "geojson")` was impossible from Python while the command
        // line wrote it. It costs a `BTreeMap`.
        let backend = energyml_proj::Proj4::new();
        let mut options = energyml_export::Options::new().with_reprojector(&backend);
        if let Some(name) = placement {
            options = options.with_placement(Placement::parse(name)?);
        }
        // Parsed before anything is read, so a spec with a typo in it fails
        // where the typo is rather than after a package has been walked.
        let spec = match metadata {
            Some(text) => {
                Some(energyml_domain::metadata::Spec::from_json(text).map_err(|why| format!("{why}"))?)
            }
            None => None,
        };

        // **The one door** (`scene::of_package`): one walk for the lot, the
        // properties read once for the package rather than once per object,
        // and the colour ramps resolved — which needs the whole package, so
        // nothing reading shape by shape could have done it.
        let resources = self.store.resources();
        let only: Vec<String> = uris
            .iter()
            .map(|uri| {
                resources
                    .iter()
                    .find(|summary| &summary.uri == uri)
                    .map(|summary| summary.uuid.clone())
                    .ok_or_else(|| format!("this package holds no object at {uri}"))
            })
            .collect::<Result<_, _>>()?;
        let read = scene::of_package(
            &resources,
            &self.store,
            &scene::Request {
                only: &only,
                spec: spec.as_ref(),
                properties: match with_properties {
                    true => scene::Properties::All,
                    false => scene::Properties::None,
                },
                ..Default::default()
            },
        );

        let mut ready = Vec::new();
        for uri in uris {
            // **A shape that did not come back is a refusal, not a gap.** The
            // walk keeps what it could evaluate; handing back a document
            // missing an object somebody asked for by name would look complete.
            let built = read.objects.iter().find(|one| &one.uri == uri).ok_or_else(|| {
                match read.refused.iter().find(|line| line.contains(uri)) {
                    Some(why) => why.to_string(),
                    None => format!(
                        "{uri} was not evaluated: {}",
                        read.refused
                            .iter()
                            .chain(read.unreadable.iter())
                            .next()
                            .map(String::as_str)
                            .unwrap_or("it carries no geometry this build reads")
                    ),
                }
            })?;
            let prepared =
                energyml_export::registry::prepare(built, &options).map_err(|why| format!("{uri}: {why}"))?;
            if !exporter.handles_with(&prepared, &options) {
                return Err(format!(
                    "`{format}` does not write {} ({})",
                    prepared.qualified_type, uri
                ));
            }
            ready.push(prepared.into_owned());
        }

        let mut out = Vec::new();
        exporter
            .write_all(&ready, &mut out, &options)
            .map_err(|why| format!("{why}"))?;
        Ok(out)
    }

    /// Copy objects — **and everything they need** — out of another package.
    ///
    /// What a partial export cannot stand in for: this package keeps what it
    /// has, and receives the objects asked for plus their closure. Asking for
    /// a representation brings its interpretation, the feature behind it and
    /// the CRS they hang off, because a representation without them opens and
    /// means nothing.
    ///
    /// Returns what was copied, **what the closure added**, what is referenced
    /// and missing from the source, and how many datasets travelled. `arrays`
    /// is `copy` or `leave` — leaving them makes this package name the other
    /// one's data file, which is legal and rarely what anyone means.
    pub fn copy_from(&mut self, source: &Package, uris: &[String], arrays: &str) -> Result<Copied, String> {
        let how = match arrays {
            "copy" => energyml_storage::extract::Arrays::Copy,
            "leave" => energyml_storage::extract::Arrays::Leave,
            other => {
                return Err(format!(
                    "unknown array handling `{other}`: `copy` (the datasets travel) or `leave` (this package names the other one's file)"
                ));
            }
        };
        let done = energyml_storage::extract::copy_into(&source.store, &mut self.store, uris, how)
            .map_err(|why| format!("{why}"))?;
        Ok(Copied {
            objects: done.objects,
            added: done.added,
            missing: done.missing,
            arrays: done.arrays,
            arrays_missed: done.arrays_missed,
            unreadable: done.unreadable,
        })
    }

    /// Judge this package: everything that can be said against it.
    ///
    /// The sections are the ones `energyml check` prints, and the arithmetic is
    /// the same — this reads the report, it does not recompute it. `schema` is
    /// the slowest section and the one a caller who has just validated
    /// elsewhere can do without; `rules` are the business constraints the
    /// technical guides state and no XSD can enforce.
    ///
    /// **Not checked is not the same as clean**, which is why the report says
    /// which sections ran.
    pub fn check(&self, schema: bool, rules: bool) -> CheckReport {
        use energyml_domain::check;

        let report = check::of_store(
            &self.store,
            &check::Options {
                schema,
                rules,
                extra_rules: Vec::new(),
            },
        );

        CheckReport {
            errors: report.errors(),
            warnings: report.warnings(),
            diagnostics: report
                .diagnostics
                .iter()
                .map(|one| {
                    let severity = match one.severity {
                        energyml_storage::error::Severity::Warning => "warning",
                        _ => "note",
                    };
                    (severity.to_string(), one.part.clone(), one.message.clone())
                })
                .collect(),
            unreadable: report.unreadable.clone(),
            schema_checked: report.schema_checked,
            schema: report.schema.clone(),
            properties: report.properties.clone(),
            properties_unresolved: report.properties_unresolved,
            counts: report.counts.clone(),
            counts_unchecked: report.counts_unchecked,
            rules_checked: report.rules_checked,
            rule_count: report.rule_count,
            rules: report.rules.clone(),
            references: report
                .references
                .iter()
                .map(|one| {
                    (
                        one.source.clone(),
                        one.path.clone(),
                        one.target_uuid.clone(),
                        format!("{one}"),
                        one.is_error(),
                        one.is_info(),
                    )
                })
                .collect(),
            relationships: report
                .relationships
                .iter()
                .map(|one| (one.part.clone(), one.target.clone(), format!("{one}")))
                .collect(),
        }
    }

    /// What kind of store this is: `EPC`.
    ///
    /// From the mother trait, and it is the answer a caller writes its message
    /// with the day a second kind exists.
    pub fn store_kind(&self) -> &'static str {
        energyml_storage::EnergymlStorage::store_kind(&self.store)
    }

    /// The dataspaces this store holds. An EPC is exactly one.
    pub fn dataspaces(&self) -> Vec<Dataspace> {
        vec![self.store.dataspace()]
            .into_iter()
            .map(dataspace_of)
            .collect()
    }

    /// The citations of these objects, without reading the objects.
    ///
    /// Plural because the mother trait is plural: ETP asks for a hundred in one
    /// message, and an interface that could only ask for one would condemn the
    /// network store to be slow. A uri that cannot be read is **missing from
    /// the answer rather than failing it** — ninety-nine good ones must not be
    /// lost to one bad one.
    pub fn headers(&self, uris: &[String]) -> Vec<Header> {
        self.store
            .headers(uris)
            .into_iter()
            .filter_map(|one| one.ok())
            .map(header_of)
            .collect()
    }

    /// The types this store can read — the generated model behind it.
    pub fn supported_types(&self) -> Vec<TypeSummary> {
        self.store
            .supported_types()
            .into_iter()
            .map(summarise_type)
            .collect()
    }

    /// The parts of the package that are not objects: whatever was put beside
    /// them.
    pub fn attachments(&self) -> Vec<String> {
        energyml_storage::LocalPackage::attachments(&self.store)
    }

    /// One attachment's bytes.
    pub fn read_attachment(&self, part: &str) -> Result<Vec<u8>, String> {
        energyml_storage::LocalPackage::read_attachment(&self.store, part).map_err(|why| format!("{why}"))
    }

    /// The relationships recorded for a part.
    ///
    /// **An index, not the truth.** What an object references comes from its
    /// own DORs — `references` answers that — and the two disagreeing is
    /// exactly what `check` reports.
    pub fn relationships(&self, part: &str) -> Result<Vec<Relationship>, String> {
        self.store
            .relationships(part)
            .map(|found| {
                found
                    .into_iter()
                    .map(|one| Relationship {
                        id: one.id,
                        kind: one.kind,
                        target: one.target,
                        external: one.external,
                    })
                    .collect()
            })
            .map_err(|why| format!("{why}"))
    }

    /// Is there anything to save?
    pub fn is_dirty(&self) -> bool {
        self.store.is_dirty()
    }

    /// Write the package elsewhere, and return what the write had to say.
    ///
    /// The diagnostics are **returned, not swallowed**: a save that had to
    /// repair a relationship or set aside an unreadable part did so for good,
    /// and a caller who does not learn it believes it copied identically.
    pub fn save_as(&self, path: impl AsRef<Path>) -> Result<Vec<String>, String> {
        self.store
            .save_as(path.as_ref(), &SaveOptions::default())
            .map(|found| found.iter().map(|one| format!("{one}")).collect())
            .map_err(|why| format!("{why}"))
    }
}

/// The metadata spec this workspace ships, as JSON.
///
/// What `energyml export --standard-metadata` writes, handed over as a document
/// rather than as a keyword: [`Package::export`] takes a spec **as JSON**, so
/// a caller starts from this one, edits it and passes it back. A keyword would
/// have made the argument mean two things.
pub fn standard_metadata() -> String {
    let spec = energyml_domain::metadata::Spec::standard();
    let map: serde_json::Map<String, serde_json::Value> = spec
        .entries()
        .iter()
        .map(|(key, path)| (key.clone(), serde_json::Value::String(path.clone())))
        .collect();
    serde_json::Value::Object(map).to_string()
}

/// One patch of `energyml-geometry`, flattened for numpy.
fn patch_of(patch: &geom::Patch) -> Patch {
    Patch {
        index: patch.index,
        node_offset: patch.node_offset,
        points: patch.points.iter().flat_map(|p| p.iter().copied()).collect(),
        topology: match &patch.topology {
            geom::Topology::Points => Topology::Points,
            geom::Topology::Triangles(all) => {
                Topology::Triangles(all.iter().flat_map(|t| t.iter().copied()).collect())
            }
            geom::Topology::Quads(all) => {
                Topology::Quads(all.iter().flat_map(|q| q.iter().copied()).collect())
            }
            geom::Topology::Polygons(rings) => Topology::Polygons {
                counts: rings.iter().map(|ring| ring.len() as u32).collect(),
                indices: rings.iter().flatten().copied().collect(),
            },
            geom::Topology::Polylines(counts) => Topology::Polylines {
                counts: counts.clone(),
                closed: patch.closed.clone(),
            },
            geom::Topology::Polyhedra(cells) => {
                // The private runs are not reachable, and asking for the faces
                // cell by cell is the accessor that exists — it also hands them
                // already wound outward, which is the one thing a caller cannot
                // work out for itself.
                let mut cell_face_counts = Vec::with_capacity(cells.cell_count());
                let mut face_node_counts = Vec::new();
                let mut face_nodes = Vec::new();
                for cell in 0..cells.cell_count() {
                    let mut faces = 0_u32;
                    for face in cells.faces_outward(cell) {
                        faces += 1;
                        face_node_counts.push(face.len() as u32);
                        face_nodes.extend(face);
                    }
                    cell_face_counts.push(faces);
                }
                Topology::Polyhedra {
                    cell_face_counts,
                    face_node_counts,
                    face_nodes,
                }
            }
            geom::Topology::Hexahedra {
                cells,
                dimensions,
                index,
            } => Topology::Hexahedra {
                cells: cells.iter().flat_map(|c| c.iter().copied()).collect(),
                dimensions: *dimensions,
                index: index.clone(),
            },
            geom::Topology::Grid { fastest, slowest } => Topology::Grid {
                fastest: *fastest,
                slowest: *slowest,
            },
            geom::Topology::Segments(all) => {
                Topology::Segments(all.iter().flat_map(|s| s.iter().copied()).collect())
            }
        },
    }
}

/// A dataspace, for the bridge.
fn dataspace_of(one: energyml_storage::resource::Dataspace) -> Dataspace {
    Dataspace {
        uri: one.uri,
        name: one.name,
        created: one.created,
        last_changed: one.last_changed,
    }
}

/// A citation, for the bridge.
fn header_of(one: energyml_storage::resource::ObjectHeader) -> Header {
    Header {
        summary: Summary {
            uri: one.summary.uri,
            uuid: one.summary.uuid,
            qualified_type: one.summary.qualified_type,
            title: one.title.unwrap_or_default(),
            object_version: version_of(one.object_version.clone()),
        },
        originator: one.originator,
        creation: one.creation,
        last_update: one.last_update,
        format: one.format,
        object_version: one.object_version,
        schema_version: one.schema_version,
    }
}

/// An `objectVersion` as the bridge shows it: `None` when it is absent or blank.
fn version_of(version: Option<String>) -> Option<String> {
    version
        .map(|version| version.trim().to_string())
        .filter(|version| !version.is_empty())
}
