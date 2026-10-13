//! What a triangulated horizon says about itself: dip, dip azimuth, curvature.
//!
//! The first of the geological operations, and the one with no dependency
//! at all: local differential geometry over the mesh the file already states.
//! Curvature is the most used fracture proxy there is, and dip and azimuth are
//! what every structural map is made of - each comes back as one value per
//! node, which is a `ContinuousProperty` and nothing else.
//!
//! # Which way is up is the caller's to say
//!
//! A mesh has no orientation: triangle winding in real packages is neither
//! consistent nor meaningful, and the vertical axis of a RESQML CRS may
//! increase **downward**, which most of the corpus does. So [`Up`] is a
//! parameter, and everything here is computed after the third coordinate has
//! been turned into an elevation. This is the architecture `t2d` already
//! follows: the law lives in the crate, applying it to an object is the
//! caller's seam - it is the caller that has read the CRS.
//!
//! Flipping the vertical is a **reflection**, so it is an isometry, and the two
//! curvatures do not answer to it the same way:
//!
//! - **Gaussian curvature is intrinsic** - it is a function of edge lengths
//!   alone (Gauss's *Theorema Egregium*), so it does not move;
//! - **mean and principal curvatures are signed against the normal**, so they
//!   change sign.
//!
//! That difference is pinned by a test, because a sign convention that is only
//! written in a comment is a sign convention that drifts.
//!
//! # What is refused, and what is merely absent
//!
//! A malformed call refuses by name - a coordinate count that is not a multiple
//! of three, a triangle naming a node the surface does not have. What is *not*
//! a refusal is a value that does not exist at a node:
//!
//! - **the azimuth of a horizontal patch.** Steepest descent has no direction
//!   there, and a zero would read as due north - the one bearing somebody might
//!   act on. It is `NaN`, and the count of those nodes is part of the answer;
//! - **the azimuth of a vertical patch**, for the same reason from the other
//!   end: steepest descent is straight down, so its horizontal projection is
//!   nothing;
//! - **the curvature of a boundary node.** The discrete operators below need a
//!   closed ring of triangles around the node - the Voronoi area and the
//!   angular defect both assume it. On the edge of a surface there is no ring,
//!   and extrapolating one would draw a rim of invented structure exactly where
//!   people look for the edge of the data.
//!
//! # The operators, and why these ones
//!
//! Curvature follows Meyer, Desbrun, Schröder and Barr (2003), *Discrete
//! Differential-Geometry Operators for Triangulated 2-Manifolds*: the cotangent
//! Laplacian over the **mixed** area for the mean curvature normal, the angular
//! defect over the same area for the Gaussian. The mixed area is the part worth
//! knowing about - a plain Voronoi area goes negative on obtuse triangles, and
//! a negative area turns a curvature inside out without failing.

use std::collections::HashMap;

use crate::Error;

/// Which way is up, on the third coordinate.
///
/// RESQML says it per CRS (`ZIncreasingDownward`), and most of the corpus does
/// increase downward. Guessing it from the data is not possible - a bowl and a
/// dome are the same triangles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Up {
    /// The third coordinate is an elevation: larger is higher.
    PlusZ,
    /// The third coordinate is a depth: larger is lower. The usual RESQML case.
    MinusZ,
}

impl Up {
    /// What the third coordinate must be multiplied by to become an elevation.
    pub(crate) fn sign(self) -> f64 {
        match self {
            Up::PlusZ => 1.0,
            Up::MinusZ => -1.0,
        }
    }
}

/// How steep the surface is at each node, and which way it falls.
#[derive(Debug, Clone, PartialEq)]
pub struct Slope {
    /// Degrees from horizontal: 0 is flat, 90 is a wall. One per node.
    pub dip: Vec<f64>,
    /// Dip azimuth in degrees, clockwise from +Y (north), in `[0, 360)`.
    ///
    /// The direction the surface **falls**, which is the one a structural map
    /// draws. `NaN` where it does not exist - see the module documentation.
    pub azimuth: Vec<f64>,
    /// Nodes where the surface is horizontal, so the azimuth is `NaN`.
    pub flat: usize,
    /// Nodes where the surface is vertical, so the azimuth is `NaN` too.
    pub sheer: usize,
    /// Triangles set aside: no area, or a corner that is not a finite point.
    pub set_aside: usize,
    /// Nodes no usable triangle reaches - everything about them is `NaN`.
    pub unreached: usize,
}

/// How the surface bends at each node.
#[derive(Debug, Clone, PartialEq)]
pub struct Curvature {
    /// Mean curvature, signed against *up*: positive over an antiform.
    pub mean: Vec<f64>,
    /// Gaussian curvature - intrinsic, so the vertical convention does not move it.
    pub gaussian: Vec<f64>,
    /// The larger principal curvature (*most positive*, the usual fracture proxy).
    pub maximum: Vec<f64>,
    /// The smaller principal curvature (*most negative*).
    pub minimum: Vec<f64>,
    /// Nodes on the boundary of the surface - all four values are `NaN` there.
    pub boundary: usize,
    /// Triangles set aside: no area, or a corner that is not a finite point.
    pub set_aside: usize,
    /// Nodes no usable triangle reaches.
    pub unreached: usize,
}

/// Dip and dip azimuth at every node.
///
/// The node normal is the **area-weighted** mean of the normals of the
/// triangles that meet there - the cross product is left unnormalised, so its
/// length is twice the triangle's area and the weighting is free. Averaging
/// unit normals instead would let a sliver triangle count as much as the face
/// beside it that is a thousand times larger.
pub fn slope(points: &[f64], triangles: &[u32], up: Up) -> Result<Slope, Error> {
    let mesh = Mesh::of(points, triangles, up)?;
    let nodes = mesh.nodes();
    let mut dip = vec![f64::NAN; nodes];
    let mut azimuth = vec![f64::NAN; nodes];
    let (mut flat, mut sheer, mut unreached) = (0usize, 0usize, 0usize);

    for at in 0..nodes {
        let Some(n) = mesh.normal(at) else {
            unreached += 1;
            continue;
        };
        // `n` is oriented upward, so `n[2] >= 0` and the arc cosine is in
        // `[0, 90]` - a dip is an angle with the horizontal, never a bearing.
        dip[at] = n[2].clamp(-1.0, 1.0).acos().to_degrees();
        let flatness = n[0].hypot(n[1]);
        if flatness <= LEVEL {
            // Horizontal here: the normal is straight up, so steepest descent
            // has no direction. Not zero - zero is due north.
            flat += 1;
            continue;
        }
        if n[2] <= LEVEL {
            // Vertical here: steepest descent is straight down, so it has no
            // horizontal projection either. The same absence from the far end.
            sheer += 1;
            continue;
        }
        // The horizontal part of an upward normal points **downhill**: for a
        // surface z = f(x, y) the upward normal is proportional to
        // `(-fx, -fy, 1)`, and `-grad f` is the direction of steepest descent.
        azimuth[at] = bearing(n[0], n[1]);
    }

    Ok(Slope {
        dip,
        azimuth,
        flat,
        sheer,
        set_aside: mesh.set_aside,
        unreached,
    })
}

/// How steep a **lattice** is at each node, and which way it falls - a 2D
/// grid (`Grid2dRepresentation`), its nodes row after row, the fast axis first.
///
/// **No cell is cut in two.** Splitting each cell by a diagonal would hand the
/// triangle reading a choice the file never made, and the two diagonals give
/// two answers on a twisted cell. The normal at a node is instead the cross
/// product of its two **central differences** - `P(i+1, j) − P(i−1, j)` along
/// the fast axis, `P(i, j+1) − P(i, j−1)` along the slow one, one-sided on the
/// border - the finite-difference slope every terrain model is read with
/// (Zevenbergen & Thorne, 1987, *Quantitative analysis of land surface
/// topography*, Earth Surface Processes and Landforms 12(1):47-56, the `G`
/// and `H` terms). The differences are taken on the **positions**, not on
/// indices times a spacing, so a lattice turned in its frame reads right.
///
/// A node whose differences reach a non-finite point - a hole of the grid -
/// is `unreached`, every value `NaN`, never a slope drawn across the hole.
pub fn lattice_slope(points: &[f64], fastest: usize, slowest: usize, up: Up) -> Result<Slope, Error> {
    if !points.len().is_multiple_of(3) {
        return Err(Error::NotTriples { given: points.len() });
    }
    let nodes = points.len() / 3;
    if nodes != fastest * slowest {
        return Err(Error::NotALattice {
            nodes,
            fastest,
            slowest,
        });
    }
    let sign = up.sign();
    let at = |i: usize, j: usize| {
        let k = 3 * (j * fastest + i);
        [points[k], points[k + 1], sign * points[k + 2]]
    };
    let finite = |p: [f64; 3]| p.iter().all(|c| c.is_finite());
    // The two neighbours along one axis - both when inside, the node itself
    // and its one neighbour on the border; `None` when the axis has one node.
    let span = |k: usize, count: usize| -> Option<(usize, usize)> {
        match count {
            0 | 1 => None,
            _ if k == 0 => Some((0, 1)),
            _ if k + 1 == count => Some((k - 1, k)),
            _ => Some((k - 1, k + 1)),
        }
    };
    let mut dip = vec![f64::NAN; nodes];
    let mut azimuth = vec![f64::NAN; nodes];
    let (mut flat, mut sheer, mut unreached) = (0usize, 0usize, 0usize);
    for j in 0..slowest {
        for i in 0..fastest {
            let index = j * fastest + i;
            let (Some((i0, i1)), Some((j0, j1))) = (span(i, fastest), span(j, slowest)) else {
                unreached += 1;
                continue;
            };
            // The node itself too: its central differences skip it, and a hole
            // would otherwise get the slope of its neighbours - found by the
            // test, which counted four unreached where five are.
            let (a, b, c, d) = (at(i0, j), at(i1, j), at(i, j0), at(i, j1));
            if !(finite(at(i, j)) && finite(a) && finite(b) && finite(c) && finite(d)) {
                unreached += 1;
                continue;
            }
            let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let v = [d[0] - c[0], d[1] - c[1], d[2] - c[2]];
            let mut n = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if length <= 0.0 || !length.is_finite() {
                unreached += 1;
                continue;
            }
            // Upward, whichever way the axes turn.
            let orient = (if n[2] < 0.0 { -1.0 } else { 1.0 }) / length;
            n = [n[0] * orient, n[1] * orient, n[2] * orient];
            dip[index] = n[2].clamp(-1.0, 1.0).acos().to_degrees();
            if n[0].hypot(n[1]) <= LEVEL {
                flat += 1;
                continue;
            }
            if n[2] <= LEVEL {
                sheer += 1;
                continue;
            }
            azimuth[index] = bearing(n[0], n[1]);
        }
    }
    Ok(Slope {
        dip,
        azimuth,
        flat,
        sheer,
        set_aside: 0,
        unreached,
    })
}

/// Mean, Gaussian and principal curvatures at every interior node.
pub fn curvature(points: &[f64], triangles: &[u32], up: Up) -> Result<Curvature, Error> {
    let mesh = Mesh::of(points, triangles, up)?;
    let nodes = mesh.nodes();
    let mut mean = vec![f64::NAN; nodes];
    let mut gaussian = vec![f64::NAN; nodes];
    let mut maximum = vec![f64::NAN; nodes];
    let mut minimum = vec![f64::NAN; nodes];
    let (mut boundary, mut unreached) = (0usize, 0usize);

    // The three accumulators of Meyer et al.: the mixed area, the cotangent
    // Laplacian of the position, and the angles that make up the defect.
    let mut area = vec![0.0f64; nodes];
    let mut laplacian = vec![[0.0f64; 3]; nodes];
    let mut angles = vec![0.0f64; nodes];

    for face in mesh.faces() {
        let corners = face.corners;
        let p = [mesh.at(corners[0]), mesh.at(corners[1]), mesh.at(corners[2])];
        let interior = [
            angle_at(p[0], p[1], p[2]),
            angle_at(p[1], p[2], p[0]),
            angle_at(p[2], p[0], p[1]),
        ];
        let obtuse = interior.iter().position(|one| *one > std::f64::consts::FRAC_PI_2);

        // **The cotangent stiffness, assembled edge by edge.** Each edge takes
        // the cotangent of the angle facing it and gives it to **both** of its
        // ends. Walking the corners instead and giving the term to `i` alone
        // was the first version, and it is wrong by exactly a factor of two: a
        // vertex then collects only one of the two cotangents of each of its
        // edges - the one from the triangle where the far end follows it in
        // winding order. Nothing about it looks wrong, and the sign, the shape
        // of the map and the Gaussian curvature all stay right; only the scale
        // of the mean curvature is halved. The sphere and the cylinder caught
        // it, both off by the same factor, which is what said it was assembly
        // and not discretisation.
        for corner in 0..3 {
            let (i, j) = (corners[(corner + 1) % 3], corners[(corner + 2) % 3]);
            let cot = cotangent(p[corner], p[(corner + 1) % 3], p[(corner + 2) % 3]);
            let (from, to) = (mesh.at(i), mesh.at(j));
            for axis in 0..3 {
                laplacian[i][axis] += cot * (from[axis] - to[axis]);
                laplacian[j][axis] += cot * (to[axis] - from[axis]);
            }
        }

        for corner in 0..3 {
            let i = corners[corner];
            let j = corners[(corner + 1) % 3];
            let k = corners[(corner + 2) % 3];
            angles[i] += interior[corner];

            area[i] += match obtuse {
                // **The mixed area.** A plain Voronoi cell escapes its own
                // triangle when that triangle is obtuse, and the formula then
                // returns a *negative* area - which turns the curvature inside
                // out with nothing to show for it.
                Some(at) if at == corner => face.area / 2.0,
                Some(_) => face.area / 4.0,
                None => {
                    let cot_j = cotangent(p[(corner + 1) % 3], p[corner], p[(corner + 2) % 3]);
                    let cot_k = cotangent(p[(corner + 2) % 3], p[corner], p[(corner + 1) % 3]);
                    let to_j = squared(mesh.at(i), mesh.at(j));
                    let to_k = squared(mesh.at(i), mesh.at(k));
                    (to_k * cot_j + to_j * cot_k) / 8.0
                }
            };
        }
    }

    for at in 0..nodes {
        if mesh.normal(at).is_none() {
            unreached += 1;
            continue;
        }
        if mesh.on_boundary(at) {
            boundary += 1;
            continue;
        }
        if area[at] <= 0.0 || !area[at].is_finite() {
            // Every triangle around it had no area, which the count already
            // says. A division here would put an infinity in a property.
            unreached += 1;
            continue;
        }
        let normal = mesh.normal(at).unwrap_or([0.0, 0.0, 1.0]);
        // Meyer's mean curvature normal is `K = laplacian / (2 A)` and its
        // length is `2 H`. The sign comes from the normal, and the normal is
        // oriented **up**, so a dome answers positive and a bowl negative -
        // the geological reading of *most positive curvature*.
        let scale = 1.0 / (2.0 * area[at]);
        let h = 0.5 * dot(laplacian[at], normal) * scale;
        let k = (std::f64::consts::TAU - angles[at]) / area[at];
        mean[at] = h;
        gaussian[at] = k;
        // `h² - k` is non-negative for any real surface; a small negative is
        // discretisation noise, and clamping it is what keeps two equal
        // principal curvatures from coming back as `NaN` on a sphere.
        let spread = (h * h - k).max(0.0).sqrt();
        maximum[at] = h + spread;
        minimum[at] = h - spread;
    }

    Ok(Curvature {
        mean,
        gaussian,
        maximum,
        minimum,
        boundary,
        set_aside: mesh.set_aside,
        unreached,
    })
}

/// Below this, a direction cosine is no direction at all.
///
/// It is a **cosine**, so it is dimensionless and the same number is right for
/// a field in metres and one in feet - which is the reason the threshold can be
/// written here rather than asked of the caller. `1e-9` is about
/// six ten-thousandths of a degree from horizontal.
const LEVEL: f64 = 1e-9;

/// Degrees clockwise from +Y, in `[0, 360)`.
///
/// `atan2(east, north)`, in that order: the usual `atan2(y, x)` measures
/// anticlockwise from east, and a bearing measures clockwise from north.
/// Swapping the arguments *is* the conversion, which is why it is written once.
fn bearing(east: f64, north: f64) -> f64 {
    let degrees = east.atan2(north).to_degrees();
    if degrees < 0.0 { degrees + 360.0 } else { degrees }
}

/// The interior angle at `a` in the triangle `a b c`.
fn angle_at(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> f64 {
    let u = minus(b, a);
    let v = minus(c, a);
    let cross = norm(cross(u, v));
    if cross <= 0.0 {
        return 0.0;
    }
    cross.atan2(dot(u, v))
}

/// The cotangent of the angle at `a` in the triangle `a b c`.
///
/// From the cross and dot products rather than from `1 / tan`: the tangent of a
/// right angle is an infinity, and a triangle with a right angle is entirely
/// ordinary.
fn cotangent(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> f64 {
    let u = minus(b, a);
    let v = minus(c, a);
    let cross = norm(cross(u, v));
    if cross <= 0.0 {
        return 0.0;
    }
    dot(u, v) / cross
}

fn squared(a: [f64; 3], b: [f64; 3]) -> f64 {
    let d = minus(a, b);
    dot(d, d)
}

fn minus(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

/// One usable triangle.
struct Face {
    corners: [usize; 3],
    area: f64,
}

/// The mesh as both operators want it: elevations, faces, node normals, and
/// which nodes sit on the edge of the surface.
///
/// Built once and shared, because [`slope`] and [`curvature`] are almost always
/// asked for together and the boundary walk is the expensive part.
struct Mesh {
    /// Points with the third coordinate turned into an elevation.
    points: Vec<[f64; 3]>,
    faces: Vec<Face>,
    /// The area-weighted normal at each node, oriented upward; `None` when no
    /// usable triangle reaches it.
    normals: Vec<Option<[f64; 3]>>,
    boundary: Vec<bool>,
    set_aside: usize,
}

impl Mesh {
    fn of(points: &[f64], triangles: &[u32], up: Up) -> Result<Self, Error> {
        if !points.len().is_multiple_of(3) {
            return Err(Error::NotTriples { given: points.len() });
        }
        if !triangles.len().is_multiple_of(3) {
            return Err(Error::NotTriangles {
                given: triangles.len(),
            });
        }
        let nodes = points.len() / 3;
        if let Some(&past) = triangles.iter().find(|at| **at as usize >= nodes) {
            return Err(Error::NodeOutOfRange {
                at: past as usize,
                nodes,
            });
        }

        let sign = up.sign();
        let held: Vec<[f64; 3]> = points
            .as_chunks::<3>()
            .0
            .iter()
            .map(|c| c.as_slice())
            .map(|one| [one[0], one[1], sign * one[2]])
            .collect();

        let mut faces = Vec::with_capacity(triangles.len() / 3);
        let mut set_aside = 0usize;
        // An edge met once belongs to the boundary; met twice or more, it does
        // not. Sorted, so the two windings of one edge are one key - a mesh
        // whose triangles are wound inconsistently is ordinary, and keying on
        // the ordered pair would report every one of its edges as a boundary.
        let mut edges: HashMap<(usize, usize), usize> = HashMap::new();
        let mut sums = vec![[0.0f64; 3]; nodes];

        for face in triangles.as_chunks::<3>().0.iter().map(|c| c.as_slice()) {
            let corners = [face[0] as usize, face[1] as usize, face[2] as usize];
            let p = [held[corners[0]], held[corners[1]], held[corners[2]]];
            let n = cross(minus(p[1], p[0]), minus(p[2], p[0]));
            let twice = norm(n);
            // Set aside rather than repaired: a corner that is not a finite
            // point, or three corners on one line. Both make every operator
            // below divide by nothing, and a repaired triangle would be
            // geometry nobody wrote.
            let finite = p.iter().flatten().all(|one| one.is_finite());
            if !twice.is_finite() || twice <= 0.0 || !finite {
                set_aside += 1;
                continue;
            }
            for corner in 0..3 {
                let (a, b) = (corners[corner], corners[(corner + 1) % 3]);
                *edges.entry((a.min(b), a.max(b))).or_insert(0) += 1;
                // Unnormalised on purpose: the length of the cross product is
                // twice the area, so summing it weights each face by its size.
                for axis in 0..3 {
                    sums[corners[corner]][axis] += n[axis];
                }
            }
            faces.push(Face {
                corners,
                area: twice / 2.0,
            });
        }

        let mut boundary = vec![false; nodes];
        for ((a, b), count) in &edges {
            if *count == 1 {
                boundary[*a] = true;
                boundary[*b] = true;
            }
        }

        let normals = sums
            .into_iter()
            .map(|sum| {
                let length = norm(sum);
                // A node whose faces cancel each other out - the tip of a fold
                // closed on itself, a duplicated triangle wound both ways - has
                // no normal, and inventing one would give it a dip.
                if length <= 0.0 || !length.is_finite() {
                    return None;
                }
                let mut n = [sum[0] / length, sum[1] / length, sum[2] / length];
                // Oriented **upward**, which is what makes the winding of the
                // file irrelevant: real packages are not consistent about it,
                // and a dip that flipped with the winding would be a map that
                // contradicts itself from one triangle to the next.
                if n[2] < 0.0 {
                    n = [-n[0], -n[1], -n[2]];
                }
                Some(n)
            })
            .collect();

        Ok(Mesh {
            points: held,
            faces,
            normals,
            boundary,
            set_aside,
        })
    }

    fn nodes(&self) -> usize {
        self.points.len()
    }

    fn at(&self, node: usize) -> [f64; 3] {
        self.points[node]
    }

    fn faces(&self) -> &[Face] {
        &self.faces
    }

    fn normal(&self, node: usize) -> Option<[f64; 3]> {
        self.normals[node]
    }

    fn on_boundary(&self, node: usize) -> bool {
        self.boundary[node]
    }
}

#[cfg(test)]
mod tests {

    /// A plane of a lattice read by central differences, at map coordinates
    /// and on a lattice turned 30° in its frame: the dip and the downhill
    /// bearing are the plane's, everywhere, and the triangle reading of the
    /// same plane - the other implementation - agrees. A hole is no slope.
    #[test]
    fn a_lattice_reads_the_slope_of_its_plane_turned_or_not_and_not_across_a_hole() {
        // z = 40 + 0.1 (x − x0) − 0.05 (y − y0), an elevation: downhill is
        // (−0.1, +0.05), so the bearing is atan2(−0.1, 0.05) and the dip
        // atan(√(0.01 + 0.0025)).
        let (x0, y0) = (651_000.0, 6_861_000.0);
        let plane = |x: f64, y: f64| 40.0 + 0.1 * (x - x0) - 0.05 * (y - y0);
        let dip = (0.0125f64).sqrt().atan().to_degrees();
        let bearing_expected = (-0.1f64).atan2(0.05).to_degrees() + 360.0;
        let (fast, slow) = (6usize, 5usize);
        for turn in [0.0f64, 30.0] {
            let (c, s_) = (turn.to_radians().cos(), turn.to_radians().sin());
            let mut points = Vec::new();
            for j in 0..slow {
                for i in 0..fast {
                    let (u, v) = (25.0 * i as f64, 25.0 * j as f64);
                    let (x, y) = (x0 + u * c - v * s_, y0 + u * s_ + v * c);
                    points.extend([x, y, plane(x, y)]);
                }
            }
            let read = lattice_slope(&points, fast, slow, Up::PlusZ).expect("a lattice");
            assert_eq!(read.unreached, 0);
            for (d, a) in read.dip.iter().zip(&read.azimuth) {
                assert!((d - dip).abs() < 1e-9, "turned {turn}: dip {d} for {dip}");
                assert!(
                    (a - bearing_expected).abs() < 1e-9,
                    "turned {turn}: azimuth {a} for {bearing_expected}"
                );
            }
            // Depths of the same surface: the same slope.
            let depths: Vec<f64> = points.chunks(3).flat_map(|p| [p[0], p[1], -p[2]]).collect();
            let down = lattice_slope(&depths, fast, slow, Up::MinusZ).expect("a lattice");
            assert!((down.dip[7] - dip).abs() < 1e-9 && (down.azimuth[7] - bearing_expected).abs() < 1e-9);
            // The triangle reading of the same plane.
            let mut triangles = Vec::new();
            for j in 0..slow - 1 {
                for i in 0..fast - 1 {
                    let a = (j * fast + i) as u32;
                    let (b, d) = (a + 1, a + fast as u32);
                    triangles.extend([a, b, d + 1, a, d + 1, d]);
                }
            }
            let other = slope(&points, &triangles, Up::PlusZ).expect("triangles");
            assert!(
                (other.dip[7] - read.dip[7]).abs() < 1e-9
                    && (other.azimuth[7] - read.azimuth[7]).abs() < 1e-9
            );
        }
        // A hole: the node itself and the ones whose differences reach it.
        let mut points = Vec::new();
        for j in 0..slow {
            for i in 0..fast {
                let (x, y) = (x0 + 25.0 * i as f64, y0 + 25.0 * j as f64);
                points.extend([x, y, plane(x, y)]);
            }
        }
        points[3 * (2 * fast + 2) + 2] = f64::NAN;
        let read = lattice_slope(&points, fast, slow, Up::PlusZ).expect("a lattice");
        assert_eq!(read.unreached, 5, "the hole and its four neighbours");
        assert!(read.dip[2 * fast + 1].is_nan() && read.dip[2 * fast + 2].is_nan());
        assert!(read.dip[0].is_finite());
        assert!(matches!(
            lattice_slope(&points, 4, 5, Up::PlusZ),
            Err(Error::NotALattice { .. })
        ));
    }

    use super::*;

    /// A parametric patch as a grid of quads, each split into two triangles.
    ///
    /// One helper for the plane, the sphere and the cylinder, so that the three
    /// analytic cases are not three meshes written three ways - the one thing a
    /// test of curvature must not get wrong is its own input.
    ///
    /// `wrap_u` closes the grid in the first parameter, which is what turns a
    /// strip into a tube: without it every node of the seam is a boundary node
    /// and the interior a test looks at is not the interior it thinks.
    fn patch(nu: usize, nv: usize, wrap_u: bool, f: impl Fn(f64, f64) -> [f64; 3]) -> (Vec<f64>, Vec<u32>) {
        let columns = if wrap_u { nu } else { nu + 1 };
        let mut points = Vec::with_capacity(columns * (nv + 1) * 3);
        for row in 0..=nv {
            for column in 0..columns {
                let p = f(column as f64 / nu as f64, row as f64 / nv as f64);
                points.extend_from_slice(&p);
            }
        }
        let mut triangles = Vec::new();
        let at = |row: usize, column: usize| (row * columns + column % columns) as u32;
        for row in 0..nv {
            for column in 0..nu {
                let (a, b) = (at(row, column), at(row, column + 1));
                let (c, d) = (at(row + 1, column), at(row + 1, column + 1));
                triangles.extend_from_slice(&[a, b, d]);
                triangles.extend_from_slice(&[a, d, c]);
            }
        }
        (points, triangles)
    }

    /// A flat grid on `[0, 1]²`, raised by `z(x, y)`.
    fn sheet(n: usize, z: impl Fn(f64, f64) -> f64) -> (Vec<f64>, Vec<u32>) {
        patch(n, n, false, |u, v| [u, v, z(u, v)])
    }

    /// A sphere of radius `r`, its poles left out - they are the one place a
    /// latitude/longitude mesh is not a fair test of anything.
    fn sphere(r: f64, nu: usize, nv: usize) -> (Vec<f64>, Vec<u32>) {
        patch(nu, nv, true, |u, v| {
            let phi = u * std::f64::consts::TAU;
            // `v` runs over the middle 80% of the colatitude, so the rings that
            // do touch the boundary are far from the band the test reads.
            let theta = (0.1 + 0.8 * v) * std::f64::consts::PI;
            [
                r * theta.sin() * phi.cos(),
                r * theta.sin() * phi.sin(),
                r * theta.cos(),
            ]
        })
    }

    /// A vertical tube of radius `r` - the case that tells `H` from `K`.
    fn cylinder(r: f64, height: f64, nu: usize, nv: usize) -> (Vec<f64>, Vec<u32>) {
        patch(nu, nv, true, |u, v| {
            let phi = u * std::f64::consts::TAU;
            [r * phi.cos(), r * phi.sin(), v * height]
        })
    }

    /// The nodes a curvature test may read: interior, and answered.
    fn answered(one: &[f64]) -> Vec<f64> {
        one.iter().copied().filter(|v| v.is_finite()).collect()
    }

    fn close(got: f64, want: f64, tolerance: f64) -> bool {
        (got - want).abs() <= tolerance * want.abs().max(1.0)
    }

    // -- Dip and azimuth --------------------------------------------------

    #[test]
    fn a_tilted_plane_dips_by_its_tilt_everywhere() {
        // The one case whose answer is known at every node, boundary included:
        // dip and azimuth need no ring of triangles, only a normal.
        let tilt: f64 = 30.0;
        let slant = tilt.to_radians().tan();
        let (points, triangles) = sheet(4, |x, _| slant * x);
        let made = slope(&points, &triangles, Up::PlusZ).expect("a plane");
        for one in &made.dip {
            assert!(close(*one, tilt, 1e-9), "dip {one} where {tilt} was expected");
        }
        // It rises toward +x, which is east, so it **falls** toward west: 270.
        for one in &made.azimuth {
            assert!(close(*one, 270.0, 1e-9), "azimuth {one} where 270 was expected");
        }
        assert_eq!((made.flat, made.sheer, made.unreached), (0, 0, 0));
    }

    #[test]
    fn a_horizontal_plane_has_no_azimuth_at_all() {
        // **And not zero.** A zero would read as due north - the one bearing
        // somebody might act on - where the honest answer is that steepest
        // descent has no direction.
        let (points, triangles) = sheet(3, |_, _| 0.0);
        let made = slope(&points, &triangles, Up::PlusZ).expect("a plane");
        assert!(made.dip.iter().all(|one| *one == 0.0));
        assert!(made.azimuth.iter().all(|one| one.is_nan()));
        assert_eq!(made.flat, points.len() / 3);
    }

    #[test]
    fn a_vertical_wall_has_no_azimuth_either() {
        // The same absence from the far end: steepest descent is straight down,
        // so its horizontal projection is nothing. Counted apart from *flat*,
        // because a wall and a table are not the same news.
        let (points, triangles) = patch(4, 4, false, |u, v| [u, 0.0, v]);
        let made = slope(&points, &triangles, Up::PlusZ).expect("a wall");
        assert!(made.dip.iter().all(|one| close(*one, 90.0, 1e-9)));
        assert!(made.azimuth.iter().all(|one| one.is_nan()));
        assert_eq!((made.flat, made.sheer), (0, points.len() / 3));
    }

    #[test]
    fn the_vertical_convention_turns_the_azimuth_around_and_leaves_the_dip() {
        // A depth axis is a **reflection**, so the angle with the horizontal
        // does not move and the direction of fall reverses. Getting this wrong
        // draws a map whose arrows all point the wrong way - and every dip on
        // it is right, which is what makes it hard to catch by eye.
        let slant: f64 = 25.0_f64.to_radians().tan();
        let (points, triangles) = sheet(3, |x, _| slant * x);
        let elevation = slope(&points, &triangles, Up::PlusZ).expect("plane");
        let depth = slope(&points, &triangles, Up::MinusZ).expect("plane");
        assert!(close(elevation.dip[0], depth.dip[0], 1e-12));
        assert!(close(elevation.azimuth[0], 270.0, 1e-9));
        assert!(close(depth.azimuth[0], 90.0, 1e-9));
    }

    #[test]
    fn the_winding_of_the_file_does_not_change_the_map() {
        // Real packages are not consistent about it, and a dip that flipped
        // with the winding would be a map contradicting itself from one
        // triangle to the next. The normals are oriented upward for this.
        let slant: f64 = 20.0_f64.to_radians().tan();
        let (points, triangles) = sheet(3, |x, _| slant * x);
        let flipped: Vec<u32> = triangles
            .as_chunks::<3>()
            .0
            .iter()
            .map(|c| c.as_slice())
            .flat_map(|one| [one[0], one[2], one[1]])
            .collect();
        let straight = slope(&points, &triangles, Up::PlusZ).expect("plane");
        let reversed = slope(&points, &flipped, Up::PlusZ).expect("plane");
        assert_eq!(straight.dip, reversed.dip);
        assert_eq!(straight.azimuth, reversed.azimuth);
    }

    // -- Curvature --------------------------------------------------------

    #[test]
    fn a_plane_has_no_curvature_and_its_rim_has_none_to_give() {
        let (points, triangles) = sheet(6, |_, _| 0.0);
        let made = curvature(&points, &triangles, Up::PlusZ).expect("a plane");
        for one in answered(&made.mean) {
            assert!(one.abs() < 1e-9, "mean {one} on a plane");
        }
        for one in answered(&made.gaussian) {
            assert!(one.abs() < 1e-9, "gaussian {one} on a plane");
        }
        // A 7×7 grid has 25 interior nodes and 24 on the rim, and the rim is
        // `NaN` rather than zero: the operators need a closed ring, and
        // extrapolating one would draw invented structure exactly where people
        // look for the edge of the data.
        assert_eq!(made.boundary, 24);
        assert_eq!(answered(&made.gaussian).len(), 25);
    }

    #[test]
    fn a_sphere_curves_by_its_radius_both_ways() {
        // `K = 1/r²` and `|H| = 1/r`, which is the pair that tells the two
        // operators apart: a mean curvature mistaken for a Gaussian one passes
        // every flat test there is and fails here by a factor of `r`.
        let r = 4.0;
        let (points, triangles) = sphere(r, 60, 30);
        let made = curvature(&points, &triangles, Up::PlusZ).expect("a sphere");
        let gaussian = answered(&made.gaussian);
        assert!(gaussian.len() > 1000, "{} interior nodes", gaussian.len());
        for one in &gaussian {
            assert!(
                close(*one, 1.0 / (r * r), 0.05),
                "gaussian {one} for 1/r² = {}",
                1.0 / (r * r)
            );
        }
        for one in answered(&made.mean) {
            assert!(
                close(one.abs(), 1.0 / r, 0.05),
                "|mean| {} for 1/r = {}",
                one.abs(),
                1.0 / r
            );
        }
    }

    #[test]
    fn a_dome_is_positive_and_a_bowl_is_negative() {
        // The sign convention, pinned rather than described: the normal is
        // oriented **up**, so an antiform answers positive. It is the reading
        // *most positive curvature* has in every structural map, and a
        // convention that lives only in a comment is one that drifts.
        let r = 4.0;
        let (points, triangles) = sphere(r, 60, 30);
        let mean = curvature(&points, &triangles, Up::PlusZ).expect("a sphere").mean;
        let cap = |above: bool| -> Vec<f64> {
            points
                .as_chunks::<3>()
                .0
                .iter()
                .map(|c| c.as_slice())
                .enumerate()
                .filter(|(at, p)| {
                    mean[*at].is_finite() && if above { p[2] > 0.8 * r } else { p[2] < -0.8 * r }
                })
                .map(|(at, _)| mean[at])
                .collect()
        };
        let (top, bottom) = (cap(true), cap(false));
        assert!(
            top.len() > 10 && bottom.len() > 10,
            "{} and {}",
            top.len(),
            bottom.len()
        );
        assert!(top.iter().all(|one| *one > 0.0), "the upper cap is not a dome");
        assert!(bottom.iter().all(|one| *one < 0.0), "the lower cap is not a bowl");
    }

    #[test]
    fn a_cylinder_bends_one_way_only() {
        // The case that separates the principal curvatures: `k1 = 1/r`,
        // `k2 = 0`, so `K = 0` and `|H| = 1/(2r)`. A sphere alone would let an
        // implementation that confused the two principal curvatures pass,
        // because on a sphere they are equal.
        let r = 3.0;
        let (points, triangles) = cylinder(r, 6.0, 60, 30);
        let made = curvature(&points, &triangles, Up::PlusZ).expect("a cylinder");
        for one in answered(&made.gaussian) {
            assert!(
                one.abs() < 0.02,
                "gaussian {one} on a cylinder, where 0 is expected"
            );
        }
        for one in answered(&made.mean) {
            assert!(close(one.abs(), 0.5 / r, 0.05), "|mean| {}", one.abs());
        }
        // One principal curvature is `1/r`, the other is nothing - which of the
        // two carries which name depends on the sign of the tube's normal, so
        // the test reads the pair rather than either name.
        for (a, b) in answered(&made.maximum).iter().zip(answered(&made.minimum)) {
            let (a, b) = (a.abs(), b.abs());
            let (big, small) = if a > b { (a, b) } else { (b, a) };
            assert!(close(big, 1.0 / r, 0.05), "the bent direction is {big}");
            assert!(small < 0.02, "the straight direction is {small}");
        }
    }

    #[test]
    fn the_gaussian_curvature_ignores_the_vertical_convention_and_the_mean_does_not() {
        // *Theorema egregium*, as a test. Flipping the vertical is a
        // reflection: the Gaussian curvature is a function of edge lengths
        // alone so it cannot move, while the mean one is signed against the
        // normal so it must change sign. An implementation that flipped both,
        // or neither, is wrong in a way no single-orientation test can see.
        let (points, triangles) = sphere(4.0, 40, 20);
        let up = curvature(&points, &triangles, Up::PlusZ).expect("a sphere");
        let down = curvature(&points, &triangles, Up::MinusZ).expect("a sphere");
        let (mut compared, mut signed) = (0usize, 0usize);
        for at in 0..up.gaussian.len() {
            if !up.gaussian[at].is_finite() || !down.gaussian[at].is_finite() {
                continue;
            }
            compared += 1;
            assert!(
                close(up.gaussian[at], down.gaussian[at], 1e-9),
                "gaussian moved: {} then {}",
                up.gaussian[at],
                down.gaussian[at]
            );
            assert!(
                close(up.mean[at], -down.mean[at], 1e-9),
                "mean did not turn around"
            );
            if up.mean[at].abs() > 1e-6 {
                signed += 1;
            }
        }
        // Without this the assertion above would be satisfied by a mean
        // curvature that is zero everywhere - a test that cannot fail.
        assert!(
            compared > 500 && signed > 500,
            "{compared} compared, {signed} signed"
        );
    }

    // -- What is set aside, and what is refused ---------------------------

    #[test]
    fn a_triangle_with_no_area_is_set_aside_and_counted() {
        // Set aside rather than repaired: three corners on one line make every
        // operator divide by nothing, and a repaired triangle would be geometry
        // nobody wrote.
        let points = vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let triangles = vec![0, 1, 2, 0, 1, 3];
        let made = slope(&points, &triangles, Up::PlusZ).expect("a mesh");
        assert_eq!(made.set_aside, 1);
        // And node 2, which only the flat triangle reached, has no answer at all.
        assert!(made.dip[2].is_nan());
        assert_eq!(made.unreached, 1);
    }

    #[test]
    fn a_corner_that_is_not_a_point_takes_its_triangle_out() {
        let points = vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, f64::NAN];
        let triangles = vec![0, 1, 2];
        let made = slope(&points, &triangles, Up::PlusZ).expect("a mesh");
        assert_eq!(made.set_aside, 1);
        assert_eq!(made.unreached, 3);
    }

    #[test]
    fn a_malformed_call_refuses_by_name() {
        // A refusal and an absent value are different news: one says the call
        // was wrong, the other says the surface has nothing to say there.
        assert_eq!(
            slope(&[0.0, 1.0], &[], Up::PlusZ),
            Err(Error::NotTriples { given: 2 })
        );
        assert_eq!(
            slope(&[0.0, 0.0, 0.0], &[0, 0], Up::PlusZ),
            Err(Error::NotTriangles { given: 2 })
        );
        assert_eq!(
            curvature(&[0.0, 0.0, 0.0], &[0, 1, 2], Up::PlusZ),
            Err(Error::NodeOutOfRange { at: 1, nodes: 1 })
        );
    }
}
