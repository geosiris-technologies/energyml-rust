//! A painted surface keeps its paint along a section's cut
//! (`paint::surfaces::trace_pieces`, and the `sources` the reader gives the cut).
//!
//! Every surface here is fabricated: a square of two triangles, a paint of a few
//! named colours. The rules asserted are the ones written at the head of
//! `paint/surfaces.rs`; the numbering of the front end's nodes against the reader's
//! is confronted on a real package in a downstream test.

use energyml_section::model::{
    Blend, CutShape, CutSource, ObjectId, Paint, SectionKey, SectionTrace, SurfaceCut, SurfaceRead, TraceRole,
};
use energyml_section::paint::surfaces::trace_pieces;
use energyml_section::read::finite::finite_surface;
use energyml_section::read::surfaces::{Prepared, cut_prepared};

const RED: &str = "#ff0000ff";
const GREEN: &str = "#00ff00ff";
const NONE: &str = "#12345600";

fn paint(blend: Blend, nodes: &[&str]) -> Paint {
    Paint {
        nodes: nodes.iter().map(|n| n.to_string()).collect(),
        blend,
    }
}

fn source(a: u32, b: u32, t: f64) -> CutSource {
    CutSource { a, b, t }
}

// --- trace_pieces ------------------------------------------------------------

#[test]
fn smooth_runs_from_one_nodes_colour_to_the_next_through_their_mix() {
    // Three points: on node 0 (red), halfway between 0 and 1 (red and green),
    // on node 1 (green). Smooth mixes at `t`, component by component.
    let span = [[0.0, 0.0], [10.0, 0.0], [20.0, 0.0]];
    let sources = [source(0, 1, 0.0), source(0, 1, 0.5), source(0, 1, 1.0)];
    let pieces = trace_pieces(&span, &sources, &paint(Blend::Smooth, &[RED, GREEN])).unwrap();
    // The whole span is covered, from x = 0 to x = 20, with no hole.
    assert_eq!(pieces.first().unwrap().points.first(), Some(&[0.0, 0.0]));
    assert_eq!(pieces.last().unwrap().points.last(), Some(&[20.0, 0.0]));
    // The point halfway is the mix of red and green: (127.5, 127.5, 0) — the
    // two sub-pieces meeting there wear colours on either side of it, red-ish
    // before, green-ish after, never one flat colour for the whole span.
    let colours: Vec<&str> = pieces.iter().map(|p| p.colour.as_str()).collect();
    assert!(colours.len() > 2, "a ramp is split into steps: {colours:?}");
    assert_eq!(
        colours.first().copied(),
        Some("#f90600"),
        "a sub-piece wears the colour at its middle"
    );
    assert_eq!(colours.last().copied(), Some("#06f900"));
    assert!(pieces.iter().all(|p| p.opacity == 1.0));
    // Symmetric: the two halves mirror each other (red/green swapped).
    let n = colours.len();
    for k in 0..n / 2 {
        let (head, tail) = (&colours[k], &colours[n - 1 - k]);
        assert_eq!(&head[1..3], &tail[3..5], "{colours:?}");
    }
}

#[test]
fn net_never_mixes_two_categories() {
    // A discrete property: red and green are two facies, and a point halfway
    // between is one of them, never yellow.
    let span = [[0.0, 0.0], [10.0, 0.0], [20.0, 0.0]];
    let sources = [source(0, 1, 0.2), source(0, 1, 0.4), source(0, 1, 0.9)];
    let pieces = trace_pieces(&span, &sources, &paint(Blend::Net, &[RED, GREEN])).unwrap();
    // Points 0 and 1 are nearer node 0 (red), point 2 nearer node 1 (green): one
    // red run over the first segment, then the change cut at the middle of the
    // second segment (x = 15), each half in its own end's colour.
    assert_eq!(pieces.len(), 2, "{pieces:?}");
    assert_eq!(pieces[0].colour, "#ff0000");
    assert_eq!(pieces[0].points, vec![[0.0, 0.0], [10.0, 0.0], [15.0, 0.0]]);
    assert_eq!(pieces[1].colour, "#00ff00");
    assert_eq!(pieces[1].points, vec![[15.0, 0.0], [20.0, 0.0]]);
    // Halfway on the edge goes to the node it ends on (`t >= 1/2` is `b`).
    let tie = trace_pieces(
        &span[..2],
        &[source(0, 1, 0.5), source(0, 1, 0.5)],
        &paint(Blend::Net, &[RED, GREEN]),
    )
    .unwrap();
    assert_eq!(tie[0].colour, "#00ff00");
}

#[test]
fn a_node_with_no_value_leaves_a_gap_never_a_line() {
    // Node 2 has alpha 0: *no value there*. The point leaning on it has no
    // colour, and the segments on either side of it are not drawn.
    let span = [[0.0, 0.0], [10.0, 0.0], [20.0, 0.0], [30.0, 0.0], [40.0, 0.0]];
    let sources = [
        source(0, 1, 0.0),
        source(0, 1, 1.0),
        source(1, 2, 0.5),
        source(1, 0, 0.5),
        source(0, 1, 0.0),
    ];
    for blend in [Blend::Net, Blend::Smooth] {
        let pieces = trace_pieces(&span, &sources, &paint(blend, &[RED, GREEN, NONE])).unwrap();
        // Segment 0-1 and 3-4 stand; 1-2 and 2-3 (one end on the empty node) go.
        let covered: Vec<(f64, f64)> = pieces
            .iter()
            .map(|p| (p.points.first().unwrap()[0], p.points.last().unwrap()[0]))
            .collect();
        assert!(
            covered
                .iter()
                .all(|&(from, to)| (from >= 0.0 && to <= 10.0) || (from >= 30.0 && to <= 40.0)),
            "{blend:?}: {covered:?}"
        );
        assert!(covered.iter().any(|&(from, _)| from == 0.0) && covered.iter().any(|&(_, to)| to == 40.0));
    }
    // A point exactly **on** a node with a value does not lean on its edge's
    // other end: `t = 0` ignores node `b`, even if `b` is empty.
    let on_a = trace_pieces(
        &[[0.0, 0.0], [10.0, 0.0]],
        &[source(0, 2, 0.0), source(0, 2, 0.0)],
        &paint(Blend::Smooth, &[RED, GREEN, NONE]),
    )
    .unwrap();
    assert_eq!(on_a.len(), 1);
    assert_eq!(on_a[0].colour, "#ff0000");
}

#[test]
fn the_opacity_is_the_alpha() {
    let half = "#336699";
    let pieces = trace_pieces(
        &[[0.0, 0.0], [1.0, 0.0]],
        &[source(0, 1, 0.0), source(0, 1, 0.0)],
        &paint(Blend::Net, &["#33669980", half]),
    )
    .unwrap();
    assert_eq!(pieces[0].colour, half);
    assert!((pieces[0].opacity - 128.0 / 255.0).abs() < 1e-12);
}

#[test]
fn what_does_not_fit_is_refused_by_name() {
    let span = [[0.0, 0.0], [1.0, 0.0]];
    let fine = [source(0, 1, 0.0), source(0, 1, 1.0)];
    let two = paint(Blend::Net, &[RED, GREEN]);
    // A source naming a node beyond the paint.
    let why = trace_pieces(&span, &[source(0, 1, 0.0), source(0, 7, 1.0)], &two).unwrap_err();
    assert!(why.contains("node 7") && why.contains("2 node"), "{why}");
    // Even at a weight of zero: the numbering is wrong, whatever the weight.
    assert!(trace_pieces(&span, &[source(0, 1, 0.0), source(0, 9, 0.0)], &two).is_err());
    // Not one source per point.
    let why = trace_pieces(&span, &fine[..1], &two).unwrap_err();
    assert!(why.contains("2 point") && why.contains("1 source"), "{why}");
    // A colour that does not read.
    for bad in ["red", "#ff00", "#gg0000ff", "ff0000ff", "#ff0000ffee", "#éé0000"] {
        let why = trace_pieces(&span, &fine, &paint(Blend::Net, &[RED, bad])).unwrap_err();
        assert!(why.contains("node 1") && why.contains(bad), "{bad}: {why}");
    }
    // A `t` that is not a place on the edge.
    for t in [f64::NAN, -0.5, 1.5] {
        assert!(
            trace_pieces(&span, &[source(0, 1, t), source(0, 1, 0.0)], &two).is_err(),
            "t = {t}"
        );
    }
    // An empty span is no piece, not a refusal.
    assert_eq!(trace_pieces(&[], &[], &two).unwrap(), Vec::new());
}

// --- the sources the reader gives a cut -------------------------------------

/// A square of two triangles, (0,0) to (10,10), on `z = x + 2y`: every node has
/// its own height. The preparation's numbering is **not** the identity — the
/// representation's nodes 3, 4, 6, 7, node 5 having been a hole `drawable`
/// compacted out — so a source that still named the kept points' own indices
/// would be wrong, and say so.
fn prepared() -> Prepared {
    Prepared {
        object: ObjectId {
            store: "s1".into(),
            uri: "eml:///resqml22.TriangulatedSetRepresentation(aa)".into(),
            uuid: "aa".into(),
            title: "Square".into(),
            qualified_type: "resqml22.TriangulatedSetRepresentation".into(),
        },
        role: TraceRole::Horizon,
        time: false,
        colour: None,
        age: None,
        feature: None,
        points: vec![
            [0.0, 0.0, 0.0],
            [10.0, 0.0, 10.0],
            [10.0, 10.0, 30.0],
            [0.0, 10.0, 20.0],
        ],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        nodes: vec![3, 4, 6, 7],
        grid: None,
    }
}

fn trace(points: &[[f64; 2]]) -> SectionTrace {
    SectionTrace {
        key: SectionKey::Long,
        trace: points.to_vec(),
        left: 10.0,
        right: 10.0,
        crossing: None,
    }
}

/// The 3D point a source names, in the representation's numbering.
fn place(source: &CutSource) -> [f64; 3] {
    let corner = |node: u32| -> [f64; 3] {
        match node {
            3 => [0.0, 0.0, 0.0],
            4 => [10.0, 0.0, 10.0],
            6 => [10.0, 10.0, 30.0],
            7 => [0.0, 10.0, 20.0],
            other => panic!("node {other} is not one of the square's"),
        }
    };
    let (a, b) = (corner(source.a), corner(source.b));
    [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * source.t)
}

#[test]
fn a_cut_says_where_each_of_its_points_lies_on_the_surface() {
    // The trace runs along y = 3 from x = -5 to 15: the surface is cut between
    // x = 0 and x = 10, the diagonal (nodes 3 and 6) is crossed at x = 3.
    let read = cut_prepared(&prepared(), &[trace(&[[-5.0, 3.0], [15.0, 3.0]])]);
    assert_eq!(read.cuts.len(), 1);
    let cut = &read.cuts[0];
    let CutShape::Trace { spans } = &cut.shape else {
        panic!("a trace cut")
    };
    assert_eq!(spans.len(), 1);
    assert_eq!(cut.sources.len(), spans.len(), "one list per span");
    assert_eq!(cut.sources[0].len(), spans[0].len(), "one source per point");
    assert_eq!(spans[0].len(), 3);
    for (point, source) in spans[0].iter().zip(&cut.sources[0]) {
        // The span's point is (s, z); the source recomputes (x, y, z) from the
        // nodes — in the **representation's** numbering — and s is the distance
        // from the trace's start (-5, 3) along it.
        let [x, y, z] = place(source);
        assert!((y - 3.0).abs() < 1e-9, "the point is on the trace: {source:?}");
        assert!((x + 5.0 - point[0]).abs() < 1e-9, "s: {point:?} against x = {x}");
        assert!((z - point[1]).abs() < 1e-9, "z: {point:?} against {z}");
    }
    // By hand: x = 0, 3, 10 — the middle one on the diagonal, nodes 3 and 6.
    let middle = cut.sources[0][1];
    assert!([(3, 6), (6, 3)].contains(&(middle.a, middle.b)), "{middle:?}");
    assert!((place(&middle)[0] - 3.0).abs() < 1e-9);
}

#[test]
fn a_preparation_without_a_numbering_gives_no_sources_and_the_cut_still_stands() {
    let mut flat = prepared();
    flat.nodes.clear();
    let read = cut_prepared(&flat, &[trace(&[[-5.0, 3.0], [15.0, 3.0]])]);
    let cut = &read.cuts[0];
    assert!(cut.sources.is_empty(), "the reader cannot say: flat colour");
    let CutShape::Trace { spans } = &cut.shape else {
        panic!()
    };
    assert_eq!(spans[0].len(), 3);
}

// --- finite.rs keeps the sources parallel ------------------------------------

fn read_of(spans: Vec<Vec<[f64; 2]>>, sources: Vec<Vec<CutSource>>) -> SurfaceRead {
    let mut read = SurfaceRead {
        object: prepared().object,
        role: TraceRole::Horizon,
        time: false,
        colour: None,
        age: None,
        feature: None,
        cuts: Vec::new(),
    };
    read.cuts.push(SurfaceCut {
        section: SectionKey::Long,
        shape: CutShape::Trace { spans },
        notes: Vec::new(),
        sources,
    });
    read
}

#[test]
fn breaking_a_cut_at_a_hole_takes_the_sources_of_the_points_that_go() {
    // Seven points; the fourth is not a number: two spans of three, each with
    // its own sources — a source is its point's, so node numbers identify them.
    let at = |n: u32| source(n, n + 100, 0.25);
    let span: Vec<[f64; 2]> = (0..7).map(|k| [k as f64, 1.0]).collect();
    let mut spans = vec![span.clone()];
    spans[0][3] = [f64::NAN, 1.0];
    let mut read = read_of(spans, vec![(0..7).map(at).collect()]);
    finite_surface(&mut read);
    let cut = &read.cuts[0];
    let CutShape::Trace { spans } = &cut.shape else {
        panic!()
    };
    assert_eq!(spans.len(), 2);
    assert_eq!(cut.sources.len(), 2);
    assert_eq!(cut.sources[0], (0..3).map(at).collect::<Vec<_>>());
    assert_eq!(cut.sources[1], (4..7).map(at).collect::<Vec<_>>());
    for (span, sources) in spans.iter().zip(&cut.sources) {
        assert_eq!(span.len(), sources.len());
        // The source of the first point of a span is that point's own.
        assert_eq!(sources[0].a as f64, span[0][0]);
    }
}

#[test]
fn a_piece_too_short_to_draw_is_dropped_with_its_sources() {
    // A lone point between two holes is no span; the sources go with it, and
    // the one span left is the long one, paired with **its** sources.
    let at = |n: u32| source(n, n + 100, 0.5);
    let mut points: Vec<[f64; 2]> = (0..6).map(|k| [k as f64, 0.0]).collect();
    points[1] = [f64::NAN, 0.0];
    points[3] = [f64::NAN, 0.0];
    // Points 0 (alone), 2 (alone), 4 and 5 (a span).
    let mut read = read_of(vec![points], vec![(0..6).map(at).collect()]);
    finite_surface(&mut read);
    let cut = &read.cuts[0];
    let CutShape::Trace { spans } = &cut.shape else {
        panic!()
    };
    assert_eq!(spans.len(), 1);
    assert_eq!(cut.sources, vec![vec![at(4), at(5)]]);
}

#[test]
fn sources_that_are_not_parallel_are_cleared_not_left_to_name_the_wrong_points() {
    let mut points: Vec<[f64; 2]> = (0..4).map(|k| [k as f64, 0.0]).collect();
    points[1] = [f64::NAN, 0.0];
    // Two sources for four points: they cannot be carried.
    let mut read = read_of(vec![points], vec![vec![source(0, 1, 0.0), source(0, 1, 0.5)]]);
    finite_surface(&mut read);
    assert!(read.cuts[0].sources.is_empty());
}

#[test]
fn a_cut_with_no_sources_stays_without() {
    let mut points: Vec<[f64; 2]> = (0..4).map(|k| [k as f64, 0.0]).collect();
    points[1] = [f64::NAN, 0.0];
    let mut read = read_of(vec![points], Vec::new());
    finite_surface(&mut read);
    assert!(read.cuts[0].sources.is_empty());
}

// --- the numbering the reader's points have in the representation -------------

fn patch(
    index: usize,
    node_offset: usize,
    points: Vec<[f64; 3]>,
    triangles: Vec<[u32; 3]>,
) -> energyml_geometry::geometry::Patch {
    energyml_geometry::geometry::Patch {
        index,
        node_offset,
        points,
        topology: energyml_geometry::geometry::Topology::Triangles(triangles),
        closed: Vec::new(),
    }
}

/// `drawable` compacts the nodes that hold no location and renumbers; the front end's
/// paint is numbered by the representation (`point_count`, holes included).
/// `drawn_nodes` is the bridge, and `drawable` itself is its witness here: the
/// i-th point it keeps must be the representation's node `nodes[i]`.
#[test]
fn the_numbering_of_the_kept_points_is_the_representations_across_holes_and_patches() {
    let nan = f64::NAN;
    // Two patches. Patch 0: five nodes, the third undefined. Patch 1: four
    // nodes, the first undefined. Nodes 0..=8 in the representation; 2 and 5
    // are holes. Triangles avoid the holes (as `drawable` would drop the rest).
    let geometry = energyml_geometry::geometry::Geometry {
        kind: "TriangulatedSetRepresentation",
        patches: vec![
            patch(
                0,
                0,
                vec![
                    [0.0, 0.0, 0.0],
                    [1.0, 0.0, 1.0],
                    [nan; 3],
                    [1.0, 1.0, 2.0],
                    [0.0, 1.0, 3.0],
                ],
                vec![[0, 1, 3], [0, 3, 4]],
            ),
            patch(
                1,
                5,
                vec![[nan; 3], [5.0, 0.0, 6.0], [5.0, 1.0, 7.0], [6.0, 1.0, 8.0]],
                vec![[1, 2, 3]],
            ),
        ],
    };
    let drawn = geometry.drawable();
    assert_eq!(drawn.dropped, 2);
    let nodes = energyml_section::read::surfaces::drawn_nodes(&geometry, drawn.points.len());
    assert_eq!(nodes, vec![0, 1, 3, 4, 6, 7, 8]);
    // The witness: `drawable`'s own i-th point is the representation's node.
    let raw: Vec<[f64; 3]> = geometry.patches.iter().flat_map(|p| p.points.clone()).collect();
    assert_eq!(raw.len(), geometry.point_count());
    for (kept, node) in drawn.points.iter().zip(&nodes) {
        assert_eq!(*kept, raw[*node as usize], "node {node}");
    }
    // A count that does not agree (a node dropped for another reason than being
    // undefined) is not guessed at: no numbering.
    assert!(energyml_section::read::surfaces::drawn_nodes(&geometry, drawn.points.len() - 1).is_empty());
}
