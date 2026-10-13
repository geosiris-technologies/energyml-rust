//! A representation, turned into patches of points and how they connect.
//!
//! RESQML splits a representation into **patches**, and the split is not
//! cosmetic: a triangulated surface has one patch per set of triangles, each
//! with its own point array and its own indices, and merging them would
//! renumber everything. So the result keeps them apart and leaves the decision
//! to whoever draws it.
//!
//! # Which representations
//!
//! | | |
//! |---|---|
//! | `TriangulatedSetRepresentation` | triangles, per patch |
//! | `PointSetRepresentation` | points, per patch |
//! | `PolylineRepresentation`, `PolylineSetRepresentation` | polylines |
//! | `Grid2dRepresentation` | a lattice, usually with z from a file |
//! | `IjkGridRepresentation` | pillars, split coordinate lines, cells of eight corners |
//! | `WellboreTrajectoryRepresentation` | the survey stations, joined |
//! | `Trajectory` (WITSML) | its stations, placed by minimum curvature from its datum |
//! | `UnstructuredGridRepresentation` | cells of explicit faces, faces of explicit nodes |
//! | `SubRepresentation` | elements picked out of another representation |
//!
//! A grid with no geometry of its own is not in that list and is not missing
//! from it: a **local grid** derives its points from its parent's window, which
//! is [`crate::parent`]'s business, reached through the IJK row above.
//!
//! The types are recognised by [`energyml_core::reflect::TypeInfo::type_key`],
//! which is the same across versions: one branch reads RESQML 2.0.1 and
//! RESQML 2.2 without knowing which it has.

use energyml_core::value::{FieldValue, Reflect};

use crate::error::{Error, Result};
use crate::points::{self, Point};
use crate::readers::{Readers, RepresentationReader};
use crate::source::ValueSource;
use crate::values;

/// How the points of a patch connect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Topology {
    /// Points that mean nothing together.
    Points,
    /// Triples of indices into the patch's points.
    Triangles(Vec<[u32; 3]>),
    /// Quadrilaterals: four indices into the patch's points, in ring order.
    ///
    /// What a 2D grid's cells are, and what a hexahedron's faces are. Kept as
    /// four corners rather than split into two triangles, because that is what
    /// the file says a cell *is* - OBJ, VTK and GeoJSON each write a
    /// quadrilateral directly, and halving it here would hand all three a
    /// decision none of them asked for.
    ///
    /// The ring is oriented so its signed area in the XY plane is positive -
    /// **measured, not assumed**. That is the winding RFC 7946 asks of a
    /// GeoJSON exterior ring, and the one a renderer reads as front-facing.
    Quads(Vec<[u32; 4]>),
    /// Rings of any length: one entry per ring, each a list of point indices.
    ///
    /// The general case [`Topology::Quads`] is the cheap special case of. Both
    /// exist on purpose: a grid produces hundreds of thousands of cells and a
    /// `Vec<u32>` each would be that many allocations, while an outline
    /// produces one ring of arbitrary length that no fixed array can hold.
    ///
    /// Wound like a quadrilateral - positive signed area in XY, measured - and
    /// **not closed**: the repeat of the first point is a GeoJSON requirement,
    /// added where GeoJSON is written rather than carried everywhere.
    Polygons(Vec<Vec<u32>>),
    /// One or more runs of consecutive points; each entry is a run's length.
    ///
    /// Closed polylines are not marked here - RESQML says so in a separate
    /// boolean array, and inventing a convention would be worse than leaving
    /// it out.
    Polylines(Vec<u32>),
    /// Cells of eight corners, indexed into the patch's points.
    ///
    /// The corners are in VTK's order - face `(0,1,2,3)`'s right-hand normal
    /// points at face `(4,5,6,7)` - because that is the one convention every
    /// consumer states explicitly, and a hexahedron built the other way round
    /// renders as a hole.
    Hexahedra {
        /// One entry per cell, eight point indices each.
        cells: Vec<[u32; 8]>,
        /// `Ni`, `Nj`, `Nk` - the shape the cells were built from.
        dimensions: [usize; 3],
        /// **The RESQML cell index of each entry**, or empty when they coincide.
        ///
        /// A position here is *not* always the number RESQML calls the cell.
        /// The reader drops a cell its `CellGeometryIsDefined` flag marks
        /// undefined, and one whose pillars carry no location - so `cells` is
        /// **compacted**, and on a grid with holes every entry after the first
        /// hole sits one place before the cell it belongs to.
        ///
        /// It matters to anything that addresses a cell **by the number the
        /// file uses**: a `GridConnectionSetRepresentation` names cell pairs
        /// that way, and a reader that indexed straight into `cells` would be
        /// right on an unbroken grid and quietly wrong on exactly the faulted
        /// and eroded ones a connection set exists for. PaleoGTM has 3 647
        /// undefined cells out of 33 048.
        ///
        /// **Empty means the identity** - nothing was dropped, so a position is
        /// the index - which is the ordinary case and costs no memory on a grid
        /// of ten million cells. Read it through [`Topology::cell_at`] rather
        /// than directly, so that convention lives in one place.
        index: Vec<u32>,
    },
    /// A regular grid, `fastest` × `slowest` points, row by row.
    Grid {
        /// Points along the fastest-varying axis.
        fastest: usize,
        /// Points along the slowest-varying axis.
        slowest: usize,
    },
    /// Cells bounded by faces of any shape: an unstructured grid.
    Polyhedra(Polyhedra),
    /// Indexed segments: pairs of node indices, each pair one edge.
    ///
    /// What [`Topology::Polylines`] cannot say - that topology addresses
    /// **consecutive** points, and the edges of a wireframe join nodes that
    /// are nowhere near each other in the array. This is the variant its
    /// module doc promised (*"adding `Topology::Segments` later would make it
    /// work with no change here"*), and what `Shape::Lines` on a surface now
    /// produces: the **cell edges**, each stored once. Derived data, like the
    /// skin of a volume - not an invention.
    Segments(Vec<[u32; 2]>),
}

impl Topology {
    /// How many **cells** this topology holds - one per triangle, quad, ring,
    /// polyline or volume cell, which is what a per-cell property has one
    /// value for. The one definition; [`Geometry::cell_count`] and the
    /// triangle→cell numbering of [`Geometry::drawable`] both sum it, and two
    /// copies of this match is how they would drift apart.
    pub fn cell_count(&self) -> usize {
        match self {
            Topology::Triangles(triangles) => triangles.len(),
            Topology::Quads(quads) => quads.len(),
            Topology::Polygons(rings) => rings.len(),
            Topology::Polylines(runs) => runs.len(),
            Topology::Hexahedra { cells, .. } => cells.len(),
            Topology::Grid { fastest, slowest } => fastest.saturating_sub(1) * slowest.saturating_sub(1),
            Topology::Polyhedra(cells) => cells.cell_count(),
            // A segment is a wireframe's cell - which is also what drops
            // per-cell attributes through the conversion: the count changes,
            // and a surface-cell value has no honest place on an edge shared
            // by two cells.
            Topology::Segments(pairs) => pairs.len(),
            Topology::Points => 0,
        }
    }
}

/// Cells bounded by faces, each face a ring of nodes.
///
/// The shape an unstructured grid has and a hexahedron does not. A cell is not
/// eight corners in a known order; it is a list of **faces**, each face a list
/// of **nodes**, and one face is shared by the two cells on either side of it.
/// That last part is why the orientation cannot live on the face: the stored
/// winding turns outward for one of the two cells and inward for the other, so
/// the flag belongs to the *pair*.
///
/// Stored flat - four runs and a parallel flag array - because a corpus grid is
/// two cells and a real one is millions, and three levels of `Vec` for the
/// latter is a lot of allocations to describe a list of integers.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Polyhedra {
    /// Nodes of every face, concatenated.
    face_nodes: Vec<u32>,
    /// Where each face begins in `face_nodes`, with a final entry holding the
    /// total: face `f` is `face_nodes[face_starts[f]..face_starts[f + 1]]`.
    face_starts: Vec<u32>,
    /// Faces of every cell, concatenated.
    cell_faces: Vec<u32>,
    /// Where each cell begins in `cell_faces`, same convention.
    cell_starts: Vec<u32>,
    /// One flag per entry of `cell_faces`: whether that face's stored winding
    /// already turns its right-hand normal **out of that cell**.
    outward: Vec<bool>,
}

impl Polyhedra {
    /// Build from the four runs, with the flags parallel to `cell_faces`.
    ///
    /// `face_starts` and `cell_starts` are exclusive running totals *with* a
    /// leading zero and a trailing total, so both have one more entry than the
    /// thing they index. RESQML's own `CumulativeLength` has neither; the
    /// conversion happens where the file is read, once.
    pub fn new(
        face_nodes: Vec<u32>,
        face_starts: Vec<u32>,
        cell_faces: Vec<u32>,
        cell_starts: Vec<u32>,
        outward: Vec<bool>,
    ) -> Self {
        Self {
            face_nodes,
            face_starts,
            cell_faces,
            cell_starts,
            outward,
        }
    }

    /// How many cells.
    pub fn cell_count(&self) -> usize {
        self.cell_starts.len().saturating_sub(1)
    }

    /// How many distinct faces - not the number of cell-face pairs, which is
    /// larger wherever a face is shared.
    pub fn face_count(&self) -> usize {
        self.face_starts.len().saturating_sub(1)
    }

    /// The nodes of one face, in the order the file stored them.
    pub fn face(&self, face: usize) -> &[u32] {
        let (from, to) = (
            self.face_starts[face] as usize,
            self.face_starts[face + 1] as usize,
        );
        &self.face_nodes[from..to]
    }

    /// The faces of one cell, each with whether its stored winding points out
    /// of *this* cell.
    pub fn cell(&self, cell: usize) -> impl Iterator<Item = (usize, bool)> + '_ {
        let (from, to) = (
            self.cell_starts[cell] as usize,
            self.cell_starts[cell + 1] as usize,
        );
        (from..to).map(move |slot| (self.cell_faces[slot] as usize, self.outward[slot]))
    }

    /// The nodes of one cell's faces, each already wound so the right-hand
    /// normal points **out** of the cell.
    ///
    /// What every consumer wants and none of them says twice: a face written
    /// for the neighbour comes back reversed here, so a caller can hand the
    /// result straight to a renderer without carrying the flag around.
    pub fn faces_outward(&self, cell: usize) -> impl Iterator<Item = Vec<u32>> + '_ {
        self.cell(cell).map(move |(face, outward)| {
            let nodes = self.face(face);
            if outward {
                nodes.to_vec()
            } else {
                nodes.iter().rev().copied().collect()
            }
        })
    }

    /// Every cell's boundary as outward-facing triangles.
    ///
    /// A fan from each face's first node. Exact for a triangle, exact for a
    /// planar polygon, and an approximation for a face whose corners are not
    /// coplanar - which the schema neither forbids nor says how to split, the
    /// one place it comes close being the optional hinge-node faces. The fan is
    /// stated rather than hidden: a caller wanting the hinge-node tessellation
    /// has to do it from the face lists, which are right here.
    pub fn triangles(&self) -> Vec<[u32; 3]> {
        let mut out = Vec::new();
        for cell in 0..self.cell_count() {
            for nodes in self.faces_outward(cell) {
                for corner in 1..nodes.len().saturating_sub(1) {
                    out.push([nodes[0], nodes[corner], nodes[corner + 1]]);
                }
            }
        }
        out
    }

    /// The grid's **outside**: the faces belonging to one cell only, each wound
    /// so its right-hand normal points out of the grid.
    ///
    /// # Why the boundary and not every face
    ///
    /// [`Polyhedra::triangles`] emits every cell's every face, interior ones
    /// included - right for counting, wrong for drawing. A grid of a hundred
    /// thousand cells has a few thousand faces on its skin and hundreds of
    /// thousands inside it, each shared by two cells and each drawn twice, one
    /// of the two facing the camera. What comes out is an opaque block whose
    /// surface z-fights with itself, at ten times the cost of the picture
    /// actually wanted.
    ///
    /// A face is on the boundary when exactly one cell claims it - the same
    /// rule the hexahedral path uses, and here it needs no geometry at all,
    /// because the shared faces are already shared *by index*: the file states
    /// which cells meet at which face, so counting the claims is exact where
    /// hashing corner sets is a reconstruction.
    ///
    /// The winding comes from the flag the file carries for that cell-face
    /// pair, so the ring returned turns outward for the one cell that owns it -
    /// which, for a boundary face, is outward from the grid.
    ///
    /// Rings of any length, not triangles: a face may be a quadrilateral, and
    /// splitting it here would hand a consumer that wants polygons - GeoJSON,
    /// VTK, OBJ - a decision it did not ask for. [`Geometry::drawable`] fans
    /// them where a renderer needs triangles.
    pub fn boundary(&self) -> Vec<Vec<u32>> {
        let mut claims = vec![0u32; self.face_count()];
        for cell in 0..self.cell_count() {
            for (face, _) in self.cell(cell) {
                if let Some(count) = claims.get_mut(face) {
                    *count += 1;
                }
            }
        }
        let mut out = Vec::new();
        for cell in 0..self.cell_count() {
            for (face, outward) in self.cell(cell) {
                if claims.get(face).copied() != Some(1) {
                    continue;
                }
                let nodes = self.face(face);
                out.push(if outward {
                    nodes.to_vec()
                } else {
                    nodes.iter().rev().copied().collect()
                });
            }
        }
        out
    }

    /// How many cell-face pairs there are, summed over the cells.
    ///
    /// The length `CellFaceIsRightHanded` has to be, and larger than
    /// [`Polyhedra::face_count`] by one per shared face.
    pub fn slot_count(&self) -> usize {
        self.cell_faces.len()
    }
}

/// One patch: its points, and what they form.
#[derive(Debug, Clone, PartialEq)]
pub struct Patch {
    /// Index of the patch inside the representation, as RESQML numbers them.
    pub index: usize,
    /// Where this patch's points start in the representation's own node
    /// numbering - the running sum of the preceding patches' point counts.
    ///
    /// A subrepresentation, a property, or anything else that indexes *nodes of
    /// the representation* counts from here; [`Topology`] indices do not, see
    /// [`Patch::topology`].
    pub node_offset: usize,
    /// The points, in the representation's local coordinate system.
    pub points: Vec<Point>,
    /// How they connect - always indexed into this patch's own [`points`],
    /// whichever convention the file used.
    ///
    /// [`points`]: Patch::points
    pub topology: Topology,
    /// Which of this patch's polylines close back on their first node.
    ///
    /// One entry per run of [`Topology::Polylines`], **empty when the file
    /// says nothing** - which is not the same as *all open*: a producer that
    /// omits `ClosedPolylines` has stated nothing, and a reader that drew the
    /// closing edge anyway would invent a ring.
    ///
    /// It lives here and not in [`Topology::Polylines`] because RESQML states
    /// it in a boolean array beside the counts, and the topology carries
    /// lengths. Added after a Volve polyline the file
    /// calls closed was drawn open: the reader took
    /// `NodeCountPerPolyline` and never `ClosedPolylines`, so the last segment
    /// of every ring was missing.
    pub closed: Vec<bool>,
}

impl Topology {
    /// The RESQML cell index of the `at`-th cell of this topology.
    ///
    /// The one place that knows what an empty `index` means. `None` when `at`
    /// is past the end, which a caller walking its own list cannot produce and
    /// a caller resolving a number out of a file certainly can.
    pub fn cell_at(&self, at: usize) -> Option<u32> {
        match self {
            Topology::Hexahedra { cells, index, .. } => {
                if at >= cells.len() {
                    return None;
                }
                Some(index.get(at).copied().unwrap_or(at as u32))
            }
            _ => (at < self.cell_count()).then_some(at as u32),
        }
    }

    /// Where the cell RESQML numbers `cell` sits in this topology, if it is here.
    ///
    /// `None` for a cell the reader dropped - undefined, or with no located
    /// pillar. That is an answer and not a failure: the file names a cell that
    /// has no shape, and drawing something for it would be drawing a place the
    /// grid says is not one.
    pub fn position_of(&self, cell: u32) -> Option<usize> {
        match self {
            Topology::Hexahedra { cells, index, .. } => {
                if index.is_empty() {
                    return ((cell as usize) < cells.len()).then_some(cell as usize);
                }
                // Sorted by construction - the reader walks k, then j, then i -
                // so a binary search rather than a scan over ten million cells.
                index.binary_search(&cell).ok()
            }
            _ => ((cell as usize) < self.cell_count()).then_some(cell as usize),
        }
    }
}

/// Everything a representation says about its shape.
#[derive(Debug, Clone, PartialEq)]
pub struct Geometry {
    /// The representation's own type, for a caller that wants to label it.
    pub kind: &'static str,
    /// One entry per patch, in order.
    pub patches: Vec<Patch>,
}

impl Geometry {
    /// Total number of points across every patch.
    pub fn point_count(&self) -> usize {
        self.patches.iter().map(|patch| patch.points.len()).sum()
    }

    /// Whether there is anything to draw.
    pub fn is_empty(&self) -> bool {
        self.point_count() == 0
    }

    /// **The nodes of these ranks, in the order asked** - the representation's
    /// own numbering, the running sum of its patches' points ([`Patch::node_offset`]).
    ///
    /// The numbering a click in the scene returns and `transform`'s `nodes`
    /// reads. A rank the shape does not hold is refused by name: a node taken
    /// from past the end would be another shape's, or nothing.
    pub fn nodes_at(&self, ranks: &[usize]) -> std::result::Result<Vec<[f64; 3]>, String> {
        let all: Vec<&[f64; 3]> = self
            .patches
            .iter()
            .flat_map(|patch| patch.points.iter())
            .collect();
        ranks
            .iter()
            .map(|&rank| {
                all.get(rank).map(|p| **p).ok_or_else(|| {
                    format!(
                        "node {rank} is named, and the shape holds {} (ranks from 0)",
                        all.len()
                    )
                })
            })
            .collect()
    }

    /// Total number of **cells**, whatever kind of cell this shape has.
    ///
    /// A triangle, a polyline, a hexahedron and a grid cell all count as one,
    /// because that is what a per-cell property has one value for - and what a
    /// consumer will line those values up against. A shape of loose points has
    /// no cells at all.
    pub fn cell_count(&self) -> usize {
        self.patches.iter().map(|patch| patch.topology.cell_count()).sum()
    }

    /// Every patch's points in one array, with the triangles renumbered into it.
    ///
    /// One vertex buffer and one index buffer is what a renderer actually
    /// uploads, and doing the renumbering here is the point: it is the one
    /// place [`Patch::node_offset`] has to be applied, and applying it wrong
    /// draws a mesh instead of failing.
    ///
    /// Patches that are not triangulated contribute their points and no
    /// indices. That includes both volumetric kinds: a cell is not a surface,
    /// and the exporters draw its faces themselves rather than take a
    /// triangulation decided here. [`Polyhedra::triangles`] is there for a
    /// caller that does want one.
    pub fn merged(&self) -> (Vec<Point>, Vec<[u32; 3]>) {
        let mut points = Vec::with_capacity(self.point_count());
        let mut triangles = Vec::new();
        for patch in &self.patches {
            let base = points.len() as u32;
            points.extend_from_slice(&patch.points);
            if let Topology::Triangles(patch_triangles) = &patch.topology {
                triangles.extend(
                    patch_triangles
                        .iter()
                        .map(|t| [t[0] + base, t[1] + base, t[2] + base]),
                );
            }
        }
        (points, triangles)
    }

    /// Points, triangles **and line segments**, ready for a graphics API.
    ///
    /// # Why this exists next to [`Geometry::merged`]
    ///
    /// `merged` emits indices only for [`Topology::Triangles`], which is right
    /// for what it is for. But a viewer given nothing else draws a **point
    /// cloud** - and that is what a 2D grid and a well trajectory came out as:
    /// a horizon rendered as a spray of dots, a borehole as a dotted line. The
    /// data was there the whole time; nothing turned a cell into two triangles
    /// or a station list into segments.
    ///
    /// So every topology gets an answer here:
    ///
    /// - a **grid** becomes two triangles per cell, in the lattice's own
    ///   row-major order;
    /// - **quadrilaterals** and **polygons** are fanned from their first
    ///   corner, which is correct for the convex rings RESQML produces;
    /// - **polylines** become the segments between consecutive stations;
    /// - bare **points** stay points, because that is what they are.
    ///
    /// A cell with a non-finite corner is **dropped rather than drawn**. A grid
    /// with holes is ordinary - an eroded horizon, a survey clipped to a
    /// licence block - and a triangle through `NaN` does not render as a hole,
    /// it renders as a spike across the whole scene.
    ///
    /// And the node itself goes with it. Dropping only the cells leaves the
    /// `NaN` in the buffer a viewer uploads, which is not a missing dot: three
    /// numbers that are not numbers make the object's bounding sphere `NaN`, the
    /// frustum test fails for the whole object, and the framing of the **entire
    /// scene** goes with it - one console line per frame, about a node that only
    /// ever meant "no data here". So [`Drawable`] carries locations only, the
    /// indices are renumbered onto them, and [`Drawable::dropped`] says how many
    /// there were: a surface that comes out empty because every node was `NaN`
    /// must not look like a reader that found nothing.
    pub fn drawable(&self) -> Drawable {
        let mut points: Vec<Point> = Vec::with_capacity(self.point_count());
        let mut triangles: Vec<[u32; 3]> = Vec::new();
        let mut triangle_cells: Vec<u32> = Vec::new();
        let mut lines: Vec<[u32; 2]> = Vec::new();
        let mut node_cells: Vec<u32> = Vec::new();
        // The cell numbering is the **original** topology's, patch by patch -
        // the numbering `cell_count` sums and a per-cell property lines up
        // against - even where the drawing swaps in a derived skin.
        let mut cell_base = 0u32;
        // One skinned patch and the table is a lie for the whole object: a
        // boundary face is not a cell, so the mapping is withheld entirely
        // rather than guessed for part of it.
        let mut unmappable = false;

        for patch in &self.patches {
            let base = points.len() as u32;
            // **A volume's node that no cell names is not a location.** The
            // IJK reader drops a cell its `CellGeometryIsDefined` flag marks
            // undefined, and the standard makes that flag override the
            // numbers - which stay finite in the file, since nothing obliges a
            // writer to null them. Left in the buffer, such a node was drawn
            // as a dot exactly where the grid says there is nothing (seen
            // on ALWYN's IJK grids). A lattice or a point
            // set keeps its lone nodes: there a node is a sample in its own
            // right, and the ALWYN horizons are nothing but lone nodes.
            let placed = orphans_as_holes(&patch.points, &patch.topology);
            points.extend_from_slice(&placed);
            // Every node enters with no cell; the polylines arm - the one
            // place membership is unique - rewrites its own below.
            node_cells.resize(points.len(), Drawable::NO_CELL);
            let finite = |index: u32| -> bool {
                placed
                    .get(index as usize)
                    .is_some_and(|p| p.iter().all(|c| c.is_finite()))
            };
            let mut triangle = |a: u32, b: u32, c: u32, cell: u32| {
                if finite(a) && finite(b) && finite(c) {
                    triangles.push([a + base, b + base, c + base]);
                    // In lockstep with the push above: a triangle dropped for a
                    // non-finite corner drops its cell entry too, or every
                    // later triangle wears its neighbour's value.
                    triangle_cells.push(cell);
                }
            };

            // **A volume is drawn by its skin**, and the skin is derived here
            // rather than left to the caller. Leaving it out is what made an
            // IJK grid and an unstructured grid come out as point clouds: the
            // arms below had nothing to match, so the viewer fell through to
            // its last resort and drew the nodes. The data was complete the
            // whole time - nothing turned a cell into faces.
            //
            // Through `shape::reshape` rather than a second implementation:
            // that module already decides which faces are on the outside, and
            // two derivations of the same boundary is exactly the pair of
            // readers that agree on a total and disagree on what it means.
            //
            // A refusal is kept rather than swallowed - the arms below still
            // have a volume case, which emits nothing, so a topology whose
            // boundary this build cannot derive stays a point cloud instead of
            // becoming silently empty.
            let skin = match &patch.topology {
                volume @ (Topology::Hexahedra { .. } | Topology::Polyhedra(_)) => {
                    crate::shape::reshape(volume, &patch.points, crate::shape::Shape::Surface).ok()
                }
                _ => None,
            };

            if skin.is_some() {
                unmappable = true;
            }
            match skin.as_ref().unwrap_or(&patch.topology) {
                Topology::Triangles(found) => {
                    for (at, t) in found.iter().enumerate() {
                        triangle(t[0], t[1], t[2], cell_base + at as u32);
                    }
                }
                Topology::Quads(found) => {
                    for (at, q) in found.iter().enumerate() {
                        triangle(q[0], q[1], q[2], cell_base + at as u32);
                        triangle(q[0], q[2], q[3], cell_base + at as u32);
                    }
                }
                Topology::Polygons(rings) => {
                    for (cell, ring) in rings.iter().enumerate() {
                        for at in 1..ring.len().saturating_sub(1) {
                            triangle(ring[0], ring[at], ring[at + 1], cell_base + cell as u32);
                        }
                    }
                }
                Topology::Grid { fastest, slowest } => {
                    // Row-major, the fast axis first - the order the lattice
                    // evaluates its points in, so a cell is four neighbours and
                    // not an index computation anybody has to re-derive.
                    for row in 0..slowest.saturating_sub(1) {
                        for column in 0..fastest.saturating_sub(1) {
                            let a = (row * fastest + column) as u32;
                            let b = a + 1;
                            let c = a + *fastest as u32;
                            let d = c + 1;
                            let cell = cell_base + (row * (fastest - 1) + column) as u32;
                            triangle(a, b, d, cell);
                            triangle(a, d, c, cell);
                        }
                    }
                }
                Topology::Polylines(runs) => {
                    let mut at: u32 = 0;
                    for (cell, run) in runs.iter().enumerate() {
                        // Every node of a run belongs to that run and no
                        // other - the membership that makes per-cell painting
                        // of lines a lookup instead of a de-index. `get_mut`
                        // rather than an index: a run declaring more nodes
                        // than the patch holds is a malformed file, not a
                        // panic.
                        for step in 0..*run {
                            if let Some(entry) = node_cells.get_mut((base + at + step) as usize) {
                                *entry = cell_base + cell as u32;
                            }
                        }
                        for step in 0..run.saturating_sub(1) {
                            let from = at + step;
                            if finite(from) && finite(from + 1) {
                                lines.push([from + base, from + 1 + base]);
                            }
                        }
                        // And the segment that returns to the first node, when
                        // the file says the line is closed. **The node is not
                        // repeated**: a RESQML ring counts its nodes once, and
                        // adding one would shift every property indexed on
                        // them.
                        if patch.closed.get(cell).copied().unwrap_or(false)
                            && *run >= 3
                            && finite(at)
                            && finite(at + run - 1)
                        {
                            lines.push([at + run - 1 + base, at + base]);
                        }
                        at += run;
                    }
                }
                Topology::Segments(pairs) => {
                    for pair in pairs {
                        if finite(pair[0]) && finite(pair[1]) {
                            lines.push([pair[0] + base, pair[1] + base]);
                        }
                    }
                }
                // Only reachable when the conversion above refused: the points
                // stay, and `dropped` is not the place that says so. A volume
                // this build cannot skin is rare enough to be worth seeing as
                // the nodes it does have rather than as nothing.
                Topology::Hexahedra { .. } | Topology::Polyhedra(_) => {}
                Topology::Points => {}
            }
            // The **original** topology advances the numbering, skin or not:
            // a property counts the volume's cells whether or not this build
            // can map its boundary back onto them.
            cell_base += patch.topology.cell_count() as u32;
        }
        if unmappable {
            triangle_cells.clear();
        }
        compact(points, triangles, triangle_cells, lines, node_cells)
    }
}

/// A volume's points, with every node no cell names turned into a hole.
///
/// Borrowed when every node is named - the ordinary grid pays one pass and no
/// copy - and owned with `NaN` on the orphans otherwise, so that [`compact`]
/// drops them the way it drops a node the file left null, and counts them in
/// [`Drawable::dropped`]. Surfaces, lines and point sets are handed back as
/// they are: a node of theirs that no cell names is still a location.
pub(crate) fn orphans_as_holes<'a>(
    points: &'a [Point],
    topology: &Topology,
) -> std::borrow::Cow<'a, [Point]> {
    let named: Vec<u32> = match topology {
        Topology::Hexahedra { cells, .. } => cells.iter().flatten().copied().collect(),
        Topology::Polyhedra(cells) => (0..cells.cell_count())
            .flat_map(|cell| cells.cell(cell).map(|(face, _)| face))
            .flat_map(|face| cells.face(face).iter().copied())
            .collect(),
        _ => return std::borrow::Cow::Borrowed(points),
    };
    let mut used = vec![false; points.len()];
    for index in named {
        if let Some(slot) = used.get_mut(index as usize) {
            *slot = true;
        }
    }
    if used.iter().all(|u| *u) {
        return std::borrow::Cow::Borrowed(points);
    }
    let holed: Vec<Point> = points
        .iter()
        .zip(&used)
        .map(|(point, used)| if *used { *point } else { [f64::NAN; 3] })
        .collect();
    std::borrow::Cow::Owned(holed)
}

/// The nodes that are locations, and the indices renumbered onto them.
///
/// Cheap where it is usually needed: a shape with nothing to drop is handed
/// back untouched, so the ordinary case walks the points once and allocates
/// nothing. Only a shape with holes pays for the map.
///
/// An index into a node that was dropped is filtered rather than clamped - by
/// construction there is none, since a cell with a non-finite corner is never
/// emitted, but a bent index draws a triangle across the model and there is no
/// version of that worth having.
fn compact(
    points: Vec<Point>,
    triangles: Vec<[u32; 3]>,
    triangle_cells: Vec<u32>,
    lines: Vec<[u32; 2]>,
    node_cells: Vec<u32>,
) -> Drawable {
    let is_location = |point: &Point| point.iter().all(|c| c.is_finite());
    if points.iter().all(is_location) {
        return Drawable {
            points,
            triangles,
            lines,
            dropped: 0,
            triangle_cells,
            node_cells,
        };
    }

    const GONE: u32 = u32::MAX;
    let mut moved = Vec::with_capacity(points.len());
    let mut kept = Vec::with_capacity(points.len());
    for point in &points {
        if is_location(point) {
            moved.push(kept.len() as u32);
            kept.push(*point);
        } else {
            moved.push(GONE);
        }
    }
    let at = |index: u32| moved.get(index as usize).copied().unwrap_or(GONE);
    // The cell table is filtered **with** its triangle - zipped, not re-derived
    // - or every triangle after a dropped one wears its neighbour's cell. An
    // empty table (a volume's skin) stays empty rather than gaining entries.
    let mapped = !triangle_cells.is_empty();
    let mut cells_kept = Vec::new();
    let triangles_kept: Vec<[u32; 3]> = triangles
        .into_iter()
        .enumerate()
        .filter_map(|(index, t)| {
            let renumbered = [at(t[0]), at(t[1]), at(t[2])];
            if renumbered.contains(&GONE) {
                return None;
            }
            if mapped && let Some(cell) = triangle_cells.get(index) {
                cells_kept.push(*cell);
            }
            Some(renumbered)
        })
        .collect();
    // The node table follows its nodes: an entry per **kept** point, in the
    // same order the points were kept - filtered any other way, every node
    // after a hole would wear its neighbour's cell.
    let node_cells_kept: Vec<u32> = node_cells
        .into_iter()
        .zip(&points)
        .filter(|(_, point)| is_location(point))
        .map(|(cell, _)| cell)
        .collect();
    Drawable {
        dropped: points.len() - kept.len(),
        points: kept,
        triangles: triangles_kept,
        triangle_cells: cells_kept,
        node_cells: node_cells_kept,
        lines: lines
            .into_iter()
            .map(|l| [at(l[0]), at(l[1])])
            .filter(|l| !l.contains(&GONE))
            .collect(),
    }
}

/// What a viewer needs: the points, and how to join them.
///
/// Both index lists at once because a representation can have some of each - a
/// wellbore frame is stations *and* markers - and a caller that had to ask
/// twice would walk the patches twice.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Drawable {
    /// Every patch's points, concatenated - and every one of them a location.
    pub points: Vec<Point>,
    /// Triples into `points`.
    pub triangles: Vec<[u32; 3]>,
    /// Pairs into `points`: the segments of every polyline.
    pub lines: Vec<[u32; 2]>,
    /// Nodes left out because they hold no location - a `NaN` corner of a grid
    /// with holes, the padding of a short parametric line.
    ///
    /// Counted rather than swallowed: a representation whose nodes are *all*
    /// undefined comes back empty, and empty is exactly what a reader that
    /// found nothing also returns. This is what tells the two apart.
    pub dropped: usize,
    /// The **cell** each triangle was cut from - `triangle_cells[i]` for
    /// `triangles[i]` - in the numbering of [`Geometry::cell_count`], which is
    /// the numbering a per-cell property's values line up against.
    ///
    /// This is what lets a viewer paint a flat colour per cell: three.js
    /// colours vertices, so an aplat needs the geometry de-indexed - each
    /// triangle its own three vertices - and each new vertex needs to know
    /// which cell's value it wears. Deriving this table outside the merge
    /// would mean re-deriving the fan of a polygon, the split of a quad and
    /// the row order of a lattice, and agreeing with them forever - the exact
    /// pair of readers this repository refuses.
    ///
    /// **Empty when the mapping does not exist**, never guessed: a volume is
    /// drawn by its boundary skin, and a boundary face is a *face*, not a
    /// cell - naming one per triangle would line a property's values up
    /// against elements they were not measured on. Empty and full are the two
    /// honest states; a caller that finds it empty refuses per-cell painting
    /// by name, exactly as it did before this field existed.
    pub triangle_cells: Vec<u32>,
    /// The cell each **node** belongs to, where that is unique - one entry per
    /// point, [`Drawable::NO_CELL`] where it is not.
    ///
    /// The unique case is a polyline: every node of a run belongs to that run
    /// and no other, so painting a `PolylineSetRepresentation` per cell - one
    /// colour a polyline - needs no de-indexing at all: segments of one run
    /// share vertices and share a colour, and two runs never share a vertex.
    /// A surface's node is the opposite - shared between cells, which is
    /// exactly why faces go through [`Drawable::triangle_cells`] and a
    /// de-index instead. `NO_CELL` there is the honest entry, and a viewer's
    /// out-of-range guard turns it into the transparent it already gives a
    /// missing value.
    pub node_cells: Vec<u32>,
}

impl Drawable {
    /// The entry of [`Drawable::node_cells`] for a node that belongs to no
    /// single cell - a surface node, shared between the cells around it.
    pub const NO_CELL: u32 = u32::MAX;
}

/// The geometry of a representation, using the readers this build ships.
pub fn of(representation: &dyn Reflect, source: &dyn ValueSource) -> Result<Geometry> {
    of_with(crate::readers::default_readers(), representation, source)
}

/// The geometry of a representation, using a registry the caller controls.
///
/// The entry point for an application that has taught the crate a
/// representation it does not know - see [`crate::readers`].
pub fn of_with(
    readers: &Readers,
    representation: &dyn Reflect,
    source: &dyn ValueSource,
) -> Result<Geometry> {
    let info = representation.type_info();
    let Some(reader) = readers.find(info.type_key) else {
        return Err(Error::unsupported(leak(info.type_key), info.name));
    };
    let patches = reader.read(representation, source)?;
    // The crate's rule, applied to every reader at once: a representation that
    // evaluates to nothing is an error naming it, never a silent empty result.
    // The corpus has a real case - a `TriangulatedSetRepresentation` of 891
    // bytes with no `TrianglePatch` at all - and "no geometry" must not look
    // like a working read of a representation with nothing to draw.
    if patches.is_empty() {
        return Err(Error::malformed(
            info.name,
            "the representation declares no patch at all",
        ));
    }
    Ok(Geometry {
        kind: info.name,
        patches,
    })
}

/// Whether this build knows this representation, without reading anything.
///
/// What a listing uses to grey out the rows it cannot show.
pub fn is_supported(representation: &dyn Reflect) -> bool {
    supports(representation.type_info().type_key)
}

/// [`is_supported`] from the concept's name alone - `TriangulatedSetRepresentation`,
/// without `obj_` - so a listing can ask **before** parsing the object.
///
/// The same answer by construction: `is_supported` is this, on the type key the
/// object would report. What it buys is measured: `drawable` over a package of
/// 77 090 objects parsed every one of them to keep 5 402 (measured on a large
/// package).
pub fn supports(type_key: &str) -> bool {
    crate::readers::default_readers().handles(type_key)
}

/// How many nodes the document states this representation holds - no read.
///
/// The pair of [`is_supported`]: that one says *can this be drawn at all*, from
/// the concept's name alone; this one says *how big is it*, from what the
/// document states about itself. Together they let a viewer list a thousand
/// representations without touching the data file, which is the difference
/// between a package that opens and one that appears to hang.
///
/// `None` when nothing states it. Read
/// [`RepresentationReader::declared_nodes`] before showing the number: it is
/// what the document *claims*, not what [`of`] will produce, and the two differ
/// wherever the reader drops or derives nodes.
///
/// [`RepresentationReader::declared_nodes`]: crate::readers::RepresentationReader::declared_nodes
pub fn declared_nodes(representation: &dyn Reflect) -> Option<u64> {
    crate::readers::default_readers().declared_nodes(representation)
}

/// Every reader this crate implements.
///
/// One entry per concept rather than one per version: `type_key` is the same
/// across versions by construction, so these cover RESQML 2.0.1 and 2.2 alike,
/// and the next release too unless it respells the insides.
pub(crate) fn builtin_readers() -> Vec<Box<dyn RepresentationReader>> {
    /// A reader defined by a concept name and a function.
    struct Builtin {
        name: &'static str,
        key: &'static str,
        read: fn(&dyn Reflect, &dyn ValueSource) -> Result<Vec<Patch>>,
        /// What the document states this concept holds, when it states it.
        ///
        /// `None` here means the concept says nothing a listing could use, and
        /// most of them do not - see [`RepresentationReader::declared_nodes`].
        nodes: Option<fn(&dyn Reflect) -> Option<u64>>,
    }

    impl RepresentationReader for Builtin {
        fn name(&self) -> &'static str {
            self.name
        }
        fn handles(&self, type_key: &str) -> bool {
            type_key == self.key
        }
        fn read(&self, representation: &dyn Reflect, source: &dyn ValueSource) -> Result<Vec<Patch>> {
            (self.read)(representation, source)
        }
        fn declared_nodes(&self, representation: &dyn Reflect) -> Option<u64> {
            (self.nodes?)(representation)
        }
    }

    macro_rules! reader {
        ($key:literal, $f:path) => {
            reader!($key, $f, None)
        };
        ($key:literal, $f:path, $nodes:expr) => {
            Box::new(Builtin {
                name: $key,
                key: $key,
                read: $f,
                nodes: $nodes,
            }) as Box<dyn RepresentationReader>
        };
    }

    // The second argument is deliberately absent more often than not: a concept
    // that keeps its node count in an array - a `PolylineSetRepresentation`'s
    // `NodeCountPerPolyline` - or that states elements rather than nodes - a
    // `SubRepresentation`, an IJK grid drawn by its skin - has nothing to say
    // here that would not have to be measured anyway.
    vec![
        reader!(
            "TriangulatedSetRepresentation",
            triangulated,
            Some(triangulated_nodes)
        ),
        reader!("PointSetRepresentation", point_set, Some(patch_count_nodes)),
        reader!("PolylineRepresentation", polyline, Some(patch_count_nodes)),
        reader!("PolylineSetRepresentation", polyline_set),
        reader!("Grid2dRepresentation", grid_2d, Some(grid_2d_nodes)),
        reader!(
            "WellboreFrameRepresentation",
            crate::well::frame,
            Some(stated_node_count)
        ),
        reader!("IjkGridRepresentation", crate::ijk::read),
        reader!(
            "WellboreTrajectoryRepresentation",
            trajectory,
            Some(trajectory_nodes)
        ),
        reader!("SubRepresentation", crate::parent::sub_representation),
        reader!(
            "UnstructuredGridRepresentation",
            crate::unstructured::read,
            Some(stated_node_count)
        ),
        reader!("PlaneSetRepresentation", crate::plane::read),
        reader!("GridConnectionSetRepresentation", crate::connection::read),
        // Not a RESQML representation: a WITSML trajectory, drawn from its
        // stations (`crate::stations`). Keyed on the concept like the rest, so
        // a WITSML 2.0 `Trajectory` reaches it too - and is refused there by
        // name, having no `DefaultMdDatum` to count its depths from.
        reader!(
            "Trajectory",
            crate::stations::read,
            Some(crate::stations::declared)
        ),
    ]
}

// --- What the document states, with nothing read ---------------------------
//
// Each of these reads by the **field present**, never by the release: a
// `TrianglePatch` spells `NodeCount` the same way in RESQML 2.0.1 and 2.2, and
// a concept that drops the field simply stops answering. Spelling a version
// here would give one release a number and the other a silence, which reads as
// a package holding nothing.

/// `NodeCount`, wherever the concept hangs it.
///
/// A `WellboreFrameRepresentation` states it on the object; an
/// `UnstructuredGridRepresentation` states `CellCount` there and puts
/// `NodeCount` on its `Geometry`, beside the `Points` it counts. Both are read
/// by looking for the field, which is also what makes this survive a release
/// that moves it - the mistake `grid_2d` already had to accommodate.
fn stated_node_count(representation: &dyn Reflect) -> Option<u64> {
    let count = crate::values::scalar_i64(representation, "NodeCount").or_else(|| {
        let geometry = field_object(representation, "Geometry")?;
        crate::values::scalar_i64(geometry, "NodeCount")
    })?;
    non_negative(count)
}

/// A trajectory's `KnotCount`: the control points of its parametric line.
///
/// The line is a **single** one, which is what makes this answerable - the
/// `KnotCount` of a line *set* is the longest line's, and the short ones are
/// padded with `NaN`, so a set states an upper bound and not a count (see
/// `parametric.rs`). A trajectory with no `Geometry` at all falls back to its
/// datum and says nothing here.
fn trajectory_nodes(representation: &dyn Reflect) -> Option<u64> {
    let geometry = field_object(representation, "Geometry")?;
    non_negative(crate::values::scalar_i64(geometry, "KnotCount")?)
}

/// The nodes of every `TrianglePatch`, which each state their own.
fn triangulated_nodes(representation: &dyn Reflect) -> Option<u64> {
    sum_over(representation, &["TrianglePatch"], "NodeCount")
}

/// The `Count` of every node patch: *number of items in the patch*.
///
/// RESQML 2.2's `NodePatchGeometry` carries no count, so a 2.2 point set
/// answers nothing rather than zero.
fn patch_count_nodes(representation: &dyn Reflect) -> Option<u64> {
    sum_over(representation, NODE_PATCH, "Count")
}

/// A lattice states both its axes, and their product is its nodes.
///
/// **Declared, not drawn.** A node of the lattice may carry no location - that
/// is ALWYN's every-other-node horizons - and the reader drops those. So this
/// is an upper bound on what will appear, which is what a size hint is for.
fn grid_2d_nodes(representation: &dyn Reflect) -> Option<u64> {
    // 2.0.1 puts the two counts on `Grid2dPatch`, 2.2 on the representation:
    // the same fallback `grid_2d` itself uses, so the listing and the reader
    // cannot disagree about where they are.
    let patch = field_object(representation, "Grid2dPatch").unwrap_or(representation);
    let fastest = non_negative(crate::values::scalar_i64(patch, "FastestAxisCount")?)?;
    let slowest = non_negative(crate::values::scalar_i64(patch, "SlowestAxisCount")?)?;
    fastest.checked_mul(slowest)
}

/// Add one stated field across a concept's patches, whatever they are called.
///
/// `None` when no patch states it: a sum over nothing is zero, and zero is a
/// claim about the contents rather than about what the document says.
fn sum_over(representation: &dyn Reflect, patches: &[&str], field: &str) -> Option<u64> {
    let mut total = 0u64;
    let mut stated = false;
    for name in patches {
        for patch in list(representation, name) {
            let Some(count) = crate::values::scalar_i64(patch, field) else {
                continue;
            };
            total = total.checked_add(non_negative(count)?)?;
            stated = true;
        }
    }
    stated.then_some(total)
}

/// A stated count that is negative is not a count; it is a broken document.
fn non_negative(value: i64) -> Option<u64> {
    u64::try_from(value).ok()
}

// ---------------------------------------------------------------------------

/// One patch of a representation, whichever way its version wraps it.
///
/// RESQML 2.0.1 puts a `PointGeometry` under a patch element - `NodePatch`,
/// `LinePatch`, `TrianglePatch` - as a child called `Geometry`. RESQML 2.2
/// sometimes drops the wrapper and names the geometry itself, as in
/// `NodePatchGeometry`. The two are told apart by looking for `Points`: an
/// element that has one *is* the geometry.
///
/// The distinction cost real points. Reading only the 2.0.1 spelling gave a 2.2
/// point set zero patches, and zero patches is a successful call returning
/// nothing to draw - so [`patch_points`] refuses a patch with no geometry
/// rather than yielding an empty vector.
struct PatchParts<'a> {
    /// The element carrying the sibling fields - triangle indices, node counts.
    owner: &'a dyn Reflect,
    /// The `PointGeometry`, if the patch declares one.
    geometry: Option<&'a dyn Reflect>,
}

fn patches_of<'a>(representation: &'a dyn Reflect, fields: &[&str]) -> Vec<PatchParts<'a>> {
    let mut out = Vec::new();
    for field in fields {
        for item in list(representation, field) {
            // An element holding `Points` *is* the geometry; one holding a
            // `Geometry` child is a wrapper around it.
            let geometry = if field_object(item, "Points").is_some() {
                Some(item)
            } else {
                field_object(item, "Geometry")
            };
            out.push(PatchParts {
                owner: item,
                geometry,
            });
        }
        if !out.is_empty() {
            return out;
        }
    }
    out
}

fn triangulated(representation: &dyn Reflect, source: &dyn ValueSource) -> Result<Vec<Patch>> {
    let mut out = Vec::new();
    let mut node_offset = 0usize;
    for (index, patch) in patches_of(representation, &["TrianglePatch"])
        .into_iter()
        .enumerate()
    {
        let at = format!("TrianglePatch[{index}]");
        let points = patch_points(&patch, source, &at)?;
        let patch = patch.owner;

        let triangles = match field_object(patch, "Triangles") {
            Some(array) => {
                let flat = values::read_integers(array, source, &format!("{at}.Triangles"))?;
                if flat.len() % 3 != 0 {
                    return Err(Error::malformed(
                        &at,
                        format!("{} index/indices is not a whole number of triangles", flat.len()),
                    ));
                }
                let base = node_offset_used(&flat, node_offset, points.len(), &at)?;
                flat.as_chunks::<3>()
                    .0
                    .iter()
                    .map(|c| [(c[0] - base) as u32, (c[1] - base) as u32, (c[2] - base) as u32])
                    .collect()
            }
            None => Vec::new(),
        };
        node_offset += points.len();
        out.push(Patch {
            index,
            node_offset: node_offset - points.len(),
            points,
            topology: Topology::Triangles(triangles),
            closed: Vec::new(),
        });
    }
    Ok(out)
}

/// Which numbering a patch's triangle indices are written in.
///
/// RESQML calls a triangulated set *"a representation … which gets its geometry
/// from a 1D array of points"*, and the two reference implementations read that
/// sentence differently: FESAPI writes indices into that single array - patch 1
/// of a 5-node patch 0 starts at 5 - while resqpy writes them relative to each
/// patch and adds the offset only when it merges. Both kinds of file exist, so
/// this decides from the numbers instead of picking a side, and returns the base
/// to subtract.
///
/// The two agree on patch 0, where the offset is zero. Where they differ, only
/// one of the two ranges can contain every index; an array that fits neither is
/// a broken file and is refused rather than drawn.
fn node_offset_used(indices: &[i64], offset: usize, points: usize, at: &str) -> Result<i64> {
    let Some(&min) = indices.iter().min() else {
        return Ok(0);
    };
    let Some(&max) = indices.iter().max() else {
        return Ok(0);
    };
    let (offset, points) = (offset as i64, points as i64);

    if min >= 0 && max < points {
        return Ok(0);
    }
    if min >= offset && max < offset + points {
        return Ok(offset);
    }
    Err(Error::malformed(
        at,
        format!(
            "triangle indices run {min}..={max}, which is neither this patch's own \
             0..{points} nor its place {offset}..{} in the representation's nodes",
            offset + points
        ),
    ))
}

/// The field names a version may use for a node patch, most specific first.
const NODE_PATCH: &[&str] = &["NodePatch", "NodePatchGeometry"];

fn point_set(representation: &dyn Reflect, source: &dyn ValueSource) -> Result<Vec<Patch>> {
    let mut out = Vec::new();
    let mut node_offset = 0usize;
    for (index, patch) in patches_of(representation, NODE_PATCH).into_iter().enumerate() {
        let at = format!("NodePatch[{index}]");
        let points = patch_points(&patch, source, &at)?;
        let start = node_offset;
        node_offset += points.len();
        out.push(Patch {
            index,
            node_offset: start,
            points,
            topology: Topology::Points,
            closed: Vec::new(),
        });
    }
    Ok(out)
}

fn polyline(representation: &dyn Reflect, source: &dyn ValueSource) -> Result<Vec<Patch>> {
    let mut out = Vec::new();
    let mut node_offset = 0usize;
    for (index, patch) in patches_of(representation, NODE_PATCH).into_iter().enumerate() {
        let at = format!("NodePatch[{index}]");
        let points = patch_points(&patch, source, &at)?;
        // One patch is one run of consecutive nodes. `IsClosed` says whether
        // the last joins the first, and it is left to the caller - see
        // `Topology::Polylines`.
        let length = points.len() as u32;
        let start = node_offset;
        node_offset += points.len();
        out.push(Patch {
            index,
            node_offset: start,
            points,
            topology: Topology::Polylines(vec![length]),
            closed: Vec::new(),
        });
    }
    Ok(out)
}

fn polyline_set(representation: &dyn Reflect, source: &dyn ValueSource) -> Result<Vec<Patch>> {
    let mut out = Vec::new();
    let mut node_offset = 0usize;
    for (index, patch) in patches_of(representation, &["LinePatch"]).into_iter().enumerate() {
        let at = format!("LinePatch[{index}]");
        let points = patch_points(&patch, source, &at)?;
        // `NodeCountPerPolyline` says where one line ends and the next begins;
        // without it the patch is one run.
        let runs = match field_object(patch.owner, "NodeCountPerPolyline") {
            Some(array) => values::read_integers(array, source, &at)?
                .into_iter()
                .map(|n| n.max(0) as u32)
                .collect(),
            None => vec![points.len() as u32],
        };
        // **And which ones close.** A separate array that this reader used
        // to ignore: a closed polyline came out without its last segment and
        // was drawn open (seen on Volve). Empty when the file says nothing,
        // which is not *all open*: a producer that omits `ClosedPolylines`
        // has said nothing, and closing anyway would invent a ring.
        let closed = match field_object(patch.owner, "ClosedPolylines") {
            Some(array) => {
                let mut flags = values::read_flags(array, source, &format!("{at}.ClosedPolylines"))?;
                // An array that states more flags than there are lines is
                // truncated; one that states fewer leaves the last lines
                // unanswered rather than inventing an answer for them.
                flags.truncate(runs.len());
                flags
            }
            None => Vec::new(),
        };
        let start = node_offset;
        node_offset += points.len();
        out.push(Patch {
            index,
            node_offset: start,
            points,
            topology: Topology::Polylines(runs),
            closed,
        });
    }
    Ok(out)
}

/// A wellbore trajectory: one parametric line, at its own knots.
///
/// The knots are the stations the well was surveyed at, and they are what comes
/// out - not a resampling of the curve between them. Two reasons, and the
/// second is the binding one: a spline passes through its own knots, so
/// sampling between them changes how smooth the drawn path looks and nothing
/// about where it goes; and the nodes of a trajectory are what its properties
/// are indexed by, so adding points would move every log value off its station.
///
/// The line is still *evaluated* rather than its control points copied out. It
/// matters for a vertical well, where the schema fills the third coordinate
/// from the parameter and leaves the control point's own slot meaningless -
/// see [`crate::parametric::Line::stations`].
fn trajectory(representation: &dyn Reflect, source: &dyn ValueSource) -> Result<Vec<Patch>> {
    let at = "WellboreTrajectoryRepresentation";
    let Some(geometry) = field_object(representation, "Geometry") else {
        return crate::well::datum_trajectory(representation, source, at);
    };
    let points = crate::parametric::Line::single(geometry, source, at)?.stations();
    if points.is_empty() {
        return Err(Error::malformed(at, "a trajectory with no control point"));
    }
    let length = points.len() as u32;
    Ok(vec![Patch {
        index: 0,
        node_offset: 0,
        points,
        topology: Topology::Polylines(vec![length]),
        closed: Vec::new(),
    }])
}

fn grid_2d(representation: &dyn Reflect, source: &dyn ValueSource) -> Result<Vec<Patch>> {
    let patch = field_object(representation, "Grid2dPatch").unwrap_or(representation);
    let points = geometry_points(patch, source, "Grid2dPatch")?;
    let fastest = values::scalar_i64(patch, "FastestAxisCount").unwrap_or(0).max(0) as usize;
    let slowest = values::scalar_i64(patch, "SlowestAxisCount").unwrap_or(0).max(0) as usize;

    // The two counts and the point count must agree, and when they do not it is
    // the file that is wrong - worth saying rather than drawing a wrong grid.
    let mut points = points;
    if fastest * slowest != points.len() {
        // One known writer disagrees in a provable way: SKUA-GOCAD sizes its
        // lattice `Spacing.Count` in points instead of intervals, so the
        // evaluated lattice is exactly one point too large per axis. When the
        // arithmetic pins it - (fastest+1) × (slowest+1) points - the extra
        // row and column are dropped; see `points::drop_last_per_axis`.
        if (fastest + 1) * (slowest + 1) == points.len() {
            points = crate::points::drop_last_per_axis(points, &[slowest + 1, fastest + 1]);
        } else {
            return Err(Error::malformed(
                "Grid2dPatch",
                format!(
                    "{fastest} × {slowest} axes describe {} point(s), the geometry has {}",
                    fastest * slowest,
                    points.len()
                ),
            ));
        }
    }
    Ok(vec![Patch {
        index: 0,
        node_offset: 0,
        points,
        topology: Topology::Grid { fastest, slowest },
        closed: Vec::new(),
    }])
}

// ---------------------------------------------------------------------------

/// The points of one patch.
///
/// Every step of the way out is an error, never an empty vector: a patch with
/// no geometry, or a geometry with no `Points`, means this code did not find
/// what the file holds. Returning nothing would be reported as a successful
/// read of a representation with nothing to draw.
fn patch_points(patch: &PatchParts<'_>, source: &dyn ValueSource, at: &str) -> Result<Vec<Point>> {
    let Some(geometry) = patch.geometry else {
        return Err(Error::malformed(at, "a patch that declares no geometry"));
    };
    let Some(array) = field_object(geometry, "Points") else {
        return Err(Error::malformed(at, "a geometry with no `Points`"));
    };
    points::read(
        array,
        source,
        &format!("{at}.Geometry.Points"),
        crate::parametric::Vertical::read(geometry, source),
    )
}

/// The points of a `PointGeometry` hanging directly on an element.
fn geometry_points(owner: &dyn Reflect, source: &dyn ValueSource, at: &str) -> Result<Vec<Point>> {
    let geometry = match field_object(owner, "Geometry") {
        Some(geometry) => geometry,
        // 2.2 hangs the geometry on the representation itself.
        None if field_object(owner, "Points").is_some() => owner,
        None => return Err(Error::malformed(at, "a patch that declares no geometry")),
    };
    let Some(array) = field_object(geometry, "Points") else {
        return Err(Error::malformed(at, "a geometry with no `Points`"));
    };
    points::read(
        array,
        source,
        &format!("{at}.Geometry.Points"),
        crate::parametric::Vertical::read(geometry, source),
    )
}

/// A field's objects, whether the schema made it a list or a single element.
///
/// Shared with the readers that live in their own modules - one implementation
/// of *what does this field hold*, because two would differ the first time one
/// of them met a field the schema declares singular and a file repeats.
pub(crate) fn field_list<'a>(object: &'a dyn Reflect, field: &str) -> Vec<&'a dyn Reflect> {
    list(object, field)
}

fn list<'a>(object: &'a dyn Reflect, field: &str) -> Vec<&'a dyn Reflect> {
    match object.field(field) {
        Some(FieldValue::List(items)) => items.iter().filter_map(|item| item.as_object()).collect(),
        Some(FieldValue::Object(one)) => vec![one],
        _ => Vec::new(),
    }
}

pub(crate) fn field_object<'a>(object: &'a dyn Reflect, field: &str) -> Option<&'a dyn Reflect> {
    match object.field(field)? {
        FieldValue::Object(child) => Some(child),
        _ => None,
    }
}

/// `type_key` is `&'static str` already; this only re-borrows it for the error.
pub(crate) fn leak(key: &str) -> &'static str {
    // Every `type_key` comes from a `&'static TypeInfo`, so the lifetime is
    // real; the signature of `field` just does not carry it.
    Box::leak(key.to_string().into_boxed_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_patch_reads_the_same_either_way() {
        // Offset zero: the two conventions coincide, and nothing has to be
        // guessed.
        assert_eq!(node_offset_used(&[0, 1, 3, 1, 4, 3], 0, 5, "t").unwrap(), 0);
    }

    #[test]
    fn a_later_patch_written_by_fesapi_is_offset() {
        // testingPackageCpp.epc, patch 1 of ea6943d0: five nodes numbered 5..9.
        assert_eq!(
            node_offset_used(&[5, 6, 8, 6, 9, 8, 6, 7, 9], 5, 5, "t").unwrap(),
            5
        );
    }

    #[test]
    fn a_later_patch_written_by_resqpy_is_not() {
        // The same patch with resqpy's numbering: indices fit its own points,
        // so no offset is removed.
        assert_eq!(node_offset_used(&[0, 1, 3, 1, 4, 3], 5, 5, "t").unwrap(), 0);
    }

    #[test]
    fn an_index_belonging_to_neither_is_refused() {
        // Drawing this would use whatever point happened to sit at that slot.
        let refused = node_offset_used(&[0, 1, 99], 5, 5, "TrianglePatch[1]");
        assert!(refused.is_err(), "{refused:?}");
    }

    #[test]
    fn merging_renumbers_into_one_buffer() {
        let geometry = Geometry {
            kind: "TriangulatedSetRepresentation",
            patches: vec![
                Patch {
                    index: 0,
                    node_offset: 0,
                    points: vec![[0.0; 3], [1.0; 3], [2.0; 3]],
                    topology: Topology::Triangles(vec![[0, 1, 2]]),
                    closed: Vec::new(),
                },
                Patch {
                    index: 1,
                    node_offset: 3,
                    points: vec![[3.0; 3], [4.0; 3], [5.0; 3]],
                    topology: Topology::Triangles(vec![[0, 1, 2]]),
                    closed: Vec::new(),
                },
            ],
        };
        let (points, triangles) = geometry.merged();
        assert_eq!(points.len(), 6);
        assert_eq!(triangles, vec![[0, 1, 2], [3, 4, 5]]);
    }
}
