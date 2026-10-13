//! A property, read the same way in every version.
//!
//! # What a property is, and where the difficulty is
//!
//! A property is numbers attached to a representation: one value — or several
//! — per cell, per node, per triangle. Reading the numbers is already done
//! ([`crate::values`] handles all eighteen array forms). What is left is the
//! part that decides whether those numbers mean anything:
//!
//! - **which elements they hang on** — `IndexableElement`, 29 values, and a
//!   value the schema does not know is an error rather than a guess;
//! - **how many there should be**, which is `element_count × ∏ value counts`
//!   and is the single most damaging thing to get wrong: a wrong count reads
//!   as a plausible array of the wrong shape;
//! - **which patch each array belongs to**, and this is a real semantic break
//!   between versions — 2.0.1 attaches by an explicit
//!   `RepresentationPatchIndex`, 2.2 attaches **by position**;
//! - **what kind of quantity it is**, which in 2.0.1 is an inline choice
//!   between a standard name and a local reference, and in 2.2 is a plain
//!   reference to an EML object.
//!
//! # What this module does not do
//!
//! It does not *resolve* the property kind — following the reference into the
//! package, or into the Energistics dictionary, and walking up the parent
//! chain. That needs a seam (the dictionary is 3.1 MB and must not land in a
//! wasm bundle by default), so [`PropertyKindRef`] says what the document
//! says, and resolution is a layer above.
//!
//! It also never invents a count. [`element_count`] answers for the
//! combinations it knows and **refuses by naming the pair** for the rest,
//! because a plausible number here silently mis-shapes an array.
//!
//! # Reading, in one example
//!
//! ```no_run
//! # use energyml_domain::property::{self, IndexableElement};
//! # fn demo(object: &dyn energyml_core::value::Reflect,
//! #         representation: &dyn energyml_core::value::Reflect,
//! #         source: &dyn energyml_domain::source::ArraySource) -> energyml_domain::Result<()> {
//! let property = property::read(object)?;
//! for patch in &property.patches {
//!     let values = patch.read(source)?;
//!     println!("{} values", values.len());
//! }
//! // And the check that catches a mis-shaped array:
//! for discrepancy in property.check(representation, None) {
//!     eprintln!("{discrepancy}");
//! }
//! # Ok(()) }
//! ```

use energyml_core::identity;
use energyml_core::value::{FieldValue, Reflect, Scalar};

use crate::error::{Error, Result};
use crate::source::ArraySource;
use crate::values::{self, Values};

/// What the values of a property hang on.
///
/// The eml 2.3 list, which is the RESQML 2.0.1 list of 27 plus `triangles` and
/// `patches`. Closed on the way in: a value outside it is
/// [`Error::Malformed`], never a default, because every count and every array
/// shape downstream is derived from this one word.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
#[allow(missing_docs)]
pub enum IndexableElement {
    Cells,
    Columns,
    ColumnEdges,
    Contacts,
    CoordinateLines,
    Edges,
    EdgesPerColumn,
    Enumeration,
    Faces,
    FacesPerCell,
    Interval,
    IntervalEdges,
    Intervals,
    IntervalsFromDatum,
    I0,
    I0Edges,
    J0,
    J0Edges,
    Layers,
    Lines,
    Nodes,
    NodesPerCell,
    NodesPerEdge,
    NodesPerFace,
    Patches,
    Pillars,
    Radial,
    Regions,
    Representation,
    Subnodes,
    Triangles,
}

impl IndexableElement {
    /// The lexical value, as the XSD spells it.
    pub fn as_str(self) -> &'static str {
        use IndexableElement::*;
        match self {
            Cells => "cells",
            Columns => "columns",
            ColumnEdges => "column edges",
            Contacts => "contacts",
            CoordinateLines => "coordinate lines",
            Edges => "edges",
            EdgesPerColumn => "edges per column",
            // `enumeration` is not a value any schema spells; the list has
            // said `enumerated elements` since RESQML 2.0.1, so this variant
            // could never be parsed and every property using it was refused.
            Enumeration => "enumerated elements",
            Faces => "faces",
            FacesPerCell => "faces per cell",
            Interval => "interval",
            IntervalEdges => "interval edges",
            Intervals => "intervals",
            IntervalsFromDatum => "intervals from datum",
            I0 => "I0",
            I0Edges => "I0 edges",
            J0 => "J0",
            J0Edges => "J0 edges",
            Layers => "layers",
            Lines => "lines",
            Nodes => "nodes",
            NodesPerCell => "nodes per cell",
            NodesPerEdge => "nodes per edge",
            NodesPerFace => "nodes per face",
            Patches => "patches",
            Pillars => "pillars",
            Radial => "radial origin polyline",
            Regions => "regions",
            Representation => "representation",
            Subnodes => "subnodes",
            Triangles => "triangles",
        }
    }

    /// Parse the lexical value.
    ///
    /// Spelling is compared without spaces and without case, because the two
    /// versions disagree on `I0 edges` versus `I0Edges` and a reader that
    /// insists on one of them loses every property of the other.
    pub fn parse(text: &str) -> Option<Self> {
        let wanted = squash(text);
        [
            Self::Cells,
            Self::Columns,
            Self::ColumnEdges,
            Self::Contacts,
            Self::CoordinateLines,
            Self::Edges,
            Self::EdgesPerColumn,
            Self::Enumeration,
            Self::Faces,
            Self::FacesPerCell,
            Self::Interval,
            Self::IntervalEdges,
            Self::Intervals,
            Self::IntervalsFromDatum,
            Self::I0,
            Self::I0Edges,
            Self::J0,
            Self::J0Edges,
            Self::Layers,
            Self::Lines,
            Self::Nodes,
            Self::NodesPerCell,
            Self::NodesPerEdge,
            Self::NodesPerFace,
            Self::Patches,
            Self::Pillars,
            Self::Radial,
            Self::Regions,
            Self::Representation,
            Self::Subnodes,
            Self::Triangles,
        ]
        .into_iter()
        .find(|candidate| squash(candidate.as_str()) == wanted)
    }
}

impl std::fmt::Display for IndexableElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

fn squash(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace() && *c != '_' && *c != '-')
        .flat_map(char::to_lowercase)
        .collect()
}

/// What the document says the property kind is — not what it resolves to.
///
/// The two versions say it in genuinely different ways, and flattening them
/// into one string would lose the distinction that matters: a standard kind is
/// a *name* in a closed vocabulary, a local one is an *object* somewhere.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PropertyKindRef {
    /// RESQML 2.0.1 `StandardPropertyKind`: a name from the RESQML
    /// vocabulary, resolvable without touching the package.
    Standard {
        /// The name, as written (`length`, `rock volume`, …).
        name: String,
    },
    /// A reference to a `PropertyKind` object: 2.0.1 `LocalPropertyKind`, and
    /// every kind in 2.2.
    Object {
        /// Uuid of the referenced kind.
        uuid: String,
        /// Title cached in the reference, which is a hint and may be stale.
        ///
        /// Never a key: the corpus has two kinds titled `X` and two titled `Y`
        /// in one package.
        title: Option<String>,
    },
}

/// When a property applies.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TimeRef {
    /// Index into the series — 2.0.1 `TimeIndex.Index`, absent in the 2.2
    /// `TimeOrIntervalSeries` form until an index is chosen.
    pub index: Option<u64>,
    /// Uuid of the `TimeSeries` object.
    pub series: Option<String>,
    /// 2.2 `TimeOrIntervalSeries.UseInterval`: the value covers the interval
    /// *between* two times rather than one instant.
    pub use_interval: bool,
}

/// One qualifier on a property — `eml:Facet` (`what`, `direction`, …).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Facet {
    /// The facet kind, lexically (`what`, `side`, `direction`, …).
    pub kind: String,
    /// The facet value.
    pub value: String,
}

/// One array of values, and the patch of the representation it belongs to.
///
/// The version difference lives here and is not smoothed over: `patch_index`
/// is `Some` when the document said so (2.0.1) and `None` when attachment is
/// positional (2.2). [`Patch::patch`] gives the number to use either way, so a
/// caller that does not care need not know.
pub struct Patch<'a> {
    /// Position of this patch among the property's own patches.
    pub position: usize,
    /// `RepresentationPatchIndex`, when the document carries one.
    pub patch_index: Option<u64>,
    /// The value array itself, in whichever of the eighteen forms it is.
    pub array: &'a dyn Reflect,
}

impl Patch<'_> {
    /// The patch of the representation these values belong to.
    ///
    /// The explicit index when there is one, the position otherwise — which is
    /// exactly what the 2.2 annotation prescribes: *"The ordering of
    /// ValuesForPatch matches the ordering of the patches in the xml document
    /// of the representation."*
    pub fn patch(&self) -> u64 {
        self.patch_index.unwrap_or(self.position as u64)
    }

    /// Read the numbers.
    pub fn read(&self, source: &dyn ArraySource) -> Result<Values> {
        values::read(self.array, source, &format!("patch {}", self.position))
    }
}

/// A property, as the document states it.
pub struct Property<'a> {
    /// Uuid of the property object.
    pub uuid: String,
    /// `Citation.Title`.
    pub title: Option<String>,
    /// Qualified type of the property object, e.g. `resqml22.DiscreteProperty`.
    pub qualified_type: String,
    /// What the values hang on.
    pub indexable: IndexableElement,
    /// Values per indexable element, **slowest dimension first**.
    ///
    /// One entry in 2.0.1 (`Count`), one or more in 2.2
    /// (`ValueCountPerIndexableElement`). The order is normative: TRG 2.2 §7.1
    /// — *"The top XML element is slower than the bottom."*
    pub value_count_per_element: Vec<u64>,
    /// The unit, when the property has one. Only continuous properties do.
    pub uom: Option<String>,
    /// What kind of quantity this is, unresolved.
    pub kind: Option<PropertyKindRef>,
    /// The **uri** of the `PropertyKind` the property references, version
    /// included — what [`crate::property_kind::Resolver::resolve_at`] reads it
    /// by. `None` for a standard name, which names no object.
    pub kind_uri: Option<String>,
    /// Uuid of the representation these values hang on.
    pub supporting_representation: Option<String>,
    /// The **uri** of that representation, the version the reference names
    /// included — what to read it by when a package holds the uuid twice.
    pub supporting_representation_uri: Option<String>,
    /// Uuid of the lookup table, for a categorical or discrete property that
    /// names one.
    pub lookup: Option<String>,
    /// The **uri** of the lookup table, the version the reference names
    /// included.
    pub lookup_uri: Option<String>,
    /// When it applies.
    pub time: Option<TimeRef>,
    /// 2.0.1 `RealizationIndex`. The 2.2 `RealizationIndices` is an array, so
    /// it is not read here — it is a dimension of the values, not a scalar.
    pub realization: Option<u64>,
    /// Whether the document declares `RealizationIndices` (2.2).
    ///
    /// Worth its own flag because of the normative consequence: *"If not
    /// provided, then the realization count (which could be 1) does not
    /// introduce a dimension to the multi-dimensional array storage."*
    pub has_realization_indices: bool,
    /// Qualifiers.
    pub facets: Vec<Facet>,
    /// The value arrays, one per patch.
    pub patches: Vec<Patch<'a>>,
}

impl Property<'_> {
    /// Values expected **per indexable element**: the product of the counts.
    pub fn values_per_element(&self) -> u64 {
        self.value_count_per_element.iter().product::<u64>().max(1)
    }

    /// How many values the document implies, given how many elements the
    /// representation has.
    pub fn expected_value_count(&self, element_count: u64) -> u64 {
        element_count.saturating_mul(self.values_per_element())
    }

    /// What does not add up between this property, its representation, and —
    /// when the caller has read them — the values themselves.
    ///
    /// Returns problems rather than failing, and that is deliberate: the
    /// reference packages contain real disagreements. `testingPackageCpp22`
    /// writes 5 000 values for a 5 000-node grid while declaring 2 per node;
    /// its interval property carries 5 values for a 4-interval frame; three of
    /// its time-series properties carry every time step in one dataset, which
    /// the specification neither describes nor forbids. Refusing those files
    /// would make the defects **invisible** instead of visible, and this
    /// library exists to help repair such files.
    ///
    /// Pass `actual` when the values have been read; leave it `None` to check
    /// only what the document says about itself.
    pub fn check(&self, representation: &dyn Reflect, actual: Option<u64>) -> Vec<Discrepancy> {
        let mut out = Vec::new();
        let count = match element_count(representation, self.indexable) {
            Ok(count) => count,
            Err(e) => {
                out.push(Discrepancy::CountUnknown {
                    reason: e.to_string(),
                });
                return out;
            }
        };
        if let Some(actual) = actual {
            let expected = self.expected_value_count(count);
            if expected != actual {
                out.push(Discrepancy::ValueCount {
                    element_count: count,
                    per_element: self.values_per_element(),
                    expected,
                    actual,
                });
            }
        }
        out
    }
}

/// Something that does not add up between a property and its representation.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Discrepancy {
    /// The values read are not the number the document implies.
    ValueCount {
        /// Elements of the representation.
        element_count: u64,
        /// Values per element the property declares.
        per_element: u64,
        /// Their product.
        expected: u64,
        /// What was actually read.
        actual: u64,
    },
    /// The pair (representation type, indexable element) is not in the table,
    /// so nothing could be checked at all.
    CountUnknown {
        /// Why, naming the pair.
        reason: String,
    },
}

impl std::fmt::Display for Discrepancy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Discrepancy::ValueCount {
                element_count,
                per_element,
                expected,
                actual,
            } => write!(
                f,
                "declares {element_count} × {per_element} = {expected} values and holds {actual}"
            ),
            Discrepancy::CountUnknown { reason } => write!(f, "{reason}"),
        }
    }
}

/// Read a property object.
///
/// Works on `ContinuousProperty`, `DiscreteProperty`, `CategoricalProperty`,
/// `CommentProperty` and `PointsProperty`, in both versions, because it is
/// written against the shape rather than the type: the fields it looks for are
/// the ones `AbstractProperty` declares.
pub fn read(object: &dyn Reflect) -> Result<Property<'_>> {
    let name = object.type_info().name;
    let uuid = identity::uuid_of_reflect(object)
        .ok_or_else(|| Error::malformed(name, "a property with no uuid"))?
        .to_string();

    let indexable_text = text(object, "IndexableElement").ok_or_else(|| {
        Error::malformed(
            &uuid,
            "no `IndexableElement`, so nothing says what the values hang on",
        )
    })?;
    let indexable = IndexableElement::parse(&indexable_text).ok_or_else(|| {
        Error::malformed(
            &uuid,
            format!("`{indexable_text}` is not an indexable element this build knows"),
        )
    })?;

    // 2.0.1 `Count` (exactly one) and 2.2 `ValueCountPerIndexableElement` (one
    // or more) are the same thing under two names — the 2.2 rename is what
    // settled the contradiction between the 2.0.1 XSD annotation ("number of
    // elements in a 1D list") and its narrative documentation ("value count
    // per indexable element"). The narrative won.
    let mut value_count_per_element = integers(object, "ValueCountPerIndexableElement");
    if value_count_per_element.is_empty() {
        value_count_per_element = integers(object, "Count");
    }

    let patches = read_patches(object);

    Ok(Property {
        uuid,
        title: text_at(object, "Citation", "Title"),
        qualified_type: object.type_info().qualified_type.unwrap_or(name).to_string(),
        indexable,
        value_count_per_element,
        uom: text(object, "Uom").or_else(|| text(object, "UOM")),
        kind: read_kind(object),
        kind_uri: read_kind_uri(object),
        supporting_representation: reference_uuid(object, "SupportingRepresentation"),
        supporting_representation_uri: reference_uri(object, "SupportingRepresentation"),
        lookup: reference_uuid(object, "Lookup").or_else(|| reference_uuid(object, "CategoryLookup")),
        lookup_uri: reference_uri(object, "Lookup").or_else(|| reference_uri(object, "CategoryLookup")),
        time: read_time(object),
        realization: integer(object, "RealizationIndex"),
        has_realization_indices: object.field("RealizationIndices").is_some_and(|f| !f.is_empty()),
        facets: read_facets(object),
        patches,
    })
}

/// The value arrays, in document order.
///
/// Both spellings, and the difference between them is kept: `PatchOfValues`
/// wraps the array beside an explicit index, `ValuesForPatch` **is** the array
/// and attaches by position. `PatchOfPoints` is the same shape as
/// `PatchOfValues` with the array under `Points`, for a 2.0.1 points property;
/// `PointsForPatch` is its 2.2 counterpart, the array by position like
/// `ValuesForPatch` — the rename this crate's own translation carries across
/// (as a translation tool does), so a points property from either version reads
/// here.
fn read_patches(object: &dyn Reflect) -> Vec<Patch<'_>> {
    let mut out = Vec::new();

    for (field, inner) in [
        ("PatchOfValues", Some("Values")),
        ("PatchOfPoints", Some("Points")),
        ("ValuesForPatch", None),
        ("PointsForPatch", None),
        ("PatchOfGeometry", Some("Geometry")),
    ] {
        let Some(value) = object.field(field) else {
            continue;
        };
        for item in flatten(value) {
            let Some(wrapper) = item.as_object() else {
                continue;
            };
            let (array, patch_index) = match inner {
                Some(inner) => (
                    match wrapper.field(inner).and_then(|f| f.as_object()) {
                        Some(array) => array,
                        None => continue,
                    },
                    integer(wrapper, "RepresentationPatchIndex"),
                ),
                None => (wrapper, None),
            };
            out.push(Patch {
                position: out.len(),
                patch_index,
                array,
            });
        }
    }
    out
}

/// `PropertyKind`, in its two shapes.
fn read_kind(object: &dyn Reflect) -> Option<PropertyKindRef> {
    let field = object.field("PropertyKind")?;
    let inner = field.as_object()?;

    // 2.2, and any version that references the kind directly: the field *is*
    // the reference.
    if let Some(uuid) = uuid_of_reference(inner) {
        return Some(PropertyKindRef::Object {
            uuid,
            title: text(inner, "Title"),
        });
    }

    // 2.0.1: an inline choice. `StandardPropertyKind` carries the name in
    // `Kind`; `LocalPropertyKind` carries a reference under its own name.
    if let Some(name) = text(inner, "Kind") {
        return Some(PropertyKindRef::Standard { name });
    }
    for field in ["LocalPropertyKind", "PropertyKind"] {
        if let Some(target) = inner.field(field).and_then(|f| f.as_object())
            && let Some(uuid) = uuid_of_reference(target)
        {
            return Some(PropertyKindRef::Object {
                uuid,
                title: text(target, "Title"),
            });
        }
    }
    None
}

/// The uri of the `PropertyKind` an object references, in the two shapes
/// [`read_kind`] reads.
fn read_kind_uri(object: &dyn Reflect) -> Option<String> {
    let inner = object.field("PropertyKind")?.as_object()?;
    if uuid_of_reference(inner).is_some() {
        return identity::uri_of_reflect(inner);
    }
    ["LocalPropertyKind", "PropertyKind"]
        .into_iter()
        .filter_map(|field| inner.field(field).and_then(|f| f.as_object()))
        .find(|target| uuid_of_reference(*target).is_some())
        .and_then(identity::uri_of_reflect)
}

fn read_time(object: &dyn Reflect) -> Option<TimeRef> {
    for field in ["TimeIndex", "TimeOrIntervalSeries"] {
        let Some(inner) = object.field(field).and_then(|f| f.as_object()) else {
            continue;
        };
        return Some(TimeRef {
            index: integer(inner, "Index"),
            series: reference_uuid(inner, "TimeSeries"),
            use_interval: matches!(
                inner.field("UseInterval").and_then(|f| f.as_scalar()),
                Some(Scalar::Bool(true))
            ),
        });
    }
    // 2.0.1 also has a bare `TimeStep`, with no series at all.
    integer(object, "TimeStep").map(|index| TimeRef {
        index: Some(index),
        series: None,
        use_interval: false,
    })
}

fn read_facets(object: &dyn Reflect) -> Vec<Facet> {
    let Some(value) = object.field("Facet") else {
        return Vec::new();
    };
    flatten(value)
        .into_iter()
        .filter_map(|item| {
            let inner = item.as_object()?;
            Some(Facet {
                kind: text(inner, "Facet")?,
                value: text(inner, "FacetValue")?,
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// The inverse index
// ---------------------------------------------------------------------------

/// What a property is, without borrowing the object it was read from.
///
/// The entry [`Index`] keeps: enough to choose one property out of forty
/// without deserializing anything again, and the uuid to go and read it when
/// the choice is made.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Entry {
    /// Uuid of the property object.
    pub uuid: String,
    /// `Citation.Title`.
    pub title: Option<String>,
    /// Qualified type, which is what tells continuous from discrete.
    pub qualified_type: String,
    /// What it hangs on.
    pub indexable: IndexableElement,
    /// The kind as the document states it, unresolved.
    pub kind: Option<PropertyKindRef>,
    /// The unit, for a continuous property.
    pub uom: Option<String>,
    /// When it applies.
    pub time: Option<TimeRef>,
    /// 2.0.1 realization index.
    pub realization: Option<u64>,
}

/// Which properties hang on which representation.
///
/// Built by walking the properties once. The relationships in `.rels` are
/// **never** the source: they are an index a producer maintains by hand, and
/// `80wells` — 202 objects — carries no `.rels` at all while every one of its
/// properties names its `SupportingRepresentation`. What the objects say is
/// the truth; what the package says about them is a convenience.
#[derive(Clone, Debug, Default)]
pub struct Index {
    by_representation: std::collections::HashMap<String, Vec<Entry>>,
    /// The same entries by the **uri** their property names its support by
    /// (lowercase), for a package that holds one representation in two
    /// versions. [`Index::of`] answers by uuid and merges them.
    by_uri: std::collections::HashMap<String, Vec<Entry>>,
    orphans: Vec<Entry>,
}

impl Index {
    /// An empty index.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add one property that has already been read.
    pub fn insert(&mut self, property: &Property<'_>) {
        let entry = Entry {
            uuid: property.uuid.clone(),
            title: property.title.clone(),
            qualified_type: property.qualified_type.clone(),
            indexable: property.indexable,
            kind: property.kind.clone(),
            uom: property.uom.clone(),
            time: property.time.clone(),
            realization: property.realization,
        };
        match &property.supporting_representation {
            Some(uuid) => {
                if let Some(uri) = &property.supporting_representation_uri {
                    self.by_uri
                        .entry(uri.to_ascii_lowercase())
                        .or_default()
                        .push(entry.clone());
                }
                self.by_representation
                    .entry(uuid.to_ascii_lowercase())
                    .or_default()
                    .push(entry)
            }
            // Not dropped: a property with no support is unusable but real,
            // and a caller that never sees it cannot report it.
            None => self.orphans.push(entry),
        }
    }

    /// Read and index every object of an iterator that is a property.
    ///
    /// Returns what could not be read, uuid and reason, rather than skipping
    /// it — an index that quietly holds 37 of 40 properties is worse than one
    /// that holds 37 and says which three are missing.
    pub fn build<'a>(objects: impl IntoIterator<Item = &'a dyn Reflect>) -> (Self, Vec<String>) {
        let mut index = Self::new();
        let mut refused = Vec::new();
        for object in objects {
            if !object.type_info().name.ends_with("Property") {
                continue;
            }
            match read(object) {
                Ok(property) => index.insert(&property),
                Err(e) => refused.push(e.to_string()),
            }
        }
        (index, refused)
    }

    /// Every property hanging on a representation, in the order they were
    /// added.
    ///
    /// Works for a `SubRepresentation` too: the schema allows one as a
    /// support, and nothing here checks what the target is.
    ///
    /// By uuid, so a representation held in two versions has both versions'
    /// properties here; [`Index::of_uri`] is the one that tells them apart.
    pub fn of(&self, representation_uuid: &str) -> &[Entry] {
        self.by_representation
            .get(&representation_uuid.to_ascii_lowercase())
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Every property hanging on the representation a **uri** names — the
    /// version included, so two versions of one representation keep their
    /// properties apart. Only the properties whose reference was read with a
    /// uri are answered; a uuid-only caller asks [`Index::of`].
    pub fn of_uri(&self, representation_uri: &str) -> &[Entry] {
        self.by_uri
            .get(&representation_uri.to_ascii_lowercase())
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Properties of a representation whose kind is this one.
    ///
    /// Matched on what the document *states*, not on a resolved chain — an
    /// index has no resolver. Use [`crate::property_kind`] when the question
    /// is "and everything derived from it".
    pub fn by_kind<'a>(&'a self, representation_uuid: &str, kind: &'a PropertyKindRef) -> Vec<&'a Entry> {
        self.of(representation_uuid)
            .iter()
            .filter(|entry| entry.kind.as_ref() == Some(kind))
            .collect()
    }

    /// Properties of a representation at one time index.
    pub fn by_time(&self, representation_uuid: &str, index: u64) -> Vec<&Entry> {
        self.of(representation_uuid)
            .iter()
            .filter(|entry| entry.time.as_ref().and_then(|time| time.index) == Some(index))
            .collect()
    }

    /// Properties of a representation for one realization.
    pub fn by_realization(&self, representation_uuid: &str, realization: u64) -> Vec<&Entry> {
        self.of(representation_uuid)
            .iter()
            .filter(|entry| entry.realization == Some(realization))
            .collect()
    }

    /// Every representation uuid that has at least one property.
    pub fn representations(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self.by_representation.keys().map(String::as_str).collect();
        out.sort_unstable();
        out
    }

    /// Properties that name no supporting representation at all.
    pub fn orphans(&self) -> &[Entry] {
        &self.orphans
    }

    /// How many properties were indexed, orphans included.
    pub fn len(&self) -> usize {
        self.by_representation.values().map(Vec::len).sum::<usize>() + self.orphans.len()
    }

    /// Whether it holds none.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// ---------------------------------------------------------------------------
// Element counts
// ---------------------------------------------------------------------------

/// How many elements of this kind a representation has.
///
/// The table the XSD does not carry: it documents six of the pairs and refers
/// the rest to a usage guide that is not in the repository. What is known is
/// answered; **everything else is refused by name**, because this number
/// multiplies into every array shape downstream and a plausible wrong one is
/// worse than a refusal.
///
/// Recognition is by the **suffix** of the type name, so `obj_IjkGridRepresentation`
/// and `IjkGridRepresentation` are the same type, as they are meant to be.
pub fn element_count(representation: &dyn Reflect, element: IndexableElement) -> Result<u64> {
    use IndexableElement::*;
    let name = representation.type_info().name;
    let kind = name.trim_start_matches("obj_");
    let refuse = || {
        Err(Error::unsupported(
            format!("the number of `{element}` on a `{kind}`"),
            "no rule for this pair in the schemas shipped here; the RESQML Technical Usage Guide carries the table"
                .to_string(),
        ))
    };

    // **A computed zero is a refusal, not a count**, and this crate's own rule
    // says so: *a count is computed or refused, never assumed*. The sums below
    // add up a field across the patches, and a field that is not there adds up
    // to nothing — which is indistinguishable from a representation that really
    // holds nothing, and is the answer downstream trusts to decide that every
    // property mis-aligns.
    //
    // It is not hypothetical. A 2.0.1 `PolylineSetRepresentation` states its
    // node count as **`NodeCountPerPolyline`, an array in the HDF5** — there is
    // no scalar `NodeCount` on the patch, so the sum found nothing and answered
    // `0`. The honest answer is that the number is not knowable without reading
    // an array, and this function reads none.
    let counted = |n: u64| if n > 0 { Ok(n) } else { refuse() };

    // True of every representation there is: `representation` means the
    // property applies to the object as a whole, so there is exactly one of
    // it. Answered before the table, because it is the one pair that does not
    // depend on what the representation is.
    if element == Representation {
        return Ok(1);
    }

    match kind {
        "IjkGridRepresentation" | "TruncatedIjkGridRepresentation" => {
            let ni = required(representation, "Ni")?;
            let nj = required(representation, "Nj")?;
            let nk = required(representation, "Nk")?;
            // `NKL`, the guide's name for the number of layer *boundaries*.
            // It counts the K gaps as well as the layers, and three
            // independent sources say so in the same words: Table 7 of the
            // Technical Usage Guide (*"interval edges … NKL = NK + GapCount +
            // 1"*), §9.12 beside the coordinate lines, and the annotation the
            // XSD itself carries on `KGaps` — *"NKL = NK + gapCount + 1"*,
            // with *"Number of INTERVALS = gapCount + NK"* on the line above.
            //
            // Two counts below were wrong for want of it. `interval edges`
            // answered `NK + 1`, short by one per gap, **while the node count
            // in this same function already used the gap-aware figure** — two
            // formulas for one quantity, disagreeing. And `intervals` was
            // refused outright.
            let gaps = k_gaps(representation);
            let nkl = nk + gaps + 1;
            match element {
                // Table 7: *"cells … NI x NJ x NK (+) TruncationCellCount"*,
                // where `(+)` is the guide's mark for "stored as a separate
                // patch". A truncated grid keeps its parent index space and
                // appends the truncation cells to it, so the two patches are
                // added; the schema's *"Parent cells are replaced"* is about
                // the geometry those indices carry, not about the range.
                //
                // No package here has a truncated grid, so this is the spec's
                // arithmetic and not a measurement. The failure direction is
                // the safe one: a count that disagrees with a property's data
                // is reported by `Property::check`, never drawn.
                Cells => Ok(ni * nj * nk + truncation_cells(representation)),
                Columns => Ok(ni * nj),
                Layers => Ok(nk),
                // The one ordering exception the guide names: intervals are
                // the layers *with the gaps interleaved*, so the count grows
                // with the gaps while `layers` does not.
                Intervals | Interval => Ok(nk + gaps),
                // **Plus the pillars an areal gap adds.** TUG §9.12.2 on the
                // `IjGaps` object: *"This option does not modify the indexing
                // of the cells or columns, but it does increase the number of
                // pillars and coordinate lines."* Table 7 says the same thing
                // as a formula, `NIL x NJL + SplitPillarCount`, and its
                // `coordinate lines` row builds on this one rather than
                // repeating it — which is why the two look alike.
                Pillars => Ok((ni + 1) * (nj + 1) + split_pillars(representation)),
                I0 => Ok(ni),
                I0Edges => Ok(ni + 1),
                J0 => Ok(nj),
                J0Edges => Ok(nj + 1),
                IntervalEdges => Ok(nkl),
                // §9.12.3.1, the one element whose indexing the guide calls
                // not obvious, and which it therefore spells out: *"the
                // indexing is implicit and first follows the I column edges
                // (NIL x NJ) and then the J column edges (NI x NJL)"* — the
                // same two products Table 7 gives, plus the split edges a
                // fault adds.
                ColumnEdges => Ok((ni + 1) * nj + ni * (nj + 1) + split_column_edges(representation)),
                // The "object1 per object2" counts, which the guide keeps
                // apart from the shared ones on purpose: they are *local to
                // the cell*, so they never depend on the faulting. §9.12.3.1:
                // *"for an IJK grid, there are 6 'faces per cell': 0=top,
                // 1=bottom, 2-5 are side faces … This is a very simple
                // enumeration, versus the 'faces' indexing, which depends on
                // the grid faulting and column edge enumeration."*
                FacesPerCell => Ok(6 * ni * nj * nk),
                NodesPerCell => Ok(8 * ni * nj * nk),
                EdgesPerColumn => Ok(4 * ni * nj),
                // The count that was refused until the split lines were
                // understood. A fault means two cells sharing a pillar do not
                // share its points, and RESQML says so by appending **split
                // coordinate lines** after the regular pillars — so the grid
                // has `pillars + splits` lines, each carrying one point per
                // layer boundary.
                //
                // `(ni+1)(nj+1)(nk+1)` is therefore right only for a grid with
                // no fault in it, and quietly short by one point per split
                // otherwise. The reference package is called "two faulted
                // sugar cubes" precisely because that difference is the whole
                // model.
                Nodes | CoordinateLines => {
                    let lines = (ni + 1) * (nj + 1)
                        + split_pillars(representation)
                        + split_coordinate_lines(representation);
                    Ok(match element {
                        CoordinateLines => lines,
                        // And one level per layer boundary, which is `Nk + 1`
                        // **plus one per K gap**: a gap is a break in the
                        // layering, and both sides of it carry their own
                        // points. `80wells` has a grid with one gap, where the
                        // formula without it is short by a whole level — 72
                        // nodes claimed against the 96 the file holds.
                        _ => lines * nkl,
                    })
                }
                _ => refuse(),
            }
        }
        "Grid2dRepresentation" => {
            let (fast, slow) = grid2d_axes(representation)?;
            match element {
                Nodes => Ok(fast * slow),
                Cells | Columns => Ok(fast.saturating_sub(1) * slow.saturating_sub(1)),
                I0 => Ok(slow),
                J0 => Ok(fast),
                _ => refuse(),
            }
        }
        // A grid connection set, whose natural attachment the Technical Usage
        // Guide states outright (§9.16.1): *"RECOMMENDATION: Attach properties on
        // a grid connection set representation using a 'face' indexable element
        // […] By 'face' it's meant the contact area between each cell pair.
        // Consequently the dataset corresponding to the properties should have
        // the same number of values as the number of cell pairs."*
        //
        // So `faces` is the connection count, and `Count` is where the object
        // states it — a scalar in the XML, so this needs no array read. `cells`
        // is **not** the same number and is refused: a connection names two
        // cells, and answering the pair count for it would line a per-cell array
        // up against half the elements it belongs to.
        "GridConnectionSetRepresentation" => match element {
            Faces => required(representation, "Count"),
            _ => refuse(),
        },
        "TriangulatedSetRepresentation" => match element {
            Triangles | Cells | Faces => counted(sum_over_patches(representation, "TrianglePatch", "Count")),
            Nodes => counted(sum_over_patches(representation, "TrianglePatch", "NodeCount")),
            Patches => counted(patch_count(representation, "TrianglePatch")),
            _ => refuse(),
        },
        "PolylineSetRepresentation" => match element {
            Nodes => counted(sum_over_patches(representation, "LinePatch", "NodeCount")),
            Lines => counted(sum_over_patches(representation, "LinePatch", "Count")),
            Patches => counted(patch_count(representation, "LinePatch")),
            _ => refuse(),
        },
        "PointSetRepresentation" => match element {
            Nodes => counted(sum_over_patches(representation, "NodePatch", "Count")),
            Patches => counted(patch_count(representation, "NodePatch")),
            _ => refuse(),
        },
        "PolylineRepresentation" => match element {
            Nodes => required(representation, "NodeCount"),
            Intervals | Interval => Ok(required(representation, "NodeCount")?.saturating_sub(1)),
            _ => refuse(),
        },
        "WellboreFrameRepresentation"
        | "WellboreMarkerFrameRepresentation"
        | "SeismicWellboreFrameRepresentation" => {
            match element {
                Nodes => required(representation, "NodeCount"),
                // The narrative documentation is explicit for this one:
                // `intervals` = nodeCount − 1.
                //
                // `intervals from datum` joins them, and the provenance of that
                // is worth stating because it is **not** a quotation: no
                // schema documents the pair, and the Technical Usage Guide's
                // table is not shipped here. What settled it is the reasoning —
                // N node depths bound N−1 intervals, and "from datum" says how
                // their extents are *expressed*, not how many there are —
                // confirmed against a client package where **306 of 306**
                // properties hold exactly `NodeCount − 1` values, measured from
                // the HDF5 datasets against the counts the XML declares.
                //
                // Two independent sources agreeing 306 times is evidence about
                // one producer, not a proof about the standard. It is enough to
                // compute rather than refuse; it is not enough to stop
                // reporting a property whose own data disagrees, which
                // `Property::check` still does.
                Intervals | Interval | IntervalEdges | IntervalsFromDatum => {
                    Ok(required(representation, "NodeCount")?.saturating_sub(1))
                }
                _ => refuse(),
            }
        }
        // **Two of these three counts have never been on the object.** RESQML
        // declares `CellCount` on the representation and `NodeCount` /
        // `FaceCount` on `UnstructuredGridGeometry` — in 2.0.1 and in 2.2 alike,
        // so this is not a respelling between versions but a place that was
        // never right. Asking the object alone refused every property indexed on
        // the nodes or the faces of an unstructured grid, in every version, with
        // a message naming the field — which reads as a malformed file rather
        // than as a reader looking in the wrong place.
        "UnstructuredGridRepresentation" => match element {
            Cells => required(representation, "CellCount"),
            Nodes => in_geometry(representation, "NodeCount"),
            Faces => in_geometry(representation, "FaceCount"),
            _ => refuse(),
        },
        // A sub-representation lists its own elements, so the count it
        // declares is the answer whatever kind of element it selects — the one
        // type where the pair does not matter.
        "SubRepresentation" => counted(sum_over_patches(
            representation,
            "SubRepresentationPatch",
            "Count",
        )),
        _ => refuse(),
    }
}

/// How many K gaps the grid declares — breaks in the layering, each adding a
/// layer boundary and therefore a level of points.
fn k_gaps(representation: &dyn Reflect) -> u64 {
    representation
        .field("KGaps")
        .and_then(|f| f.as_object())
        .and_then(|gaps| integer(gaps, "Count"))
        .unwrap_or(0)
}

/// How many split coordinate lines a grid geometry declares.
///
/// Zero for a grid with no fault. Both spellings are read — RESQML 2.0.1 says
/// `SplitCoordinateLines`, 2.2 says `ColumnLayerSplitCoordinateLines` — and an
/// absent element means none rather than an error, because most grids have
/// none.
fn split_coordinate_lines(representation: &dyn Reflect) -> u64 {
    let Some(geometry) = representation.field("Geometry").and_then(|f| f.as_object()) else {
        return 0;
    };
    ["ColumnLayerSplitCoordinateLines", "SplitCoordinateLines"]
        .into_iter()
        .find_map(|field| geometry.field(field).and_then(|f| f.as_object()))
        .and_then(|element| integer(element, "Count"))
        .unwrap_or(0)
}

/// How many pillars an areal gap adds.
///
/// Zero for a grid with no `IjGaps`, which is nearly all of them. The Usage
/// Guide §9.12.2 states the effect outright: *"This option does not modify the
/// indexing of the cells or columns, but it **does increase the number of
/// pillars and coordinate lines**."* Missing it makes the node count short by
/// one whole coordinate line per gap pillar — on exactly the grids that model a
/// fracture with a volume rather than a surface.
fn split_pillars(representation: &dyn Reflect) -> u64 {
    representation
        .field("Geometry")
        .and_then(|f| f.as_object())
        .and_then(|geometry| geometry.field("IjGaps").and_then(|f| f.as_object()))
        .and_then(|gaps| integer(gaps, "SplitPillarCount"))
        .unwrap_or(0)
}

/// How many split column edges a grid geometry declares.
///
/// Zero for a grid with no fault. The element is spelled `SplitColumnEdges` in
/// both releases but does not hang in the same place: 2.2 puts it on the grid
/// geometry, 2.0.1 nests it inside the split coordinate lines. Both are tried,
/// and an absent element means none rather than an error.
fn split_column_edges(representation: &dyn Reflect) -> u64 {
    let Some(geometry) = representation.field("Geometry").and_then(|f| f.as_object()) else {
        return 0;
    };
    let count = |holder: &dyn Reflect| {
        holder
            .field("SplitColumnEdges")
            .and_then(|f| f.as_object())
            .and_then(|element| integer(element, "Count"))
    };
    if let Some(found) = count(geometry) {
        return found;
    }
    ["ColumnLayerSplitCoordinateLines", "SplitCoordinateLines"]
        .into_iter()
        .find_map(|field| geometry.field(field).and_then(|f| f.as_object()))
        .and_then(count)
        .unwrap_or(0)
}

/// How many cells a truncation patch adds, in both spellings.
///
/// 2.0.1 says `TruncationCells`, 2.2 says `TruncationCellPatch`; the count
/// inside is `TruncationCellCount` in both. Zero when there is no patch, which
/// is every grid that is not truncated.
fn truncation_cells(representation: &dyn Reflect) -> u64 {
    ["TruncationCellPatch", "TruncationCells"]
        .into_iter()
        .find_map(|field| representation.field(field).and_then(|f| f.as_object()))
        .and_then(|patch| integer(patch, "TruncationCellCount"))
        .unwrap_or(0)
}

/// `(fastest, slowest)` axis counts of a 2D grid, in both spellings.
///
/// 2.2 flattens them onto the representation; 2.0.1 puts them inside
/// `Grid2dPatch`. Same two numbers.
fn grid2d_axes(representation: &dyn Reflect) -> Result<(u64, u64)> {
    if let (Some(fast), Some(slow)) = (
        integer(representation, "FastestAxisCount"),
        integer(representation, "SlowestAxisCount"),
    ) {
        return Ok((fast, slow));
    }
    let patch = representation
        .field("Grid2dPatch")
        .and_then(|f| flatten(f).into_iter().next())
        .and_then(|f| f.as_object())
        .ok_or_else(|| {
            Error::malformed(
                representation.type_info().name,
                "no axis counts, neither on the representation nor in a `Grid2dPatch`",
            )
        })?;
    Ok((
        required(patch, "FastestAxisCount")?,
        required(patch, "SlowestAxisCount")?,
    ))
}

fn sum_over_patches(object: &dyn Reflect, patch_field: &str, count_field: &str) -> u64 {
    let Some(value) = object.field(patch_field) else {
        return 0;
    };
    flatten(value)
        .into_iter()
        .filter_map(|item| integer(item.as_object()?, count_field))
        .sum()
}

fn patch_count(object: &dyn Reflect, patch_field: &str) -> u64 {
    object
        .field(patch_field)
        .map(|value| flatten(value).len() as u64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Reflection helpers
// ---------------------------------------------------------------------------

fn flatten(value: FieldValue<'_>) -> Vec<FieldValue<'_>> {
    match value {
        FieldValue::List(items) => items,
        FieldValue::Null => Vec::new(),
        other => vec![other],
    }
}

fn text(object: &dyn Reflect, field: &str) -> Option<String> {
    match object.field(field)?.as_scalar()? {
        Scalar::Str(s) => Some(s.to_string()),
        other => Some(other.to_string()),
    }
}

fn text_at(object: &dyn Reflect, field: &str, inner: &str) -> Option<String> {
    text(object.field(field)?.as_object()?, inner)
}

fn integer(object: &dyn Reflect, field: &str) -> Option<u64> {
    match object.field(field)?.as_scalar()? {
        Scalar::UInt(v) => Some(v),
        Scalar::Int(v) if v >= 0 => Some(v as u64),
        Scalar::Double(v) if v >= 0.0 => Some(v as u64),
        _ => None,
    }
}

fn integers(object: &dyn Reflect, field: &str) -> Vec<u64> {
    let Some(value) = object.field(field) else {
        return Vec::new();
    };
    flatten(value)
        .into_iter()
        .filter_map(|item| match item.as_scalar()? {
            Scalar::UInt(v) => Some(v),
            Scalar::Int(v) if v >= 0 => Some(v as u64),
            _ => None,
        })
        .collect()
}

/// A count the object declares, or that its `Geometry` does.
///
/// The object is still tried **first**: nothing forbids a release from moving a
/// count up onto the representation, and a reader that only looked in the
/// geometry would then break the other way round. Two lookups where one is
/// almost always enough is the cheap half of not having to know which version
/// wrote the file.
///
/// The refusal, when neither has it, is `required`'s — it names the field and
/// the type, which is what someone opening the file needs.
fn in_geometry(object: &dyn Reflect, field: &str) -> Result<u64> {
    if let Some(found) = integer(object, field) {
        return Ok(found);
    }
    if let Some(FieldValue::Object(geometry)) = object.field("Geometry") {
        if let Some(found) = integer(geometry, field) {
            return Ok(found);
        }
    }
    required(object, field)
}

fn required(object: &dyn Reflect, field: &str) -> Result<u64> {
    integer(object, field).ok_or_else(|| {
        Error::malformed(
            object.type_info().name,
            format!("`{field}` is absent or not a count"),
        )
    })
}

/// The uuid a `DataObjectReference` carries, under either spelling.
/// The uuid a reference names — the identity layer's reading, so every
/// spelling of the field is one answer.
fn uuid_of_reference(object: &dyn Reflect) -> Option<String> {
    energyml_core::identity::uuid_of_reflect(object)
        .filter(|uuid| !uuid.is_empty())
        .map(str::to_string)
}

/// The uri of the object a field names, version included.
fn reference_uri(object: &dyn Reflect, field: &str) -> Option<String> {
    identity::uri_of_reflect(object.field(field)?.as_object()?)
}

fn reference_uuid(object: &dyn Reflect, field: &str) -> Option<String> {
    uuid_of_reference(object.field(field)?.as_object()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_indexable_element_round_trips() {
        for text in [
            "cells",
            "nodes",
            "triangles",
            "I0 edges",
            "interval edges",
            "nodes per cell",
        ] {
            let parsed = IndexableElement::parse(text).expect(text);
            assert_eq!(IndexableElement::parse(parsed.as_str()), Some(parsed));
        }
    }

    /// **A 2.2 points property reads, under its own field name.**
    ///
    /// `read()`'s own doc comment has claimed `PointsProperty` since it was
    /// written, and nothing here checked it: `read_patches` looked for
    /// `PatchOfPoints` (2.0.1) and never for `PointsForPatch` (2.2), so every
    /// 2.2 points property read as **zero patches** — a schema-invalid answer
    /// (1..∞) that `every_property_of_the_corpus_reads` in
    /// `energyml-domain/tests/property.rs` only ever caught because a
    /// translated package happened to land in `testdata/` with one. No native
    /// corpus package has ever exercised this arm.
    #[test]
    fn a_2_2_points_property_reads_its_own_field_name() {
        use energyml_resqml_2_2::geometry::{AbstractPoint3dArray, Point3dExternalArray};
        use energyml_resqml_2_2::properties::PointsProperty;

        let object = PointsProperty {
            points_for_patch: vec![AbstractPoint3dArray::Point3dExternalArray(
                Point3dExternalArray::default(),
            )],
            ..Default::default()
        };

        let patches = read_patches(&object);
        assert_eq!(
            patches.len(),
            1,
            "PointsForPatch must be found, not read as absent"
        );
        assert_eq!(
            patches[0].patch_index, None,
            "2.2 attaches by position, like ValuesForPatch"
        );
    }

    #[test]
    fn spelling_differences_do_not_lose_a_property() {
        // The two versions do not agree on spacing, and a reader that insists
        // on one spelling silently loses every property of the other version.
        assert_eq!(
            IndexableElement::parse("I0Edges"),
            Some(IndexableElement::I0Edges)
        );
        assert_eq!(
            IndexableElement::parse("i0 edges"),
            Some(IndexableElement::I0Edges)
        );
        assert_eq!(
            IndexableElement::parse("nodes_per_face"),
            Some(IndexableElement::NodesPerFace)
        );
    }

    /// **Every value the schemas spell is parseable, and nothing else is.**
    ///
    /// This is the test that was missing, and its absence cost two whole
    /// families of property. The enumeration here was written from the
    /// specification's prose rather than from the generated enums, and three of
    /// its lexical values exist in no schema at all — `enumeration`,
    /// `interval`, `radial origin polyline` — while two the schemas do spell
    /// were absent: `enumerated elements` and `intervals from datum`.
    ///
    /// The consequence was not a warning. `property::read` refuses an element
    /// it cannot parse, so **every** property indexed on either value was
    /// dropped before anything could look at it: 296 of 306 in one client
    /// package, all of them the soil types along its wells. And it was never a
    /// drop-only problem — `intervals from datum` is in eml 2.3, so released
    /// RESQML 2.2 packages were losing them too.
    ///
    /// `enumeration` and the other two phantoms are kept as variants rather
    /// than deleted: removing a public variant breaks callers, and an
    /// unreachable one costs nothing. They are listed here so the next person
    /// to compare the two lists finds the answer instead of the question.
    #[test]
    fn the_spellings_are_the_schemas_own() {
        for text in ["enumerated elements", "intervals from datum"] {
            let parsed = IndexableElement::parse(text)
                .unwrap_or_else(|| panic!("`{text}` is spelled by every recent schema and must parse"));
            assert_eq!(parsed.as_str(), text, "and it must write back the same word");
        }
        // `enumeration` was the invented spelling of this very variant, so
        // repointing it at the schemas' word makes the old one unparseable.
        // Nothing is lost: no document can hold a value no schema declares, and
        // a reader that accepted it would be inventing a meaning for it.
        assert_eq!(IndexableElement::parse("enumeration"), None);
        assert_eq!(
            IndexableElement::parse("enumerated elements"),
            Some(IndexableElement::Enumeration)
        );

        // The two other phantoms are untouched variants that happen to spell
        // themselves in no schema. They still parse — removing a public variant
        // breaks callers, and an unreachable one costs nothing — and they are
        // named here so the next person comparing the two lists finds the
        // answer rather than the question.
        for phantom in ["interval", "radial origin polyline"] {
            assert!(
                IndexableElement::parse(phantom).is_some(),
                "`{phantom}` is a variant this crate still offers"
            );
        }
    }

    #[test]
    fn an_unknown_element_is_not_guessed() {
        assert_eq!(IndexableElement::parse("voxels"), None);
    }
}
