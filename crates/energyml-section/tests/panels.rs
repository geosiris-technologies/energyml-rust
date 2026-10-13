//! The 3D panels are the drawing's fills put back on the trace (ADR-0015) — hand-built catalogues, no package is read.

use energyml_geometry::fence::point_at;
use energyml_section::layout::{Shape, layout};
use energyml_section::model::*;
use energyml_section::panels::panels;

use crate::layout::{catalogue, id, trace, view};

fn key(n: &str) -> String {
    id(n, "TriangulatedSetRepresentation").key()
}

/// H at -100 and L at -300, both with a vertex at 600 m, on a trace that bends
/// there: east for 600 m, then north for 400.
fn bent() -> Catalogue {
    let mut cat = catalogue(vec![
        Item::Trace(trace("H", vec![vec![[0.0, -100.0], [600.0, -100.0], [1000.0, -100.0]]])),
        Item::Trace(trace("L", vec![vec![[0.0, -300.0], [600.0, -300.0], [1000.0, -300.0]]])),
    ]);
    cat.long.trace = vec![[0.0, 0.0], [600.0, 0.0], [600.0, 400.0]];
    cat
}

fn fill_down(n: &str) -> FillChoice {
    FillChoice { key: key(n), up: false, down: true }
}

#[test]
fn a_vertex_of_the_drawing_is_a_vertex_of_the_panel_on_the_trace() {
    let cat = bent();
    let mut v = view();
    v.fills = vec![fill_down("H")];

    // The drawing's polygon, its pixels taken back to (s, z).
    let l = layout(&cat, &v);
    let mut drawn: Vec<[f64; 2]> = Vec::new();
    for object in &l.layer("fills").unwrap().objects {
        for shape in &object.shapes {
            if let Shape::Polygon { points, .. } = shape {
                for p in points {
                    drawn.push([
                        l.window.x[0] + (p[0] - l.frame.x) / l.scale[0],
                        l.window.y[0] + (l.frame.y + l.frame.h - p[1]) / l.scale[1],
                    ]);
                }
            }
        }
    }
    assert!(!drawn.is_empty(), "the control: the drawing fills H down to L");

    let made = panels(&cat, &v);
    assert_eq!(made.len(), 1);
    let panel = &made[0];
    assert_eq!((panel.section, panel.key.as_str()), (SectionKey::Long, key("H").as_str()));
    let vertices: Vec<[f64; 3]> = panel.polygons.iter().flatten().copied().collect();
    assert_eq!(vertices.len(), drawn.len(), "the same vertices, none added or lost");
    for [s, z] in drawn {
        let [x, y] = point_at(&cat.long.trace, s);
        assert!(
            vertices.iter().any(|p| (p[0] - x).abs() < 1e-6 && (p[1] - y).abs() < 1e-6 && (p[2] - z).abs() < 1e-6),
            "(s = {s}, z = {z}) should be ({x}, {y}, {z}) in the panel: {vertices:?}"
        );
    }
    // The bend is really used: some vertex is north of the first leg.
    assert!(vertices.iter().any(|p| p[1] > 1.0 && (p[0] - 600.0).abs() < 1e-6));
}

#[test]
fn no_fill_chosen_no_panel() {
    let cat = bent();
    assert!(panels(&cat, &view()).is_empty());
}

#[test]
fn a_hidden_trace_fills_nothing_and_is_nobodys_neighbour() {
    let cat = bent();
    let mut v = view();
    v.fills = vec![fill_down("H")];
    assert_eq!(panels(&cat, &v).len(), 1, "the control");
    v.hidden = vec![key("L")];
    assert!(panels(&cat, &v).is_empty(), "H has no line below");
    v.hidden = vec![key("H")];
    assert!(panels(&cat, &v).is_empty(), "H is hidden");
}

#[test]
fn the_colour_is_the_drawings_the_scene_first() {
    let cat = bent();
    let mut v = view();
    v.fills = vec![fill_down("H")];
    let derived = panels(&cat, &v)[0].colour.clone();
    v.colours = vec![(key("H"), "#123456".into())];
    assert_ne!(derived, "#123456");
    assert_eq!(panels(&cat, &v)[0].colour, "#123456");
}

/// The panels are the drawing's fills, so they obey the same rule: a fault is
/// neither a neighbour nor filled, two versions of one feature are not each
/// other's neighbour, and the neighbour changes along the trace.
#[test]
fn the_panels_follow_the_same_rule_as_the_drawing() {
    use crate::layout::surface;
    let cat = catalogue(vec![
        surface("H", TraceRole::Horizon, Some("fh"), vec![vec![[0.0, -100.0], [1000.0, -100.0]]]),
        surface("H2", TraceRole::Horizon, Some("fh"), vec![vec![[0.0, -100.01], [1000.0, -100.01]]]),
        surface("F", TraceRole::Fault, Some("ff"), vec![vec![[200.0, -50.0], [600.0, -350.0]]]),
        surface("A", TraceRole::Horizon, Some("fa"), vec![vec![[0.0, -200.0], [600.0, -200.0]]]),
        surface("B", TraceRole::Horizon, Some("fb"), vec![vec![[0.0, -300.0], [1000.0, -300.0]]]),
    ]);
    let mut v = view();
    v.fills = vec![fill_down("H"), FillChoice { key: key("F"), up: true, down: true }];
    let made = panels(&cat, &v);
    assert_eq!(made.len(), 1, "the fault has no panel: {made:?}");
    let panel = &made[0];
    assert_eq!(panel.key, key("H"));
    assert_eq!(panel.sz.len(), 2, "A, then B: {:?}", panel.sz);
    let floor = |poly: &Vec<[f64; 2]>| poly.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
    let mut floors: Vec<f64> = panel.sz.iter().map(floor).collect();
    floors.sort_by(f64::total_cmp);
    assert_eq!(floors, vec![-300.0, -200.0], "{:?}", panel.sz);
    // And they are the drawing's polygons, vertex for vertex.
    let drawn: usize = layout(&cat, &v).layer("fills").unwrap().objects.iter().map(|o| o.shapes.len()).sum();
    assert_eq!(drawn, panel.sz.len());
}

/// The panel says its polygons in the section's (s, z) as well, vertex for
/// vertex: where they are flat, so where the front end triangulates them.
#[test]
fn a_panel_carries_its_polygons_in_the_sections_own_s_z() {
    let cat = bent();
    let mut v = view();
    v.fills = vec![fill_down("H")];
    let made = panels(&cat, &v);
    let panel = &made[0];
    assert_eq!(panel.sz.len(), panel.polygons.len());
    for (flat, placed) in panel.sz.iter().zip(&panel.polygons) {
        assert_eq!(flat.len(), placed.len());
        for (&[s, z], &[x, y, pz]) in flat.iter().zip(placed) {
            let [px, py] = point_at(&cat.long.trace, s);
            assert!((px - x).abs() < 1e-9 && (py - y).abs() < 1e-9 && (pz - z).abs() < 1e-9);
        }
    }
}

