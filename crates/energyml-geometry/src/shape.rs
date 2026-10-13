//! Asking a representation for a *form* rather than taking the one it has.
//!
//! # Why this exists
//!
//! A representation carries one topology, and an exporter used to be stuck with
//! it: a 2D grid came out as loose points because that is what
//! [`Topology::Grid`] literally is, even though its cells are perfectly well
//! defined and a horizon drawn as 300 000 dots is not what anyone wanted.
//!
//! But "always draw a grid's cells" would only move the problem. Someone
//! exporting to GeoJSON for QGIS wants polygons; someone feeding a point-cloud
//! pipeline wants the nodes; someone checking a triangulated surface's node
//! spacing wants its points and not its faces. **The right form depends on what
//! the file is for, which is knowledge the caller has and this crate does not.**
//!
//! So the caller asks, in the vocabulary of what is on screen rather than of
//! what is in the schema:
//!
//! | [`Shape`] | What comes out |
//! |---|---|
//! | [`Shape::Auto`] | the richest form this topology supports — the default |
//! | [`Shape::Points`] | the nodes, and nothing joining them |
//! | [`Shape::Lines`] | polylines |
//! | [`Shape::Surface`] | triangles and quadrilaterals |
//! | [`Shape::Volume`] | cells with an inside |
//! | [`Shape::Outline`] | **one** polygon: the convex hull of the whole object |
//! | [`Shape::Centroid`] | **one** point: the mean of the nodes |
//!
//! # What is derived, and what is refused
//!
//! Every conversion here is **arithmetic on indices the file already states**.
//! Nothing invents a connection: a grid's cell corners come from the indexing
//! rule RESQML writes down (`index = fastest + fastestCount × slowest`), a
//! hexahedron's faces come from the corner order the topology already
//! guarantees, and edges come from cells that already exist.
//!
//! What is **refused, by name**, is everything that would need a decision the
//! file does not contain:
//!
//! - **Polylines → Surface.** A line is not a boundary unless it closes, and
//!   RESQML states closure in a separate boolean array this build does not
//!   read. Filling a set of lines would draw a surface nobody described.
//! - **Anything → Volume that has no inside.** A triangulated surface is not a
//!   solid, and pretending otherwise is not a rendering choice, it is a
//!   different object.
//! - **Points → anything.** Loose points carry no connection to recover.
//!
//! Three conversions that *are* available, and all three are **derived
//! boundaries** rather than inventions:
//!
//! - **Cells → Lines** gives the wireframe: the edges of the cells, each
//!   stored once, as [`Topology::Segments`] — the indexed-segment topology
//!   this module's earlier refusal said would make it work with no change
//!   here. The edges are the *cells'* edges, never the triangulation's: a
//!   quad contributes four, not the diagonal its drawing splits it by, because
//!   the diagonal is an artefact of rendering and not a line the file
//!   describes. A volume goes through its boundary first, so its wireframe is
//!   the outside's edges — the same rule as its surface. Decided in favour of
//!   a wireframe over a named refusal: the precedent is the skin below, and an edge of a
//!   cell is data-derived exactly the way a boundary face is.
//!
//! - **Hexahedra → Surface** and **Polyhedra → Surface** give the grid's
//!   **outside**: the faces exactly one cell claims, wound outward. Emitting
//!   every face instead draws each interior one twice, one of the two facing
//!   the camera, and a hundred-thousand-cell grid becomes an opaque block
//!   z-fighting with itself at ten times the cost of the picture wanted.
//!   Polyhedra come back as [`Topology::Polygons`] because a face is not
//!   always four corners; [`Shape::Volume`] still keeps them whole, and VTK
//!   writes those natively.
//!
//! A refusal names both ends — *"a PolylineSet cannot be exported as a
//! surface"* — because the caller who asked for it wrote a rule, and a rule
//! that silently does nothing is worse than one that fails.

use std::collections::HashMap;

use crate::error::{Error, Result};
use crate::geometry::{Geometry, Patch, Topology};
use crate::points::Point;

/// The form a caller wants a representation drawn in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Shape {
    /// The richest form this topology supports.
    ///
    /// A grid becomes its cells, a polyline set stays lines, a hexahedral grid
    /// stays a volume. What every export did before there was anything to ask.
    #[default]
    Auto,
    /// The nodes alone.
    Points,
    /// Polylines.
    Lines,
    /// Triangles and quadrilaterals.
    Surface,
    /// Cells with an inside.
    Volume,
    /// One polygon: the convex hull of every node, whatever the topology was.
    ///
    /// A **summary**, not a rendering — the first of the two shapes here whose
    /// output is not the object. It answers "where is this", which is what a
    /// discovery index wants and what OSDU's `SpatialArea` asks for: *"a polygon
    /// boundary that reflects the locale of the content"*. (OSDU).
    ///
    /// Convex and not concave, deliberately: an alpha-shape follows a horizon's
    /// bays and holes far better, and it needs a parameter whose wrong value
    /// silently cuts the outline into pieces. A hull has no parameter and no
    /// wrong answer — it is simply the smallest convex thing containing the
    /// data.
    Outline,
    /// One point: the mean of the nodes.
    ///
    /// The other summary, and OSDU's `SpatialPoint`: *"a centroid point that
    /// reflects the locale of the content"*.
    ///
    /// The mean of the **nodes**, which is not the centroid of the area — a
    /// densely sampled corner pulls it. Saying so rather than implying a
    /// precision this does not have; for "where is this on a map", at the scale
    /// these are used, the difference is invisible.
    Centroid,
}

impl Shape {
    /// The name a caller writes on a command line.
    pub fn name(&self) -> &'static str {
        match self {
            Shape::Auto => "auto",
            Shape::Points => "points",
            Shape::Lines => "lines",
            Shape::Surface => "surface",
            Shape::Volume => "volume",
            Shape::Outline => "outline",
            Shape::Centroid => "centroid",
        }
    }

    /// Every shape there is, for a caller listing them.
    pub fn all() -> [Shape; 7] {
        [
            Shape::Auto,
            Shape::Points,
            Shape::Lines,
            Shape::Surface,
            Shape::Volume,
            Shape::Outline,
            Shape::Centroid,
        ]
    }

    /// Whether this shape summarises the object instead of drawing it.
    ///
    /// The two are different in a way a caller has to know about: a summary
    /// **replaces the points**, so nothing that hangs on nodes or cells — a
    /// property, a colour ramp — comes through it. Four points that say where a
    /// horizon is cannot carry a value per node of the horizon.
    pub fn is_summary(&self) -> bool {
        matches!(self, Shape::Outline | Shape::Centroid)
    }

    /// Parse a name, case-insensitively.
    ///
    /// The plural and singular both work — someone writing `--as Grid=polygon`
    /// means the same thing as `surface`, and refusing them over an `s` teaches
    /// nothing.
    pub fn parse(text: &str) -> Option<Shape> {
        match text.trim().to_ascii_lowercase().as_str() {
            "auto" | "default" | "natural" => Some(Shape::Auto),
            "point" | "points" | "pointcloud" | "multipoint" => Some(Shape::Points),
            "line" | "lines" | "polyline" | "polylines" | "multilinestring" => Some(Shape::Lines),
            "surface" | "surfaces" | "polygon" | "polygons" | "multipolygon" | "face" | "faces" => {
                Some(Shape::Surface)
            }
            "volume" | "volumes" | "cell" | "cells" | "solid" => Some(Shape::Volume),
            "outline" | "hull" | "footprint" | "extent" | "area" => Some(Shape::Outline),
            "centroid" | "centre" | "center" | "middle" => Some(Shape::Centroid),
            _ => None,
        }
    }
}

impl std::fmt::Display for Shape {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

impl Topology {
    /// The richest form this topology supports.
    pub fn natural_shape(&self) -> Shape {
        match self {
            Topology::Points => Shape::Points,
            Topology::Polylines(_) | Topology::Segments(_) => Shape::Lines,
            Topology::Triangles(_) | Topology::Quads(_) | Topology::Polygons(_) => Shape::Surface,
            Topology::Grid { .. } => Shape::Surface,
            Topology::Hexahedra { .. } | Topology::Polyhedra(_) => Shape::Volume,
        }
    }

    /// Whether this topology already **is** the shape asked for.
    ///
    /// The one place that question is answered, so the fast path and the
    /// conversion cannot disagree. [`Topology::Grid`] is the exception worth
    /// naming: its natural shape is a surface, but it is not one *yet* — its
    /// cells have to be derived from the lattice indexing first.
    pub fn is_already(&self, shape: Shape) -> bool {
        // A summary is never what an object already is: it replaces the points.
        if shape.is_summary() || matches!(self, Topology::Grid { .. }) {
            return false;
        }
        matches!(shape, Shape::Auto) || shape == self.natural_shape()
    }

    /// A word for this topology, for an error message a person has to act on.
    pub fn describe(&self) -> &'static str {
        match self {
            Topology::Points => "a point set",
            Topology::Triangles(_) => "a triangulated surface",
            Topology::Quads(_) => "a quadrilateral surface",
            Topology::Polygons(_) => "a polygonal surface",
            Topology::Polylines(_) => "a set of polylines",
            Topology::Segments(_) => "a wireframe of segments",
            Topology::Grid { .. } => "a 2D grid",
            Topology::Hexahedra { .. } => "a hexahedral grid",
            Topology::Polyhedra(_) => "an unstructured grid",
        }
    }
}

impl Geometry {
    /// Whether every patch can be drawn in this shape.
    ///
    /// Asked *before* an export starts, so a caller can report or skip rather
    /// than find a half-written file — the same reason
    /// `Exporter::handles` exists.
    pub fn can_be(&self, shape: Shape) -> bool {
        self.patches
            .iter()
            .all(|patch| reshape(&patch.topology, &patch.points, shape).is_ok())
    }

    /// This geometry in the shape asked for, or a refusal naming both ends.
    ///
    /// The points are untouched — every conversion here is arithmetic on
    /// indices — so a node's position, and anything indexed on nodes, survives
    /// unchanged. What changes is only what joins them.
    pub fn as_shape(&self, shape: Shape) -> Result<Geometry> {
        // A summary is about the **object**, not a patch of it: the hull of a
        // surface cut into eleven patches is one hull, not eleven. So this one
        // branch looks across all of them and returns a single patch, which is
        // also why it is the only case where the points change.
        if shape.is_summary() {
            return self.summarise(shape);
        }

        let mut patches = Vec::with_capacity(self.patches.len());
        for patch in &self.patches {
            patches.push(Patch {
                index: patch.index,
                node_offset: patch.node_offset,
                points: patch.points.clone(),
                topology: reshape(&patch.topology, &patch.points, shape)?,
                closed: Vec::new(),
            });
        }
        Ok(Geometry {
            kind: self.kind,
            patches,
        })
    }

    /// The cell edges, **indexed on the points [`Geometry::drawable`] gives**.
    ///
    /// # The bug this exists for
    ///
    /// Reported as *IJK grids with edges shooting off to the origin*,
    /// and the reading was never the problem: a viewer drew the faces from
    /// `drawable()` and the wireframe from `as_shape(Lines).drawable()`, and
    /// **those two do not share a point list**.
    ///
    /// They diverge for exactly one reason, and it is a rule this crate applies
    /// on purpose: a node of a *volume* that no cell names is not a location —
    /// `CellGeometryIsDefined` marks cells undefined while their coordinates
    /// stay perfectly finite in the file — so [`Geometry::drawable`] turns those
    /// orphans into holes and drops them. Reshaping to segments first destroys
    /// the volume topology, nothing is an orphan of a segment list, and every
    /// node survives. On `h+g_PaleoGTM.epc` that is 37 772 points against the
    /// mesh's 36 363, and **156 099 of the wireframe's indices addressed
    /// vertices that were not there**.
    ///
    /// Out-of-range indices are not an error a viewer reports; WebGL reads
    /// whatever the buffer holds, so the edges land somewhere — near the origin,
    /// as it happened. A wrong picture with no message is this workspace's
    /// least favourite kind of failure.
    ///
    /// # Why the orphans are holed first
    ///
    /// So that the same nodes are dropped, in the same order, by the same
    /// compaction. The survivors of a reshaped geometry are then exactly the
    /// survivors of the original, and an index means the same thing in both —
    /// which is the property a caller drawing one buffer with two index lists
    /// needs, and the one it cannot check for itself.
    pub fn wireframe(&self) -> Result<Vec<[u32; 2]>> {
        let holed = Geometry {
            kind: self.kind,
            patches: self
                .patches
                .iter()
                .map(|patch| Patch {
                    index: patch.index,
                    node_offset: patch.node_offset,
                    points: crate::geometry::orphans_as_holes(&patch.points, &patch.topology)
                        .into_owned(),
                    topology: patch.topology.clone(),
                    closed: Vec::new(),
                })
                .collect(),
        };
        Ok(holed.as_shape(Shape::Lines)?.drawable().lines)
    }

    /// One patch that says *where* this object is, rather than what it is.
    fn summarise(&self, shape: Shape) -> Result<Geometry> {
        let nodes: Vec<Point> = self
            .patches
            .iter()
            .flat_map(|patch| patch.points.iter().copied())
            .filter(crate::defined::is_defined)
            .collect();
        if nodes.is_empty() {
            return Err(Error::shape("an object with no placed node", "a summary"));
        }

        let (points, topology) = match shape {
            Shape::Centroid => {
                let count = nodes.len() as f64;
                let mean = |axis: usize| nodes.iter().map(|p| p[axis]).sum::<f64>() / count;
                (vec![[mean(0), mean(1), mean(2)]], Topology::Points)
            }
            _ => {
                let ring = hull(&nodes);
                let indices: Vec<u32> = (0..ring.len() as u32).collect();
                // Fewer than three points cannot bound an area — a straight
                // line of nodes, or two of them. The nodes themselves are the
                // honest answer, and a caller who asked for an area gets told
                // by the geometry type rather than by a degenerate polygon.
                let topology = if ring.len() >= 3 {
                    Topology::Polygons(vec![indices])
                } else {
                    Topology::Points
                };
                (ring, topology)
            }
        };

        Ok(Geometry {
            kind: self.kind,
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points,
                topology,
                closed: Vec::new(),
}],
        })
    }
}

/// The convex hull of a set of points, in XY, counterclockwise.
///
/// Andrew's monotone chain: sort, then sweep the lower and upper boundaries,
/// dropping any point that does not turn left. `O(n log n)`, dominated by the
/// sort, and exact in the sense that matters — every input point is inside or
/// on the result, and no smaller convex polygon is.
///
/// Z is carried through from the point that contributed each corner rather than
/// recomputed. A hull is a *plan* view, so its corners have no depth of their
/// own; taking the one that was actually there beats inventing a mean.
fn hull(points: &[Point]) -> Vec<Point> {
    let mut sorted = points.to_vec();
    sorted.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    sorted.dedup_by(|a, b| a[0] == b[0] && a[1] == b[1]);
    if sorted.len() < 3 {
        return sorted;
    }

    // > 0 when o→a→b turns left.
    let cross = |o: &Point, a: &Point, b: &Point| {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };

    let mut chain: Vec<Point> = Vec::with_capacity(sorted.len() * 2);
    for pass in 0..2 {
        let start = chain.len();
        let iter: Box<dyn Iterator<Item = &Point>> = if pass == 0 {
            Box::new(sorted.iter())
        } else {
            Box::new(sorted.iter().rev())
        };
        for point in iter {
            while chain.len() >= start + 2
                && cross(&chain[chain.len() - 2], &chain[chain.len() - 1], point) <= 0.0
            {
                chain.pop();
            }
            chain.push(*point);
        }
        // The last point of each pass is the first of the next.
        chain.pop();
    }
    chain
}

/// One patch's topology, in the shape asked for.
/// The edges of a set of cells, each stored **once**.
///
/// Undirected: the shared edge of two neighbouring cells is one line, not two
/// drawn on top of each other — the same de-duplication reasoning as a
/// volume's boundary, one dimension down. The pair is normalised low-first for
/// the lookup and kept in first-seen order in the output, so the result is
/// stable for a stable input.
fn unique_edges<'a>(cells: impl Iterator<Item = &'a [u32]>) -> Vec<[u32; 2]> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for ring in cells {
        for at in 0..ring.len() {
            let (a, b) = (ring[at], ring[(at + 1) % ring.len()]);
            if a == b {
                continue;
            }
            if seen.insert((a.min(b), a.max(b))) {
                out.push([a, b]);
            }
        }
    }
    out
}

pub(crate) fn reshape(topology: &Topology, points: &[Point], shape: Shape) -> Result<Topology> {
    let wanted = match shape {
        Shape::Auto => topology.natural_shape(),
        other => other,
    };
    if topology.is_already(wanted) {
        return Ok(topology.clone());
    }

    match wanted {
        // Always available: dropping what joins the nodes needs nothing from
        // the file.
        Shape::Points => Ok(Topology::Points),

        // Only from polylines, and the reason is a limit worth stating rather
        // than working around — until `Topology::Segments` existed. The edges
        // are the **cells'**, never the triangulation's: a quad contributes
        // four, not the diagonal its drawing splits it by. A volume goes
        // through its boundary first, so its wireframe is the outside's edges,
        // the same rule as its surface.
        Shape::Lines => match topology {
            Topology::Polylines(_) | Topology::Segments(_) => Ok(topology.clone()),
            Topology::Triangles(cells) => Ok(Topology::Segments(unique_edges(
                cells.iter().map(|cell| cell.as_slice()),
            ))),
            Topology::Quads(cells) => Ok(Topology::Segments(unique_edges(
                cells.iter().map(|cell| cell.as_slice()),
            ))),
            Topology::Polygons(rings) => Ok(Topology::Segments(unique_edges(
                rings.iter().map(|ring| ring.as_slice()),
            ))),
            Topology::Grid { fastest, slowest } => {
                let cells = grid_cells(*fastest, *slowest, points);
                Ok(Topology::Segments(unique_edges(
                    cells.iter().map(|cell| cell.as_slice()),
                )))
            }
            Topology::Hexahedra { cells, .. } => {
                let boundary = boundary_of(cells, points);
                Ok(Topology::Segments(unique_edges(
                    boundary.iter().map(|cell| cell.as_slice()),
                )))
            }
            Topology::Polyhedra(cells) => {
                let boundary = cells.boundary();
                Ok(Topology::Segments(unique_edges(
                    boundary.iter().map(|ring| ring.as_slice()),
                )))
            }
            _ => Err(refuse(topology, "lines")),
        },

        Shape::Surface => match topology {
            Topology::Triangles(_) | Topology::Quads(_) | Topology::Polygons(_) => {
                Ok(topology.clone())
            }
            Topology::Grid { fastest, slowest } => {
                Ok(Topology::Quads(grid_cells(*fastest, *slowest, points)))
            }
            Topology::Hexahedra { cells, .. } => Ok(Topology::Quads(boundary_of(cells, points))),
            // The same rule one level of generality up. A hexahedron's faces
            // are quadrilaterals and its sharing has to be rediscovered by
            // hashing corner sets; an unstructured cell's faces are rings of
            // any length and the file already says which cells meet at which
            // face — so the boundary is counted, not reconstructed, and the
            // result is `Polygons` because a face is not always four corners.
            Topology::Polyhedra(cells) => Ok(Topology::Polygons(cells.boundary())),
            _ => Err(refuse(topology, "a surface")),
        },

        Shape::Volume => match topology {
            Topology::Hexahedra { .. } | Topology::Polyhedra(_) => Ok(topology.clone()),
            _ => Err(refuse(topology, "a volume")),
        },

        // `Auto` was resolved above; the summaries never reach here, because
        // `Geometry::as_shape` handles them across the whole object.
        Shape::Auto | Shape::Outline | Shape::Centroid => Ok(topology.clone()),
    }
}

/// The refusal, naming what was asked of what.
///
/// Both ends, always: a caller who wrote `--as PolylineSet=surface` needs to be
/// told which rule of theirs met which object, not that "an export failed".
fn refuse(topology: &Topology, wanted: &'static str) -> Error {
    Error::shape(topology.describe(), wanted)
}

/// The cells of a 2D grid, four corners each, wound the right way round.
///
/// The corner indices come from the rule RESQML writes for a `Grid2dPatch` —
/// *"Index = FastestIndex + FastestAxisCount × SlowestIndex"* — so nothing here
/// is a convention this crate chose. The **winding** is not stated anywhere, so
/// it is not assumed either: the signed area is measured and the ring reversed
/// when it comes out negative, which is what RFC 7946 asks of a GeoJSON
/// exterior ring and what a renderer reads as front-facing.
///
/// A cell whose corners fall outside the point array is skipped rather than
/// clamped: the counts and the geometry disagreeing is a malformed file, and
/// bending an index would draw a cell across the grid.
fn grid_cells(fastest: usize, slowest: usize, points: &[Point]) -> Vec<[u32; 4]> {
    if fastest < 2 || slowest < 2 {
        return Vec::new();
    }
    let mut cells = Vec::with_capacity((fastest - 1) * (slowest - 1));
    for j in 0..slowest - 1 {
        for i in 0..fastest - 1 {
            let corner = |di: usize, dj: usize| ((j + dj) * fastest + i + di) as u32;
            let ring = [
                corner(0, 0),
                corner(1, 0),
                corner(1, 1),
                corner(0, 1),
            ];
            if ring.iter().any(|c| *c as usize >= points.len()) {
                continue;
            }
            cells.push(wound(ring, points));
        }
    }
    cells
}

/// A ring whose signed area in the XY plane is positive.
///
/// Measured rather than assumed — the same move `ijk::oriented` makes for a
/// hexahedron, and for the same reason: the flags that would decide it a priori
/// interact, and each combination is a chance to be wrong. A degenerate ring —
/// zero area, a collapsed cell — is left as it came, because there is no
/// orientation to find and reversing it would only be noise.
fn wound(ring: [u32; 4], points: &[Point]) -> [u32; 4] {
    let mut area = 0.0;
    for corner in 0..4 {
        let a = points[ring[corner] as usize];
        let b = points[ring[(corner + 1) % 4] as usize];
        area += a[0] * b[1] - b[0] * a[1];
    }
    if area < 0.0 {
        [ring[3], ring[2], ring[1], ring[0]]
    } else {
        ring
    }
}

/// The outside of a hexahedral grid: the faces used by exactly one cell.
///
/// Every face of every cell would draw the interior ones twice, back to back —
/// which renders as z-fighting and multiplies a big grid's file size for
/// nothing anyone can see. A face shared by two cells is interior *by
/// definition*, so the boundary is the faces that appear once, and that is a
/// count rather than a guess.
///
/// The six faces come from the corner order [`Topology::Hexahedra`] already
/// guarantees (VTK's), so their winding is inherited rather than chosen.
fn boundary_of(cells: &[[u32; 8]], points: &[Point]) -> Vec<[u32; 4]> {
    // VTK's hexahedron: bottom 0-1-2-3, top 4-5-6-7.
    const FACES: [[usize; 4]; 6] = [
        [0, 3, 2, 1],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [1, 2, 6, 5],
        [2, 3, 7, 6],
        [3, 0, 4, 7],
    ];

    let mut seen: HashMap<[u32; 4], (usize, [u32; 4])> = HashMap::new();
    for cell in cells {
        for face in FACES {
            let ring = [
                cell[face[0]],
                cell[face[1]],
                cell[face[2]],
                cell[face[3]],
            ];
            // The key ignores winding and starting corner, so the same face
            // seen from either of its two cells hashes the same.
            let mut key = ring;
            key.sort_unstable();
            let entry = seen.entry(key).or_insert((0, ring));
            entry.0 += 1;
        }
    }
    seen.into_values()
        .filter(|(count, _)| *count == 1)
        .map(|(_, ring)| wound(ring, points))
        .collect()
}
