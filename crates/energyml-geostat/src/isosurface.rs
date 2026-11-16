//! Iso-surfaces of a scalar over a structured lattice: marching tetrahedra.
//!
//! The third dimension of [`crate::isocontour`]: the surface where a field
//! takes a chosen value, over `ni x nj x nl` nodes whose positions are given
//! slice by slice. The result is a welded triangle mesh in the lattice's own
//! coordinates, and a [`Crossing`] that says what happened when there is
//! nothing to draw - never an empty mesh that reads as "nothing to see".
//!
//! # Tetrahedra, not cubes
//!
//! Every cell is cut into six tetrahedra by the Kuhn subdivision: all of them
//! share the diagonal `(0,0,0)-(1,1,1)`. Two neighbouring cells therefore cut
//! their common face along the same diagonal, so the surface has no crack, and
//! a tetrahedron has no ambiguous case (one corner alone on its side gives a
//! triangle, two and two give a quadrilateral cut in two triangles), so there
//! is no lookup table of 256 cases to get wrong.
//!
//! # Vertices
//!
//! A vertex lies on an edge of the lattice, and the edge is always interpolated
//! from its smaller node index to its larger one, so two tetrahedra sharing an
//! edge land on the same bits. The vertex is keyed by that edge (see
//! [`crate::isocontour::At`]), never by its coordinates, and numbered at first
//! use in the order of the walk: the same lattice gives the same mesh, bit for
//! bit.
//!
//! # A folded lattice
//!
//! A lattice that folds back on itself (two neighbouring slices that cross, a
//! radius of curvature smaller than the largest offset) gives a wrong surface
//! and nothing here can see it: this function only knows positions. The caller
//! refuses **before** the call when its own fold check (`TransverseGrid::check_folds`
//! of the well geometry) is not empty.
//!
//! TODO(reference): verify - Kuhn (1960), *Some combinatorial lemmas in
//! topology*, for the subdivision; Doi and Koide (1991), *An efficient method of
//! triangulating equi-valued surfaces by using tetrahedral cells*, IEICE Trans.
//! E74(1), for the method. Neither was re-read.

use std::collections::HashMap;

use crate::Error;
use crate::isocontour::At;
use crate::vector::{cross, dot, sub};

/// How a value is taken to vary along an edge between two nodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueSpacing {
    /// Linearly in the value.
    Linear,
    /// Linearly in the base-10 logarithm of the value. A value <= 0 has none:
    /// the cells it belongs to are set aside and counted, never filled.
    Log10,
}

/// What the level does with respect to the field - said, never an empty mesh
/// that reads as "nothing to see".
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Crossing {
    /// At least one triangle was made.
    Crossed,
    /// Every value of the usable cells is below the level; `max` is the largest.
    AllBelow { max: f64 },
    /// Every value of the usable cells is at or above the level; `min` is the smallest.
    AllAbove { min: f64 },
    /// Some tetrahedra straddle the level, but it only touches nodes or edges:
    /// no surface with an area.
    Grazed,
    /// Usable cells lie on both sides of the level, yet none straddles it: the
    /// crossing is inside cells that were set aside.
    InSetAsideCells,
}

/// An iso-surface, and what the pass had to set aside.
#[derive(Clone, Debug, PartialEq)]
pub struct IsoSurface {
    /// The level traced, as given (not its logarithm).
    pub level: f64,
    /// The spacing used along the edges.
    pub spacing: ValueSpacing,
    /// The vertices, in the lattice's own coordinates (what `positions` gave).
    pub points: Vec<[f64; 3]>,
    /// Where each vertex sits on the lattice: the edge between two nodes (node
    /// indices, smaller first) or a node itself - the identity that welds two
    /// cells sharing a face, see `isocontour::At`.
    pub on: Vec<At>,
    /// Three vertex indices each, wound so that the normal points towards the
    /// higher values.
    pub triangles: Vec<[u32; 3]>,
    /// For each triangle, the lattice cell `[i, j, l]` it was cut from.
    pub cell_of: Vec<[u32; 3]>,
    /// Cells set aside: a corner with no value (NaN, or <= 0 under `Log10`) or
    /// no position (a non-finite coordinate). Counted, never filled.
    pub set_aside: usize,
    /// See [`Crossing`].
    pub crossing: Crossing,
}

/// The six Kuhn tetrahedra of a cell: one per permutation of the axes, in this
/// fixed order.
const KUHN: [[usize; 3]; 6] = [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]];

/// One corner of a cell.
#[derive(Clone, Copy)]
struct Corner {
    /// Global node index `(i * nj + j) * nl + l`.
    node: u32,
    /// The working value (the value, or its base-10 logarithm).
    w: f64,
    pos: [f64; 3],
}

/// A cut of the level through an edge.
#[derive(Clone, Copy)]
struct Cut {
    point: [f64; 3],
    at: At,
}

fn mean(corners: &[&Corner]) -> [f64; 3] {
    let n = corners.len() as f64;
    let mut sum = [0.0; 3];
    for c in corners {
        for (s, p) in sum.iter_mut().zip(c.pos) {
            *s += p;
        }
    }
    [sum[0] / n, sum[1] / n, sum[2] / n]
}

/// The cut of the level through the edge between two corners on opposite sides
/// of it. Always interpolated from the smaller node index to the larger one, so
/// that every tetrahedron sharing the edge computes the same bits.
fn cut(a: &Corner, b: &Corner, level_w: f64) -> Cut {
    let (p, q) = if a.node < b.node { (a, b) } else { (b, a) };
    // `q.w - p.w` cannot be zero: the corners are on opposite sides of the
    // level, so their working values differ.
    let t = (level_w - p.w) / (q.w - p.w);
    let point = [
        p.pos[0] + t * (q.pos[0] - p.pos[0]),
        p.pos[1] + t * (q.pos[1] - p.pos[1]),
        p.pos[2] + t * (q.pos[2] - p.pos[2]),
    ];
    // A cut at `t = 0` or `t = 1` is not on the edge, it **is** a node, and the
    // same node is reached from every edge that meets it.
    let at = if t == 0.0 {
        At::Node(p.node)
    } else if t == 1.0 {
        At::Node(q.node)
    } else {
        At::Edge(p.node, q.node)
    };
    Cut { point, at }
}

/// Marching tetrahedra over a lattice of `dims = [ni, nj, nl]` nodes.
///
/// `values` holds one value per node, **the slowest dimension first**: node
/// `(i, j, l)` is `values[(i * nj + j) * nl + l]` - the order of a cube on the
/// nodes of a frame, `[nodes, across, lateral]` (RESQML TRG 2.2 §7, the note of
/// the figure p. 221; §7.1 p. 222). `positions(i)` gives the `nj * nl` points of
/// slice `i` in the same `(j, l)` order; a node with no position is given as a
/// non-finite point. Only two slices are held at a time, and each slice is
/// asked once.
///
/// A lattice that folds back on itself gives a wrong surface without anything
/// showing it here: the caller must refuse before the call when its fold check
/// (`TransverseGrid::check_folds`) is not empty.
pub fn marching_tetrahedra(
    dims: [usize; 3],
    positions: &dyn Fn(usize) -> Vec<[f64; 3]>,
    values: &[f64],
    level: f64,
    spacing: ValueSpacing,
) -> Result<IsoSurface, Error> {
    let [ni, nj, nl] = dims;
    if ni < 2 || nj < 2 || nl < 2 {
        return Err(Error::NotAVolume { dims });
    }
    let most = u32::MAX as usize;
    let nodes = ni
        .checked_mul(nj)
        .and_then(|n| n.checked_mul(nl))
        .filter(|n| *n <= most);
    let Some(nodes) = nodes else {
        let all = ni.checked_mul(nj).and_then(|n| n.checked_mul(nl));
        return Err(Error::TooManyNodes {
            nodes: all.unwrap_or(usize::MAX),
            most,
        });
    };
    if values.len() != nodes {
        return Err(Error::LatticeValues {
            values: values.len(),
            nodes,
        });
    }
    if !level.is_finite() {
        return Err(Error::BadLevel { given: level });
    }
    if spacing == ValueSpacing::Log10 && level <= 0.0 {
        return Err(Error::LevelNotPositive { given: level });
    }

    let work = |v: f64| -> f64 {
        match spacing {
            ValueSpacing::Linear => v,
            ValueSpacing::Log10 => {
                if v > 0.0 {
                    v.log10()
                } else {
                    f64::NAN
                }
            }
        }
    };
    let level_w = work(level);
    let w: Vec<f64> = values.iter().map(|v| work(*v)).collect();

    let per_slice = nj * nl;
    let slice = |i: usize| -> Result<Vec<[f64; 3]>, Error> {
        let given = positions(i);
        if given.len() != per_slice {
            return Err(Error::SliceLength {
                slice: i,
                given: given.len(),
                expected: per_slice,
            });
        }
        Ok(given)
    };

    let mut points: Vec<[f64; 3]> = Vec::new();
    let mut on: Vec<At> = Vec::new();
    let mut triangles: Vec<[u32; 3]> = Vec::new();
    let mut cell_of: Vec<[u32; 3]> = Vec::new();
    let mut index_of: HashMap<At, u32> = HashMap::new();
    let mut set_aside = 0usize;
    let mut usable = 0usize;
    let mut straddled = 0usize;
    let (mut min_w, mut max_w) = (f64::INFINITY, f64::NEG_INFINITY);
    let (mut min_v, mut max_v) = (f64::INFINITY, f64::NEG_INFINITY);

    let mut previous = slice(0)?;
    for i in 0..ni - 1 {
        let next = slice(i + 1)?;
        for j in 0..nj - 1 {
            for l in 0..nl - 1 {
                // The eight corners, indexed `di * 4 + dj * 2 + dl`.
                let mut corners = [Corner {
                    node: 0,
                    w: 0.0,
                    pos: [0.0; 3],
                }; 8];
                let mut ok = true;
                for (d, corner) in corners.iter_mut().enumerate() {
                    let (di, dj, dl) = (d >> 2, (d >> 1) & 1, d & 1);
                    let node = (i + di) * nj * nl + (j + dj) * nl + (l + dl);
                    let held = if di == 0 { &previous } else { &next };
                    let pos = held[(j + dj) * nl + (l + dl)];
                    *corner = Corner {
                        node: node as u32,
                        w: w[node],
                        pos,
                    };
                    if !corner.w.is_finite() || !pos.iter().all(|c| c.is_finite()) {
                        ok = false;
                    }
                }
                if !ok {
                    set_aside += 1;
                    continue;
                }
                usable += 1;
                for corner in &corners {
                    min_w = min_w.min(corner.w);
                    max_w = max_w.max(corner.w);
                    let v = values[corner.node as usize];
                    min_v = min_v.min(v);
                    max_v = max_v.max(v);
                }

                for sigma in KUHN {
                    let mut c = [0usize; 3];
                    let mut ids = [0usize; 4];
                    ids[0] = 0;
                    c[sigma[0]] = 1;
                    ids[1] = c[0] * 4 + c[1] * 2 + c[2];
                    c[sigma[1]] = 1;
                    ids[2] = c[0] * 4 + c[1] * 2 + c[2];
                    ids[3] = 7;
                    let tetra = [
                        &corners[ids[0]],
                        &corners[ids[1]],
                        &corners[ids[2]],
                        &corners[ids[3]],
                    ];
                    // `>=`: a value on the level counts as above it, as in the
                    // iso-contour, so an edge lying on the level is traced.
                    let mut above: Vec<&Corner> = Vec::with_capacity(4);
                    let mut below: Vec<&Corner> = Vec::with_capacity(4);
                    for corner in tetra {
                        if corner.w >= level_w {
                            above.push(corner);
                        } else {
                            below.push(corner);
                        }
                    }
                    if above.is_empty() || below.is_empty() {
                        continue;
                    }
                    straddled += 1;

                    let g = {
                        let (a, b) = (mean(&above), mean(&below));
                        sub(a, b)
                    };
                    let mut made: Vec<[Cut; 3]> = Vec::with_capacity(2);
                    match above.len() {
                        1 | 3 => {
                            let (lone, others) = if above.len() == 1 {
                                (above[0], &below)
                            } else {
                                (below[0], &above)
                            };
                            made.push([
                                cut(lone, others[0], level_w),
                                cut(lone, others[1], level_w),
                                cut(lone, others[2], level_w),
                            ]);
                        }
                        _ => {
                            let (a1, a2, b1, b2) = (above[0], above[1], below[0], below[1]);
                            let q = [
                                cut(a1, b1, level_w),
                                cut(a1, b2, level_w),
                                cut(a2, b2, level_w),
                                cut(a2, b1, level_w),
                            ];
                            made.push([q[0], q[1], q[2]]);
                            made.push([q[0], q[2], q[3]]);
                        }
                    }

                    for mut tri in made {
                        // The level only touches a node: two ends are one vertex.
                        if tri[0].at == tri[1].at || tri[1].at == tri[2].at || tri[0].at == tri[2].at {
                            continue;
                        }
                        let n = cross(sub(tri[1].point, tri[0].point), sub(tri[2].point, tri[0].point));
                        if dot(n, g) < 0.0 {
                            tri.swap(1, 2);
                        }
                        let mut face = [0u32; 3];
                        for (slot, one) in face.iter_mut().zip(tri) {
                            *slot = *index_of.entry(one.at).or_insert_with(|| {
                                points.push(one.point);
                                on.push(one.at);
                                (points.len() - 1) as u32
                            });
                        }
                        triangles.push(face);
                        cell_of.push([i as u32, j as u32, l as u32]);
                    }
                }
            }
        }
        previous = next;
    }

    if usable == 0 {
        return Err(Error::NoSample {
            given: (ni - 1) * (nj - 1) * (nl - 1),
            set_aside,
        });
    }
    let crossing = if !triangles.is_empty() {
        Crossing::Crossed
    } else if straddled > 0 {
        Crossing::Grazed
    } else if max_w < level_w {
        Crossing::AllBelow { max: max_v }
    } else if min_w >= level_w {
        Crossing::AllAbove { min: min_v }
    } else {
        Crossing::InSetAsideCells
    };
    Ok(IsoSurface {
        level,
        spacing,
        points,
        on,
        triangles,
        cell_of,
        set_aside,
        crossing,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vector::norm;
    use std::cell::Cell;
    use std::collections::HashMap;

    /// The positions of slice `i` of a lattice of `n` nodes per axis spread
    /// regularly over the cube `[lo, hi]^3`, `(j, l)` order.
    fn lattice(n: usize, lo: f64, hi: f64, i: usize) -> Vec<[f64; 3]> {
        let at = |k: usize| lo + (hi - lo) * k as f64 / (n - 1) as f64;
        let mut out = Vec::new();
        for j in 0..n {
            for l in 0..n {
                out.push([at(i), at(j), at(l)]);
            }
        }
        out
    }

    /// One value per node, the slowest dimension first, from the node position.
    fn field(n: usize, lo: f64, hi: f64, f: impl Fn([f64; 3]) -> f64) -> Vec<f64> {
        let mut out = Vec::new();
        for i in 0..n {
            for p in lattice(n, lo, hi, i) {
                out.push(f(p));
            }
        }
        out
    }

    /// A unit-spaced lattice of the given dimensions: the node `(i, j, l)` is at
    /// `[i, j, l]`.
    fn unit_slice(dims: [usize; 3], i: usize) -> Vec<[f64; 3]> {
        let mut out = Vec::new();
        for j in 0..dims[1] {
            for l in 0..dims[2] {
                out.push([i as f64, j as f64, l as f64]);
            }
        }
        out
    }

    /// The sphere field of the geometry tests: 21^3 nodes on `[-1, 1]^3`,
    /// `v = |p|`. The level is 0.605, not 0.6: no node of this lattice sits
    /// exactly on it (the squared norms are multiples of 0.01), so no vertex is
    /// a node and the mesh stays a closed manifold the tests can count.
    fn sphere() -> IsoSurface {
        let n = 21;
        let values = field(n, -1.0, 1.0, norm);
        marching_tetrahedra(
            [n, n, n],
            &|i| lattice(n, -1.0, 1.0, i),
            &values,
            0.605,
            ValueSpacing::Linear,
        )
        .expect("a sphere field")
    }

    #[test]
    fn iso_of_a_sphere_field_sits_on_the_sphere() {
        let s = sphere();
        assert_eq!(s.crossing, Crossing::Crossed);
        assert!(!s.points.is_empty());
        for p in &s.points {
            assert!((norm(*p) - 0.605).abs() < 0.03, "{p:?}");
        }
    }

    #[test]
    fn log_spacing_puts_20_halfway_between_10_and_40() {
        let dims = [2, 2, 2];
        // `l` is the fastest dimension: `l = 0` is 10, `l = 1` is 40.
        let values: Vec<f64> = (0..8).map(|k| if k % 2 == 0 { 10.0 } else { 40.0 }).collect();
        let log = marching_tetrahedra(dims, &|i| unit_slice(dims, i), &values, 20.0, ValueSpacing::Log10)
            .expect("a crossing");
        assert!(!log.points.is_empty());
        for p in &log.points {
            assert!((p[2] - 0.5).abs() < 1e-12, "{p:?}");
        }
        let lin = marching_tetrahedra(
            dims,
            &|i| unit_slice(dims, i),
            &values,
            20.0,
            ValueSpacing::Linear,
        )
        .expect("a crossing");
        for p in &lin.points {
            assert!((p[2] - 1.0 / 3.0).abs() < 1e-12, "{p:?}");
        }
    }

    #[test]
    fn no_crossing_is_said_not_drawn() {
        let dims = [2, 2, 2];
        let values: Vec<f64> = (1..=8).map(f64::from).collect();
        let high = marching_tetrahedra(
            dims,
            &|i| unit_slice(dims, i),
            &values,
            20.0,
            ValueSpacing::Linear,
        )
        .expect("a field");
        assert!(high.triangles.is_empty());
        assert_eq!(high.crossing, Crossing::AllBelow { max: 8.0 });
        let low = marching_tetrahedra(dims, &|i| unit_slice(dims, i), &values, 0.5, ValueSpacing::Linear)
            .expect("a field");
        assert!(low.triangles.is_empty());
        assert_eq!(low.crossing, Crossing::AllAbove { min: 1.0 });
    }

    /// Two cells in `i`, a clean crossing in `l` everywhere (1 then 100), and
    /// the value of node `(2, 0, 0)` replaced: that node belongs to the cell
    /// `i = 1` only.
    fn two_cells(replacement: f64) -> Vec<f64> {
        let mut values: Vec<f64> = (0..12).map(|k| if k % 2 == 0 { 1.0 } else { 100.0 }).collect();
        values[(2 * 2) * 2] = replacement;
        values
    }

    #[test]
    fn a_nan_corner_sets_the_cell_aside() {
        let dims = [3, 2, 2];
        let s = marching_tetrahedra(
            dims,
            &|i| unit_slice(dims, i),
            &two_cells(f64::NAN),
            10.0,
            ValueSpacing::Linear,
        )
        .expect("a field");
        assert_eq!(s.set_aside, 1);
        assert!(!s.cell_of.is_empty());
        assert!(s.cell_of.iter().all(|c| c[0] != 1), "{:?}", s.cell_of);
    }

    #[test]
    fn a_value_at_or_below_zero_is_set_aside_under_log10() {
        let dims = [3, 2, 2];
        let values = two_cells(0.0);
        let log = marching_tetrahedra(dims, &|i| unit_slice(dims, i), &values, 10.0, ValueSpacing::Log10)
            .expect("a field");
        assert_eq!(log.set_aside, 1);
        let lin = marching_tetrahedra(
            dims,
            &|i| unit_slice(dims, i),
            &values,
            10.0,
            ValueSpacing::Linear,
        )
        .expect("a field");
        assert_eq!(lin.set_aside, 0);
    }

    /// How many triangles each undirected edge of the mesh belongs to.
    fn edge_use(s: &IsoSurface) -> HashMap<(u32, u32), usize> {
        let mut uses: HashMap<(u32, u32), usize> = HashMap::new();
        for t in &s.triangles {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                *uses.entry((a.min(b), a.max(b))).or_insert(0) += 1;
            }
        }
        uses
    }

    #[test]
    fn the_tetrahedra_leave_no_crack() {
        let s = sphere();
        for (edge, count) in edge_use(&s) {
            assert_eq!(count, 2, "edge {edge:?}");
        }
    }

    #[test]
    fn the_surface_of_a_sphere_is_a_sphere() {
        let s = sphere();
        let (v, e, f) = (
            s.points.len() as i64,
            edge_use(&s).len() as i64,
            s.triangles.len() as i64,
        );
        assert_eq!(v - e + f, 2, "V={v} E={e} F={f}");
    }

    #[test]
    fn the_normals_point_to_higher_values() {
        let s = sphere();
        for t in &s.triangles {
            let [p0, p1, p2] = [
                s.points[t[0] as usize],
                s.points[t[1] as usize],
                s.points[t[2] as usize],
            ];
            let n = cross(sub(p1, p0), sub(p2, p0));
            let centroid = [
                (p0[0] + p1[0] + p2[0]) / 3.0,
                (p0[1] + p1[1] + p2[1]) / 3.0,
                (p0[2] + p1[2] + p2[2]) / 3.0,
            ];
            assert!(dot(n, centroid) > 0.0, "{t:?}");
        }
    }

    #[test]
    fn the_same_lattice_gives_the_same_surface() {
        assert_eq!(sphere(), sphere());
    }

    #[test]
    fn a_level_through_a_node_only_is_grazed() {
        let dims = [2, 2, 2];
        let mut values = vec![0.0; 8];
        values[7] = 1.0;
        let s = marching_tetrahedra(dims, &|i| unit_slice(dims, i), &values, 1.0, ValueSpacing::Linear)
            .expect("a field");
        assert!(s.triangles.is_empty());
        assert_eq!(s.crossing, Crossing::Grazed);
    }

    #[test]
    fn a_crossing_hidden_in_set_aside_cells_is_said() {
        let dims = [6, 2, 2];
        let mut values = Vec::new();
        for i in 0..6 {
            let v = match i {
                0 | 1 => 1.0,
                2 => f64::NAN,
                _ => 30.0,
            };
            values.extend([v; 4]);
        }
        let s = marching_tetrahedra(
            dims,
            &|i| unit_slice(dims, i),
            &values,
            20.0,
            ValueSpacing::Linear,
        )
        .expect("a field");
        assert_eq!(s.crossing, Crossing::InSetAsideCells);
        assert_eq!(s.set_aside, 2);
    }

    #[test]
    fn refusals_are_named() {
        let ok = |dims: [usize; 3]| unit_slice(dims, 0);
        let eight = vec![1.0; 8];
        let call = |dims: [usize; 3], values: &[f64], level: f64, spacing: ValueSpacing| {
            marching_tetrahedra(dims, &|i| unit_slice(dims, i), values, level, spacing)
        };
        assert!(matches!(
            call([1, 2, 2], &eight, 1.0, ValueSpacing::Linear),
            Err(Error::NotAVolume { .. })
        ));
        assert!(matches!(
            call([2, 2, 2], &eight[..7], 1.0, ValueSpacing::Linear),
            Err(Error::LatticeValues { values: 7, nodes: 8 })
        ));
        assert!(matches!(
            call([2, 2, 2], &eight, f64::NAN, ValueSpacing::Linear),
            Err(Error::BadLevel { .. })
        ));
        assert!(matches!(
            call([2, 2, 2], &eight, 0.0, ValueSpacing::Log10),
            Err(Error::LevelNotPositive { .. })
        ));
        // A slice of three points where four are expected.
        let short = marching_tetrahedra(
            [2, 2, 2],
            &|i| {
                let mut s = ok([2, 2, 2]);
                if i == 1 {
                    s.pop();
                }
                s
            },
            &eight,
            1.0,
            ValueSpacing::Linear,
        );
        assert!(matches!(
            short,
            Err(Error::SliceLength {
                slice: 1,
                given: 3,
                expected: 4
            })
        ));
    }

    #[test]
    fn each_slice_is_asked_once() {
        let dims = [4, 2, 2];
        let asked = Cell::new(0usize);
        let values: Vec<f64> = (0..16).map(|k| if k % 2 == 0 { 1.0 } else { 100.0 }).collect();
        marching_tetrahedra(
            dims,
            &|i| {
                asked.set(asked.get() + 1);
                unit_slice(dims, i)
            },
            &values,
            10.0,
            ValueSpacing::Linear,
        )
        .expect("a field");
        assert_eq!(asked.get(), 4);
    }
}
