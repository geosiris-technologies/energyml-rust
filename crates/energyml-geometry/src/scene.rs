//! What an exporter is given: shape, place, and identity.
//!
//! # Why identity travels with the geometry
//!
//! An exported surface that is only a set of triangles is a dead end. The
//! question asked of an export is almost never "what shape is this" alone — it
//! is "which horizon is this, which interpretation does it come from, and where
//! do I look it up again". So a [`SceneObject`] carries the uuid and the ETP
//! URI alongside the points, and the interpretation and feature above them when
//! the package names them.
//!
//! That chain is a RESQML habit worth knowing: a representation *represents* an
//! interpretation, and an interpretation *interprets* a feature. Three objects
//! for one horizon — the shape, the reading of it, and the thing itself. An
//! export that drops the top two produces a file nobody can trace back.
//!
//! # Why the CRS is carried and not applied
//!
//! [`SceneObject::points_in`] places the points on request, because different
//! exporters want different answers: OBJ wants metres near the origin, GeoJSON
//! wants degrees of longitude and latitude, and a viewer may want the local
//! system untouched. Applying a placement here would make that choice once, for
//! everybody, invisibly.

use energyml_core::value::{FieldValue, Reflect};
use energyml_domain::crs::{self, Crs};
use energyml_domain::metadata::{Metadata, Spec};

use crate::error::Result;
use crate::geometry::{self, Geometry};
use crate::shape::Shape;
use crate::points::Point;
use crate::readers::Readers;
use crate::source::ValueSource;

/// Which coordinates an exporter wants.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Placement {
    /// The representation's own axes and units, untouched.
    Local,
    /// Placed in the projected system: rotation, offset, and the depth flip.
    #[default]
    Projected,
    /// Placed and normalised to metres.
    ///
    /// Fails rather than guessing when the CRS states a unit this build cannot
    /// convert — a model in feet read as metres is 3.28 times too far out with
    /// every coordinate still looking plausible.
    Metres,
}

impl Placement {
    /// The name a caller types, and the one this prints back.
    ///
    /// Here rather than in whatever binding needs it first: the command line,
    /// the front end and the Python bridge must not each invent their own spelling
    /// of *projected*, or the same model comes out differently depending on
    /// which door it left by.
    pub fn name(self) -> &'static str {
        match self {
            Placement::Local => "local",
            Placement::Projected => "projected",
            Placement::Metres => "metres",
        }
    }

    /// A placement by name, or a refusal that lists what is accepted.
    ///
    /// `meters` is taken too, because half the world spells it that way and a
    /// refusal over an `r` teaches nobody anything. **Anything else is
    /// refused**: a placement quietly defaulting to the wrong one moves a model
    /// by its CRS offset and still draws something plausible.
    pub fn parse(name: &str) -> std::result::Result<Placement, String> {
        match name {
            "local" => Ok(Placement::Local),
            "projected" => Ok(Placement::Projected),
            "metres" | "meters" => Ok(Placement::Metres),
            other => Err(format!(
                "unknown placement `{other}`: one of `local` (the representation's own axes), \
`projected` (rotation, offset and depth flip applied) or `metres` (and refused when the CRS \
states a unit this build cannot convert)"
            )),
        }
    }
}

/// One representation, ready to be written out.
#[derive(Clone, Debug)]
pub struct SceneObject {
    /// The representation's uuid.
    pub uuid: String,
    /// Its ETP URI — what an application looks it up by.
    pub uri: String,
    /// Its citation title, or an empty string when it has none.
    pub title: String,
    /// Its qualified type, e.g. `resqml20.obj_TriangulatedSetRepresentation`.
    pub qualified_type: String,
    /// The shape.
    pub geometry: Geometry,
    /// Where it is, when it names a coordinate system.
    pub crs: Option<Crs>,
    /// The interpretation this represents, when it names one.
    pub interpretation: Option<Related>,
    /// The feature that interpretation interprets, when it can be followed.
    pub feature: Option<Related>,
    /// Whatever a [`Spec`] was asked to collect.
    ///
    /// Empty unless the caller supplied one. Exporters write these verbatim, so
    /// the same spec produces the same keys in an OBJ comment, a GeoJSON
    /// property and a VTK field array — one configuration, every format.
    pub metadata: Metadata,
    /// Values hanging on this shape: one number per node, per cell, per
    /// triangle.
    ///
    /// Empty unless the caller filled it with [`attributes_for`], because
    /// reading them means reading every property of the package and their
    /// external arrays — which an export of pure geometry has no reason to
    /// pay for.
    pub attributes: Vec<Attribute>,
}

/// A property's values, lined up with the points or the cells of a shape.
///
/// What a viewer colours by. The scope is decided from the property's
/// `IndexableElement` and **checked against the geometry** — a mismatch is
/// reported and the attribute dropped rather than written against the wrong
/// elements, where it would draw a plausible picture of nothing.
#[derive(Clone, Debug, PartialEq)]
pub struct Attribute {
    /// Uuid of the property object.
    pub uuid: String,
    /// A name for the array: the property's title, or its uuid.
    pub name: String,
    /// What it measures, when the kind could be resolved.
    pub kind: Option<String>,
    /// Its unit, when it has one.
    pub uom: Option<String>,
    /// Whether the values hang on the points or on the cells.
    pub scope: AttributeScope,
    /// Values per element: 1 for a scalar property, more for a vector one.
    pub components: u64,
    /// The values, widened to `f64` and in element order.
    pub values: Vec<f64>,
    /// A colour ramp for these values, sampled from the package's own colour
    /// map when it has one: RGBA rows in 0–1, low value first.
    ///
    /// Empty when the package states no colour for this property, and empty is
    /// the right answer — the standard defines no default ramp, and inventing
    /// one would draw a picture the file does not describe. Filled by the
    /// caller, because resolving a colour needs the whole package (see
    /// `energyml_domain::color::Cascade`).
    pub lut: Vec<[f64; 4]>,
}

/// Which elements an attribute's values belong to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AttributeScope {
    /// One value per point.
    Points,
    /// One value per cell — a triangle, a line segment, a hexahedron.
    Cells,
}

/// An object named by a reference, resolved far enough to label it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Related {
    /// Its uuid.
    pub uuid: String,
    /// Its title, when the reference or the object carries one.
    pub title: String,
    /// Its type, as the reference spells it.
    pub qualified_type: String,
}

impl SceneObject {
    /// The points of every patch, placed as asked, in one array.
    ///
    /// The order matches [`Geometry::merged`], so the indices that call returns
    /// address these points.
    pub fn points_in(&self, placement: Placement) -> Result<Vec<Point>> {
        let (points, _) = self.geometry.merged();
        self.place(points, placement)
    }

    /// The shape itself, every patch placed as asked — its topology untouched.
    ///
    /// What an operation computing **for the scene** must read: the scene draws
    /// the projected placement, so a geometry computed in the local frame and
    /// handed back as an artefact lands where the file's numbers are, not where
    /// the object is drawn — offsets, rotation and the vertical flip missing.
    /// The same `place` as every other placement here, so the two cannot drift.
    pub fn geometry_in(&self, placement: Placement) -> Result<Geometry> {
        let mut out = self.geometry.clone();
        for patch in &mut out.patches {
            patch.points = self.place(std::mem::take(&mut patch.points), placement)?;
        }
        Ok(out)
    }

    /// Points and triangles together, placed as asked.
    pub fn mesh_in(&self, placement: Placement) -> Result<(Vec<Point>, Vec<[u32; 3]>)> {
        let (points, triangles) = self.geometry.merged();
        Ok((self.place(points, placement)?, triangles))
    }

    /// Points, triangles **and line segments**, placed.
    ///
    /// What a viewer wants: [`mesh_in`] answers only for triangles, so a grid
    /// and a trajectory came back with no indices at all and were drawn as
    /// point clouds. See [`crate::geometry::Geometry::drawable`].
    ///
    /// [`mesh_in`]: SceneObject::mesh_in
    pub fn drawable_in(&self, placement: Placement) -> Result<crate::geometry::Drawable> {
        let drawn = self.geometry.drawable();
        Ok(crate::geometry::Drawable {
            points: self.place(drawn.points, placement)?,
            triangles: drawn.triangles,
            lines: drawn.lines,
            dropped: drawn.dropped,
            triangle_cells: drawn.triangle_cells,
            node_cells: drawn.node_cells,
        })
    }

    /// Place points that are not part of this object's own geometry — in the
    /// same coordinate system, through the same `place` every patch goes
    /// through, so a caller drawing something *alongside* the shape (a
    /// frame's head portion, `crate::well::head_portion`) cannot place it
    /// differently from the shape itself.
    pub fn place_points(&self, points: Vec<Point>, placement: Placement) -> Result<Vec<Point>> {
        self.place(points, placement)
    }

    fn place(&self, points: Vec<Point>, placement: Placement) -> Result<Vec<Point>> {
        let Some(crs) = &self.crs else {
            // No CRS is not an error: plenty of representations name none, and
            // their local coordinates are the only answer there is.
            return Ok(points);
        };
        match placement {
            Placement::Local => Ok(points),
            Placement::Projected => Ok(points.into_iter().map(|p| crs.to_projected(p)).collect()),
            Placement::Metres => points
                .into_iter()
                .map(|p| crs.to_metres(p))
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(Into::into),
        }
    }

    /// The EPSG code of the system these coordinates sit in, when there is one.
    pub fn epsg(&self) -> Option<u64> {
        self.crs.as_ref().and_then(|crs| crs.projected.epsg())
    }

    /// This object drawn in the shape asked for.
    ///
    /// Borrowed when the shape is already what the object naturally is, which
    /// is the ordinary case — [`Shape::Auto`] on anything, or a rule that
    /// happens to agree with the file.
    ///
    /// **The points never move**, because every conversion in [`crate::shape`]
    /// is arithmetic on indices. A point-scoped property therefore comes
    /// through untouched. A **cell**-scoped one may not: asking a hexahedral
    /// grid for its surface replaces a million cells with the faces around
    /// them, and there is no value per face to be had. Such an attribute is
    /// dropped rather than written against elements it was not measured on —
    /// the same rule as everywhere else here, and the reason it is safe for the
    /// case that matters is that a 2D grid drawn as quadrilaterals has exactly
    /// as many cells as before, so nothing is lost there.
    pub fn as_shape(&self, shape: Shape) -> Result<std::borrow::Cow<'_, SceneObject>> {
        if self
            .geometry
            .patches
            .iter()
            .all(|patch| patch.topology.is_already(shape))
        {
            return Ok(std::borrow::Cow::Borrowed(self));
        }

        let cells_before = self.geometry.cell_count();
        let points_before = self.geometry.point_count();
        let geometry = self.geometry.as_shape(shape)?;

        // A **summary** — an outline, a centroid — replaces the points, so
        // nothing that hangs on them survives it. Four corners saying where a
        // horizon is cannot carry a value per node of the horizon, and keeping
        // the array would line it up against corners it was never measured on.
        let attributes = if shape.is_summary() || geometry.point_count() != points_before {
            Vec::new()
        } else if geometry.cell_count() == cells_before {
            self.attributes.clone()
        } else {
            // The points held, the cells did not — asking a hexahedral grid for
            // its outside. A per-node property still lines up; a per-cell one
            // has no value for a face.
            self.attributes
                .iter()
                .filter(|attribute| attribute.scope == AttributeScope::Points)
                .cloned()
                .collect()
        };

        Ok(std::borrow::Cow::Owned(SceneObject {
            geometry,
            attributes,
            ..self.clone()
        }))
    }

    /// This object with every node that has no position removed — or `None`
    /// when every node has one, which is the ordinary case and costs one pass
    /// and no copy.
    ///
    /// **What [`crate::geometry::of`] returns is what the file says; this is
    /// what can be drawn.** The two are kept apart on purpose: a package with
    /// 34 000 null Z values should be *reported* as holding them and *exported*
    /// without them. Reading and drawing are two questions, and answering the
    /// first with the second's answer is how a node count silently stops
    /// matching the file. See [`crate::defined`] for what the standard does and
    /// does not say about those nulls.
    ///
    /// The attributes come along. Their values go through the same index maps
    /// the geometry was compacted with, so a property still lines up with the
    /// elements it was written for — leaving them alone would colour the
    /// survivors with their neighbours' numbers, which is the worst of the three
    /// possible outcomes because it is the one that looks right.
    ///
    /// An attribute whose length never matched the geometry is passed through
    /// untouched, to be refused by the check that already exists rather than
    /// silently reshaped here.
    pub fn drawable_only(&self) -> Option<SceneObject> {
        let compaction = self.geometry.defined_only()?;

        let points_before = self.geometry.point_count();
        let cells_before = self.geometry.cell_count();
        let attributes = self
            .attributes
            .iter()
            .map(|attribute| {
                let (map, expected) = match attribute.scope {
                    AttributeScope::Points => (&compaction.points, points_before),
                    AttributeScope::Cells => (&compaction.cells, cells_before),
                };
                let components = attribute.components.max(1) as usize;
                if attribute.values.len() != expected * components {
                    return attribute.clone();
                }
                let mut values = Vec::with_capacity(map.len() * components);
                for element in map {
                    let start = element * components;
                    values.extend_from_slice(&attribute.values[start..start + components]);
                }
                Attribute {
                    values,
                    ..attribute.clone()
                }
            })
            .collect();

        Some(SceneObject {
            geometry: compaction.geometry,
            attributes,
            ..self.clone()
        })
    }
}

/// Read one representation into a scene object.
///
/// Everything an exporter needs, gathered in one pass: the geometry through the
/// reader registry, the CRS through the store, and the interpretation and
/// feature by following the references the representation carries.
pub fn object_of(representation: &dyn Reflect, source: &dyn ValueSource, uri: &str) -> Result<SceneObject> {
    object_of_with(
        crate::readers::default_readers(),
        representation,
        source,
        uri,
        None,
    )
}

/// The same, with a reader registry and a metadata spec the caller controls.
pub fn object_of_with(
    readers: &Readers,
    representation: &dyn Reflect,
    source: &dyn ValueSource,
    uri: &str,
    spec: Option<&Spec>,
) -> Result<SceneObject> {
    let info = representation.type_info();
    let geometry = geometry::of_with(readers, representation, source)?;
    // The CRS is read even when it is absent or unreadable: a shape with no
    // stated place is still a shape, and refusing to export it would be worse
    // than exporting it unplaced.
    let crs = crs::of(representation, source).unwrap_or(None);

    let interpretation = related(representation, INTERPRETS);
    // The feature hangs off the interpretation, not the representation, so it
    // needs one more hop through the store — and it is worth it: the feature is
    // the name a person recognises.
    //
    // Followed by the **reference**, so the version it names is the version
    // read: a package holding the interpretation twice has two features to
    // tell apart.
    let feature = interpretation_of(representation, source)
        .and_then(|object| related(object.as_reflect(), REPRESENTS_FEATURE));

    // Evaluated here rather than in the exporter, because this is where the
    // store is: a path that crosses a reference needs to resolve it, and an
    // exporter is given an object rather than a package.
    let metadata = match spec {
        Some(spec) => spec.read(representation, source),
        None => Metadata::default(),
    };

    Ok(SceneObject {
        uuid: energyml_core::identity::uuid_of_reflect(representation)
            .unwrap_or_default()
            .to_string(),
        uri: uri.to_string(),
        title: title_of(representation),
        // **The qualified type, not the class name**, which is what this field
        // has always been documented as and what every hand-built
        // `SceneObject` in the export tests holds. It was the Rust type name,
        // so an OBJ comment, a GeoJSON property and a VTK field array all
        // carried `PointSetRepresentation` where the package says
        // `resqml20.obj_PointSetRepresentation` — a name that does not say
        // which release it came from, in the one line of an export meant to
        // lead back to the package.
        qualified_type: energyml_core::json::qualified_type_of(info),
        geometry,
        crs,
        interpretation,
        feature,
        metadata,
        attributes: Vec::new(),
    })
}

/// Where a property's colour decision comes from — **the seam that keeps this
/// crate below the caches**.
///
/// A scene needs *what colour the package gives this property*, and the answer
/// is the inheritance order (`energyml_domain::color_inherit`) over the package
/// index, which a cache holds (`energyml-cache`, above this crate). A caller
/// that holds one passes it here, and its colours are the front end's, to the byte;
/// a caller that does not gets [`Cascade::resolve`](energyml_domain::color::Cascade::resolve),
/// the shorter order that predates the index and ignores the collections and
/// the model.
pub trait Decide {
    /// The decision for one request against the package's merged set, or
    /// `None` when no level answers (or the walk could not be made).
    fn decide(
        &self,
        set: &energyml_domain::color::GraphicalSet,
        request: &energyml_domain::color::Request<'_>,
    ) -> Option<energyml_domain::color::Decision>;
}

/// The colour ramp of one attribute, sampled from whatever colour map the
/// package states for its property.
///
/// 256 rows over the values actually present, which is what a legacy VTK
/// lookup table is. Empty when nothing states a colour: the standard defines
/// no default ramp, and inventing one draws something the file does not say.
///
/// `colours` decides the property's colour ([`Decide`]); `None` falls back on
/// the cascade's own `resolve`.
pub fn colour_ramp(
    objects: &dyn energyml_domain::source::ObjectSource,
    cascade: &energyml_domain::color::Cascade<'_>,
    colours: Option<&dyn Decide>,
    kinds: &energyml_domain::property_kind::Resolver<'_>,
    object: &SceneObject,
    attribute: &Attribute,
) -> Vec<[f64; 4]> {
    use energyml_domain::color::{Paint, Request, Scope};
    use energyml_domain::property;

    // The property object has to outlive the borrow of what is read from it.
    let Some(object_of_property) = objects.object(&attribute.uuid) else {
        return Vec::new();
    };
    let Ok(read) = property::read(object_of_property.as_reflect()) else {
        return Vec::new();
    };

    let chain: Vec<String> = read
        .kind
        .as_ref()
        .and_then(|reference| kinds.resolve(reference).ok())
        .map(|resolution| {
            resolution
                .chain
                .iter()
                .map(|kind| kind.uuid.clone())
                .filter(|uuid| !uuid.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let asked = Request {
        // By uri: two versions of one representation are two requests.
        representation: &object.uri,
        property: Some(&attribute.uuid),
        kind_chain: &chain,
        element: Scope::WholeObject,
    };
    let decision = match colours {
        Some(colours) => colours.decide(cascade.set, &asked),
        None => cascade.resolve(&asked, objects).ok().flatten(),
    };
    let Some(decision) = decision else {
        return Vec::new();
    };
    let Paint::Mapped { map, scaling, .. } = &decision.paint else {
        // A constant colour is not a ramp: it colours the object, not its
        // values, and the object's colour is already `energyml colors`.
        return Vec::new();
    };

    let finite: Vec<f64> = attribute.values.iter().copied().filter(|v| v.is_finite()).collect();
    let (low, high) = match (
        finite.iter().copied().reduce(f64::min),
        finite.iter().copied().reduce(f64::max),
    ) {
        (Some(low), Some(high)) if high > low => (low, high),
        _ => return Vec::new(),
    };

    (0..256)
        .filter_map(|step| {
            let value = low + (high - low) * (step as f64) / 255.0;
            map.sample(Some(value), scaling).map(|rgba| [rgba.r, rgba.g, rgba.b, rgba.a])
        })
        .collect()
}

/// `n - 1` interval values as `n` node values.
///
/// **Each node takes exactly one interval** — the one it opens — and the
/// deepest node takes the one it closes. That is what makes this a
/// correspondence rather than a reduction: nothing is averaged, no majority is
/// taken and there is no tie to break, which matters because these values are
/// categories. The mean of two soil types is a third rock, not a shade between
/// them.
///
/// The rule is **this build's**, not a quotation: a marker at a depth names the
/// unit beneath it, which is how a depth-indexed log is read. Callers report it
/// rather than applying it silently.
fn intervals_onto_nodes(
    values: &[f64],
    components: usize,
    point_count: usize,
    segments: usize,
) -> Vec<f64> {
    let mut out = Vec::with_capacity(point_count * components);
    for node in 0..point_count {
        let interval = node.min(segments.saturating_sub(1));
        let at = interval * components;
        out.extend_from_slice(&values[at..at + components]);
    }
    out
}

/// A package's properties, grouped by the representation they hang on.
///
/// **Built once, asked per object.** An export handed [`attributes_for`] every
/// property of the package for every object it wrote, and that function reads
/// each one to learn what it hangs on: 2 697 wellbore frames against the
/// ~56 000 properties of the SIGEOL package is some 150 million reads for an
/// answer an index holds (an OBJ export of the 2 000+ wells and their surfaces
/// took far too long). The same quadratic shape as a front end's
/// `colourChains` (×1 870).
///
/// The grouping rule is [`energyml_domain::property::Index`]'s — case-blind
/// `SupportingRepresentation` — not a second one written here; this keeps the
/// objects beside it, because [`attributes_for`] reads their values.
pub struct PropertiesOf<'a> {
    index: energyml_domain::property::Index,
    by_uuid: std::collections::HashMap<String, &'a dyn Reflect>,
    unread: Vec<String>,
}

impl<'a> PropertiesOf<'a> {
    /// Read every property once.
    pub fn new(properties: &[&'a dyn Reflect]) -> Self {
        let (index, unread) = energyml_domain::property::Index::build(properties.iter().copied());
        let by_uuid = properties
            .iter()
            .filter_map(|property| {
                energyml_core::identity::uuid_of_reflect(*property).map(|uuid| (uuid.to_ascii_lowercase(), *property))
            })
            .collect();
        Self { index, by_uuid, unread }
    }

    /// The properties hanging on this representation, in package order —
    /// what to hand [`attributes_for`] for it.
    pub fn of(&self, representation_uuid: &str) -> Vec<&'a dyn Reflect> {
        self.index
            .of(representation_uuid)
            .iter()
            .filter_map(|entry| self.by_uuid.get(&entry.uuid.to_ascii_lowercase()).copied())
            .collect()
    }

    /// The properties that could not be read — said once, not once per object.
    pub fn unread(&self) -> &[String] {
        &self.unread
    }
}

/// Line a package's properties up with a shape.
///
/// The properties are passed in rather than searched for: nothing in a
/// representation lists them — every link points the other way — so finding
/// them means walking the package, which is the caller's job and not this
/// function's (see `energyml_domain::property::Index`).
///
/// Returns what could be attached and, separately, **why each of the others
/// could not**. A property silently missing from an export is the failure this
/// whole path exists to avoid: the picture looks finished and is not.
pub fn attributes_for(
    object: &SceneObject,
    properties: &[&dyn Reflect],
    source: &dyn ValueSource,
) -> (Vec<Attribute>, Vec<String>) {
    use energyml_domain::property::{self, IndexableElement};

    let point_count = object.geometry.point_count();
    // Not the triangles of the merged mesh: a per-cell property of an IJK grid
    // has one value per **hexahedron**, and of a polyline set one per line.
    let cell_count = object.geometry.cell_count();
    // **One line, and only one**: a wellbore frame, whose `intervals` are its
    // segments (RESQML 2.2 Usage Guide §4.2.1, Table 1, p. 38: *"intervals —
    // wellbore frames, column-layer grids"*). Its geometry counts the whole
    // line as one cell, so `n - 1` interval values have no array of their
    // length until they are moved onto the nodes. A set of several lines is
    // left out on purpose: "interval `i` is the one node `i` opens" only holds
    // when the nodes are one run.
    let one_line = matches!(
        object.geometry.patches.as_slice(),
        [patch] if matches!(&patch.topology, crate::Topology::Polylines(runs) if runs.len() == 1)
    );
    let mut out = Vec::new();
    let mut refused = Vec::new();

    for property in properties {
        let read = match property::read(*property) {
            Ok(read) => read,
            Err(e) => {
                refused.push(format!("a property did not read: {e}"));
                continue;
            }
        };
        if read
            .supporting_representation
            .as_deref()
            .map(str::to_ascii_lowercase)
            != Some(object.uuid.to_ascii_lowercase())
        {
            continue;
        }

        // The indexable element decides which array it goes into. Anything
        // that is neither a point nor a cell of the *merged* geometry is
        // refused by name: writing it anyway would line values up against
        // elements they do not belong to.
        let scope = match read.indexable {
            IndexableElement::Nodes => AttributeScope::Points,
            // A frame's `intervals`: the same correspondence as `intervals
            // from datum` below. They used to go to the cells — one, the
            // whole line — and every interval property of a frame was refused
            // (*« `BSS000NSYT — formation` has 3 value(s) for 1 cell(s) »*),
            // which is what the well column and its colours asked for.
            IndexableElement::Intervals if one_line => AttributeScope::Points,
            IndexableElement::Cells
            | IndexableElement::Faces
            | IndexableElement::Triangles
            | IndexableElement::Columns
            | IndexableElement::Intervals => AttributeScope::Cells,
            // **A frame’s intervals are its segments, and they are moved onto
            // its nodes rather than refused.**
            //
            // `Topology::Polylines` counts *lines*, so a frame of `n` node
            // depths is one cell and `n - 1` intervals: there is no array of
            // that length to attach to, and lining them up against the line
            // itself would put every value on one element.
            //
            // What makes this a correspondence rather than a reduction is that
            // **each node takes exactly one interval** — the one it opens, and
            // for the deepest node the one it closes. So there is no average,
            // no majority and no tie to break, which matters because these are
            // categories: a soil type between two others is a third rock, not
            // a shade of them. `transfer` is deliberately not used, because it
            // reduces over the geometry’s own cells, and a polyline’s cell is
            // the whole line.
            //
            // The rule is **ours**, not a quotation: a marker at a depth names
            // the unit beneath it, which is how a depth-indexed log is read.
            // It is reported alongside the attachment so nobody has to guess
            // which end of the interval a colour came from.
            IndexableElement::IntervalsFromDatum => AttributeScope::Points,
            other => {
                refused.push(format!(
                    "`{}` hangs on {other}, which is neither the points nor the cells of the \
                     exported geometry; it was not attached",
                    read.title.as_deref().unwrap_or(&read.uuid)
                ));
                continue;
            }
        };

        let mut values = Vec::new();
        let mut failed = None;
        for patch in &read.patches {
            match patch.read(source) {
                Ok(read) => match (read.floating(), read.integer()) {
                    (Some(v), _) => values.extend_from_slice(v),
                    (_, Some(v)) => values.extend(v.iter().map(|n| *n as f64)),
                    _ => failed = Some("its values are neither numbers nor integers".to_string()),
                },
                Err(e) => failed = Some(e.to_string()),
            }
        }
        if let Some(reason) = failed {
            refused.push(format!(
                "`{}`: {reason}",
                read.title.as_deref().unwrap_or(&read.uuid)
            ));
            continue;
        }

        // The interval → node correspondence, done before anything counts:
        // `n - 1` interval values become `n` node values, each node taking the
        // interval it opens and the deepest one the interval it closes. Only
        // where the numbers say the property really is per segment — otherwise
        // the count check below is left to report the disagreement, which is
        // its job.
        let frame_intervals = read.indexable == IndexableElement::IntervalsFromDatum
            || (read.indexable == IndexableElement::Intervals && one_line);
        if frame_intervals {
            let components = read.values_per_element().max(1) as usize;
            let segments = point_count.saturating_sub(cell_count);
            if segments > 0 && values.len() == segments * components {
                values = intervals_onto_nodes(&values, components, point_count, segments);
                refused.push(format!(
                    "`{}` hangs on {}: its {segments} interval value(s) were moved onto \
                     the {point_count} node(s), each taking the interval below it and the \
                     deepest one the interval above. Nothing was averaged — these are \
                     categories — but the correspondence is this build's rule, not the \
                     standard's.",
                    read.title.as_deref().unwrap_or(&read.uuid),
                    read.indexable
                ));
            }
        }

        let expected = match scope {
            AttributeScope::Points => point_count,
            AttributeScope::Cells => cell_count,
        };
        let mut components = read.values_per_element().max(1);
        // The corpus contains a property that declares two values per node and
        // stores one — the producer ignoring its own
        // `ValueCountPerIndexableElement`. The values are usable as scalars,
        // and refusing them would lose a real array over a wrong number, so
        // they are attached **as scalars with the disagreement reported**.
        if components > 1 && expected > 0 && values.len() == expected {
            refused.push(format!(
                "`{}` declares {components} values per element and holds exactly one; it is \
                 attached as a scalar array, and the document's own count is what is wrong",
                read.title.as_deref().unwrap_or(&read.uuid)
            ));
            components = 1;
        }
        if expected == 0 || values.len() != expected * components as usize {
            // Reported, not repaired: padding or truncating would put values
            // on the wrong elements, which is exactly the failure this check
            // exists for.
            refused.push(format!(
                "`{}` has {} value(s) for {expected} {} × {components} component(s); it was not \
                 attached, because lining it up would put values on elements they do not belong to",
                read.title.as_deref().unwrap_or(&read.uuid),
                values.len(),
                match scope {
                    AttributeScope::Points => "point(s)",
                    AttributeScope::Cells => "cell(s)",
                }
            ));
            continue;
        }

        out.push(Attribute {
            name: read
                .title
                .clone()
                .unwrap_or_else(|| read.uuid.clone()),
            kind: match &read.kind {
                Some(energyml_domain::property::PropertyKindRef::Standard { name }) => Some(name.clone()),
                Some(energyml_domain::property::PropertyKindRef::Object { title, .. }) => title.clone(),
                None => None,
            },
            uom: read.uom.clone(),
            scope,
            components,
            values,
            uuid: read.uuid.clone(),
            lut: Vec::new(),
        });
    }

    (out, refused)
}

/// The field names a representation uses to name its interpretation.
///
/// Two spellings, both in the corpus: RESQML 2.0.1 says `RepresentedInterpretation`,
/// 2.2 says `RepresentedObject` — the second is wider, since 2.2 lets a
/// representation represent things that are not interpretations.
const INTERPRETS: &[&str] = &["RepresentedInterpretation", "RepresentedObject"];

/// And the ones an interpretation uses to name its feature.
const REPRESENTS_FEATURE: &[&str] = &["InterpretedFeature", "InterpretedObject"];

/// The interpretation a representation names, followed **by its reference**:
/// the version the reference states is the version read, where asking the
/// store for the uuid alone would find two objects (or the wrong one) in a
/// package holding the interpretation twice. `None` when the representation
/// names none or the store does not hold it.
pub fn interpretation_of(
    representation: &dyn Reflect,
    source: &dyn energyml_domain::source::ObjectSource,
) -> Option<Box<dyn energyml_core::registry::DynObject>> {
    INTERPRETS.iter().find_map(|field| match representation.field(field) {
        Some(FieldValue::Object(reference)) => crate::follow::object_of(source, reference),
        _ => None,
    })
}

fn related(object: &dyn Reflect, fields: &[&str]) -> Option<Related> {
    for field in fields {
        let Some(FieldValue::Object(reference)) = object.field(field) else {
            continue;
        };
        let uuid = energyml_core::identity::uuid_of_reflect(reference)?;
        if uuid.is_empty() {
            continue;
        }
        return Some(Related {
            uuid: uuid.to_string(),
            title: string_field(reference, "Title").unwrap_or_default(),
            qualified_type: string_field(reference, "QualifiedType")
                .or_else(|| string_field(reference, "ContentType"))
                .unwrap_or_default(),
        });
    }
    None
}

fn title_of(object: &dyn Reflect) -> String {
    match object.field("Citation") {
        Some(FieldValue::Object(citation)) => string_field(citation, "Title").unwrap_or_default(),
        _ => String::new(),
    }
}

fn string_field(object: &dyn Reflect, field: &str) -> Option<String> {
    match object.field(field)? {
        FieldValue::Scalar(scalar) => scalar.as_str().map(str::to_string).filter(|s| !s.is_empty()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A node takes the interval below it; the deepest takes the one above.
    ///
    /// Reported from the front end as *"`SP170 nom_geol` hangs on intervals from
    /// datum, which is neither the points nor the cells"*. The refusal was
    /// right — a frame of `n` node depths is **one** polyline cell and `n - 1`
    /// intervals, so there was no array of that length to attach to — and the
    /// answer is to move the values onto the nodes rather than to invent a
    /// cell for them.
    ///
    /// The values below are indices, not measurements, which is the whole
    /// reason this is a correspondence and not an average: `(1 + 2) / 2` is not
    /// a soil type.
    #[test]
    fn each_node_takes_one_interval_and_nothing_is_averaged() {
        // Three nodes, two intervals: sand then clay.
        let moved = intervals_onto_nodes(&[1.0, 2.0], 1, 3, 2);
        assert_eq!(
            moved,
            vec![1.0, 2.0, 2.0],
            "node 0 opens interval 0, node 1 opens interval 1, node 2 closes it"
        );
        // Every output value came from the input: no mean was taken anywhere.
        assert!(moved.iter().all(|v| *v == 1.0 || *v == 2.0));
    }

    /// The shortest frame there can be: two nodes, one interval.
    #[test]
    fn two_nodes_and_one_interval_both_take_it() {
        assert_eq!(intervals_onto_nodes(&[7.0], 1, 2, 1), vec![7.0, 7.0]);
    }

    /// A vector-valued property moves whole tuples, never a stride of one.
    ///
    /// Getting this wrong would shift every component by one node and produce
    /// an array of exactly the right length — the shape of error no count check
    /// can see.
    #[test]
    fn components_travel_together() {
        // Two intervals, three components each.
        let values = [10.0, 11.0, 12.0, 20.0, 21.0, 22.0];
        assert_eq!(
            intervals_onto_nodes(&values, 3, 3, 2),
            vec![10.0, 11.0, 12.0, 20.0, 21.0, 22.0, 20.0, 21.0, 22.0]
        );
    }
}

// -- Reading a whole package ---------------------------------------------------

/// Which properties to hang on the shapes.
///
/// [`Properties::None`] by default, and the default is the point: attaching
/// them means reading **every** property of the package and its external
/// arrays, which an export of pure geometry has no reason to pay for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Properties<'a> {
    /// None at all.
    #[default]
    None,
    /// Every property the package holds.
    All,
    /// Only these uuids, compared without case.
    Only(&'a [String]),
}

/// What to read out of a package, and how much of it.
#[derive(Clone, Copy, Default)]
pub struct Request<'a> {
    /// Keep only representations whose qualified type matches
    /// ([`energyml_storage::resource::type_matches`]). `None` keeps every one.
    ///
    /// Applied to the **summary**, before anything is deserialized: a filtered
    /// run neither reads what it will discard nor reports it as unreadable.
    pub of_type: Option<&'a str>,
    /// Keep only these representations, by uuid, compared without case.
    pub only: &'a [String],
    /// The reader registry. `None` means the built-in one.
    pub readers: Option<&'a Readers>,
    /// What to collect as metadata on each object.
    pub spec: Option<&'a Spec>,
    /// Which properties to attach.
    pub properties: Properties<'a>,
    /// Move the values onto the points or onto the cells when the file hangs
    /// them elsewhere. `None` leaves them where the file put them.
    pub onto: Option<crate::transfer::Scope>,
    /// How to move them. Ignored without `onto`.
    pub method: Option<crate::transfer::Method>,
    /// Where the colour of a property comes from. `None` is the cascade's own
    /// `resolve`; a face that holds a cache passes it, and colours as the front end
    /// does ([`Decide`]).
    pub colours: Option<&'a dyn Decide>,
}

/// A package read into shapes, **with what could not be read**.
///
/// Three lists and not one, because they are three different pieces of news
/// repaired in three different places: a part that will not deserialize is a
/// schema problem, a shape that cannot be evaluated is a geometry this build
/// does not support or a file whose numbers are missing, and a property that
/// will not attach is a count disagreeing with the shape it names.
#[derive(Debug, Default)]
pub struct Scene {
    /// The shapes, in the order the package lists them.
    pub objects: Vec<SceneObject>,
    /// Parts that would not deserialize: uuid, type, and why.
    pub unreadable: Vec<String>,
    /// Shapes that could not be evaluated: uuid, type, and why.
    pub refused: Vec<String>,
    /// Properties that could not be placed on any shape, and why.
    pub unattached: Vec<String>,
}

/// Read a package into scene objects, with their properties and their colours.
///
/// # Why this is here rather than in each caller
///
/// It was written three times — `energyml export`, `energyml geometry` and the
/// Python bridge — and the three had begun to differ: the colour ramps were
/// resolved by one of them and reachable from neither of the others. A piece of
/// reading repeated in every face is a piece of reading that answers
/// differently depending on the door somebody came in by, which is the fault
/// this workspace hunts everywhere else.
///
/// # What it does not decide
///
/// Placement, reprojection, tube sweeping and the choice of an exporter stay
/// with the caller: they are what to *do* with the shapes, and a command line
/// that exports and a viewer that draws want different answers.
///
/// The source is a [`ValueSource`] and a list of summaries rather than a store,
/// so an ETP dataspace reads the way a file does — the rule that holds for
/// every reader here (ADR-0002).
pub fn of_package(
    resources: &[energyml_storage::ObjectSummary],
    source: &dyn ValueSource,
    request: &Request<'_>,
) -> Scene {
    // A `match` and not `unwrap_or_else`: the built-in registry is `'static`,
    // and the closure form makes the borrow checker demand that the caller's be
    // `'static` too — which no caller holding its own registry can give.
    let readers = match request.readers {
        Some(readers) => readers,
        None => crate::readers::default_readers(),
    };
    let mut out = Scene::default();

    for summary in resources {
        if let Some(filter) = request.of_type
            && !energyml_storage::resource::type_matches(&summary.qualified_type, filter)
        {
            continue;
        }
        if !request.only.is_empty()
            && !request
                .only
                .iter()
                .any(|uuid| uuid.eq_ignore_ascii_case(&summary.uuid))
        {
            continue;
        }
        // `read_object` and not `object`: a walk over a whole package has to
        // say **why** a part did not come back, because that is the sentence
        // naming the switch that would help.
        let object = match source.read_object_at(&summary.uri) {
            Ok(object) => object,
            Err(why) => {
                out.unreadable.push(format!(
                    "{}  {}  {why}",
                    summary.uuid, summary.qualified_type
                ));
                continue;
            }
        };
        // Not a refusal and not a silence: a package is mostly objects that are
        // not shapes at all — a CRS, an interpretation, a property — and
        // listing those as refused would bury the ones that really are.
        if !geometry::is_supported(object.as_reflect()) {
            continue;
        }
        match object_of_with(readers, object.as_reflect(), source, &summary.uri, request.spec) {
            Ok(built) => out.objects.push(built),
            Err(why) => out.refused.push(format!(
                "{}  {}  {why}",
                summary.uuid, summary.qualified_type
            )),
        }
    }

    attach(resources, source, request, &mut out);
    out
}

/// The properties, and the colours that go with them.
///
/// Read **once for the package** and offered to every shape, because nothing in
/// a representation lists its properties: every link points the other way, so
/// finding them per shape would be one pass over the package per shape.
fn attach(
    resources: &[energyml_storage::ObjectSummary],
    source: &dyn ValueSource,
    request: &Request<'_>,
    out: &mut Scene,
) {
    let wanted: &[String] = match request.properties {
        Properties::None => return,
        Properties::All => &[],
        Properties::Only(only) => only,
    };

    let mut properties: Vec<Box<dyn energyml_core::registry::DynObject>> = Vec::new();
    for summary in resources {
        if !summary.qualified_type.ends_with("Property") {
            continue;
        }
        if !wanted.is_empty()
            && !wanted.iter().any(|uuid| uuid.eq_ignore_ascii_case(&summary.uuid))
        {
            continue;
        }
        match source.read_object_at(&summary.uri) {
            Ok(object) => properties.push(object),
            Err(why) => out.unattached.push(format!("{}  {why}", summary.uuid)),
        }
    }
    let reflected: Vec<&dyn Reflect> = properties.iter().map(|one| one.as_reflect()).collect();
    let by_representation = PropertiesOf::new(&reflected);
    out.unattached.extend(by_representation.unread().iter().cloned());

    // The colour ramp comes from the package's own maps, so the cascade needs
    // the whole package — which is why it is resolved here rather than by an
    // exporter, which is handed one object at a time.
    // Every set, in package order, read as one (`GraphicalSet::merged`).
    let sets: Vec<energyml_domain::color::GraphicalSet> = resources
        .iter()
        .filter(|summary| summary.qualified_type.ends_with("GraphicalInformationSet"))
        .filter_map(|summary| source.object_at(&summary.uri))
        .map(|object| energyml_domain::color::GraphicalSet::read(object.as_reflect()))
        .collect();
    let set = (!sets.is_empty()).then(|| energyml_domain::color::GraphicalSet::merged(sets));
    let kinds = energyml_domain::property_kind::Resolver::new(source);

    for object in &mut out.objects {
        let (mut attributes, refused) = attributes_for(object, &by_representation.of(&object.uuid), source);
        out.unattached.extend(refused);
        if let Some(target) = request.onto {
            let method = request.method.unwrap_or(crate::transfer::Method::Average);
            out.unattached
                .extend(move_attributes(&mut attributes, target, method, &object.geometry));
        }
        if let Some(set) = &set {
            let cascade = energyml_domain::color::Cascade::new(set);
            for attribute in &mut attributes {
                attribute.lut = colour_ramp(source, &cascade, request.colours, &kinds, object, attribute);
            }
        }
        object.attributes = attributes;
    }
}

/// Move values from the elements the file hangs them on to the ones a caller
/// wants, and **say so in the name**.
///
/// A `facies` written on the faces and a `facies` written on the nodes are two
/// different arrays: a viewer that shows both must not label them the same, nor
/// let anyone think the file said either.
pub fn move_attributes(
    attributes: &mut [Attribute],
    target: crate::transfer::Scope,
    method: crate::transfer::Method,
    geometry: &Geometry,
) -> Vec<String> {
    use crate::transfer::{self, Scope};

    let mut refused = Vec::new();
    for attribute in attributes.iter_mut() {
        let from = match attribute.scope {
            AttributeScope::Points => Scope::Points,
            AttributeScope::Cells => Scope::Cells,
        };
        if from == target {
            continue;
        }
        match transfer::transfer(
            &attribute.values,
            attribute.components as usize,
            from,
            target,
            method,
            geometry,
        ) {
            Ok(values) => {
                attribute.values = values;
                attribute.scope = match target {
                    Scope::Points => AttributeScope::Points,
                    Scope::Cells => AttributeScope::Cells,
                };
                // ASCII, and no brackets: a legacy VTK array name is a token,
                // and ParaView shows a name with arrows in it as one it cannot
                // select. The suffix is readable and it survives the trip
                // through every reader.
                attribute.name =
                    format!("{}__{from}_to_{target}_by_{}", attribute.name, method.name());
            }
            Err(why) => {
                refused.push(format!("`{}` was not moved onto the {target}: {why}", attribute.name))
            }
        }
    }
    refused
}
