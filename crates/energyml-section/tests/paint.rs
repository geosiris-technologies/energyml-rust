//! A well drawn in the colours the front end's scene shows along it - `Paint`,
//! `Joint` and `paint::well_pieces` (colours are either graded for continuous
//! properties or fixed per stretch for discrete ones, and the joints are either
//! where the value changes or at every MD).
//!
//! The rules under test are a front ender's (`fragmentAt`,
//! `jointFlags`, `jointsFit`), transcribed on the path in pixels; the checks
//! are on the pieces' colours and **lengths in pixels**, never on a count that
//! the code could satisfy by accident.

use energyml_section::layout::{Attr, LaidObject, Layout, layout};
use energyml_section::model::*;
use energyml_section::paint::{Piece, well_pieces};
use energyml_section::svg;

use crate::layout::{catalogue, id, view, well};

const RED: &str = "#ff0000ff";
const BLUE: &str = "#0000ffff";
const GREEN: &str = "#00ff00ff";
/// A colour with no value: alpha 0 - its rgb must never be drawn.
const NONE: &str = "#12345600";

fn paint(nodes: &[&str], blend: Blend) -> Paint {
    Paint {
        nodes: nodes.iter().map(|c| c.to_string()).collect(),
        blend,
    }
}

fn joints(kind: JointKind, px: f64, alpha: f64) -> Joint {
    Joint {
        kind,
        px,
        colour: "#000000".into(),
        alpha,
    }
}

/// A well laid along x, one node per entry, on the same row.
fn along(xs: &[f64]) -> Vec<[f64; 2]> {
    xs.iter().map(|&x| [x, 50.0]).collect()
}

/// Nodes 100 px apart: 0, 100, … 400.
fn five() -> Vec<[f64; 2]> {
    along(&[0.0, 100.0, 200.0, 300.0, 400.0])
}

fn length(piece: &Piece) -> f64 {
    piece
        .points
        .windows(2)
        .map(|p| ((p[1][0] - p[0][0]).powi(2) + (p[1][1] - p[0][1]).powi(2)).sqrt())
        .sum()
}

fn of<'a>(pieces: &'a [Piece], colour: &str) -> Vec<&'a Piece> {
    pieces.iter().filter(|p| p.colour == colour).collect()
}

fn total(pieces: &[&Piece]) -> f64 {
    pieces.iter().map(|p| length(p)).sum()
}

fn span_of(piece: &Piece) -> (f64, f64) {
    (piece.points.first().unwrap()[0], piece.points.last().unwrap()[0])
}

// --- the reading of one colour per node --------------------------------------

#[test]
fn net_reads_node_k_over_the_stretch_after_it_and_merges_equal_stretches() {
    // Nodes 0..4: red, red, blue, blue, green. The last node's colour is never
    // seen (no stretch follows it); red and blue each fill two stretches.
    let pieces = well_pieces(
        &five(),
        &paint(&[RED, RED, BLUE, BLUE, GREEN], Blend::Net),
        &Joint::default(),
    )
    .unwrap();
    assert_eq!(
        pieces.len(),
        2,
        "two colours, each over two merged stretches: {pieces:?}"
    );
    assert_eq!(
        (pieces[0].colour.as_str(), span_of(&pieces[0])),
        ("#ff0000", (0.0, 200.0))
    );
    assert_eq!(
        (pieces[1].colour.as_str(), span_of(&pieces[1])),
        ("#0000ff", (200.0, 400.0))
    );
    assert!(
        of(&pieces, "#00ff00").is_empty(),
        "the last node's colour is not drawn in the net reading"
    );
    assert!(pieces.iter().all(|p| p.opacity == 1.0));
}

#[test]
fn net_keeps_a_colour_that_comes_back_as_a_piece_of_its_own() {
    let pieces = well_pieces(
        &five(),
        &paint(&[RED, BLUE, RED, BLUE, BLUE], Blend::Net),
        &Joint::default(),
    )
    .unwrap();
    let colours: Vec<&str> = pieces.iter().map(|p| p.colour.as_str()).collect();
    assert_eq!(
        colours,
        ["#ff0000", "#0000ff", "#ff0000", "#0000ff"],
        "stretches apart are not merged"
    );
}

#[test]
fn smooth_runs_from_one_node_to_the_next() {
    // A 2 px stretch is one piece, in the colour at its middle: the mean.
    let p = paint(&["#000000ff", "#fefefeff"], Blend::Smooth);
    let pieces = well_pieces(&along(&[0.0, 2.0]), &p, &Joint::default()).unwrap();
    assert_eq!(pieces.len(), 1);
    assert_eq!(
        pieces[0].colour, "#7f7f7f",
        "the midpoint colour is the mean of the two ends"
    );
}

#[test]
fn smooth_over_a_long_stretch_is_cut_into_pieces_that_climb_evenly() {
    let p = paint(&["#000000ff", "#ff0000ff"], Blend::Smooth);
    let pieces = well_pieces(&along(&[0.0, 100.0]), &p, &Joint::default()).unwrap();
    assert!(
        pieces.len() >= 32,
        "a 100 px stretch looks continuous only in many pieces: {}",
        pieces.len()
    );
    let reds: Vec<i32> = pieces
        .iter()
        .map(|p| i32::from_str_radix(&p.colour[1..3], 16).unwrap())
        .collect();
    assert!(
        reds.windows(2).all(|w| w[0] <= w[1]),
        "red climbs from the first node's to the second's: {reds:?}"
    );
    assert!(
        reds[0] < 16 && *reds.last().unwrap() > 239,
        "it starts at the first node's colour and ends at the second's"
    );
    // The pieces abut: each starts where the previous ended, and together they cover the stretch.
    assert!(
        pieces
            .windows(2)
            .all(|w| w[0].points.last() == w[1].points.first())
    );
    assert_eq!(span_of(&pieces[0]).0, 0.0);
    assert_eq!(span_of(pieces.last().unwrap()).1, 100.0);
    // Symmetric colours around the middle: the mean of mirrored pieces is half.
    let n = reds.len();
    for j in 0..n / 2 {
        assert!(
            (reds[j] + reds[n - 1 - j] - 255).abs() <= 1,
            "piece {j} and its mirror average the two ends"
        );
    }
}

#[test]
fn smooth_does_not_blend_into_an_absent_neighbour() {
    // Node 1 has no value: node 0's stretch toward it is flat red - an
    // interpolated absent colour is a dark fringe, not a value.
    let pieces = well_pieces(
        &along(&[0.0, 100.0, 200.0]),
        &paint(&[RED, NONE, BLUE], Blend::Smooth),
        &Joint::default(),
    )
    .unwrap();
    assert_eq!(pieces.len(), 1, "{pieces:?}");
    assert_eq!(
        (pieces[0].colour.as_str(), span_of(&pieces[0])),
        ("#ff0000", (0.0, 100.0))
    );
}

#[test]
fn a_node_without_a_value_leaves_a_gap_and_never_a_colour() {
    for blend in [Blend::Net, Blend::Smooth] {
        let pieces = well_pieces(
            &five(),
            &paint(&[RED, NONE, BLUE, BLUE, BLUE], blend),
            &Joint::default(),
        )
        .unwrap();
        assert!(
            pieces.iter().all(|p| p.colour != "#123456"),
            "the absent node's rgb is never drawn ({blend:?})"
        );
        // Nothing is drawn between node 1 and node 2.
        let in_gap = pieces.iter().any(|p| {
            let (a, b) = span_of(p);
            a.min(b) < 200.0 && a.max(b) > 100.0
        });
        assert!(!in_gap, "the stretch 100..200 is a gap ({blend:?}): {pieces:?}");
        // What is on both sides is drawn.
        assert_eq!(span_of(&pieces[0]).0, 0.0);
        assert_eq!(span_of(pieces.last().unwrap()).1, 400.0);
    }
}

// --- joints --------------------------------------------------------------------

#[test]
fn joints_at_nodes_mark_every_inner_node_and_never_the_ends() {
    let pieces = well_pieces(
        &five(),
        &paint(&[RED; 5], Blend::Net),
        &joints(JointKind::Nodes, 10.0, 1.0),
    )
    .unwrap();
    let marks = of(&pieces, "#000000");
    assert_eq!(marks.len(), 3, "inner nodes 1, 2, 3 - not the two ends");
    for (mark, node) in marks.iter().zip([100.0, 200.0, 300.0]) {
        assert!(
            (length(mark) - 10.0).abs() < 1e-9,
            "a joint is `px` long: {}",
            length(mark)
        );
        let (a, b) = span_of(mark);
        assert!(
            ((a + b) / 2.0 - node).abs() < 1e-9,
            "it is centred on its node {node}: {a}..{b}"
        );
    }
    // The paint is what is left around them: 400 − 3·10 px, in four stretches.
    let body = of(&pieces, "#ff0000");
    assert_eq!(body.len(), 4);
    assert!((total(&body) - 370.0).abs() < 1e-9);
}

#[test]
fn joints_at_changes_mark_only_where_the_colour_changes() {
    // red red blue blue blue: one change, at node 2.
    let pieces = well_pieces(
        &five(),
        &paint(&[RED, RED, BLUE, BLUE, BLUE], Blend::Net),
        &joints(JointKind::Changes, 8.0, 1.0),
    )
    .unwrap();
    let marks = of(&pieces, "#000000");
    assert_eq!(marks.len(), 1, "one change, one joint: {pieces:?}");
    let (a, b) = span_of(marks[0]);
    assert!(
        (a - 196.0).abs() < 1e-9 && (b - 204.0).abs() < 1e-9,
        "8 px centred on 200: {a}..{b}"
    );
    // A change is also a value appearing after none (alpha counts).
    let pieces = well_pieces(
        &five(),
        &paint(&[RED, RED, NONE, RED, RED], Blend::Net),
        &joints(JointKind::Changes, 8.0, 1.0),
    )
    .unwrap();
    assert_eq!(
        of(&pieces, "#000000").len(),
        2,
        "absent next to a colour is a change, on both sides of node 2"
    );
    // Nowhere when nothing changes.
    let pieces = well_pieces(
        &five(),
        &paint(&[RED; 5], Blend::Net),
        &joints(JointKind::Changes, 8.0, 1.0),
    )
    .unwrap();
    assert!(of(&pieces, "#000000").is_empty());
}

#[test]
fn a_joint_of_alpha_zero_is_a_gap() {
    let pieces = well_pieces(
        &five(),
        &paint(&[RED; 5], Blend::Net),
        &joints(JointKind::Nodes, 10.0, 0.0),
    )
    .unwrap();
    assert!(
        of(&pieces, "#000000").is_empty(),
        "a gap is not the joint's colour"
    );
    assert_eq!(pieces.len(), 4, "the well is cut at each inner node: {pieces:?}");
    for node in [100.0, 200.0, 300.0] {
        let covered = pieces.iter().any(|p| {
            let (a, b) = span_of(p);
            a < node + 5.0 - 1e-9 && b > node - 5.0 + 1e-9
        });
        assert!(!covered, "nothing is drawn within 5 px of node {node}");
    }
    assert!((pieces.iter().map(length).sum::<f64>() - 370.0).abs() < 1e-9);
}

#[test]
fn a_translucent_joint_is_composited_over_the_paint_at_that_point() {
    // red → blue at node 2, a black joint at alpha 0.5: half of it lies over
    // red, half over blue - `rgb + (joint − rgb)·alpha`.
    let pieces = well_pieces(
        &five(),
        &paint(&[RED, RED, BLUE, BLUE, BLUE], Blend::Net),
        &joints(JointKind::Changes, 10.0, 0.5),
    )
    .unwrap();
    let over_red = of(&pieces, "#800000");
    let over_blue = of(&pieces, "#000080");
    assert_eq!((over_red.len(), over_blue.len()), (1, 1), "{pieces:?}");
    assert_eq!(span_of(over_red[0]), (195.0, 200.0));
    assert_eq!(span_of(over_blue[0]), (200.0, 205.0));
}

#[test]
fn a_stretch_shorter_than_two_joints_keeps_its_colour() {
    // The 15 px stretch could hold only 1.5 joints: it would be all joint - a
    // gap that looks like missing data - so it stays plain (`jointsFit`).
    let pieces = well_pieces(
        &along(&[0.0, 15.0, 200.0, 400.0]),
        &paint(&[RED, RED, RED, RED], Blend::Net),
        &joints(JointKind::Nodes, 10.0, 1.0),
    )
    .unwrap();
    let marks = of(&pieces, "#000000");
    // Node 1 lies between the 15 px stretch (no joint) and the 185 px one (its
    // half only); node 2 has both halves.
    let lengths: Vec<f64> = marks.iter().map(|m| length(m)).collect();
    assert_eq!(
        lengths.iter().filter(|l| (**l - 5.0).abs() < 1e-9).count(),
        1,
        "node 1 keeps only the half in the long stretch: {lengths:?}"
    );
    assert!(
        pieces
            .iter()
            .any(|p| span_of(p) == (0.0, 15.0) && p.colour == "#ff0000"),
        "the short stretch is plain red"
    );
}

// --- refusals -------------------------------------------------------------------

#[test]
fn a_paint_of_the_wrong_length_is_refused_by_name() {
    let err = well_pieces(&five(), &paint(&[RED, RED, RED], Blend::Net), &Joint::default()).unwrap_err();
    assert!(
        err.contains("3 node colours") && err.contains("5 nodes"),
        "the counts are named: {err}"
    );
    let err = well_pieces(
        &five(),
        &paint(&[RED, RED, "#nothex", RED, RED], Blend::Net),
        &Joint::default(),
    )
    .unwrap_err();
    assert!(
        err.contains("node 2") && err.contains("#nothex"),
        "the colour that does not read is named: {err}"
    );
    let err = well_pieces(
        &five(),
        &paint(&[RED; 5], Blend::Net),
        &Joint {
            colour: "red".into(),
            kind: JointKind::Nodes,
            ..Joint::default()
        },
    )
    .unwrap_err();
    assert!(err.contains("joint colour"), "{err}");
}

// --- in the layout and the SVG -------------------------------------------------

fn key_of(name: &str) -> String {
    id(name, "WellboreFrameRepresentation").key()
}

fn drawn<'a>(l: &'a Layout, key: &str) -> &'a LaidObject {
    l.layers
        .iter()
        .flat_map(|x| x.objects.iter())
        .find(|o| matches!(&o.attr, Attr::Key(k) if k == key))
        .expect("the well is drawn")
}

fn one_well(w: WellItem) -> Catalogue {
    catalogue(vec![Item::Well(w)])
}

/// The default view under **scene** colouring, unpainted - what a painted view
/// is compared with.
fn scene_view() -> View {
    View {
        well_colouring: WellColouring::Scene,
        ..view()
    }
}

/// A view where the scene paints `key` - under **scene** colouring, the only one
/// a scene's paint is drawn under.
fn painted_view(key: String, nodes: &[&str], blend: Blend) -> View {
    View {
        paints: vec![(key, paint(nodes, blend))],
        well_colouring: WellColouring::Scene,
        ..view()
    }
}

fn px_x(l: &Layout, s: f64) -> f64 {
    l.frame.x + (s - l.window.x[0]) * l.scale[0]
}

#[test]
fn under_scene_colouring_a_painted_well_is_drawn_in_pieces_instead_of_its_line_and_its_strata() {
    let cat = one_well(well("A", 150.0, 12.0, Side::Left));
    // Strata colouring: the unpainted well carries its two strata intervals.
    let plain = layout(
        &cat,
        &View {
            well_colouring: WellColouring::Strata,
            ..view()
        },
    );
    assert!(
        !drawn(&plain, &key_of("A")).intervals.is_empty(),
        "the unpainted well has strata"
    );

    let v = painted_view(key_of("A"), &[RED, BLUE, GREEN], Blend::Net);
    let l = layout(&cat, &v);
    let o = drawn(&l, &key_of("A"));
    // Net over three nodes: red over node 0→1, blue over node 1→2.
    let colours: Vec<&str> = o.intervals.iter().map(|i| i.colour.as_str()).collect();
    assert_eq!(colours, ["#ff0000", "#0000ff"]);
    assert!(
        o.intervals.iter().all(|i| i.unit.is_empty()),
        "no stratum is claimed: the scene's paint is what is shown"
    );
    // Laid on the well: one point per node, in pixels of this drawing.
    let first = &o.intervals[0].points;
    assert!((first[0][0] - px_x(&l, 150.0)).abs() < 1e-9, "starts at node 0");
    let last = o.intervals[1].points.last().unwrap();
    assert!(
        (last[0] - px_x(&l, 200.0)).abs() < 1e-9,
        "ends at node 2 (s = 200)"
    );
    assert_eq!(
        o.intervals[0].points.last(),
        o.intervals[1].points.first(),
        "the two stretches meet at node 1"
    );
    // The well's own line is not drawn under the pieces.
    assert!(
        !o.shapes
            .iter()
            .any(|s| matches!(s, energyml_section::layout::Shape::Polyline { .. }))
    );
    assert!(
        l.warnings.iter().all(|w| !w.contains("scene colours")),
        "{:?}",
        l.warnings
    );
    let text = svg::write(&l);
    assert!(text.contains("stroke=\"#ff0000\"") && text.contains("stroke=\"#0000ff\""));
    assert!(
        !text.contains("Bartonian"),
        "the strata are replaced by the paint"
    );
}

/// Strata colouring is what was asked: a paint the scene gives the well does
/// not replace its units, nor the ticks between them. The black ticks at the
/// stratigraphic changes had vanished and the colour looked uniform along the
/// boreholes - the scene painted the wells, and the paint
/// won over the strata the drawing was set to.
#[test]
fn under_strata_colouring_the_strata_and_their_ticks_win_over_the_scene_paint() {
    let cat = one_well(well("A", 150.0, 12.0, Side::Left));
    let plain = View {
        well_colouring: WellColouring::Strata,
        strata_ticks: true,
        ..view()
    };
    let painted = View {
        paints: vec![(key_of("A"), paint(&[RED, BLUE, GREEN], Blend::Net))],
        ..plain.clone()
    };
    let (without, with) = (layout(&cat, &plain), layout(&cat, &painted));
    let (a, b) = (drawn(&without, &key_of("A")), drawn(&with, &key_of("A")));
    assert!(
        !a.intervals.is_empty() && a.intervals.iter().all(|i| !i.unit.is_empty()),
        "the well has its strata"
    );
    assert_eq!(
        format!("{a:?}"),
        format!("{b:?}"),
        "the paint changes nothing under strata colouring"
    );
    assert!(
        b.intervals.iter().any(|i| i.tick.is_some()),
        "the boundary keeps its tick"
    );
    assert!(
        !svg::write(&with).contains("stroke=\"#ff0000\""),
        "the paint is not drawn"
    );
}

#[test]
fn the_paint_of_a_folded_object_is_used_when_the_kept_line_has_none_and_the_kept_one_wins() {
    let mut w = well("A", 150.0, 12.0, Side::Left);
    w.merged = vec![id("A-trajectory", "WellboreTrajectoryRepresentation")];
    let cat = one_well(w);
    let folded_key = id("A-trajectory", "WellboreTrajectoryRepresentation").key();

    // Only the folded trajectory is painted: its paint is the line's.
    let l = layout(
        &cat,
        &painted_view(folded_key.clone(), &[BLUE, BLUE, BLUE], Blend::Net),
    );
    let colours: Vec<&str> = drawn(&l, &key_of("A"))
        .intervals
        .iter()
        .map(|i| i.colour.as_str())
        .collect();
    assert_eq!(colours, ["#0000ff"]);

    // Both painted: the kept object's (the frame's) wins.
    let v = View {
        paints: vec![
            (folded_key, paint(&[BLUE; 3], Blend::Net)),
            (key_of("A"), paint(&[GREEN; 3], Blend::Net)),
        ],
        ..scene_view()
    };
    let colours: Vec<String> = drawn(&layout(&cat, &v), &key_of("A"))
        .intervals
        .iter()
        .map(|i| i.colour.clone())
        .collect();
    assert_eq!(colours, ["#00ff00"]);
}

#[test]
fn a_paint_that_does_not_fit_is_said_and_the_well_is_drawn_as_before() {
    let cat = one_well(well("A", 150.0, 12.0, Side::Left));
    // The refusal is a note under the plot, which takes a line of the frame's
    // height (as a refused width does): the drawings are compared without notes.
    let quiet = |mut v: View| {
        v.show.notes = false;
        v
    };
    let plain = layout(&cat, &quiet(scene_view()));
    let refused = layout(&cat, &quiet(painted_view(key_of("A"), &[RED, BLUE], Blend::Net)));
    assert!(
        refused
            .warnings
            .iter()
            .any(|w| w.contains("scene colours refused")
                && w.contains("2 node colours")
                && w.contains("3 nodes")),
        "the refusal names the counts: {:?}",
        refused.warnings
    );
    assert_eq!(
        refused.layers, plain.layers,
        "every shape of the drawing is the unpainted one"
    );
    // A colour that does not read is refused the same way.
    let refused = layout(
        &cat,
        &quiet(painted_view(key_of("A"), &[RED, "#zz", BLUE], Blend::Net)),
    );
    assert!(
        refused
            .warnings
            .iter()
            .any(|w| w.contains("scene colours refused") && w.contains("#zz")),
        "{:?}",
        refused.warnings
    );
    assert_eq!(refused.layers, plain.layers);
    // With the notes on, it is written under the drawing.
    let noted = layout(&cat, &painted_view(key_of("A"), &[RED, BLUE], Blend::Net));
    assert!(
        noted
            .written_warnings()
            .iter()
            .any(|w| w.contains("scene colours refused"))
    );
}

#[test]
fn without_a_paint_for_the_object_nothing_changes() {
    let cat = catalogue(vec![
        Item::Well(well("A", 150.0, 12.0, Side::Left)),
        Item::Well(well("B", 700.0, 30.0, Side::Right)),
    ]);
    let base = layout(&cat, &view());
    // Joints asked, and a paint given - for an object that is not in the drawing.
    let v = View {
        paints: vec![(key_of("elsewhere"), paint(&[RED; 3], Blend::Smooth))],
        joints: joints(JointKind::Nodes, 6.0, 1.0),
        ..view()
    };
    let other = layout(&cat, &v);
    assert_eq!(
        other.layers, base.layers,
        "no paint for a drawn object: the layers are those of the unpainted view"
    );
    assert_eq!(other.warnings, base.warnings);
    // The SVG differs only by what it says of the view (its metadata) - remove
    // that, and the drawings are the same bytes.
    let strip = |text: String| {
        let (a, rest) = text.split_once("<metadata>").unwrap();
        format!("{a}{}", rest.split_once("</metadata>").unwrap().1)
    };
    assert_eq!(strip(svg::write(&other)), strip(svg::write(&base)));
    // And a view with the new fields at their defaults is the plain drawing.
    assert_eq!(
        svg::write(&layout(
            &cat,
            &View {
                paints: Vec::new(),
                joints: Joint::default(),
                ..view()
            }
        )),
        svg::write(&base)
    );
}

#[test]
fn painting_one_well_leaves_the_others_as_they_were() {
    let cat = catalogue(vec![
        Item::Well(well("A", 150.0, 12.0, Side::Left)),
        Item::Well(well("B", 700.0, 30.0, Side::Right)),
    ]);
    let base = layout(&cat, &scene_view());
    let l = layout(&cat, &painted_view(key_of("A"), &[RED, BLUE, GREEN], Blend::Net));
    assert_ne!(drawn(&l, &key_of("A")), drawn(&base, &key_of("A")));
    assert_eq!(drawn(&l, &key_of("B")), drawn(&base, &key_of("B")));
}

// --- A painted surface, end to end (integration, 05/10) -----------------------
//
// The two halves were written apart: `paint::surfaces::trace_pieces` and the
// call in `layout`, behind a shim that answered `None` until the catalogue's
// item carried the cut's sources. These hold the seam: `assemble` copies the
// sources, `layout` lays the paint along them, and a cut without sources stays
// flat.

fn painted_cut() -> TraceItem {
    let mut item = crate::layout::trace("H", vec![vec![[0.0, -100.0], [500.0, -100.0], [1000.0, -100.0]]]);
    // Node 0, node 1, node 2 of the surface, each point exactly on a node.
    item.sources = vec![vec![
        CutSource { a: 0, b: 1, t: 0.0 },
        CutSource { a: 1, b: 2, t: 0.0 },
        CutSource { a: 1, b: 2, t: 1.0 },
    ]];
    item
}

#[test]
fn a_painted_surface_is_laid_along_its_cut_and_without_sources_stays_flat() {
    let key = id("H", "TriangulatedSetRepresentation").key();
    let v = View {
        paints: vec![(key.clone(), paint(&[RED, BLUE, GREEN], Blend::Net))],
        ..view()
    };
    let text = svg::write(&layout(&catalogue(vec![Item::Trace(painted_cut())]), &v));
    for colour in ["#ff0000", "#0000ff", "#00ff00"] {
        assert!(
            text.contains(&format!("stroke=\"{colour}\"")),
            "{colour} drawn along the cut"
        );
    }
    // The same cut, its sources lost: nothing to lay the paint on.
    let flat = TraceItem {
        sources: Vec::new(),
        ..painted_cut()
    };
    let text = svg::write(&layout(&catalogue(vec![Item::Trace(flat)]), &v));
    assert!(!text.contains("stroke=\"#00ff00\""), "no paint without sources");
}

#[test]
fn assemble_carries_a_cut_s_sources_onto_the_catalogue_item() {
    use energyml_section::assemble::assemble;
    let sources = painted_cut().sources;
    let surface = SurfaceRead {
        object: id("H", "TriangulatedSetRepresentation"),
        role: TraceRole::Horizon,
        time: false,
        colour: None,
        age: None,
        feature: None,
        cuts: vec![SurfaceCut {
            section: SectionKey::Long,
            shape: CutShape::Trace {
                spans: painted_cut().spans,
            },
            notes: Vec::new(),
            sources: sources.clone(),
        }],
    };
    let inputs = Inputs {
        trace: vec![[0.0, 0.0], [1000.0, 0.0]],
        system: None,
        left: 20.0,
        right: 20.0,
        crosses: Crosses::None,
        objects: Vec::new(),
        time_factor: None,
    };
    let c = Contribution {
        store: "s1".into(),
        surfaces: vec![surface],
        ..Contribution::default()
    };
    let cat = assemble(&inputs, &[c]).expect("assembles");
    let item = cat
        .long
        .items
        .iter()
        .find_map(|i| if let Item::Trace(t) = i { Some(t) } else { None })
        .expect("a trace");
    assert_eq!(item.sources, sources);
}

// --- joints at changes on a finely sampled well (05/10) ------------------------
//
// In "at value change" mode the computation must be done between the changes,
// not between every available MD.
// A finely sampled well has a node every metre: at 3.9 px/m every stretch is under two
// joints, and the old rule drew no joint at all, the change of unit included.

#[test]
fn at_changes_a_joint_is_judged_on_the_run_of_one_colour_not_on_the_stretch() {
    // Nodes every 5 px: ten red stretches, then ten blue. A joint of 3 px needs
    // 6 px - no stretch holds it, each run of one colour (50 px) does.
    let xs: Vec<f64> = (0..21).map(|k| 5.0 * k as f64).collect();
    let colours: Vec<&str> = (0..21).map(|k| if k < 10 { RED } else { BLUE }).collect();
    let pieces = well_pieces(
        &along(&xs),
        &paint(&colours, Blend::Net),
        &joints(JointKind::Changes, 3.0, 1.0),
    )
    .unwrap();
    let marks = of(&pieces, "#000000");
    // One joint, both halves, centred on node 10 (x = 50): 48.5 → 51.5.
    assert_eq!(total(&marks), 3.0, "{marks:?}");
    assert!(
        marks.iter().all(|m| {
            let (a, b) = span_of(m);
            a >= 48.5 - 1e-9 && b <= 51.5 + 1e-9
        }),
        "{marks:?}"
    );
    // The same well, joints at every node: every stretch is too short, none drawn.
    let pieces = well_pieces(
        &along(&xs),
        &paint(&colours, Blend::Net),
        &joints(JointKind::Nodes, 3.0, 1.0),
    )
    .unwrap();
    assert!(
        of(&pieces, "#000000").is_empty(),
        "at nodes the stretch still decides"
    );
}

#[test]
fn at_changes_a_run_shorter_than_two_joints_keeps_its_side_plain() {
    // Red over two 2.5 px stretches (a 5 px run), then blue over stretches of
    // 4 px (a 100 px run): the change at x = 5 has only its blue half - the red
    // run cannot hold two joints of 3 px, the blue one can, though none of its
    // stretches could (the old rule drew nothing here).
    let mut xs = vec![0.0, 2.5, 5.0];
    xs.extend((1..=25).map(|k| 5.0 + 4.0 * k as f64));
    let mut colours = vec![RED, RED];
    colours.extend(std::iter::repeat_n(BLUE, xs.len() - 2));
    let pieces = well_pieces(
        &along(&xs),
        &paint(&colours, Blend::Net),
        &joints(JointKind::Changes, 3.0, 1.0),
    )
    .unwrap();
    let marks = of(&pieces, "#000000");
    assert_eq!(total(&marks), 1.5, "{marks:?}");
    assert!(
        marks.iter().all(|m| span_of(m).0 >= 5.0 - 1e-9),
        "only the half after the change: {marks:?}"
    );
}
