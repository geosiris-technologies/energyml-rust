//! The judgement of a selection by rings, on fabricated footprints - no package.
//!
//! The rule under test is the one in `energyml_build::selection`'s module doc:
//! the region is inside one `outer` (none: everywhere) and inside no `inner`;
//! the border counts as inside; `Inside` is judged on nodes, `Touches` also on
//! edges, `Head` on a well's first node.

use energyml_build::selection::{Footprint, Request, Rings, Selected, Test, judge};

fn square(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<[f64; 2]> {
    vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
}

fn outer(ring: Vec<[f64; 2]>) -> Rings {
    Rings {
        epsg: None,
        outer: vec![ring],
        inner: vec![],
    }
}

fn line(nodes: Vec<[f64; 2]>) -> Footprint {
    let edges = (1..nodes.len() as u32).map(|at| [at - 1, at]).collect();
    Footprint {
        nodes,
        edges,
        head: None,
    }
}

fn holds(footprint: &Footprint, rings: &Rings, test: Test) -> bool {
    judge(footprint, rings, test).expect("judged").holds
}

#[test]
fn inside_wants_every_node_and_touches_wants_one() {
    let rings = outer(square(0.0, 0.0, 10.0, 10.0));
    let half = line(vec![[5.0, 5.0], [20.0, 5.0]]);
    assert!(!holds(&half, &rings, Test::Inside));
    assert!(holds(&half, &rings, Test::Touches));
    let within = line(vec![[1.0, 1.0], [9.0, 9.0]]);
    assert!(holds(&within, &rings, Test::Inside));
    let apart = line(vec![[20.0, 20.0], [30.0, 30.0]]);
    assert!(!holds(&apart, &rings, Test::Touches));
}

#[test]
fn an_edge_that_crosses_with_no_node_inside_touches_but_is_not_inside() {
    let rings = outer(square(0.0, 0.0, 10.0, 10.0));
    let across = line(vec![[-5.0, 5.0], [15.0, 5.0]]);
    assert!(holds(&across, &rings, Test::Touches));
    assert!(!holds(&across, &rings, Test::Inside));
    // The same shape a hair above the ring: nothing touches.
    let above = line(vec![[-5.0, 10.5], [15.0, 10.5]]);
    assert!(!holds(&above, &rings, Test::Touches));
}

#[test]
fn a_triangle_side_counts_as_an_edge() {
    let rings = outer(square(0.0, 0.0, 10.0, 10.0));
    // A big triangle whose corners are all outside and which swallows the ring.
    let nodes = vec![[-50.0, -50.0], [50.0, -50.0], [0.0, 50.0]];
    // Its sides pass through the ring's corner region only for the edges that
    // cross it: the left side runs from (0, 50) to (-50, -50), far away, and
    // the right side from (50, -50) to (0, 50) passes x = 10 near y = 30: none
    // of the sides crosses, the ring is inside the triangle. It touches nothing.
    let far = Footprint {
        nodes,
        edges: vec![[0, 1], [1, 2], [2, 0]],
        head: None,
    };
    assert!(!holds(&far, &rings, Test::Touches));
    let nodes = vec![[-5.0, -5.0], [-5.0, 5.0], [-20.0, 5.0]];
    let side = Footprint {
        nodes,
        edges: vec![[0, 1], [1, 2], [2, 0]],
        head: None,
    };
    assert!(!holds(&side, &rings, Test::Touches));
    let nodes = vec![[-5.0, 5.0], [15.0, 5.0], [15.0, 50.0]];
    let crossing = Footprint {
        nodes,
        edges: vec![[0, 1], [1, 2], [2, 0]],
        head: None,
    };
    assert!(holds(&crossing, &rings, Test::Touches));
}

#[test]
fn an_inner_ring_makes_a_hole_in_the_outer() {
    let rings = Rings {
        epsg: None,
        outer: vec![square(0.0, 0.0, 10.0, 10.0)],
        inner: vec![square(4.0, 4.0, 6.0, 6.0)],
    };
    assert!(!holds(&line(vec![[5.0, 5.0]]), &rings, Test::Touches));
    assert!(holds(&line(vec![[1.0, 1.0]]), &rings, Test::Touches));
    // An edge entirely inside the hole does not touch; one that leaves it does.
    assert!(!holds(&line(vec![[4.5, 5.0], [5.5, 5.0]]), &rings, Test::Touches));
    assert!(holds(&line(vec![[5.0, 5.0], [5.0, 8.0]]), &rings, Test::Touches));
    // Two nodes in the region and the edge between them through the hole: inside
    // is judged on nodes.
    assert!(holds(&line(vec![[1.0, 5.0], [9.0, 5.0]]), &rings, Test::Inside));
}

#[test]
fn no_outer_ring_means_everywhere_but_the_inner_rings() {
    let rings = Rings {
        epsg: None,
        outer: vec![],
        inner: vec![square(0.0, 0.0, 10.0, 10.0)],
    };
    assert!(holds(&line(vec![[50.0, 50.0]]), &rings, Test::Inside));
    assert!(!holds(&line(vec![[5.0, 5.0]]), &rings, Test::Inside));
    let nothing = Rings::default();
    assert!(holds(&line(vec![[1e9, -1e9]]), &nothing, Test::Inside));
}

#[test]
fn a_node_on_the_border_counts_inside() {
    let rings = outer(square(0.0, 0.0, 10.0, 10.0));
    assert!(holds(&line(vec![[10.0, 5.0]]), &rings, Test::Inside));
    assert!(holds(&line(vec![[0.0, 0.0]]), &rings, Test::Inside));
}

#[test]
fn several_outer_rings_are_a_union() {
    let rings = Rings {
        epsg: None,
        outer: vec![square(0.0, 0.0, 1.0, 1.0), square(5.0, 5.0, 6.0, 6.0)],
        inner: vec![],
    };
    assert!(holds(&line(vec![[0.5, 0.5], [5.5, 5.5]]), &rings, Test::Inside));
    assert!(!holds(&line(vec![[3.0, 3.0]]), &rings, Test::Touches));
}

#[test]
fn head_is_the_first_node_of_a_well_and_touches_for_anything_else() {
    let rings = outer(square(0.0, 0.0, 10.0, 10.0));
    // The head is outside; the rest of the well runs through the region.
    let mut well = line(vec![[-5.0, 5.0], [5.0, 5.0], [8.0, 5.0]]);
    well.head = Some([-5.0, 5.0]);
    let judged = judge(&well, &rings, Test::Head).unwrap();
    assert!(!judged.holds && !judged.substituted);
    assert!(holds(&well, &rings, Test::Touches));
    // Head inside, the body out.
    let mut well = line(vec![[5.0, 5.0], [50.0, 5.0]]);
    well.head = Some([5.0, 5.0]);
    assert!(holds(&well, &rings, Test::Head));
    // Not a well: the same line judged as touches, and it says so.
    let surface = line(vec![[-5.0, 5.0], [5.0, 5.0]]);
    let judged = judge(&surface, &rings, Test::Head).unwrap();
    assert!(judged.holds && judged.substituted);
}

#[test]
fn a_footprint_with_no_node_is_refused_not_inside() {
    let rings = outer(square(0.0, 0.0, 10.0, 10.0));
    let why = judge(&Footprint::default(), &rings, Test::Inside).unwrap_err();
    assert!(why.contains("no node"), "{why}");
}

#[test]
fn rings_that_enclose_nothing_are_refused() {
    let rings = Rings {
        epsg: None,
        outer: vec![vec![[0.0, 0.0], [1.0, 1.0]]],
        inner: vec![],
    };
    assert!(rings.check().unwrap_err().contains("outer ring #0"));
    let rings = Rings {
        epsg: None,
        outer: vec![vec![[0.0, 0.0], [1.0, f64::NAN], [1.0, 0.0]]],
        inner: vec![],
    };
    assert!(rings.check().is_err());
}

#[test]
fn the_wire_is_camel_case_and_the_defaults_are_the_empty_ones() {
    let request: Request = serde_json::from_str(
        r#"{"uris":["a"],"rings":{"epsg":2154,"outer":[[[0,0],[1,0],[1,1]]]},"test":"touches"}"#,
    )
    .expect("the front end's request reads");
    assert_eq!(request.rings.epsg, Some(2154));
    assert!(request.rings.inner.is_empty() && !request.invert);
    assert_eq!(request.test, Test::Touches);
    let answer = serde_json::to_value(Selected::default()).unwrap();
    assert_eq!(
        answer,
        serde_json::json!({"kept": [], "dropped": [], "refused": [], "notes": []})
    );
}
