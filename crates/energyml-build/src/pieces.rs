//! What an operation transforms — in a neutral currency, whatever it came from.
//!
//! A GeoJSON, an OBJ, an artefact of the 3D view and an object of the package
//! all become the same value here, so that an operation is written once and
//! never asks where its input came from. That is the whole point of the
//! design: *the same currency in both
//! directions* — [`energyml_geometry::geometry::Geometry`] is what the readers
//! produce and what the exporters consume, so it is what a creation takes.
//!
//! # No store in here
//!
//! An [`Object`](Source::Object) is *what the caller already knows* about an
//! object of the package — its reference, its CRS, its interpretation, the unit
//! of its vertical axis — resolved **before** the operation runs. An operation
//! never looks anything up: it decides on values, and the caller, who owns the
//! store, does the looking. This is what keeps `plan()` pure and testable
//! without a package, the rule every deciding module of the workspace follows.

use energyml_domain::color::Hsv;
use energyml_geometry::geometry::{Geometry, Patch, Topology};

use crate::geojson::Feature;

/// An object of the package, named — uuid, qualified type, title, and the
/// version it declares.
///
/// **The identity is the three first parts together** (internally an object is
/// always named by its full uri, ADR-0003): a package
/// may hold one uuid twice, under two `objectVersion`s, and a reference that
/// kept only the uuid would name them both. [`Ref::uri`] is the address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ref {
    /// Its uuid.
    pub uuid: String,
    /// `resqml22.TriangulatedSetRepresentation` — release included, because
    /// the release of a new object comes from what it attaches to.
    pub qualified_type: String,
    /// Its title, for a suggested name and for a message.
    pub title: String,
    /// Its `objectVersion` — or its citation's `VersionString` in eml 2.0 —
    /// when it declares one. `None` is *declares none*, never *unknown*: a
    /// blank version is no version and is never kept here.
    pub object_version: Option<String>,
}

impl Ref {
    /// A reference from its three parts, naming no version.
    pub fn new(uuid: impl Into<String>, qualified_type: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            uuid: uuid.into(),
            qualified_type: qualified_type.into(),
            title: title.into(),
            object_version: None,
        }
    }

    /// The same, with the version the object declares. A blank one is none.
    pub fn with_version(mut self, object_version: Option<String>) -> Self {
        self.object_version = object_version
            .map(|version| version.trim().to_string())
            .filter(|version| !version.is_empty());
        self
    }

    /// The reference to the object a store entry describes: its uuid, type and
    /// **version** — read from the summary's uri, which names the version
    /// whenever the entry declares one — and the title given.
    pub fn of_summary(summary: &energyml_storage::resource::ObjectSummary, title: impl Into<String>) -> Self {
        let version = energyml_core::identifiers::EtpUri::parse(&summary.uri)
            .ok()
            .and_then(|uri| uri.object_version);
        Self::new(summary.uuid.clone(), summary.qualified_type.clone(), title).with_version(version)
    }

    /// The reference an address names — the one reading of a uri
    /// (`EtpUri::parse`) — with the title given. `None` when `uri` is not the
    /// uri of an object.
    pub fn of_uri(uri: &str, title: impl Into<String>) -> Option<Self> {
        let parsed = energyml_core::identifiers::EtpUri::parse(uri).ok()?;
        let qualified_type = parsed.qualified_type()?.to_string();
        Some(Self::new(parsed.uuid?, qualified_type, title).with_version(parsed.object_version))
    }

    /// The reference an object (or a reference of any release) states of
    /// itself, through [`energyml_core::identity::ObjectId::of_reflect`]: uuid,
    /// type and version. `None` when it names no identity.
    pub fn of_reflect(object: &dyn energyml_core::value::Reflect, title: impl Into<String>) -> Option<Self> {
        let id = energyml_core::identity::ObjectId::of_reflect(object)?;
        Some(Self::new(id.uuid.clone(), id.qualified_type(), title).with_version(id.object_version.clone()))
    }

    /// **Its address**: the full uri, the version included when it has one
    /// (Identifier Specification v5.0 §3.4.2, p. 14). The only way a `Ref`
    /// becomes a uri — a uuid alone names every version of an object.
    pub fn uri(&self) -> String {
        energyml_core::identifiers::EtpUri::object_versioned(
            None,
            &self.qualified_type,
            &self.uuid.to_ascii_lowercase(),
            self.object_version.as_deref(),
        )
    }

    /// Whether `other` is the same object: same uuid (without case), same
    /// version. The type is not compared — a release apart is the same object
    /// (*a wrong version is not a wrong type*).
    pub fn same_object(&self, other: &Ref) -> bool {
        self.uuid.eq_ignore_ascii_case(&other.uuid) && self.object_version == other.object_version
    }

    /// The `eml23.DataObjectReference` this is, as JSON — with its
    /// `ObjectVersion` when it declares one.
    pub fn dor(&self) -> serde_json::Value {
        let mut dor = serde_json::json!({
            "$type": "eml23.DataObjectReference",
            "Uuid": self.uuid,
            "QualifiedType": self.qualified_type,
            "Title": self.title,
        });
        if let Some(version) = &self.object_version {
            dor["ObjectVersion"] = serde_json::Value::from(version.clone());
        }
        dor
    }
}

/// What the caller knows of an object of the package, so that an operation
/// can **infer** without reading.
///
/// The motivating case: contour lines know which
/// representation they came from, hence its CRS, the unit of its surface and
/// its interpretation — and those should be pre-filled, modifiable. Everything
/// here is optional: absent means *not known*, never *none*.
// `Eq` is not derived, only `PartialEq`: `wellhead` carries floats, and a NaN
// equals nothing, not even itself. The derive is therefore left out rather
// than lied about — nothing here puts a `Known` in a set.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Known {
    /// The object itself.
    pub reference: Option<Ref>,
    /// Its local CRS.
    pub crs: Option<Ref>,
    /// The EPSG code its projected half names, when it names one.
    ///
    /// When a `LocalEngineeringCompoundCrs` is chosen, the EPSG codes can be
    /// deduced — they are stored in these objects — so if they are given as input
    /// there is no reason to ask for codes — **unless the EPSG codes are not all
    /// filled in**. The reserve is
    /// the schema's own: the leaf is a `ProjectedEpsgCrs` — a code — or a
    /// `ProjectedLocalAuthorityCrs` / `ProjectedUnknownCrs` — none. `None` here
    /// is *the file does not name one*, and nothing is invented from it.
    pub crs_epsg: Option<i64>,
    /// The EPSG code its vertical half names, under the same reserve.
    pub vertical_epsg: Option<i64>,
    /// What it represents.
    pub interpretation: Option<Ref>,
    /// The unit of its vertical axis — `m`, `ft`, `ms`.
    pub vertical_uom: Option<String>,
    /// The vertical CRS its frame is made of — what a datum must reference so
    /// that a reader knows which way is down.
    pub vertical_crs: Option<Ref>,
    /// Whether its vertical axis points up (elevation) or down (depth).
    pub vertical_up: Option<bool>,
    /// **How its frame places a local number** — the system `crs` names, read
    /// by `energyml_domain::crs::read_with`: origin, azimuth, vertical axis and
    /// origin, units, time or length. What a pure operation needs to move
    /// numbers between frames (`change-frame`). `None` when the frame
    /// could not be read — **and when the shape beside it was read placed** (a
    /// plan for the scene): those numbers are local to no frame any more.
    pub placement: Option<energyml_domain::crs::Crs>,
    /// How many nodes its geometry places — what a field over `nodes` must
    /// match. Absent when nobody counted.
    pub node_count: Option<usize>,
    /// Where this object is **collared**, when it says so: the reference point
    /// its measured depths are counted from, **placed** — projected easting and
    /// northing and an elevation — since a datum written in a
    /// `LocalEngineeringCompoundCrs` is read through that compound.
    ///
    /// It is resolved at the boundary (`read::wellhead_of`) and not derived
    /// from the geometry, and that is the whole point: a trajectory
    /// starts at the **first stated depth**, so its first node is not the
    /// ground — reading the collar off the shape would answer thirty metres too
    /// low on a hole whose log starts deep. The datum is the only thing that
    /// states it.
    pub wellhead: Option<[f64; 3]>,
    /// The measured depths of its nodes, when it is a wellbore frame.
    ///
    /// They are **read from the package**, not derived from the geometry: a
    /// property laid along a frame is a positional array, and aligning it
    /// requires knowing *at which depth* each node is — not where it is in
    /// space. Two nodes of a deviated well may share an (x, y); their MD cannot.
    ///
    /// `None` when the object is not a frame, **or** when this build cannot
    /// read the array. Both are told by a refusal of the operation rather than
    /// by an empty array, which would read as *a frame with no node*.
    pub node_mds: Option<Vec<f64>>,
    /// The distinct values a category table holds, **in the order of its
    /// rows** — hence in the order of the codes.
    ///
    /// This is what is needed to **continue** a table rather than create a
    /// second one: a code is the rank of its row, so keeping the order is the
    /// only way not to make one integer say two things.
    ///
    /// `None` for any object that is not a table of this kind.
    pub categories: Option<Vec<String>>,
    /// The trajectory a frame names, and the datum that trajectory names —
    /// their **references**, resolved at the boundary (`read::well_refs_of`).
    ///
    /// This is what is needed to **continue** a well instead of writing a copy
    /// of it: a second trajectory must name the datum that is already there, and
    /// an enrichment must rewrite the trajectory that is already there. Deriving
    /// them would give uuids that nothing else in the package knows.
    ///
    /// `None` for what is not a frame — an answer, not a missing datum.
    pub trajectory: Option<Ref>,
    /// The datum of the trajectory above. See [`Known::trajectory`].
    pub datum: Option<Ref>,
    /// **The unit of each interval** of a frame, as its
    /// `IntervalStratigraphicUnits` name it (TRG 2.2 §5.34): the designated
    /// `StratigraphicUnitInterpretation`, or `None` for an interval the frame
    /// leaves without a unit.
    ///
    /// Resolved at the boundary (`read::interval_units_of`): the named rank is
    /// read and the index translated into a reference, so that an operation
    /// extracting tops and bases has no package to read. `None` as a whole when
    /// the frame carries no correspondence — an answer, not an empty array.
    pub interval_units: Option<Vec<Option<Ref>>>,
    /// What a **property** states, when this object is one — resolved at the
    /// boundary (`read::known_of_object`) so that an operation can carry its
    /// values onto another representation without reading a package.
    ///
    /// `None` for anything that is no property. A property whose values could
    /// not be read is still `Some`, with the reason in
    /// [`KnownProperty::values`]: *this build could not read it* and *this is
    /// no property* are two different news, and the second one is not a
    /// refusal.
    pub property: Option<KnownProperty>,
    /// **The package's ordered stratigraphy**, when the object is a
    /// stratigraphic unit or a column — its columns, ranks, units and the earth
    /// models already made of them, as `energyml_domain::stratigraphy::columns`
    /// reads them. What `earth-model-from-stratigraphy` computes a unit's
    /// subtree from without reading a package.
    ///
    /// `None` for anything else: the column of a horizon is no fact about it.
    pub stratigraphy: Option<energyml_domain::stratigraphy::Columns>,
    /// **The instants of a time series**, when the object is one — each
    /// `Time`'s `DateTime` as written, in the list's order, which is the order
    /// a time index counts in (*« The list ordering is used by the time
    /// index »*, Energistics common Technical Reference Guide v2.3 §11.14
    /// p. 297). Resolved at the boundary (`read::known_of_object`) so that an
    /// operation laying values through the series reads no package
    /// (`property-time-series`) — and never restated by hand beside
    /// it, which would be a second list of the same instants.
    ///
    /// `None` for anything that is no time series, **and** for one with an
    /// instant that states no date-time: a list with a hole in it would shift
    /// every index after the hole.
    pub instants: Option<Vec<String>>,
    /// **The object whole**, in the workspace's JSON encoding
    /// (`energyml_core::json::write_value`) — read at the boundary
    /// (`read::known_of_object`, `read::pieces_of_object`) for the one kind of
    /// operation that must see every field: one that rewrites the object as
    /// told (`edit-object`). Every other operation reads the facts
    /// above and never this: a writer that picked fields out of a whole object
    /// would be a second reader of them.
    ///
    /// `None` when the object was not read from a package — an artefact, a
    /// value built by hand.
    pub json: Option<serde_json::Value>,
    /// **The hole it runs along, evaluable at any measured depth**, when it is
    /// a wellbore trajectory or a frame (the frame's trajectory) — resolved at
    /// the boundary (`read::path_of`) by `energyml_geometry::well::path_of`, the
    /// reader that draws the well, so that an operation placing a depth
    /// *between* two stations (`computed-tops`) reads no package and
    /// evaluates no second line.
    ///
    /// `None` for anything that is no wellbore object; `Some` with the reason
    /// in [`KnownPath::path`] for a trajectory whose line cannot be evaluated —
    /// *this is no well* and *this well has no line to follow* are two news.
    pub path: Option<KnownPath>,
}

/// What a wellbore object states of the hole it runs along — see
/// [`Known::path`].
#[derive(Debug, Clone, PartialEq)]
pub struct KnownPath {
    /// The trajectory the measured depths are along — the object itself, or
    /// the one a frame names.
    pub trajectory: Ref,
    /// Its line, in the local numbers of its CRS — **or why there is none**.
    pub path: Result<energyml_geometry::parametric::Path, String>,
    /// The unit its measured depths count in — `MdInterval.Uom`.
    pub md_uom: Option<String>,
    /// The datum its measured depths count from — `MdInterval.Datum`.
    pub datum: Option<Ref>,
}

/// What a property of the package states — the facts an operation needs to
/// write **the same property** elsewhere, and nothing it would have to guess.
///
/// Needed by `contacts-from-frames`: the points extracted
/// from a wellbore frame should carry what the frame's properties hold at that
/// node. The operation is pure, so the values arrive here, read by the
/// boundary through `energyml_domain::property` and `energyml_domain::values`
/// — the one reader of both, never a second one.
///
/// **References, never copies**: the kind, the lookup table and the supporting
/// representation are the [`Ref`]s the property itself writes, so a carried
/// property names the same `PropertyKind` and the same `CategoryLookup` table,
/// and a code keeps its meaning.
#[derive(Debug, Clone, PartialEq)]
pub struct KnownProperty {
    /// The representation its values hang on — its `SupportingRepresentation`.
    pub supporting: Option<Ref>,
    /// What its values hang on, as the XSD spells it: `nodes`, `intervals`, …
    /// Empty when the property could not be read at all.
    pub indexable: String,
    /// Values per indexable element, **slowest first** — `[1]` for a scalar;
    /// more than one value per element is a vector or a tensor.
    pub value_count: Vec<u64>,
    /// Whether it is indexed in time (`TimeOrIntervalSeries`, a 2.0.1
    /// `TimeIndex`): one value per element **per step**.
    pub timed: bool,
    /// Whether it declares `RealizationIndices` — *"If not provided, then the
    /// realization count … does not introduce a dimension"*, so when provided,
    /// it does.
    pub realizations: bool,
    /// Its unit, as written.
    pub uom: Option<String>,
    /// Its property kind, as the property references it. `None` when it names
    /// none by reference — a RESQML 2.0.1 standard kind is a name, not an
    /// object.
    pub kind: Option<Ref>,
    /// The table its codes are ranks in — `CategoryLookup` in 2.2, `Lookup`
    /// in 2.0.1.
    pub lookup: Option<Ref>,
    /// The integer its value array declares as null — `NullValue`, when the
    /// array states one. A floating-point array has no such field: its null is
    /// NaN, by convention of the standard.
    pub null_value: Option<i64>,
    /// Its qualifiers — each `Facet`, as the workspace's JSON encoding writes
    /// it, so that a copy states them word for word.
    pub facets: Vec<serde_json::Value>,
    /// The values of its one patch — **or why they were not read**. An absent
    /// backend, a form this build does not evaluate, a property met by
    /// `read::every` on something that is no wellbore frame (see
    /// `read::property_of`): each says so, and none of them is an empty array.
    pub values: Result<energyml_domain::values::Values, String>,
}

/// A regular lattice of heights — what an interpolation onto a grid
/// produces, and what a `Grid2dRepresentation` stores as a map.
///
/// The order is the house lattice rule: `i` runs along x and varies
/// **fastest**, `j` along y and slowest, so `z[j·ni + i]` is the height at
/// `(x0 + i·dx, y0 + j·dy)`. A front end's interpolation answers in this order
/// and the `grid` operation writes it; a producer that has its rows the
/// other way round transposes before building this, never after.
#[derive(Debug, Clone, PartialEq)]
pub struct Lattice {
    /// `[x0, y0]`, the node `(0, 0)`.
    pub origin: [f64; 2],
    /// `[dx, dy]`, both positive.
    pub spacing: [f64; 2],
    /// `[ni, nj]`: along x, the fastest axis, and along y, the slowest.
    pub counts: [usize; 2],
    /// `ni × nj` heights; a hole is a NaN, written as such.
    pub z: Vec<f64>,
}

/// One input, whatever it came from.
///
/// The variants differ in size — a `Known` is six optional strings, a `Ramp`
/// two vectors — and that is accepted: a bag holds a handful of pieces, and
/// boxing would make every constructor say `Box::new` for nothing.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum Source {
    /// Geometry: an object read by `geometry::of`, an imported OBJ or VTK, an
    /// artefact of the scene.
    Shape {
        /// The patches and their topology.
        geometry: Geometry,
        /// Whether each polyline is closed, when the producer knows.
        ///
        /// RESQML states closure in a separate boolean array, and
        /// `Topology::Polylines` rightly refuses to invent one. A producer that
        /// knows — the contouring closes its rings — says so here; one that
        /// does not leaves `None`, and an operation that needs it *detects* it
        /// by the coincidence of the first and last point, saying so in the
        /// plan and never silently.
        closed: Option<Vec<bool>>,
    },
    /// Foreign features: each a shape **and** its properties as the producer
    /// wrote them. A GeoJSON today; a CSV one day.
    Features {
        /// Where they came from — a file name — for a suggested stage or title.
        name: String,
        /// The features, in the producer's order.
        features: Vec<Feature>,
    },
    /// A scalar field laid over a named piece of the bag.
    Field {
        /// The name of the piece the values are laid on.
        over: String,
        /// `nodes`, `cells`, … — RESQML's indexable element.
        on: String,
        /// One value per element.
        values: Vec<f64>,
        /// The unit, when known.
        uom: Option<String>,
    },
    /// A colour ramp: levels and their colours.
    Ramp {
        /// Ascending.
        levels: Vec<f64>,
        /// One per band.
        colours: Vec<Hsv>,
    },
    /// A table, and what its columns mean.
    ///
    /// **The template travels with the file**, and that is the split: a table
    /// without a mapping means nothing, and where the mapping comes from — a
    /// loaded OSDU template, or a form filled in by hand — is the caller's
    /// business. The operation always receives both.
    Table {
        /// Where it comes from — a file name.
        name: String,
        /// The rows, as the lexer read them.
        table: crate::tabular::Table,
        /// What its columns mean.
        template: crate::osdu::Template,
    },
    /// A regular lattice of heights, in the frame it was computed in.
    Lattice(Lattice),
    /// An object already in the package, resolved by the caller.
    Object(Known),
    /// A file taken **as bytes**, read for nothing but to be carried — a
    /// README, a PDF, an export — what the `media` operation puts in a package
    /// (EPC §3.2 pp. 11–12, §3.4.2 pp. 15–16).
    ///
    /// Bytes and not text, because a PDF is not UTF-8, and a piece that went
    /// through a `String` would come out as something else. It is made only
    /// where the operation's own declaration asks for a document
    /// ([`crate::entries::read_document`]): the name of a file never turns a
    /// table or a GeoJSON into one.
    Document {
        /// The file it came from — `README.md`, what the part is named after.
        name: String,
        /// The content type a server declared, when it did; a file declares none.
        content_type: Option<String>,
        /// The bytes, exactly.
        bytes: Vec<u8>,
    },
}

impl Source {
    /// **Only these nodes of a shape, in the order given** — a cloud of them.
    ///
    /// How a selection of nodes enters an operation (for example to create a
    /// polyline from the selection, or to extract them as a point cloud): the
    /// entry's nodes and nothing of its topology, so that every
    /// operation that takes a shape takes them — `pointset` makes the cloud,
    /// `polyline-through` the line, `triangulate` a surface through them. The
    /// order is kept: it is the order they were pointed in, which a line reads.
    ///
    /// A piece that is not a shape is left as it is — what is known of the
    /// object travels with the shape and is still true of its nodes. No node
    /// at all is refused: an empty cloud reads like an input with nothing in it.
    pub fn only_nodes(self, ranks: &[usize]) -> Result<Source, String> {
        match self {
            Source::Shape { geometry, .. } => {
                if ranks.is_empty() {
                    return Err("no node is named — a selection of nodes holds one at least".to_string());
                }
                let points = geometry.nodes_at(ranks)?;
                Ok(Source::Shape {
                    geometry: Geometry {
                        kind: "nodes",
                        patches: vec![Patch {
                            index: 0,
                            node_offset: 0,
                            points,
                            topology: Topology::Points,
                            closed: Vec::new(),
                        }],
                    },
                    closed: None,
                })
            }
            other => Ok(other),
        }
    }

    /// A short word for what this is, for a refusal that names both ends.
    pub fn kind(&self) -> &'static str {
        match self {
            Source::Shape { .. } => "shape",
            Source::Features { .. } => "features",
            Source::Table { .. } => "table",
            Source::Field { .. } => "field",
            Source::Ramp { .. } => "ramp",
            Source::Lattice(_) => "lattice",
            Source::Object(_) => "object",
            Source::Document { .. } => "document",
        }
    }
}

/// A **named** bag of inputs.
///
/// Named, because *a mesh and a RESQML object* is an ordinary case and two
/// anonymous inputs cannot be told apart — a `Field` says which piece it lies
/// on by name.
#[derive(Debug, Clone, Default)]
pub struct Pieces {
    entries: Vec<(String, Source)>,
}

impl Pieces {
    /// Nothing at all — what an operation with no input, like `crs`, takes.
    pub fn empty() -> Self {
        Self::default()
    }

    /// The bag with one more piece.
    pub fn with(mut self, name: impl Into<String>, source: Source) -> Self {
        self.entries.push((name.into(), source));
        self
    }

    /// Change the piece of that name in place — how placed points are brought
    /// back into a frame once the frame is chosen.
    pub fn update(&mut self, name: &str, change: impl FnOnce(&mut Source)) {
        if let Some((_, source)) = self.entries.iter_mut().find(|(n, _)| n == name) {
            change(source);
        }
    }

    /// The piece of that name.
    pub fn get(&self, name: &str) -> Option<&Source> {
        self.entries.iter().find(|(n, _)| n == name).map(|(_, s)| s)
    }

    /// Every piece, in order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Source)> {
        self.entries.iter().map(|(n, s)| (n.as_str(), s))
    }

    /// How many pieces.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// No piece at all.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The feature collections in the bag: `(piece name, source name, features)`.
    pub fn features(&self) -> impl Iterator<Item = (&str, &str, &[Feature])> {
        self.entries.iter().filter_map(|(n, s)| match s {
            Source::Features { name, features } => Some((n.as_str(), name.as_str(), features.as_slice())),
            _ => None,
        })
    }

    /// The tables in the bag: `(piece name, file name, table, template)`.
    pub fn tables(
        &self,
    ) -> impl Iterator<Item = (&str, &str, &crate::tabular::Table, &crate::osdu::Template)> {
        self.entries.iter().filter_map(|(n, s)| match s {
            Source::Table {
                name,
                table,
                template,
            } => Some((n.as_str(), name.as_str(), table, template)),
            _ => None,
        })
    }

    /// The shapes in the bag: `(piece name, geometry, closed)`.
    pub fn shapes(&self) -> impl Iterator<Item = (&str, &Geometry, Option<&[bool]>)> {
        self.entries.iter().filter_map(|(n, s)| match s {
            Source::Shape { geometry, closed } => Some((n.as_str(), geometry, closed.as_deref())),
            _ => None,
        })
    }

    /// The lattices in the bag: `(piece name, lattice)`.
    pub fn lattices(&self) -> impl Iterator<Item = (&str, &Lattice)> {
        self.entries.iter().filter_map(|(n, s)| match s {
            Source::Lattice(lattice) => Some((n.as_str(), lattice)),
            _ => None,
        })
    }

    /// The objects the caller resolved: `(piece name, what is known)`.
    pub fn objects(&self) -> impl Iterator<Item = (&str, &Known)> {
        self.entries.iter().filter_map(|(n, s)| match s {
            Source::Object(known) => Some((n.as_str(), known)),
            _ => None,
        })
    }

    /// The kinds present, for a refusal — `shape, features`.
    pub fn kinds(&self) -> String {
        let mut kinds: Vec<&str> = self.entries.iter().map(|(_, s)| s.kind()).collect();
        kinds.sort_unstable();
        kinds.dedup();
        if kinds.is_empty() {
            "nothing".to_string()
        } else {
            kinds.join(", ")
        }
    }
}
