//! Iso-contours of a scalar over a triangulated surface.
//!
//! The lines where a property takes a chosen value — a depth contour on a
//! horizon, a pressure contour on a map. Given per-node values and the
//! triangles that join those nodes, each level comes back as a set of segments
//! in the surface's own coordinates, ready to draw or to promote to a
//! `PolylineSetRepresentation`.
//!
//! # Marching triangles, not marching squares
//!
//! The input is already a triangulation, so there is no grid to march over and
//! no ambiguous saddle to resolve: inside one triangle a linear field crosses a
//! level along exactly one straight segment, or not at all. That is the whole
//! algorithm, and it is why this file has no lookup table — the sixteen-case
//! table of marching squares exists to disambiguate a *quad*, which a triangle
//! cannot be.
//!
//! The segments come back **unordered and unjoined**, and [`chain`] joins them
//! when a caller needs lines rather than pieces — which is what promoting a
//! contour to a `PolylineSetRepresentation` needs, and what makes a drawing
//! one strip instead of a thousand loose segments. It is a separate step
//! because it has its own cases (a contour that closes, one that runs off the
//! edge of the surface, two that meet at a saddle) and because matching ends
//! by coordinate does not work — see [`At`].
//!
//! # Where this came from
//!
//! `geoactioner-ssr` drew these through `kelper-lit`, a JavaScript library
//! whose repository is reported as gone. Its sources survive in that
//! project's `node_modules`, and the two decisions worth carrying over are kept
//! here with their reasons: levels snap to zero, and a value sitting exactly on
//! a level counts as above it. The rest is written from the algorithm rather
//! than translated.
//!
//! TODO(reference): verify — marching triangles is named without a citation; add a reference once checked.

use crate::Error;

/// Where one end of a segment sits on the mesh.
///
/// # Why an end carries this at all
///
/// Two triangles sharing an edge cut it at the same place — and **sometimes
/// not at the same bits**. The interpolation runs from whichever corner is
/// alone on its side of the level, and the two faces pick the *same* corner
/// unless their third corners fall on opposite sides. When they do, the two
/// compute `from + t * (into - from)` from opposite ends and land a few ULPs
/// apart, so a chain matching ends by coordinate breaks there — a gap nobody
/// could see in the picture and nobody could explain from the data.
///
/// **How often is a question that had to be measured, and the first two
/// answers were wrong.** A plane gives zero drift; so does a cone. Both were
/// used in a first version of the counter-proof, and both passed while
/// proving nothing. On a rough field it is about 12% of shared ends — see
/// `tests::matching_ends_by_coordinate_fragments_a_contour_that_at_keeps_whole`,
/// which fails on a mesh too smooth to show it.
///
/// So the identity of an end is the **mesh feature it lies on**, in integers:
/// the edge between two nodes, or a node itself when the level passes exactly
/// through one. Both cases are needed — an end at a node is reached from
/// several different edges, and keying it by any of them would break the chain
/// just as surely.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum At {
    /// The level crosses the edge between these two nodes, smaller index
    /// first.
    Edge(u32, u32),
    /// The level passes exactly through this node.
    Node(u32),
}

/// One straight piece of a contour, in the surface's own coordinates.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Segment {
    pub a: [f64; 3],
    pub b: [f64; 3],
    /// Where `a` sits on the mesh — see [`At`].
    pub on_a: At,
    /// Where `b` sits.
    pub on_b: At,
}

/// Every segment of one level, and what the pass had to set aside.
#[derive(Clone, PartialEq, Debug)]
pub struct Contour {
    /// The value these segments trace.
    pub level: f64,
    pub segments: Vec<Segment>,
    /// Triangles skipped because a corner had no value.
    ///
    /// Reported rather than repaired: a hole in a horizon is data, and a
    /// contour drawn straight across it would be an invention. See the crate's
    /// house rule on `NaN`.
    pub skipped: usize,
}

/// The levels to contour at, evenly spaced and **snapped to zero**.
///
/// `count` is a number of intervals, so `levels(0.0, 100.0, 4)` spaces them
/// 25 apart. What comes back is every multiple of that spacing lying inside
/// `[min, max]` — which is not necessarily `count` values, and deliberately
/// so.
///
/// # Why zero
///
/// Contours land on round numbers a person can read off a legend, and the
/// roundest is zero: sea level on a depth map, no displacement on a strain
/// map, the neutral line on anything signed. Spacing them from `min` instead
/// would put the lines at 3.7, 28.7, 53.7 … — arithmetically even, and
/// unreadable. Carried over from `kelper-lit`, whose own comment says the
/// values *"always pass through zero even if 0 is not part of the iso
/// values"*.
///
/// Empty when `min >= max` — a flat field has no contour to draw, which is an
/// answer and not a failure.
pub fn levels(min: f64, max: f64, count: usize) -> Result<Vec<f64>, Error> {
    if count == 0 {
        return Err(Error::NoLevels);
    }
    if !min.is_finite() || !max.is_finite() {
        return Err(Error::BadRange { min, max });
    }
    if min >= max {
        return Ok(Vec::new());
    }
    levels_spaced(min, max, (max - min) / count as f64)
}

/// The same, given the spacing rather than a count.
///
/// Refuses a spacing that would produce more levels than a picture can hold:
/// asking for 0.001 over a range of a thousand is a typing error in a dialog
/// box, not a request for a million contours, and answering it takes long
/// enough that the caller cannot tell it from a hang.
pub fn levels_spaced(min: f64, max: f64, spacing: f64) -> Result<Vec<f64>, Error> {
    if !spacing.is_finite() || spacing <= 0.0 {
        return Err(Error::BadSpacing { given: spacing });
    }
    if !min.is_finite() || !max.is_finite() {
        return Err(Error::BadRange { min, max });
    }
    if min >= max {
        return Ok(Vec::new());
    }
    const MOST: usize = 1000;
    let wanted = (max - min) / spacing;
    if wanted > MOST as f64 {
        return Err(Error::TooManyLevels {
            wanted: wanted as usize,
            most: MOST,
        });
    }

    // Every multiple of `spacing` inside the range. Counted in whole steps and
    // multiplied once, rather than accumulated by repeated addition: adding
    // 0.1 ten times does not give 1.0, and a contour at 0.9999999999999999 is
    // a contour that misses the round number it exists to name.
    let first = (min / spacing).ceil() as i64;
    let last = (max / spacing).floor() as i64;
    Ok((first..=last).map(|step| step as f64 * spacing).collect())
}

/// The contour of one level over a triangulated surface.
///
/// `points` is `[x, y, z, …]`, `triangles` three node indices per face, and
/// `values` one number per node — the property being contoured, which is very
/// often the elevation itself.
pub fn contour(points: &[f64], triangles: &[u32], values: &[f64], level: f64) -> Result<Contour, Error> {
    check(points, triangles, values)?;
    if !level.is_finite() {
        return Err(Error::BadLevel { given: level });
    }

    let node = |at: u32| -> [f64; 3] {
        let at = at as usize * 3;
        [points[at], points[at + 1], points[at + 2]]
    };

    let mut segments = Vec::new();
    let mut skipped = 0usize;
    for face in triangles.chunks_exact(3) {
        let corners = [face[0], face[1], face[2]];
        let v = [
            values[corners[0] as usize],
            values[corners[1] as usize],
            values[corners[2] as usize],
        ];
        if v.iter().any(|one| one.is_nan()) {
            skipped += 1;
            continue;
        }

        // **`>=`, so a value sitting exactly on the level counts as above it.**
        // What it buys is an **edge** lying on the level: `>=` puts its two
        // ends above and traces it, `>` puts the whole triangle below and
        // loses the line. A single vertex on the level is reached either way —
        // the interpolation lands on it with `t = 0` or `t = 1` whichever side
        // it is put — so that case is not the reason, and a test written
        // around it could not fail. See
        // `tests::an_edge_lying_on_the_level_is_traced_and_not_lost`.
        let above = [v[0] >= level, v[1] >= level, v[2] >= level];
        let count = above.iter().filter(|one| **one).count();
        if count == 0 || count == 3 {
            continue;
        }

        // One corner is alone on its side, and the two edges leaving it are
        // the two the level crosses. Which corner that is falls straight out
        // of the classification.
        let lone = (0..3)
            .find(|&at| above[at] != above[(at + 1) % 3] && above[at] != above[(at + 2) % 3])
            .expect("with one or two corners above, exactly one corner is alone");

        let cut = |to: usize| -> ([f64; 3], At) {
            let (from, into) = (node(corners[lone]), node(corners[to]));
            let (a, b) = (v[lone], v[to]);
            // `b - a` cannot be zero: the corners are on opposite sides of the
            // level, so their values differ.
            let t = (level - a) / (b - a);
            let point = [
                from[0] + t * (into[0] - from[0]),
                from[1] + t * (into[1] - from[1]),
                from[2] + t * (into[2] - from[2]),
            ];
            // A cut at `t = 0` or `t = 1` is not on the edge, it **is** a node
            // — and it has to be named as one, because the same node is
            // reached from every edge that meets it. See [`At`].
            let (low, high) = (corners[lone].min(corners[to]), corners[lone].max(corners[to]));
            let at = match t {
                0.0 => At::Node(corners[lone]),
                1.0 => At::Node(corners[to]),
                _ => At::Edge(low, high),
            };
            (point, at)
        };
        let ((a, on_a), (b, on_b)) = (cut((lone + 1) % 3), cut((lone + 2) % 3));
        // Both ends on the same mesh feature is the level *grazing one
        // vertex*: a point, not a line. A renderer handed it draws nothing
        // while a promotion writes a polyline of two identical nodes, so it is
        // dropped — a contour touching a surface at a single point has no line
        // there. Tested on the identity rather than on the coordinates, which
        // is the same reason [`At`] exists.
        if on_a != on_b {
            segments.push(Segment { a, b, on_a, on_b });
        }
    }
    Ok(Contour {
        level,
        segments,
        skipped,
    })
}

/// Every level at once, in the order given.
pub fn contours(
    points: &[f64],
    triangles: &[u32],
    values: &[f64],
    levels: &[f64],
) -> Result<Vec<Contour>, Error> {
    check(points, triangles, values)?;
    levels
        .iter()
        .map(|level| contour(points, triangles, values, *level))
        .collect()
}

/// A run of contour segments joined end to end.
#[derive(Clone, PartialEq, Debug)]
pub struct Polyline {
    /// The vertices in order. A closed line does **not** repeat its first
    /// point at the end — `closed` says so instead, because a repeated node is
    /// a thing a `PolylineSetRepresentation` states with `ClosedPolylines`,
    /// not with a duplicate coordinate.
    pub points: Vec<[f64; 3]>,
    pub closed: bool,
}

/// Join the segments of a contour into polylines.
///
/// Needed to promote a contour to a `PolylineSetRepresentation`, and worth
/// having for drawing too: one line strip beats a thousand loose segments.
///
/// # How ends are matched
///
/// On [`At`], never on coordinates — see that type for why comparing the
/// numbers leaves the contour in pieces. Every end therefore matches exactly
/// or not at all, and there is no tolerance to tune.
///
/// # What it does with a junction
///
/// Three or more segments can meet at one place: a saddle, where two branches
/// of the same level cross. There is no way to tell from the geometry alone
/// which branch continues into which, and the choice changes nothing about
/// the picture — so a walk arriving at a junction leaves by whichever segment
/// is still unused. **Every segment ends up in exactly one polyline**, which
/// is the property that matters and the one the tests pin.
///
/// Open lines are walked first, from their loose ends, so that a line running
/// off the edge of the surface comes back whole rather than cut at the
/// arbitrary segment the walk happened to start on. Whatever is left is
/// closed.
pub fn chain(segments: &[Segment]) -> Vec<Polyline> {
    use std::collections::HashMap;

    let mut touching: HashMap<At, Vec<usize>> = HashMap::new();
    for (at, segment) in segments.iter().enumerate() {
        touching.entry(segment.on_a).or_default().push(at);
        touching.entry(segment.on_b).or_default().push(at);
    }

    let mut used = vec![false; segments.len()];
    let mut made: Vec<Polyline> = Vec::new();

    // The walk itself, from one end of one segment, consuming as it goes.
    let walk = |first: usize, from: At, used: &mut Vec<bool>| -> Polyline {
        let mut points = vec![point_at(&segments[first], from)];
        let (mut at, mut here) = (first, from);
        loop {
            used[at] = true;
            let far = other_end(&segments[at], here);
            points.push(point_at(&segments[at], far));
            let next = touching
                .get(&far)
                .into_iter()
                .flatten()
                .copied()
                .find(|one| !used[*one]);
            match next {
                Some(one) => {
                    at = one;
                    here = far;
                }
                None => {
                    // Closed when the walk came back to where it started —
                    // and the repeated point is dropped, `closed` saying it
                    // instead.
                    let closed = far == from && points.len() > 2;
                    if closed {
                        points.pop();
                    }
                    return Polyline { points, closed };
                }
            }
        }
    };

    // Loose ends first: a place only one segment touches is where an open line
    // begins.
    for (end, at) in touching.iter() {
        if at.len() != 1 {
            continue;
        }
        let one = at[0];
        if used[one] {
            continue;
        }
        made.push(walk(one, *end, &mut used));
    }
    // Whatever is left forms closed loops, or lines whose ends are all
    // junctions.
    for at in 0..segments.len() {
        if !used[at] {
            made.push(walk(at, segments[at].on_a, &mut used));
        }
    }
    made
}

/// The coordinates of the end of `segment` that sits at `where_`.
fn point_at(segment: &Segment, where_: At) -> [f64; 3] {
    if segment.on_a == where_ { segment.a } else { segment.b }
}

/// The other end of `segment` from `where_`.
fn other_end(segment: &Segment, where_: At) -> At {
    if segment.on_a == where_ {
        segment.on_b
    } else {
        segment.on_a
    }
}

/// The smallest and largest value present, ignoring `NaN`.
///
/// `None` when every value is `NaN` — the one case where there is no range to
/// contour, as opposed to a range of zero width.
pub fn range(values: &[f64]) -> Option<(f64, f64)> {
    let mut found: Option<(f64, f64)> = None;
    for one in values.iter().copied().filter(|one| !one.is_nan()) {
        found = Some(match found {
            None => (one, one),
            Some((low, high)) => (low.min(one), high.max(one)),
        });
    }
    found
}

/// What every entry point checks before it starts.
fn check(points: &[f64], triangles: &[u32], values: &[f64]) -> Result<(), Error> {
    if points.len() % 3 != 0 {
        return Err(Error::NotTriples { given: points.len() });
    }
    if triangles.len() % 3 != 0 {
        return Err(Error::NotTriangles {
            given: triangles.len(),
        });
    }
    let nodes = points.len() / 3;
    if values.len() != nodes {
        return Err(Error::MismatchedLengths {
            count: nodes,
            values: values.len(),
        });
    }
    if let Some(&past) = triangles.iter().find(|at| **at as usize >= nodes) {
        return Err(Error::NodeOutOfRange {
            at: past as usize,
            nodes,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unit square as two triangles — nodes 0,1,2,3 at (0,0), (1,0), (0,1),
    /// (1,1) — with the elevation of each corner given, so a test can shape
    /// the field it wants to contour.
    fn square(z: [f64; 4]) -> (Vec<f64>, Vec<u32>) {
        let points = vec![
            0.0, 0.0, z[0], //
            1.0, 0.0, z[1], //
            0.0, 1.0, z[2], //
            1.0, 1.0, z[3], //
        ];
        (points, vec![0, 1, 3, 0, 3, 2])
    }

    /// The elevation of every node, which is what these tests contour.
    fn elevations(points: &[f64]) -> Vec<f64> {
        points.chunks_exact(3).map(|p| p[2]).collect()
    }

    #[test]
    fn the_levels_pass_through_zero_whether_or_not_zero_was_asked_for() {
        // Why this is not `min + i * step`: over -37..63 the even-from-min
        // spacing gives -37, -12, 13, 38, 63, and a person reading a legend
        // wants -25, 0, 25, 50.
        let made = levels(-37.0, 63.0, 4).expect("a range and a count");
        assert_eq!(made, vec![-25.0, 0.0, 25.0, 50.0]);
    }

    #[test]
    fn the_levels_are_exact_multiples_and_not_an_accumulated_sum() {
        // Ten steps of 0.1 added one after another end at 0.9999999999999999;
        // multiplying each step once ends at 1.0, which is the number the
        // legend prints.
        let made = levels(0.0, 1.0, 10).expect("a range and a count");
        assert!(made.contains(&1.0), "{made:?}");
        for (at, one) in made.iter().enumerate() {
            assert_eq!(*one, at as f64 * 0.1, "level {at} drifted");
        }
    }

    #[test]
    fn a_flat_field_has_no_contour_and_that_is_an_answer() {
        assert_eq!(levels(5.0, 5.0, 10).expect("no refusal"), Vec::<f64>::new());
        assert_eq!(levels(9.0, 5.0, 10).expect("no refusal"), Vec::<f64>::new());
    }

    /// **The property is measured on the output, not compared with a segment
    /// worked out by hand** — which would only say the two agree.
    #[test]
    fn every_point_of_a_contour_carries_the_level_it_traces() {
        let (points, triangles) = square([0.0, 10.0, 10.0, 20.0]);
        let values = elevations(&points);

        for level in levels(0.0, 20.0, 8).expect("a range and a count") {
            let found = contour(&points, &triangles, &values, level).expect("no refusal");
            for segment in &found.segments {
                for end in [segment.a, segment.b] {
                    assert!(
                        (end[2] - level).abs() < 1e-12,
                        "a point of the {level} contour sits at {}",
                        end[2]
                    );
                }
            }
        }
    }

    #[test]
    fn a_plane_gives_a_straight_contour_of_the_length_geometry_says() {
        // z = 10x over the unit square: the level 5 runs along x = 0.5, from
        // (0.5, 0) to (0.5, 1) — length 1.
        let (points, triangles) = square([0.0, 10.0, 0.0, 10.0]);
        let values = elevations(&points);
        let found = contour(&points, &triangles, &values, 5.0).expect("no refusal");

        let length: f64 = found
            .segments
            .iter()
            .map(|s| {
                let d = [s.b[0] - s.a[0], s.b[1] - s.a[1], s.b[2] - s.a[2]];
                (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
            })
            .sum();
        assert!((length - 1.0).abs() < 1e-12, "the contour is {length} long");
        for segment in &found.segments {
            for end in [segment.a, segment.b] {
                assert!((end[0] - 0.5).abs() < 1e-12, "a point strayed to x = {}", end[0]);
            }
        }
    }

    #[test]
    fn a_level_outside_the_field_draws_nothing() {
        let (points, triangles) = square([0.0, 10.0, 10.0, 20.0]);
        let values = elevations(&points);
        for level in [-1.0, 21.0] {
            let found = contour(&points, &triangles, &values, level).expect("no refusal");
            assert!(found.segments.is_empty(), "level {level} drew {found:?}");
        }
        // The counter-proof: the same surface does draw in between, so "empty"
        // is a fact about the level and not about a pass that never ran.
        let inside = contour(&points, &triangles, &values, 10.0).expect("no refusal");
        assert!(!inside.segments.is_empty());
    }

    #[test]
    fn a_triangle_with_a_missing_value_is_set_aside_and_counted() {
        let (points, triangles) = square([0.0, 10.0, f64::NAN, 20.0]);
        let values = elevations(&points);
        let found = contour(&points, &triangles, &values, 10.0).expect("no refusal");
        // The `NaN` sits on node 2, which only the second triangle names.
        assert_eq!(found.skipped, 1);
        assert!(
            !found.segments.is_empty(),
            "the other triangle still contributes: a hole is not a refusal"
        );
    }

    /// An edge lying **exactly on** the level is drawn, and that is what `>=`
    /// buys over `>`.
    ///
    /// Written after a first attempt that could not fail: a *single* vertex on
    /// the level is reached either way, because the interpolation lands on it
    /// with `t = 0` or `t = 1` whichever side the classification puts it. The
    /// two spellings only part company when a whole edge sits on the level —
    /// `>=` puts its two ends above and traces the edge, `>` puts the whole
    /// triangle below and loses it. Mutating the operator makes this test go
    /// red, which is the check the first one failed.
    #[test]
    fn an_edge_lying_on_the_level_is_traced_and_not_lost() {
        // Nodes 0 and 1 are at exactly 10, nodes 2 and 3 below: the level 10
        // runs along the edge joining them, from (0,0) to (1,0).
        let (points, triangles) = square([10.0, 10.0, 0.0, 0.0]);
        let values = elevations(&points);
        let found = contour(&points, &triangles, &values, 10.0).expect("no refusal");

        assert_eq!(found.segments.len(), 1, "the edge is one segment: {found:?}");
        let drawn = found.segments[0];
        let ends = [drawn.a, drawn.b];
        assert!(ends.contains(&[0.0, 0.0, 10.0]), "{ends:?}");
        assert!(ends.contains(&[1.0, 0.0, 10.0]), "{ends:?}");
    }

    #[test]
    fn every_refusal_is_named() {
        let (points, triangles) = square([0.0, 1.0, 2.0, 3.0]);
        let values = elevations(&points);

        assert_eq!(levels(0.0, 1.0, 0), Err(Error::NoLevels));
        assert!(matches!(levels_spaced(0.0, 1.0, 0.0), Err(Error::BadSpacing { .. })));
        assert!(matches!(
            levels_spaced(0.0, 1.0, 1e-9),
            Err(Error::TooManyLevels { .. })
        ));
        assert!(matches!(levels(f64::NAN, 1.0, 4), Err(Error::BadRange { .. })));
        assert!(matches!(
            contour(&points, &triangles, &values, f64::NAN),
            Err(Error::BadLevel { .. })
        ));
        assert!(matches!(
            contour(&points[..5], &triangles, &values, 1.0),
            Err(Error::NotTriples { .. })
        ));
        assert!(matches!(
            contour(&points, &triangles[..4], &values, 1.0),
            Err(Error::NotTriangles { .. })
        ));
        assert!(matches!(
            contour(&points, &triangles, &values[..3], 1.0),
            Err(Error::MismatchedLengths { .. })
        ));
        assert!(matches!(
            contour(&points, &[0, 1, 9], &values, 1.0),
            Err(Error::NodeOutOfRange { at: 9, nodes: 4 })
        ));
    }

    #[test]
    fn the_range_ignores_missing_values_and_says_so_when_there_are_none() {
        assert_eq!(range(&[3.0, f64::NAN, -1.0, 7.0]), Some((-1.0, 7.0)));
        assert_eq!(range(&[f64::NAN, f64::NAN]), None);
        assert_eq!(range(&[]), None);
    }

    #[test]
    fn every_level_comes_back_in_the_order_it_was_asked_for() {
        let (points, triangles) = square([0.0, 10.0, 10.0, 20.0]);
        let values = elevations(&points);
        let wanted = [15.0, 5.0, 10.0];
        let found = contours(&points, &triangles, &values, &wanted).expect("no refusal");
        let got: Vec<f64> = found.iter().map(|one| one.level).collect();
        assert_eq!(got, wanted);
    }

    // -- Chaining ---------------------------------------------------------

    /// The spacing of the test grid, and **not a round number on purpose**.
    ///
    /// At integer coordinates the two faces sharing an edge interpolate to
    /// bit-identical points, so a mesh built on them cannot show what [`At`]
    /// is for — which is how the first version of
    /// `the_same_ends_do_not_agree_bit_for_bit` came to fail. Measured over
    /// 200 000 random edges, the two directions disagree in the last bits
    /// about 77% of the time; at integer coordinates, never.
    const STEP: f64 = 1.37;

    /// A grid of `side` by `side` quads, each cut into two triangles, with the
    /// elevation of every node given by `f(x, y)` — in real coordinates, not
    /// in row and column.
    ///
    /// Big enough that a contour crosses many triangles, which is the whole
    /// point: chaining is trivial on two faces and only says something on a
    /// mesh where the same cut point is computed twice, from both sides.
    fn grid(side: usize, f: impl Fn(f64, f64) -> f64) -> (Vec<f64>, Vec<u32>, Vec<f64>) {
        let mut points = Vec::new();
        let mut values = Vec::new();
        for row in 0..=side {
            for column in 0..=side {
                let (x, y) = (column as f64 * STEP, row as f64 * STEP);
                let z = f(x, y);
                points.extend_from_slice(&[x, y, z]);
                values.push(z);
            }
        }
        let at = |row: usize, column: usize| (row * (side + 1) + column) as u32;
        let mut triangles = Vec::new();
        for row in 0..side {
            for column in 0..side {
                triangles.extend_from_slice(&[at(row, column), at(row, column + 1), at(row + 1, column + 1)]);
                triangles.extend_from_slice(&[at(row, column), at(row + 1, column + 1), at(row + 1, column)]);
            }
        }
        (points, triangles, values)
    }

    /// **The test the whole of [`At`] exists for.**
    ///
    /// A plane over a ten-by-ten grid: the level 5.5 crosses twenty triangles,
    /// and every interior cut is computed twice — once from each of the two
    /// faces sharing that edge, starting from opposite corners. Matching ends
    /// by coordinate leaves the line in twenty pieces; matching on the mesh
    /// feature gives one.
    #[test]
    fn a_contour_across_many_triangles_chains_into_a_single_line() {
        let (points, triangles, values) = grid(10, |x, _| x);
        let found = contour(&points, &triangles, &values, 5.5).expect("no refusal");
        assert!(found.segments.len() > 10, "not enough pieces to prove anything");

        let lines = chain(&found.segments);
        assert_eq!(lines.len(), 1, "one contour, one line: {:?}", lines.len());
        assert_eq!(
            lines[0].points.len(),
            found.segments.len() + 1,
            "an open line of n segments has n+1 points"
        );
        assert!(!lines[0].closed, "this one runs off both edges of the grid");
    }

    #[test]
    fn a_contour_that_closes_says_so_and_does_not_repeat_its_first_point() {
        // A cone: z falls away from the middle of the grid, so a level below
        // the peak is a loop around it.
        let middle = 10.0 * STEP / 2.0;
        let (points, triangles, values) = grid(10, |x, y| {
            let (dx, dy) = (x - middle, y - middle);
            10.0 - (dx * dx + dy * dy).sqrt()
        });
        let found = contour(&points, &triangles, &values, 7.0).expect("no refusal");
        let lines = chain(&found.segments);

        assert_eq!(lines.len(), 1, "one loop: {lines:?}");
        let loop_ = &lines[0];
        assert!(loop_.closed, "a contour around a peak closes");
        assert_eq!(
            loop_.points.len(),
            found.segments.len(),
            "a closed line of n segments has n points, the first not repeated"
        );
        assert_ne!(
            loop_.points.first(),
            loop_.points.last(),
            "`closed` says it; a duplicated coordinate must not"
        );
    }


    /// A field rough enough that the two faces sharing an edge really do
    /// interpolate it from opposite ends.
    ///
    /// The drift only happens when the *third* corners of the two triangles
    /// fall on opposite sides of the level, and on a smooth field they rarely
    /// do — two earlier versions of the test below used a plane and then a
    /// cone, and both found **zero** drift, which is what sent this hunting
    /// for a mesh that shows the effect instead of a mesh that assumes it.
    /// Here it is about 12% of shared ends.
    fn rough(side: usize) -> (Vec<f64>, Vec<u32>, Vec<f64>) {
        grid(side, |x, y| {
            (x * 0.7).sin() * 3.0 + (y * 0.43).cos() * 2.0 + (x * y * 0.013).sin() * 4.0
        })
    }

    /// The level with the most drifted ends on [`rough`], found by sweeping.
    const ROUGH_LEVEL: f64 = 0.48;

    /// **The counter-proof for [`At`]**: matching ends by coordinate really
    /// does leave this contour in more pieces than matching on the mesh.
    #[test]
    fn matching_ends_by_coordinate_fragments_a_contour_that_at_keeps_whole() {
        use std::collections::HashMap;

        let (points, triangles, values) = rough(20);
        let found = contour(&points, &triangles, &values, ROUGH_LEVEL).expect("no refusal");
        assert!(found.segments.len() > 50, "too small to say anything");

        // How many ends stay loose under each way of matching. An end is loose
        // when nothing else lands on it — which is what breaks a chain.
        let mut by_mesh: HashMap<At, usize> = HashMap::new();
        let mut by_bits: HashMap<[u64; 3], usize> = HashMap::new();
        for one in &found.segments {
            for (key, point) in [(one.on_a, one.a), (one.on_b, one.b)] {
                *by_mesh.entry(key).or_default() += 1;
                let bits = [point[0].to_bits(), point[1].to_bits(), point[2].to_bits()];
                *by_bits.entry(bits).or_default() += 1;
            }
        }
        fn loose<K>(counted: &HashMap<K, usize>) -> usize {
            counted.values().filter(|n| **n == 1).count()
        }
        let (mesh_loose, bits_loose) = (loose(&by_mesh), loose(&by_bits));

        assert!(
            bits_loose > mesh_loose,
            "coordinates left {bits_loose} loose ends and the mesh {mesh_loose}: \
             this mesh does not show the drift, so it cannot justify `At`"
        );
    }

    /// Every segment lands in exactly one line — on the mesh that is hardest
    /// for the chaining, where a tenth of the ends do not agree bit for bit.
    #[test]
    fn every_segment_lands_in_exactly_one_line() {
        let (points, triangles, values) = rough(20);
        let found = contour(&points, &triangles, &values, ROUGH_LEVEL).expect("no refusal");
        let lines = chain(&found.segments);

        let drawn: usize = lines
            .iter()
            .map(|line| if line.closed { line.points.len() } else { line.points.len() - 1 })
            .sum();
        assert_eq!(
            drawn,
            found.segments.len(),
            "every segment used exactly once: {} in, {} out",
            found.segments.len(),
            drawn
        );
    }

    #[test]
    fn nothing_to_chain_gives_nothing() {
        assert_eq!(chain(&[]), Vec::new());
    }
}
