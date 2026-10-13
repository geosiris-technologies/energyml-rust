//! Sweeping a circle along a polyline: a well drawn as a solid rather than a
//! hair.
//!
//! A line has no thickness, so a wellbore a kilometre long is one pixel wide at
//! any distance that shows the model. Giving it a radius is the difference
//! between a scene somebody can read and a scribble - and it is a *rendering*
//! choice, which is why the radius and the number of sides are parameters and
//! not constants.
//!
//! # Why this is written here rather than taken from a crate
//!
//! Surveyed before writing: `spade` (Delaunay, and constrained Delaunay) and
//! `poisson_reconstruction` are the right answers for building a **surface from
//! scattered points**, and both are pure Rust and wasm-clean - they are what to
//! reach for when gridding arrives. But a swept circle is sixty lines of
//! arithmetic, and the crates that offer one arrive attached to a renderer or a
//! CAD kernel. More to the point, the geometry has to agree with what the
//! browser draws, and agreeing with somebody else's sweep is not something a
//! dependency can promise.
//!
//! # The path is the data
//!
//! This sweeps **the polyline as it stands**. It does not smooth it, and that
//! is deliberate: by the time points reach here they are already the evaluated
//! curve - `parametric.rs` evaluates a minimum-curvature well path rather than
//! straightening it, precisely so that nothing downstream has to guess. Laying a
//! second interpolation over that would draw a path through positions the file
//! does not state, which is the mistake the parametric reader exists to avoid.
//!
//! # Frames, and the twist that would otherwise appear
//!
//! A circle swept along a curve needs a frame at each station, and the naive
//! choice - a fixed "up" crossed with the tangent - spins the tube where the
//! path turns through vertical, which is every deviated well. So the frame is
//! **transported**: the first is built once, and each following one is the
//! previous rotated by the smallest rotation taking one tangent to the next.
//! That is the rotation-minimising frame, and it is why a tube of a hundred
//! stations has no visible seam.

use crate::error::{Error, Result};
use crate::points::Point;

/// How thick, and how many sides.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Tube {
    /// Radius, in the units the points are in.
    ///
    /// A radius and not a diameter, because that is what every sweep in every
    /// renderer takes, and converting silently at one end of a pair of
    /// interfaces is how a well comes out twice too fat.
    pub radius: f64,
    /// How many faces go round - the arity of the cross-section.
    ///
    /// Three is a triangular prism and reads as a mistake; past about sixteen
    /// nothing on screen changes and every vertex is paid for. Eight is what the
    /// browser uses and what this defaults to.
    pub sides: usize,
}

impl Default for Tube {
    fn default() -> Self {
        Tube {
            radius: 1.0,
            sides: 8,
        }
    }
}

impl Tube {
    /// Refuse what cannot be swept, **by the number that is wrong**.
    ///
    /// A radius of zero is a line with vertices, and fewer than three sides is
    /// not a cross-section. Both are the kind of value that arrives from a
    /// slider at its end stop, and both would otherwise produce a mesh that
    /// renders as nothing at all.
    pub fn checked(radius: f64, sides: usize) -> Result<Tube> {
        if !(radius.is_finite() && radius > 0.0) {
            return Err(Error::malformed(
                "tube",
                format!("a radius of {radius} sweeps nothing"),
            ));
        }
        if sides < 3 {
            return Err(Error::malformed(
                "tube",
                format!("{sides} side(s) is not a cross-section; three is the fewest that is"),
            ));
        }
        Ok(Tube { radius, sides })
    }
}

fn subtract(a: Point, b: Point) -> Point {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: Point, b: Point) -> Point {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: Point, k: f64) -> Point {
    [a[0] * k, a[1] * k, a[2] * k]
}

fn dot(a: Point, b: Point) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: Point, b: Point) -> Point {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn length(a: Point) -> f64 {
    dot(a, a).sqrt()
}

fn normalise(a: Point) -> Option<Point> {
    let n = length(a);
    (n.is_finite() && n > f64::EPSILON).then(|| scale(a, 1.0 / n))
}

/// A vector perpendicular to `t`, chosen so it is never nearly parallel to it.
///
/// Crossing with the world's smallest component of `t` is what makes this safe:
/// crossing with a fixed axis collapses to zero whenever the tangent happens to
/// point along it, which for a vertical well is *always*.
fn any_perpendicular(t: Point) -> Point {
    let axis = if t[0].abs() <= t[1].abs() && t[0].abs() <= t[2].abs() {
        [1.0, 0.0, 0.0]
    } else if t[1].abs() <= t[2].abs() {
        [0.0, 1.0, 0.0]
    } else {
        [0.0, 0.0, 1.0]
    };
    normalise(cross(t, axis)).unwrap_or([1.0, 0.0, 0.0])
}

/// Rotate `v` by the smallest rotation taking `from` to `to`.
///
/// Rodrigues, with the two degenerate cases handled: parallel tangents rotate
/// by nothing, and opposite ones - a path that doubles back on itself - have no
/// smallest rotation at all, so the frame is rebuilt rather than spun through
/// an arbitrary half turn.
fn transport(v: Point, from: Point, to: Point) -> Point {
    let axis = cross(from, to);
    let sin = length(axis);
    let cos = dot(from, to);
    if sin <= 1e-12 {
        return match cos > 0.0 {
            true => v,
            false => any_perpendicular(to),
        };
    }
    let axis = scale(axis, 1.0 / sin);
    let angle = sin.atan2(cos);
    let (s, c) = angle.sin_cos();
    add(
        add(scale(v, c), scale(cross(axis, v), s)),
        scale(axis, dot(axis, v) * (1.0 - c)),
    )
}

/// The stations a run really has: consecutive duplicates dropped.
///
/// A repeated position has no direction, and a zero-length segment makes every
/// frame after it undefined. Dropping them is not tidying - it is the
/// difference between a tube and a `NaN`.
fn distinct(points: &[Point]) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::with_capacity(points.len());
    for point in points {
        if !point.iter().all(|v| v.is_finite()) {
            continue;
        }
        match out.last() {
            Some(last) if length(subtract(*point, *last)) <= f64::EPSILON => {}
            _ => out.push(*point),
        }
    }
    out
}

/// One run swept into a tube: its vertices, its triangles, and which station
/// each vertex came from.
#[derive(Clone, PartialEq, Debug)]
pub struct Swept {
    /// `sides` vertices per station, station by station.
    pub points: Vec<Point>,
    /// Triples into `points`.
    pub triangles: Vec<[u32; 3]>,
    /// For each vertex, the index of the station it rings - **in the run that
    /// was given**.
    ///
    /// What makes a coloured tube possible: a property has one value per
    /// station, and every vertex of a ring takes its station's value. Without
    /// it the tube would have to be drawn in one flat colour, or the values
    /// would be lined up against vertices they were never measured on.
    pub station_of: Vec<u32>,
}

/// Sweep a circle along one run of points.
///
/// Fewer than two distinct stations is not a path: refused by name rather than
/// returned empty, because an empty mesh and a mesh nobody could build look
/// identical to whatever draws them next.
pub fn sweep(points: &[Point], tube: &Tube) -> Result<Swept> {
    let stations = distinct(points);
    if stations.len() < 2 {
        return Err(Error::malformed(
            "tube",
            format!(
                "{} distinct station(s) in a run of {}: a tube needs a direction",
                stations.len(),
                points.len()
            ),
        ));
    }
    let tube = Tube::checked(tube.radius, tube.sides)?;

    // The tangent at each station: the segment ahead, except at the last, and
    // averaged in between so a corner does not pinch the tube on its inside.
    let mut tangents: Vec<Point> = Vec::with_capacity(stations.len());
    for index in 0..stations.len() {
        let ahead = (index + 1 < stations.len())
            .then(|| normalise(subtract(stations[index + 1], stations[index])))
            .flatten();
        let behind = (index > 0)
            .then(|| normalise(subtract(stations[index], stations[index - 1])))
            .flatten();
        let tangent = match (behind, ahead) {
            (Some(b), Some(a)) => normalise(add(b, a)).unwrap_or(a),
            (None, Some(a)) => a,
            (Some(b), None) => b,
            (None, None) => [0.0, 0.0, 1.0],
        };
        tangents.push(tangent);
    }

    let mut normal = any_perpendicular(tangents[0]);
    let mut points_out = Vec::with_capacity(stations.len() * tube.sides);
    let mut station_of = Vec::with_capacity(stations.len() * tube.sides);

    for (index, centre) in stations.iter().enumerate() {
        if index > 0 {
            normal = transport(normal, tangents[index - 1], tangents[index]);
            // Re-orthogonalised at every station: the transported normal drifts
            // off the plane by a rounding error each time, and a hundred
            // stations of drift is a visibly elliptical tube.
            normal = normalise(subtract(
                normal,
                scale(tangents[index], dot(normal, tangents[index])),
            ))
            .unwrap_or_else(|| any_perpendicular(tangents[index]));
        }
        let binormal = cross(tangents[index], normal);
        for side in 0..tube.sides {
            let angle = std::f64::consts::TAU * side as f64 / tube.sides as f64;
            let (sin, cos) = angle.sin_cos();
            let offset = add(
                scale(normal, cos * tube.radius),
                scale(binormal, sin * tube.radius),
            );
            points_out.push(add(*centre, offset));
            station_of.push(index as u32);
        }
    }

    let sides = tube.sides as u32;
    let mut triangles = Vec::with_capacity((stations.len() - 1) * tube.sides * 2);
    for segment in 0..stations.len() as u32 - 1 {
        let here = segment * sides;
        let next = here + sides;
        for side in 0..sides {
            let after = (side + 1) % sides;
            // Wound so the right-hand normal points **outward**, which is what
            // a renderer reads as front-facing and what a back-face cull keeps.
            // The order is the one the test measures, not the one that reads
            // naturally: going round the ring rotates from the normal toward
            // the binormal, and pairing that with the tangent the obvious way
            // gives a tube that is inside out - a hole, drawn confidently.
            triangles.push([here + side, next + after, next + side]);
            triangles.push([here + side, here + after, next + after]);
        }
    }

    Ok(Swept {
        points: points_out,
        triangles,
        station_of,
    })
}

/// A whole object's lines, swept - geometry and attributes together.
///
/// The attributes are the reason this is not just `sweep` in a loop. A property
/// has one value per **station**, and the tube has `sides` vertices per station:
/// each ring takes its station's value, so a coloured well stays coloured. A
/// per-cell property has one value per *run*, and every vertex of that run takes
/// it. Dropping them instead - which is what asking for a shape usually does -
/// would turn every tube into one flat colour without saying so.
///
/// The lines are replaced, not added to: a tube **is** the rendering of those
/// lines, and keeping both would draw the well twice.
pub fn sweep_object(object: &crate::scene::SceneObject, tube: &Tube) -> Result<crate::scene::SceneObject> {
    use crate::geometry::{Geometry, Patch, Topology};
    use crate::scene::{Attribute, AttributeScope};

    let mut patches: Vec<Patch> = Vec::new();
    let mut node_offset = 0usize;
    // For every vertex of the result, which node and which cell of the original
    // it came from - the two things an attribute is indexed by.
    let mut from_node: Vec<usize> = Vec::new();
    let mut from_cell: Vec<usize> = Vec::new();
    let mut cells_before = 0usize;
    let mut nodes_before = 0usize;

    for patch in &object.geometry.patches {
        let runs: Vec<Vec<u32>> = match &patch.topology {
            Topology::Polylines(lengths) => {
                let mut runs = Vec::with_capacity(lengths.len());
                let mut start = 0u32;
                for length in lengths {
                    runs.push((start..start + length).collect());
                    start += length;
                }
                runs
            }
            // Only lines are swept. A surface asked for a tube is a request
            // that means nothing, and answering it with the surface unchanged
            // would be the picture disagreeing with what was asked for.
            _ => {
                cells_before += patch_cell_count(&patch.topology);
                nodes_before += patch.points.len();
                continue;
            }
        };

        let mut points = Vec::new();
        let mut triangles = Vec::new();
        for (run_index, run) in runs.iter().enumerate() {
            let along: Vec<Point> = run.iter().map(|n| patch.points[*n as usize]).collect();
            let Ok(swept) = sweep(&along, tube) else {
                // A run with no direction is skipped, not fatal: a polyline set
                // of four hundred lines must not be lost to one degenerate one.
                continue;
            };
            let base = points.len() as u32;
            for (vertex, station) in swept.points.iter().zip(&swept.station_of) {
                points.push(*vertex);
                // `station_of` counts the *distinct* stations of the run, and a
                // run whose duplicates were dropped is shorter than the run the
                // property is indexed by. Clamped rather than trusted: an index
                // past the end would silently take somebody else's value.
                let node = run
                    .get(*station as usize)
                    .copied()
                    .unwrap_or(*run.last().unwrap_or(&0));
                from_node.push(nodes_before + node as usize);
                from_cell.push(cells_before + run_index);
            }
            triangles.extend(
                swept
                    .triangles
                    .iter()
                    .map(|t| [t[0] + base, t[1] + base, t[2] + base]),
            );
        }

        if !points.is_empty() {
            let count = points.len();
            patches.push(Patch {
                index: patch.index,
                node_offset,
                points,
                topology: Topology::Triangles(triangles),
                closed: Vec::new(),
            });
            node_offset += count;
        }
        cells_before += runs.len();
        nodes_before += patch.points.len();
    }

    if patches.is_empty() {
        return Err(Error::unsupported(
            "sweeping into a tube",
            format!("`{}` has no polylines to sweep", object.title),
        ));
    }

    let attributes = object
        .attributes
        .iter()
        .map(|attribute| {
            let components = attribute.components.max(1) as usize;
            let source = match attribute.scope {
                AttributeScope::Points => &from_node,
                AttributeScope::Cells => &from_cell,
            };
            let mut values = Vec::with_capacity(source.len() * components);
            for element in source {
                for component in 0..components {
                    values.push(
                        attribute
                            .values
                            .get(element * components + component)
                            .copied()
                            .unwrap_or(f64::NAN),
                    );
                }
            }
            Attribute {
                // Every vertex of the tube now carries a value, whichever
                // elements the property hung on before.
                scope: AttributeScope::Points,
                values,
                ..attribute.clone()
            }
        })
        .collect();

    Ok(crate::scene::SceneObject {
        geometry: Geometry {
            kind: object.geometry.kind,
            patches,
        },
        attributes,
        ..object.clone()
    })
}

/// [`Topology::cell_count`] - the one definition, since the drawable's cell
/// table joined the two other callers; the wrapper stays for this module's
/// call sites.
fn patch_cell_count(topology: &crate::geometry::Topology) -> usize {
    topology.cell_count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn straight(count: usize) -> Vec<Point> {
        (0..count).map(|i| [0.0, 0.0, -(i as f64)]).collect()
    }

    #[test]
    fn a_straight_run_gives_a_ring_at_every_station() {
        let swept = sweep(
            &straight(4),
            &Tube {
                radius: 2.0,
                sides: 8,
            },
        )
        .unwrap();
        assert_eq!(swept.points.len(), 4 * 8);
        // Two triangles a side, a segment at a time.
        assert_eq!(swept.triangles.len(), 3 * 8 * 2);
        assert_eq!(swept.station_of.len(), swept.points.len());
        assert_eq!(swept.station_of[0], 0);
        assert_eq!(*swept.station_of.last().unwrap(), 3);
    }

    #[test]
    fn every_vertex_sits_at_the_radius_that_was_asked_for() {
        // The one thing a person sets and can see. A radius that came out
        // scaled by the path's length, or by a frame that was not normalised,
        // is a well that is fat where it is straight.
        let swept = sweep(
            &straight(5),
            &Tube {
                radius: 3.5,
                sides: 12,
            },
        )
        .unwrap();
        for (vertex, station) in swept.points.iter().zip(&swept.station_of) {
            let centre = [0.0, 0.0, -(*station as f64)];
            let offset = subtract(*vertex, centre);
            assert!(
                (length(offset) - 3.5).abs() < 1e-9,
                "vertex at {} from the axis, radius asked was 3.5",
                length(offset)
            );
        }
    }

    #[test]
    fn the_arity_is_the_number_of_faces_round() {
        for sides in [3usize, 6, 8, 16] {
            let swept = sweep(&straight(2), &Tube { radius: 1.0, sides }).unwrap();
            assert_eq!(swept.points.len(), 2 * sides);
            assert_eq!(swept.triangles.len(), sides * 2);
        }
    }

    #[test]
    fn a_radius_or_an_arity_that_draws_nothing_is_refused_by_its_number() {
        // Both arrive from a slider at its end stop, and both would otherwise
        // produce a mesh that renders as nothing at all.
        for radius in [0.0, -1.0, f64::NAN] {
            let refused = Tube::checked(radius, 8).expect_err("a radius of {radius}");
            assert!(refused.to_string().contains("radius"), "{refused}");
        }
        let refused = Tube::checked(1.0, 2).expect_err("two sides");
        assert!(refused.to_string().contains('2'), "{refused}");
        assert!(Tube::checked(1.0, 3).is_ok(), "three sides is a cross-section");
    }

    #[test]
    fn a_vertical_path_does_not_collapse_the_frame() {
        // **The case that breaks the naive answer.** Crossing the tangent with
        // a fixed up-vector gives the zero vector when the path points along
        // it, and every wellbore in this corpus is vertical. The result would
        // be a tube of `NaN`, which draws as nothing.
        let swept = sweep(&straight(3), &Tube::default()).unwrap();
        assert!(
            swept.points.iter().all(|p| p.iter().all(|v| v.is_finite())),
            "a vertical path produced a non-finite vertex"
        );
    }

    #[test]
    fn a_turning_path_does_not_twist() {
        // A quarter turn: the frame is transported, so consecutive rings stay
        // aligned. Measured as the angle between the same side's offset from
        // one ring to the next - a spinning frame shows up as a jump.
        let path: Vec<Point> = (0..=16)
            .map(|i| {
                let a = std::f64::consts::FRAC_PI_2 * i as f64 / 16.0;
                [a.cos() * 10.0, a.sin() * 10.0, 0.0]
            })
            .collect();
        let swept = sweep(
            &path,
            &Tube {
                radius: 1.0,
                sides: 8,
            },
        )
        .unwrap();

        let mut worst: f64 = 0.0;
        for station in 1..path.len() {
            let previous = subtract(swept.points[(station - 1) * 8], path[station - 1]);
            let current = subtract(swept.points[station * 8], path[station]);
            let (a, b) = (normalise(previous).unwrap(), normalise(current).unwrap());
            worst = worst.max(dot(a, b).clamp(-1.0, 1.0).acos().to_degrees());
        }
        assert!(worst < 8.0, "the frame turned {worst} degrees between stations");
    }

    #[test]
    fn the_faces_look_outward() {
        // A hexahedron built the other way round renders as a hole, and so does
        // a tube. Measured against the axis, not assumed from the winding.
        let swept = sweep(
            &straight(3),
            &Tube {
                radius: 2.0,
                sides: 8,
            },
        )
        .unwrap();
        for triangle in &swept.triangles {
            let [a, b, c] = triangle.map(|i| swept.points[i as usize]);
            let normal = cross(subtract(b, a), subtract(c, a));
            // The centre of the segment this face belongs to, on the axis.
            let station = swept.station_of[triangle[0] as usize] as f64;
            let axis = [0.0, 0.0, -station];
            let outward = subtract(a, axis);
            assert!(dot(normal, outward) > 0.0, "a face points inward: {triangle:?}");
        }
    }

    #[test]
    fn a_repeated_station_does_not_poison_the_frame() {
        // A zero-length segment has no direction, so every frame after it would
        // be `NaN`. Dropped rather than tidied away: this is correctness.
        let path = vec![[0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, -1.0]];
        let swept = sweep(&path, &Tube::default()).unwrap();
        assert_eq!(swept.points.len(), 2 * 8, "the duplicate is one station, not two");
        assert!(swept.points.iter().all(|p| p.iter().all(|v| v.is_finite())));
    }

    #[test]
    fn a_run_with_no_direction_is_refused_rather_than_returned_empty() {
        // An empty mesh and a mesh nobody could build look identical to
        // whatever draws them next.
        let refused = sweep(&[[0.0, 0.0, 0.0]], &Tube::default()).expect_err("one station");
        assert!(refused.to_string().contains("direction"), "{refused}");
        let refused = sweep(&[], &Tube::default()).expect_err("no station");
        assert!(refused.to_string().contains('0'), "{refused}");
    }
}
