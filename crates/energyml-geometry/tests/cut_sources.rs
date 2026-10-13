//! Where a cut's points come from: the edge each end lies on
//! (`section::cut_sourced`, `chain_sourced`, `fence::curve_sourced`).
//!
//! Two claims. **Additive**: the plain functions answer what they answered, and
//! the sourced ones answer the same numbers (pinned here on values worked out by
//! hand, not on the other function's output alone). **Honest**: every end, put
//! back through its `(a, b, t)`, is the end — recomputed here from the nodes,
//! not read off the cut.

use energyml_geometry::fence::{self, curve, curve_sourced};
use energyml_geometry::section::{
    self, CutEnd, chain, chain_sourced, cut, cut_many, cut_many_sourced, cut_sourced,
};

/// A square of two triangles, from (0,0) to (10,10), on `z = x + 2y`: every
/// node has its own height, so a wrong node shows in z.
fn square() -> (Vec<[f64; 3]>, Vec<[u32; 3]>) {
    (
        vec![
            [0.0, 0.0, 0.0],
            [10.0, 0.0, 10.0],
            [10.0, 10.0, 30.0],
            [0.0, 10.0, 20.0],
        ],
        vec![[0, 1, 2], [0, 2, 3]],
    )
}

/// The point a source names, computed from the nodes.
fn place(points: &[[f64; 3]], source: &CutEnd) -> [f64; 3] {
    let (a, b) = (points[source.a as usize], points[source.b as usize]);
    [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * source.t)
}

fn close(left: [f64; 3], right: [f64; 3]) -> bool {
    (0..3).all(|k| (left[k] - right[k]).abs() < 1e-9)
}

#[test]
fn the_plain_cut_and_the_chain_answer_what_they_answered() {
    let (points, triangles) = square();
    // The trace crosses the square through the middle, at y = 5, from outside
    // to outside: z = x + 10 on it, between x = 0 and x = 10.
    let trace = [[-5.0, 5.0], [15.0, 5.0]];
    let cuts = cut(&points, &triangles, &trace);
    assert_eq!(cuts.len(), 2);
    let mut ends: Vec<[f64; 3]> = cuts.iter().flat_map(|c| c.ends).collect();
    ends.sort_by(|a, b| a[0].total_cmp(&b[0]));
    // By hand: x = 0, 5, 5, 10 (the diagonal x = y crosses y = 5 at x = 5).
    let expected = [
        [0.0, 5.0, 10.0],
        [5.0, 5.0, 15.0],
        [5.0, 5.0, 15.0],
        [10.0, 5.0, 20.0],
    ];
    for (got, want) in ends.iter().zip(expected) {
        assert!(close(*got, want), "{got:?} against {want:?}");
    }
    // The sourced twin gives the same segments, bit for bit.
    let sourced = cut_sourced(&points, &triangles, &trace);
    assert_eq!(sourced.iter().map(|s| s.cut.clone()).collect::<Vec<_>>(), cuts);
    // And the curve — the spans fence draws — is the same, and by hand:
    // s = 5, 10, 15 at z = 10, 15, 20.
    let curved = curve(&cuts);
    assert_eq!(curved.spans.len(), 1);
    let want = [[5.0, 10.0], [10.0, 15.0], [15.0, 20.0]];
    assert_eq!(curved.spans[0].len(), 3);
    for (got, want) in curved.spans[0].iter().zip(want) {
        assert!(
            (got[0] - want[0]).abs() < 1e-9 && (got[1] - want[1]).abs() < 1e-9,
            "{got:?}"
        );
    }
    let twin = curve_sourced(&sourced);
    assert_eq!(twin.curve, curved);
    let chains: Vec<_> = chain(&cuts);
    let twin_chains: Vec<_> = chain_sourced(&sourced).into_iter().map(|c| c.chain).collect();
    assert_eq!(twin_chains, chains);
}

#[test]
fn every_end_put_back_through_its_edge_is_the_end() {
    let (points, triangles) = square();
    // Inside the square on both sides: nothing is clipped, every end is on an
    // edge, and the edge and `t` must give the point.
    let trace = [[-5.0, 3.0], [15.0, 3.0]];
    let sourced = cut_sourced(&points, &triangles, &trace);
    assert!(!sourced.is_empty());
    for one in &sourced {
        for (end, source) in one.cut.ends.iter().zip(&one.sources) {
            assert!(!source.clipped);
            assert!(
                close(place(&points, source), *end),
                "{source:?} gives {:?}, the cut says {end:?}",
                place(&points, source)
            );
            // And the node pair is a real edge of the square: its two nodes differ.
            assert_ne!(source.a, source.b);
        }
    }
}

#[test]
fn a_clipped_end_says_so_and_is_not_passed_off_as_on_an_edge() {
    let (points, triangles) = square();
    // The trace stops at x = 7: the crossing of the diagonal is at x = 5, the
    // far edge (x = 10) is beyond the window, so the second triangle's end at
    // x = 7 is clipped, inside the triangle.
    let trace = [[-5.0, 3.0], [7.0, 3.0]];
    let sourced = cut_sourced(&points, &triangles, &trace);
    let clipped: Vec<_> = sourced
        .iter()
        .flat_map(|s| s.sources)
        .filter(|s| s.clipped)
        .collect();
    assert_eq!(clipped.len(), 1, "{sourced:?}");
    for one in &sourced {
        for (end, source) in one.cut.ends.iter().zip(&one.sources) {
            if !source.clipped {
                assert!(close(place(&points, source), *end));
            } else {
                // Not on the named edge — that is what the flag admits.
                assert!(!close(place(&points, source), *end));
                assert!((end[0] - 7.0).abs() < 1e-9);
            }
        }
    }
}

#[test]
fn many_traces_give_the_same_answers_and_indices_into_the_whole_surface() {
    // A surface of two squares side by side: the cut along the second one only
    // keeps the triangles of the second (the box prefilter), whose nodes are
    // 4.. — the sources must still name them in the whole surface's numbering.
    let (mut points, mut triangles) = square();
    points.extend([
        [20.0, 0.0, 1.0],
        [30.0, 0.0, 2.0],
        [30.0, 10.0, 3.0],
        [20.0, 10.0, 4.0],
    ]);
    triangles.extend([[4, 5, 6], [4, 6, 7]]);
    let traces = [vec![[-5.0, 3.0], [15.0, 3.0]], vec![[18.0, 6.0], [35.0, 6.0]]];

    let plain = cut_many(&points, &triangles, &traces);
    let sourced = section::cut_many_sourced(&points, &triangles, &traces);
    let twin: Vec<Vec<_>> = sourced
        .iter()
        .map(|t| t.iter().map(|s| s.cut.clone()).collect())
        .collect();
    assert_eq!(twin, plain);

    assert!(!sourced[1].is_empty());
    for one in &sourced[1] {
        for (end, source) in one.cut.ends.iter().zip(&one.sources) {
            assert!(
                source.a >= 4 && source.b >= 4,
                "a node of the first square: {source:?}"
            );
            if !source.clipped {
                assert!(close(place(&points, source), *end));
            }
        }
    }
    // The first trace is also what `cut_sourced` gives alone.
    let alone = cut_sourced(&points, &triangles, &traces[0]);
    assert_eq!(sourced[0], alone);
    let _ = cut_many_sourced::<[[f64; 2]; 2]>;
}

#[test]
fn a_span_keeps_one_source_per_point_and_each_is_on_its_own_point() {
    // A surface of two squares whose cut is chained, walked in whatever order:
    // each span point's source must give that point, in 3D, back — after the
    // chain was reversed and the spans sorted.
    let (mut points, mut triangles) = square();
    points.extend([
        [20.0, 0.0, 1.0],
        [30.0, 0.0, 2.0],
        [30.0, 10.0, 3.0],
        [20.0, 10.0, 4.0],
    ]);
    triangles.extend([[4, 5, 6], [4, 6, 7]]);
    // A bend, so the abscissa runs through two segments.
    let trace = [[35.0, 2.0], [18.0, 2.0], [18.0, 6.0], [-5.0, 6.0]];
    let sourced = cut_sourced(&points, &triangles, &trace);
    let made = curve_sourced(&sourced);
    assert_eq!(
        made.curve.spans.len(),
        made.sources.len(),
        "one list of sources per span"
    );
    assert!(
        made.curve.spans.len() >= 2,
        "two sheets with a gap between: {:?}",
        made.curve.spans
    );
    for (span, sources) in made.curve.spans.iter().zip(&made.sources) {
        assert_eq!(span.len(), sources.len(), "one source per point");
        for (point, source) in span.iter().zip(sources) {
            if source.clipped {
                continue;
            }
            // The z of the span point is the z of the edge point.
            assert!(
                (place(&points, source)[2] - point[1]).abs() < 1e-9,
                "{point:?} {source:?}"
            );
        }
    }
    // The spans are in increasing abscissa, and so are the lists.
    assert!(made.curve.spans.windows(2).all(|p| p[0][0][0] <= p[1][0][0]));
    // The same spans as the plain curve over the same cuts.
    let plain: Vec<_> = sourced.iter().map(|s| s.cut.clone()).collect();
    assert_eq!(made.curve, fence::curve(&plain));
}

#[test]
fn a_welded_node_keeps_the_source_of_the_segment_walked_first() {
    // The diagonal (0,2) of the square is shared by the two triangles, which
    // name it (0→2) and (2→0): the welded node is one point with two spellings.
    let (points, triangles) = square();
    let sourced = cut_sourced(&points, &triangles, &[[-5.0, 5.0], [15.0, 5.0]]);
    let chains = chain_sourced(&sourced);
    assert_eq!(chains.len(), 1);
    assert_eq!(chains[0].sources.len(), chains[0].chain.points.len());
    assert_eq!(chains[0].chain.points.len(), 3, "welded on the shared edge");
    // The middle node is on the diagonal whichever the spelling.
    let middle = chains[0].sources[1];
    let diagonal = [(0, 2), (2, 0)];
    assert!(diagonal.contains(&(middle.a, middle.b)), "{middle:?}");
    assert!(close(place(&points, &middle), chains[0].chain.points[1]));
}
