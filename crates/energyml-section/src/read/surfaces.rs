//! Reading a surface or a grid, and cutting it along every section
//! (ADR-0015).
//!
//! Contract: [`read_surface`].
//!
//! The order is the order of refusals, cheapest first, each one naming the
//! object: not in the package, not a representation this build reads (an IJK
//! grid has its own reading, [`crate::read::grids`], and its own refusals), a CRS that cannot be
//! read, a read that fails, **no triangles** (it draws as points or lines — the
//! cut is of triangles), points that cannot be brought into the scene. An
//! object that gets through is cut and never empty-handed by accident: a
//! section it does not cross has no entry, and an object crossing none is the
//! caller's `Missed`, which is not a refusal.

use energyml_core::identity;
use energyml_core::value::{FieldValue, Reflect};
use energyml_domain::ArraySource as ValueSource;
use energyml_domain::color::{Cascade, Decision, GraphicalSet, Paint, Request, Scope};
use energyml_domain::package_index::PackageIndex;
use energyml_geometry::fence;
use energyml_geometry::scene::{self, Placement, SceneObject};
use energyml_geometry::section;

use crate::model::{
    CutShape, CutSource, ObjectId, Refused, SectionTrace, SurfaceCut, SurfaceRead, TraceRole,
};
use crate::read::Scene;
use crate::read::frame::{frame_of, to_scene};
use crate::read::grids::Grid;

/// The uuid a uri names: `eml:///resqml22.Foo(uuid)` gives `uuid`, and a text
/// that is not a uri is taken as the uuid itself — `ObjectSource::object` is
/// asked by uuid.
///
/// Lower case, since a store keys its objects that way.
pub fn uuid_of_uri(uri: &str) -> String {
    energyml_core::identity::parse_address(uri).0
}

/// One surface (anything with triangles) or IJK grid, read in the scene's
/// system and cut along each section trace.
// A refusal is the cold path, met once per object, and it carries the object's
// whole identity on purpose: boxing it would change every construction site
// for no gain anyone could measure.
#[allow(clippy::result_large_err)]
pub fn read_surface(
    source: &dyn ValueSource,
    store: &str,
    uri: &str,
    sections: &[SectionTrace],
    scene: &Scene<'_>,
) -> Result<SurfaceRead, Refused> {
    read_surface_coloured(source, store, uri, sections, scene, Colouring::default())
}

/// Where a surface's colour is looked for: the package's sets, merged, and —
/// when the face has it — the package's index, for the inheritance order.
#[derive(Clone, Copy, Default)]
pub struct Colouring<'a> {
    /// The package's `GraphicalInformationSet`s, merged.
    pub set: Option<&'a GraphicalSet>,
    /// The package's index.
    pub index: Option<&'a PackageIndex>,
    /// The face's resolver ([`crate::read::PackageContext::colour_of`]): asked
    /// instead of the cascade when given.
    pub resolver: Option<&'a crate::read::ColourOf<'a>>,
}

/// [`read_surface`], with the package's colour sets merged — the surface takes
/// the colour the file gives it ([`file_colour`]).
#[allow(clippy::result_large_err)]
pub fn read_surface_coloured(
    source: &dyn ValueSource,
    store: &str,
    uri: &str,
    sections: &[SectionTrace],
    scene: &Scene<'_>,
    colours: Colouring<'_>,
) -> Result<SurfaceRead, Refused> {
    Ok(cut_prepared(
        &prepare_surface(source, store, uri, scene, colours)?,
        sections,
    ))
}

/// A surface read and brought into the scene, **before** any section cuts it:
/// what does not depend on the trace, and so what a cache keeps when the trace
/// moves (`read::cache`).
#[derive(Clone, Debug)]
pub struct Prepared {
    /// What the drawing calls it.
    pub object: ObjectId,
    /// What it is — from its interpretation.
    pub role: TraceRole,
    /// In time, carried onto the depth axis.
    pub time: bool,
    /// The colour the file gives it.
    pub colour: Option<String>,
    /// Its age ([`age_of`]), when its interpretation states one.
    pub age: Option<[f64; 2]>,
    /// The feature its interpretation interprets (`feature_of`).
    pub feature: Option<String>,
    /// Its points, in the scene's system.
    pub points: Vec<[f64; 3]>,
    /// Its triangles.
    pub triangles: Vec<[u32; 3]>,
    /// **The node numbering, reconciled**: `nodes[i]` is the number, in the
    /// representation's own numbering (the running sum of its patches' points —
    /// the numbering a front end's colour buffer indexes, so the front end's per-node
    /// paint), of `points[i]`. The two differ whenever the reader dropped a
    /// node that holds no location (`Geometry::drawable` compacts them out and
    /// renumbers): a lattice with holes — Lutetia's horizons in grid form — is
    /// the ordinary case. Empty when the reader cannot say (a grid, or a shape
    /// whose dropped nodes are not just the non-finite ones): the cut then
    /// carries no sources and is drawn in the flat colour.
    pub nodes: Vec<u32>,
    /// An IJK grid's cells, in the scene's system — then `points` and
    /// `triangles` are empty, and a section cuts its envelope ([`Grid::cut`]).
    pub grid: Option<Grid>,
}

/// The first half of [`read_surface_coloured`]: read, judged, placed in the
/// scene and coloured — every refusal is here, none in the cut.
#[allow(clippy::result_large_err)]
pub fn prepare_surface(
    source: &dyn ValueSource,
    store: &str,
    uri: &str,
    scene: &Scene<'_>,
    colours: Colouring<'_>,
) -> Result<Prepared, Refused> {
    let mut object = ObjectId {
        store: store.to_string(),
        uri: uri.to_string(),
        uuid: uuid_of_uri(uri),
        title: String::new(),
        qualified_type: String::new(),
    };
    let refuse = |object: &ObjectId, reason: String| Refused {
        object: object.clone(),
        reason,
    };

    let held = source
        // By the uri the section was given, never by its uuid: two versions of
        // an object share a uuid, and only the uri tells them apart.
        .read_object_at(uri)
        .map_err(|why| refuse(&object, format!("`{uri}` cannot be read from the package: {why}")))?;
    let representation = held.as_reflect();
    let info = representation.type_info();
    if let Some(uuid) = identity::uuid_of_reflect(representation) {
        object.uuid = uuid.to_ascii_lowercase();
    }
    object.title = identity::title_of_reflect(representation)
        .unwrap_or_default()
        .to_string();
    object.qualified_type = energyml_core::json::qualified_type_of(info);

    // An IJK grid draws as cells, but what a section shows of it is its
    // envelope, and cutting the cell faces would draw every cell wall of the
    // grid: it has its own reading (`grids`).
    if info.type_key == "IjkGridRepresentation" {
        let frame = frame_of(source, representation).map_err(|why| refuse(&object, why))?;
        let named = |why: String| format!("`{}` ({}): {why}", object.title, object.qualified_type);
        let read = scene::object_of(representation, source, uri)
            .map_err(|why| refuse(&object, named(format!("{why}"))))?;
        let placed = read
            .geometry_in(Placement::Projected)
            .map_err(|why| refuse(&object, named(format!("{why}"))))?;
        let patch = placed
            .patches
            .into_iter()
            .next()
            .ok_or_else(|| refuse(&object, named("the grid has no patch".to_string())))?;
        let mut patch = patch;
        to_scene(&mut patch.points, &frame, scene).map_err(|why| refuse(&object, named(why)))?;
        let grid = Grid::from_patch(&patch).map_err(|why| refuse(&object, named(why)))?;
        let colour = file_colour_of(colours, &object, source);
        return Ok(Prepared {
            role: role_of(representation, &read, source),
            time: frame.time,
            colour,
            age: age_of_represented(representation, source),
            feature: feature_of(&read),
            object,
            points: Vec::new(),
            triangles: Vec::new(),
            nodes: Vec::new(),
            grid: Some(grid),
        });
    }
    if !energyml_geometry::geometry::is_supported(representation) {
        return Err(refuse(
            &object,
            format!(
                "`{}` is a {}, which this build does not read as a shape",
                object.title, object.qualified_type
            ),
        ));
    }

    let frame = frame_of(source, representation).map_err(|why| refuse(&object, why))?;
    let read =
        scene::object_of(representation, source, uri).map_err(|why| refuse(&object, format!("{why}")))?;
    let drawn = read
        .drawable_in(Placement::Projected)
        .map_err(|why| refuse(&object, format!("{why}")))?;
    if drawn.triangles.is_empty() {
        return Err(refuse(
            &object,
            format!(
                "`{}` ({}) has no triangles — {} point(s) and {} line segment(s): only a triangulated shape is cut",
                object.title,
                object.qualified_type,
                drawn.points.len(),
                drawn.lines.len()
            ),
        ));
    }

    let nodes = drawn_nodes(&read.geometry, drawn.points.len());
    let mut points = drawn.points;
    // Named: the reason of `to_scene` speaks of "the object", and a list of
    // refusals is read without the rows' titles beside it (the CLI's).
    to_scene(&mut points, &frame, scene).map_err(|why| {
        refuse(
            &object,
            format!("`{}` ({}): {why}", object.title, object.qualified_type),
        )
    })?;

    let colour = file_colour_of(colours, &object, source);
    Ok(Prepared {
        role: role_of(representation, &read, source),
        time: frame.time,
        colour,
        age: age_of_represented(representation, source),
        feature: feature_of(&read),
        object,
        points,
        triangles: drawn.triangles,
        nodes,
        grid: None,
    })
}

/// The representation's node number of each point `drawable` kept
/// ([`Prepared::nodes`]).
///
/// `drawable` keeps the nodes that are locations, in order, and renumbers: so
/// the kept nodes are the finite ones, and the i-th kept is the i-th finite of
/// the raw numbering. **The count is the check**: if the finite nodes are not
/// exactly as many as the points kept (a volume's node that no cell names is
/// dropped too, `orphans_as_holes`), the mapping is not this one and nothing is
/// claimed — empty.
pub fn drawn_nodes(geometry: &energyml_geometry::geometry::Geometry, kept: usize) -> Vec<u32> {
    let finite: Vec<u32> = geometry
        .patches
        .iter()
        .flat_map(|patch| patch.points.iter())
        .enumerate()
        .filter(|(_, point)| point.iter().all(|c| c.is_finite()))
        .map(|(at, _)| at as u32)
        .collect();
    if finite.len() == kept { finite } else { Vec::new() }
}

/// The second half of [`read_surface_coloured`]: a prepared surface cut along
/// every section. Pure arithmetic — nothing is read, nothing refused.
pub fn cut_prepared(prepared: &Prepared, sections: &[SectionTrace]) -> SurfaceRead {
    if let Some(grid) = &prepared.grid {
        return SurfaceRead {
            object: prepared.object.clone(),
            role: prepared.role,
            time: prepared.time,
            colour: prepared.colour.clone(),
            age: prepared.age,
            feature: prepared.feature.clone(),
            cuts: grid.cut(sections),
        };
    }
    // One pass for every section (`cut_many`): the sheet's boxes are computed
    // once, and a cross section a few hundred metres long tests a few
    // triangles of a surface of millions.
    let traces: Vec<&[[f64; 2]]> = sections.iter().map(|one| one.trace.as_slice()).collect();
    let all = section::cut_many_sourced(&prepared.points, &prepared.triangles, &traces);

    let mut cuts = Vec::new();
    for (trace, found) in sections.iter().zip(all) {
        // A section the surface does not cross has no entry — `Missed`, if it
        // crosses none, is said by the row, not by an empty cut here.
        if found.is_empty() {
            continue;
        }
        let made = fence::curve_sourced(&found);
        // The edge each point lies on, in the representation's numbering (the
        // paint's — `Prepared::nodes`). One list per span, one source per
        // point; if any node cannot be translated, the cut says nothing
        // rather than a part of it.
        let sources = sources_of(prepared, &made.sources).unwrap_or_default();
        cuts.push(SurfaceCut {
            section: trace.key,
            shape: CutShape::Trace {
                spans: made.curve.spans,
            },
            notes: made.curve.notes,
            sources,
        });
    }

    SurfaceRead {
        object: prepared.object.clone(),
        role: prepared.role,
        time: prepared.time,
        colour: prepared.colour.clone(),
        age: prepared.age,
        feature: prepared.feature.clone(),
        cuts,
    }
}

/// The cut's sources, translated into the representation's node numbering —
/// `None` when the preparation holds no numbering, or a node is out of it.
///
/// A source whose end was **clipped** to the section's window lies inside its
/// triangle, not on an edge: it carries the nearer crossing's edge and `t`
/// (`CutEnd::clipped`), so its colour is that crossing's — the contract has no
/// way to say more, and the clipped end of a span is a fraction of one triangle
/// from the crossing it takes.
fn sources_of(prepared: &Prepared, sources: &[Vec<section::CutEnd>]) -> Option<Vec<Vec<CutSource>>> {
    if prepared.nodes.is_empty() {
        return None;
    }
    let number = |at: u32| prepared.nodes.get(at as usize).copied();
    sources
        .iter()
        .map(|span| {
            span.iter()
                .map(|end| {
                    Some(CutSource {
                        a: number(end.a)?,
                        b: number(end.b)?,
                        t: end.t,
                    })
                })
                .collect::<Option<Vec<_>>>()
        })
        .collect()
}

/// What the surface is, from the **type of the interpretation** it represents —
/// `RepresentedInterpretation` in RESQML 2.0.1, `RepresentedObject` in 2.2, the
/// pair `scene::SceneObject::interpretation` already reads. Never the title: a
/// horizon called "Fault_3" is a horizon.
///
/// The interpretation is resolved in the package when it can be (its own type
/// is the fact), and the reference's qualified type is the fallback. No
/// interpretation, or one that is neither: `Other`.
/// The colour the package gives an object: the library's cascade over the
/// package's sets — the representation, then its interpretation, then its
/// feature (`energyml_domain::color::Cascade`, where the 3D viewer's ladder
/// starts). Only a **constant** colour answers here: a map colours by a
/// property, and a section draws a surface as one line. `None` when nothing in
/// the file speaks — the drawing then falls back on the role's colour, and never
/// invents one in the file's name.
///
/// What this does not walk: the inheritance through collections and models
/// (`color_inherit`), which needs the package's index — a face that has it
/// passes the colour in the view instead (`View::colours`).
pub fn file_colour(colouring: Colouring<'_>, uuid: &str, source: &dyn ValueSource) -> Option<String> {
    let set = colouring.set?;
    cascade_colour(set, colouring.index, uuid, source)
}

/// [`file_colour`] for an object of the package: the face's resolver, when
/// given, is asked by the object's **full uri** — a uuid would hand both
/// versions of a surface the colour of one.
pub fn file_colour_of(
    colouring: Colouring<'_>,
    object: &ObjectId,
    source: &dyn ValueSource,
) -> Option<String> {
    // The face already knows: no search is made again.
    if let Some(resolver) = colouring.resolver {
        return resolver(&object.uri);
    }
    file_colour(colouring, &object.uuid, source)
}

/// The cascade's constant colour for `uuid` over `set`.
fn cascade_colour(
    set: &GraphicalSet,
    index: Option<&PackageIndex>,
    uuid: &str,
    source: &dyn ValueSource,
) -> Option<String> {
    let cascade = Cascade::new(set);
    let decision = match index {
        // The whole order — the 3D viewer's (`Cascade::inherited`): Lutetia
        // colours its horizons through the collections they belong to, which
        // the shorter walk below never reaches.
        Some(index) => cascade
            .inherited(index, source, uuid)
            .ok()
            .and_then(|walked| walked.resolution)
            .map(|found| found.value),
        None => {
            let request = Request {
                representation: uuid,
                property: None,
                kind_chain: &[],
                element: Scope::WholeObject,
            };
            cascade.resolve(&request, source).ok().flatten()
        }
    };
    match decision {
        Some(Decision {
            paint: Paint::Constant(colour),
            ..
        }) => Some(colour.to_hex()),
        _ => None,
    }
}

fn role_of(representation: &dyn Reflect, read: &SceneObject, source: &dyn ValueSource) -> TraceRole {
    let Some(related) = &read.interpretation else {
        return TraceRole::Other;
    };
    // Followed by the representation's reference, so the version it names is
    // the one whose type decides the role.
    let resolved = scene::interpretation_of(representation, source);
    let key: &str = match &resolved {
        Some(found) => found.as_reflect().type_info().type_key,
        // A qualified type (`resqml22.FaultInterpretation`) or, in 2.0.1, a
        // content type (`…;version=2.0;type=obj_FaultInterpretation`).
        None => related.qualified_type.rsplit(['.', '=']).next().unwrap_or(""),
    };
    // `obj_` prefixes a 2.0.1 type in a qualified type, never in a type key.
    match key.strip_prefix("obj_").unwrap_or(key) {
        "HorizonInterpretation" => TraceRole::Horizon,
        "FaultInterpretation" => TraceRole::Fault,
        _ => TraceRole::Other,
    }
}

/// The age of the interpretation a surface represents, `[old, young]` in
/// millions of years before present — **an interval, never a bare value**: a
/// horizon may state a span (`HasOccurredDuring`, or `OlderPossibleAge` and
/// `YoungerPossibleAge`) and the detection judges only what is disjoint, so a
/// point is the interval `[a, a]` and nothing is lost by it.
///
/// Where the interpretation says it, first found wins:
///
/// 1. `HasOccurredDuring` → `GeologicTimeBasedTimeInterval`, whose `Start` and
///    `End` each carry an `AgeOffsetAttribute` in years (positive into the
///    past, eml 2.3) — the reading `energyml_domain::stratigraphy` gives a
///    unit's age;
/// 2. `OlderPossibleAge` **and** `YoungerPossibleAge` (RESQML 2.2, years);
/// 3. `AbsoluteAge` (a `GeologicTime`), as the point.
///
/// `None` is the rule, not an error: RESQML 2.0.1 states no age on a horizon
/// interpretation (its feature's `AbsoluteAge` is a timestamp, not an age), a
/// stratigraphic unit's age is on the **unit** and no horizon names its unit
/// here, and the corpus dates almost nothing. Nothing is derived from a name, a
/// depth or the order of a column.
pub fn age_of(interpretation: &dyn Reflect) -> Option<[f64; 2]> {
    let years = |object: &dyn Reflect, name: &str| -> Option<f64> {
        match object.field(name)? {
            FieldValue::Scalar(scalar) => scalar
                .to_string()
                .trim()
                .parse::<i64>()
                .ok()
                .map(|y| y as f64 / 1_000_000.0),
            _ => None,
        }
    };
    let ordered = |a: f64, b: f64| [a.max(b), a.min(b)];
    if let Some(FieldValue::Object(interval)) = interpretation.field("HasOccurredDuring") {
        let bound = |name: &str| match interval.field(name)? {
            FieldValue::Object(time) => years(time, "AgeOffsetAttribute"),
            _ => None,
        };
        if let (Some(start), Some(stop)) = (bound("Start"), bound("End")) {
            return Some(ordered(start, stop));
        }
    }
    if let (Some(older), Some(younger)) = (
        years(interpretation, "OlderPossibleAge"),
        years(interpretation, "YoungerPossibleAge"),
    ) {
        return Some(ordered(older, younger));
    }
    if let Some(FieldValue::Object(time)) = interpretation.field("AbsoluteAge")
        && let Some(age) = years(time, "AgeOffsetAttribute")
    {
        return Some([age, age]);
    }
    None
}

/// The feature the interpretation of `read` interprets — the reflection the
/// scene already does (`SceneObject::feature`: `InterpretedFeature` in 2.0.1 and
/// 2.2, through the interpretation), as a lowercase uuid. `None` when the
/// object names no interpretation, or the package cannot be followed to the
/// feature: two surfaces are then never taken for one horizon.
fn feature_of(read: &SceneObject) -> Option<String> {
    read.feature
        .as_ref()
        .map(|f| f.uuid.to_ascii_lowercase())
        .filter(|u| !u.is_empty())
}

/// The age of what `read` represents, when it is in the package and states one.
fn age_of_represented(representation: &dyn Reflect, source: &dyn ValueSource) -> Option<[f64; 2]> {
    let held = scene::interpretation_of(representation, source)?;
    age_of(held.as_reflect())
}

/// Whether an object of this qualified type is one a section cuts: a
/// triangulated surface, a 2D grid (a horizon on a lattice — Lutetia's are), or
/// an IJK grid (its envelope).
///
/// What "every surface of the package" means for a face that lists a package
/// (the CLI's default) — decided here, so the front end and the CLI agree.
pub fn is_cut_type(qualified_type: &str) -> bool {
    [
        "TriangulatedSetRepresentation",
        "Grid2dRepresentation",
        "IjkGridRepresentation",
    ]
    .iter()
    .any(|kind| qualified_type.ends_with(kind))
}
