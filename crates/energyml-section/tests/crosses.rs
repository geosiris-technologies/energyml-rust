//! `crosses::traces` — the long section, then its cross sections
//! (ADR-0015).

use energyml_section::crosses::{locate, meet, traces};
use energyml_section::model::{CrossOwn, CrossWidths, Crosses, Inputs, Note, SectionKey, SectionTrace};

fn widths(before: f64, after: f64) -> CrossWidths {
    CrossWidths {
        before,
        after,
        left: 4.0,
        right: 6.0,
    }
}

fn inputs(trace: Vec<[f64; 2]>, crosses: Crosses) -> Inputs {
    Inputs {
        trace,
        system: None,
        left: 10.0,
        right: 12.0,
        crosses,
        objects: Vec::new(),
        time_factor: None,
    }
}

fn straight() -> Vec<[f64; 2]> {
    vec![[0.0, 0.0], [100.0, 0.0]]
}

fn bend() -> Vec<[f64; 2]> {
    vec![[0.0, 0.0], [100.0, 0.0], [100.0, 100.0]]
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn run(i: &Inputs) -> (Vec<SectionTrace>, Vec<Note>) {
    traces(i).expect("traces")
}

#[test]
fn without_crosses_there_is_the_long_section_alone() {
    let (t, notes) = run(&inputs(straight(), Crosses::None));
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].key, SectionKey::Long);
    assert_eq!(t[0].trace, straight());
    assert_eq!((t[0].left, t[0].right, t[0].crossing), (10.0, 12.0, None));
    assert!(notes.is_empty());
}

#[test]
fn every_places_the_multiples_and_the_end() {
    let (t, notes) = run(&inputs(
        straight(),
        Crosses::Every {
            spacing: 25.0,
            common: widths(40.0, 60.0),
            own: Vec::new(),
        },
    ));
    let at: Vec<f64> = t[1..].iter().map(|c| c.crossing.unwrap()[0]).collect();
    assert_eq!(at, vec![0.0, 25.0, 50.0, 75.0, 100.0]);
    assert!(notes.is_empty(), "a straight trace has no crossing: {notes:?}");
    for (index, c) in t[1..].iter().enumerate() {
        assert_eq!(c.key, SectionKey::Cross { index: index as u32 });
    }
}

#[test]
fn the_last_multiple_within_half_a_step_of_the_end_is_dropped() {
    // 0, 30, 60, 90 and the end at 100: 90 is 10 from the end, under 15.
    let (t, _) = run(&inputs(
        straight(),
        Crosses::Every {
            spacing: 30.0,
            common: widths(10.0, 10.0),
            own: Vec::new(),
        },
    ));
    let at: Vec<f64> = t[1..].iter().map(|c| c.crossing.unwrap()[0]).collect();
    assert_eq!(at, vec![0.0, 30.0, 60.0, 100.0]);
}

#[test]
fn count_spreads_evenly_ends_included_and_one_stands_in_the_middle() {
    let c = |count| Crosses::Count {
        count,
        common: widths(10.0, 10.0),
        own: Vec::new(),
    };
    let at = |count| -> Vec<f64> {
        run(&inputs(straight(), c(count))).0[1..]
            .iter()
            .map(|c| c.crossing.unwrap()[0])
            .collect()
    };
    assert_eq!(at(1), vec![50.0]);
    assert_eq!(at(3), vec![0.0, 50.0, 100.0]);
    assert_eq!(at(5), vec![0.0, 25.0, 50.0, 75.0, 100.0]);
}

#[test]
fn a_cross_is_perpendicular_runs_before_to_after_and_keeps_its_lengths() {
    let (t, _) = run(&inputs(
        straight(),
        Crosses::Every {
            spacing: 50.0,
            common: widths(40.0, 60.0),
            own: Vec::new(),
        },
    ));
    let c = &t[2]; // s = 50
    // `before` is on the left of the trace read along it (+y here), `after` on the right.
    assert_eq!(c.trace.len(), 2);
    assert!(
        near(c.trace[0][0], 50.0) && near(c.trace[0][1], 40.0),
        "{:?}",
        c.trace
    );
    assert!(
        near(c.trace[1][0], 50.0) && near(c.trace[1][1], -60.0),
        "{:?}",
        c.trace
    );
    // Where it crosses the long section: its abscissa on the long one, then on itself.
    let [on_long, on_self] = c.crossing.unwrap();
    assert!(near(on_long, 50.0) && near(on_self, 40.0));
    // Its corridor is its own (left, right as it runs).
    assert_eq!((c.left, c.right), (4.0, 6.0));
}

#[test]
fn own_overrides_one_cross_by_its_index() {
    let own = CrossOwn {
        index: 1,
        widths: CrossWidths {
            before: 5.0,
            after: 7.0,
            left: 1.0,
            right: 2.0,
        },
    };
    let (t, notes) = run(&inputs(
        straight(),
        Crosses::Count {
            count: 3,
            common: widths(40.0, 40.0),
            own: vec![own],
        },
    ));
    let len = |c: &SectionTrace| (c.trace[1][1] - c.trace[0][1]).abs();
    assert!(near(len(&t[1]), 80.0));
    assert!(near(len(&t[2]), 12.0), "{}", len(&t[2]));
    assert_eq!((t[2].left, t[2].right), (1.0, 2.0));
    assert!(near(t[2].crossing.unwrap()[1], 5.0));
    assert!(near(len(&t[3]), 80.0));
    assert_eq!((t[3].left, t[3].right), (4.0, 6.0));
    assert!(notes.is_empty());
}

#[test]
fn an_own_beyond_the_last_cross_is_said() {
    let own = CrossOwn {
        index: 9,
        widths: widths(1.0, 1.0),
    };
    let (t, notes) = run(&inputs(
        straight(),
        Crosses::Count {
            count: 3,
            common: widths(40.0, 40.0),
            own: vec![own],
        },
    ));
    assert_eq!(t.len(), 4);
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].code, "crossOwnUnused");
}

#[test]
fn at_a_vertex_the_cross_follows_the_bisector() {
    // Right-angle turn at (100, 0); the cross at s = 100 stands on the vertex.
    let (t, _) = run(&inputs(
        bend(),
        Crosses::Every {
            spacing: 50.0,
            common: widths(10.0, 10.0),
            own: Vec::new(),
        },
    ));
    let c = &t[3];
    assert!(near(c.crossing.unwrap()[0], 100.0));
    // The bisector runs along (1, 1) / sqrt 2, its normal along (-1, 1) / sqrt 2:
    // the cross goes from (100, 0) + 10 n to (100, 0) - 10 n.
    let r = 10.0 / 2f64.sqrt();
    assert!(
        near(c.trace[0][0], 100.0 - r) && near(c.trace[0][1], r),
        "{:?}",
        c.trace
    );
    assert!(
        near(c.trace[1][0], 100.0 + r) && near(c.trace[1][1], -r),
        "{:?}",
        c.trace
    );
    // Away from a vertex it is the segment's own direction: s = 150 is on the
    // second segment (going +y), where the cross is horizontal.
    let c = &t[4];
    assert!(near(c.trace[0][1], 50.0) && near(c.trace[1][1], 50.0));
    assert!(
        near(c.trace[0][0], 90.0) && near(c.trace[1][0], 110.0),
        "{:?}",
        c.trace
    );
}

#[test]
fn locate_gives_the_segment_direction_and_the_bisector_at_a_vertex() {
    let (p, d) = locate(&bend(), 40.0);
    assert!(near(p[0], 40.0) && near(p[1], 0.0) && near(d[0], 1.0) && near(d[1], 0.0));
    let (p, d) = locate(&bend(), 100.0);
    assert!(near(p[0], 100.0) && near(p[1], 0.0));
    assert!(near(d[0], 0.5f64.sqrt()) && near(d[1], 0.5f64.sqrt()));
    let (p, d) = locate(&bend(), 200.0);
    assert!(near(p[0], 100.0) && near(p[1], 100.0) && near(d[0], 0.0) && near(d[1], 1.0));
}

#[test]
fn a_tight_bend_is_noted_and_never_refused() {
    // Same turn, long crosses: 50 and 100 run through each other on the inside.
    let (t, notes) = run(&inputs(
        bend(),
        Crosses::Every {
            spacing: 50.0,
            common: widths(80.0, 80.0),
            own: Vec::new(),
        },
    ));
    assert_eq!(t.len(), 6, "every cross is computed all the same");
    let pair = |a: u32, b: u32| {
        notes.iter().any(|n| {
            n.code == "crossesCross"
                && n.sections == vec![SectionKey::Cross { index: a }, SectionKey::Cross { index: b }]
                && !n.said.is_empty()
        })
    };
    assert!(pair(1, 2), "{notes:?}");
    // The same turn with short crosses: nothing to say.
    let (_, notes) = run(&inputs(
        bend(),
        Crosses::Every {
            spacing: 50.0,
            common: widths(10.0, 10.0),
            own: Vec::new(),
        },
    ));
    assert!(notes.is_empty(), "{notes:?}");
}

#[test]
fn colinear_crosses_that_overlap_are_noted_too() {
    // Out and back along the same line: the crosses at s = 50 and s = 150 both
    // stand on x = 50 and cover the same stretch.
    let trace = vec![[0.0, 0.0], [100.0, 0.0], [0.0, 0.0]];
    let (_, notes) = run(&inputs(
        trace,
        Crosses::Every {
            spacing: 50.0,
            common: widths(10.0, 10.0),
            own: Vec::new(),
        },
    ));
    assert!(
        notes.iter().any(|n| n.code == "crossesCross"
            && n.sections == vec![SectionKey::Cross { index: 1 }, SectionKey::Cross { index: 3 }]),
        "{notes:?}"
    );
}

#[test]
fn distinct_parallel_crosses_do_not_meet() {
    assert_eq!(meet(&[[0.0, 0.0], [0.0, 10.0]], &[[5.0, 0.0], [5.0, 10.0]]), None);
    let at = meet(&[[0.0, 0.0], [10.0, 0.0]], &[[4.0, -2.0], [4.0, 3.0]]).unwrap();
    assert!(near(at[0], 4.0) && near(at[1], 2.0));
}

#[test]
fn bad_inputs_are_refused_by_name() {
    let mut i = inputs(straight(), Crosses::None);
    i.trace = vec![[1.0, 1.0]];
    assert!(traces(&i).is_err());
    // A spacing that would make a million crosses.
    let i = inputs(
        straight(),
        Crosses::Every {
            spacing: 1e-6,
            common: widths(1.0, 1.0),
            own: Vec::new(),
        },
    );
    assert!(traces(&i).unwrap_err().to_string().contains("limit"));
}
