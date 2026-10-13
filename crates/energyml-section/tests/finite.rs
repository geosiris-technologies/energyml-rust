//! Nothing that is not a number leaves the reader (`read::finite`).
//!
//! On a large real-world model: *"the contributions do not
//! read: invalid type: null, expected f64 at line 1 column 2496480"*. A `NaN`
//! is written `null` by `serde_json`, and the catalogue refused the whole
//! contribution for it. Every well, interval and cut here is built by hand;
//! what is asserted is the rule of `finite.rs`, not a measured package.

use energyml_section::assemble::assemble;
use energyml_section::model::{
    Contribution, Crosses, CutShape, Inputs, Interval, ObjectId, Outcome, SectionKey, SurfaceCut,
    SurfaceRead, TraceRole, WellKind, WellRead,
};
use energyml_section::read::finite::{NOT_A_NUMBER, finite_surface, finite_well};

const NAN: f64 = f64::NAN;

fn oid(uuid: &str, title: &str, qualified_type: &str) -> ObjectId {
    ObjectId {
        store: "s1".to_string(),
        uri: format!("eml:///{qualified_type}({uuid})"),
        uuid: uuid.to_string(),
        title: title.to_string(),
        qualified_type: qualified_type.to_string(),
    }
}

/// A vertical well at `(50, 1)`, nodes every 10 m from the head down.
fn well(title: &str, head: [f64; 3], path: Vec<[f64; 3]>, md: Option<Vec<f64>>) -> WellRead {
    WellRead {
        object: oid(title, title, "resqml22.WellboreTrajectoryRepresentation"),
        kind: WellKind::Trajectory,
        wellbore: None,
        head,
        path,
        md,
        intervals: Vec::new(),
        strata: None,
        borrowed: false,
        head_only: false,
    }
}

fn interval(from: f64, to: f64, unit: &str) -> Interval {
    Interval { from, to, colour: None, unit: unit.to_string(), unit_uri: None }
}

fn surface(cuts: Vec<SurfaceCut>, age: Option<[f64; 2]>) -> SurfaceRead {
    SurfaceRead {
        object: oid("h1", "H1", "resqml22.TriangulatedSetRepresentation"),
        role: TraceRole::Horizon,
        time: false,
        colour: None,
        age,
        feature: None,
        cuts,
    }
}

#[test]
fn a_well_whose_head_is_not_a_number_is_refused_by_name() {
    // To make it fail: let a non-finite head through → the head is written as `null`.
    let w = well("No elevation", [50.0, 1.0, NAN], vec![[50.0, 1.0, NAN], [50.0, 1.0, NAN]], None);
    let refused = finite_well(w).expect_err("a head that is not a number cannot be placed");
    assert_eq!(refused.object.title, "No elevation");
    assert!(refused.reason.contains("`No elevation`"), "{}", refused.reason);
    assert!(refused.reason.contains("head"), "{}", refused.reason);
}

#[test]
fn a_node_that_is_not_a_number_is_dropped_with_its_md_and_said() {
    // To make it fail: keep the node, or do not remove its MD → `path` and `md`
    // no longer have the same length, or a `null` is written.
    let w = well(
        "W1",
        [50.0, 1.0, 0.0],
        vec![[50.0, 1.0, 0.0], [50.0, 1.0, NAN], [50.0, 1.0, -20.0], [50.0, 1.0, -30.0]],
        Some(vec![0.0, 10.0, 20.0, 30.0]),
    );
    let (kept, note) = finite_well(w).expect("three nodes are left");
    assert_eq!(kept.path, vec![[50.0, 1.0, 0.0], [50.0, 1.0, -20.0], [50.0, 1.0, -30.0]]);
    assert_eq!(kept.md, Some(vec![0.0, 20.0, 30.0]));
    let note = note.expect("a dropped node is said");
    assert_eq!(note.code, NOT_A_NUMBER);
    assert_eq!(note.objects.len(), 1);
    assert_eq!(note.objects[0].title, "W1");
    assert!(note.said.contains("1 of its 4 nodes"), "{}", note.said);
}

#[test]
fn an_md_that_is_not_a_number_drops_its_node_too() {
    // To make it fail: look at the coordinates only → the `NaN` MD is written as `null`.
    let w = well(
        "W2",
        [50.0, 1.0, 0.0],
        vec![[50.0, 1.0, 0.0], [50.0, 1.0, -10.0], [50.0, 1.0, -20.0]],
        Some(vec![0.0, NAN, 20.0]),
    );
    let (kept, note) = finite_well(w).expect("two nodes are left");
    assert_eq!(kept.path.len(), 2);
    assert_eq!(kept.md, Some(vec![0.0, 20.0]));
    assert!(note.is_some());
}

#[test]
fn a_well_left_with_fewer_than_two_nodes_is_refused() {
    // To make it fail: keep a well of a single node → a zero-length stroke,
    // drawn as a well.
    let w = well(
        "W3",
        [50.0, 1.0, 0.0],
        vec![[50.0, 1.0, 0.0], [50.0, 1.0, NAN], [NAN, 1.0, -20.0]],
        None,
    );
    let refused = finite_well(w).expect_err("one node is not a well");
    assert!(refused.reason.contains("2 of its 3 nodes"), "{}", refused.reason);
}

#[test]
fn an_interval_without_a_top_or_base_is_not_drawn_and_the_well_stays() {
    // To make it fail: keep the interval → `from: null`; or refuse the well on its
    // account → a borehole disappears for a unit with no depth.
    let mut w = well("W4", [50.0, 1.0, 0.0], vec![[50.0, 1.0, 0.0], [50.0, 1.0, -30.0]], Some(vec![0.0, 30.0]));
    w.intervals = vec![interval(0.0, 10.0, "A"), interval(NAN, 20.0, "B"), interval(20.0, NAN, "C")];
    let (kept, note) = finite_well(w).expect("the well is drawn");
    assert_eq!(kept.intervals.iter().map(|i| i.unit.as_str()).collect::<Vec<_>>(), vec!["A"]);
    assert!(note.expect("said").said.contains("2 of its 3 intervals"));
}

#[test]
fn a_well_with_nothing_wrong_comes_back_as_it_was_and_says_nothing() {
    // The negative control: a pass that rewrote a clean well, or noted it,
    // would make every package noisy.
    let mut w = well("W5", [50.0, 1.0, 0.0], vec![[50.0, 1.0, 0.0], [50.0, 1.0, -30.0]], Some(vec![0.0, 30.0]));
    w.intervals = vec![interval(0.0, 30.0, "A")];
    let (kept, note) = finite_well(w.clone()).expect("clean");
    assert_eq!(kept, w);
    assert!(note.is_none());
}

#[test]
fn a_cut_is_broken_where_a_point_is_not_a_number_never_joined_across() {
    // To make it fail: remove the point without cutting → a segment from (1, -1) to
    // (3, -3) across the hole; or keep the one-point piece.
    let cut = SurfaceCut {
        section: SectionKey::Long,
        shape: CutShape::Trace {
            spans: vec![
                vec![[0.0, 0.0], [1.0, -1.0], [2.0, NAN], [3.0, -3.0], [4.0, -4.0], [NAN, -5.0], [6.0, -6.0]],
                vec![[10.0, 0.0], [11.0, 0.0]],
            ],
        },
        notes: Vec::new(),
        sources: Vec::new(),
    };
    let mut s = surface(vec![cut], None);
    finite_surface(&mut s);
    let CutShape::Trace { spans } = &s.cuts[0].shape else { panic!("a trace stays a trace") };
    assert_eq!(
        spans,
        &vec![
            vec![[0.0, 0.0], [1.0, -1.0]],
            vec![[3.0, -3.0], [4.0, -4.0]],
            vec![[10.0, 0.0], [11.0, 0.0]],
        ]
    );
    assert_eq!(s.cuts[0].notes.len(), 1);
    assert!(s.cuts[0].notes[0].starts_with("2 point(s)"), "{}", s.cuts[0].notes[0]);
}

#[test]
fn an_envelope_drops_a_fill_holding_a_hole_and_keeps_the_others() {
    let ring = vec![[0.0, 0.0], [1.0, 0.0], [1.0, -1.0], [0.0, 0.0]];
    let holed = vec![[2.0, 0.0], [3.0, NAN], [3.0, -1.0], [2.0, 0.0]];
    let cut = SurfaceCut {
        section: SectionKey::Cross { index: 0 },
        shape: CutShape::Envelope {
            top: vec![vec![[0.0, 0.0], [3.0, 0.0]]],
            bottom: vec![vec![[0.0, -1.0], [NAN, -1.0], [2.0, -1.0], [3.0, -1.0]]],
            fills: vec![ring.clone(), holed],
            faults: Vec::new(),
        },
        notes: Vec::new(),
        sources: Vec::new(),
    };
    let mut s = surface(vec![cut], None);
    finite_surface(&mut s);
    let CutShape::Envelope { top, bottom, fills, .. } = &s.cuts[0].shape else { panic!("an envelope stays one") };
    assert_eq!(top.len(), 1);
    assert_eq!(bottom, &vec![vec![[2.0, -1.0], [3.0, -1.0]]]);
    assert_eq!(fills, &vec![ring]);
    assert!(s.cuts[0].notes[0].starts_with("2 point(s)"), "{}", s.cuts[0].notes[0]);
}

#[test]
fn an_age_that_is_not_a_number_is_no_age() {
    let mut s = surface(Vec::new(), Some([NAN, 10.0]));
    finite_surface(&mut s);
    assert_eq!(s.age, None);
    let mut dated = surface(Vec::new(), Some([20.0, 10.0]));
    finite_surface(&mut dated);
    assert_eq!(dated.age, Some([20.0, 10.0]));
}

/// The failure itself: a contribution holding a `NaN` does not read back, and
/// one passed through `finite` does — the same `serde_json` both ways, the
/// road between a session's contribution and the catalogue.
#[test]
fn a_contribution_passed_through_finite_reads_back() {
    let raw = well(
        "W6",
        [50.0, 1.0, 0.0],
        vec![[50.0, 1.0, 0.0], [50.0, 1.0, NAN], [50.0, 1.0, -20.0]],
        None,
    );
    let mut s = surface(
        vec![SurfaceCut {
            section: SectionKey::Long,
            shape: CutShape::Trace { spans: vec![vec![[0.0, 0.0], [1.0, NAN], [2.0, -2.0], [3.0, -3.0]]] },
            notes: Vec::new(),
            sources: Vec::new(),
        }],
        Some([NAN, 1.0]),
    );

    // The bug, reproduced: the message a user read, word for word.
    let broken = Contribution { store: "s1".into(), wells: vec![raw.clone()], surfaces: vec![s.clone()], ..Contribution::default() };
    let json = serde_json::to_string(&broken).expect("serde_json writes NaN as null");
    let why = serde_json::from_str::<Contribution>(&json).expect_err("null is not an f64");
    assert!(why.to_string().contains("invalid type: null, expected f64"), "{why}");

    let (kept, note) = finite_well(raw).expect("two nodes are left");
    finite_surface(&mut s);
    let fixed = Contribution {
        store: "s1".into(),
        wells: vec![kept],
        surfaces: vec![s],
        notes: note.into_iter().collect(),
        ..Contribution::default()
    };
    let json = serde_json::to_string(&fixed).expect("writes");
    let back: Contribution = serde_json::from_str(&json).expect("reads back");
    assert_eq!(back.notes.len(), 1);
}

/// A well kept with a note is **one** row, drawn — not a drawn row and a
/// refused row for the same object — and its note reaches the catalogue.
#[test]
fn a_kept_well_is_drawn_and_its_note_reaches_the_catalogue() {
    // To make it fail: pass the note through `refused` (the first draft) → two rows
    // for W7, one of them "refused".
    let raw = well(
        "W7",
        [50.0, 1.0, 0.0],
        vec![[50.0, 1.0, 0.0], [50.0, 1.0, NAN], [50.0, 1.0, -20.0]],
        None,
    );
    let (kept, note) = finite_well(raw).expect("kept");
    let inputs = Inputs {
        trace: vec![[0.0, 0.0], [100.0, 0.0]],
        system: None,
        left: 20.0,
        right: 20.0,
        crosses: Crosses::None,
        objects: Vec::new(),
        time_factor: None,
    };
    let c = Contribution { store: "s1".into(), wells: vec![kept], notes: note.into_iter().collect(), ..Contribution::default() };
    let cat = assemble(&inputs, &[c]).expect("assembles");
    let rows: Vec<_> = cat.objects.iter().filter(|r| r.object.title == "W7").collect();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert!(matches!(rows[0].outcome, Outcome::Drawn { .. }), "{:?}", rows[0].outcome);
    assert!(cat.notes.iter().any(|n| n.code == NOT_A_NUMBER && n.objects.iter().any(|o| o.title == "W7")));
}
