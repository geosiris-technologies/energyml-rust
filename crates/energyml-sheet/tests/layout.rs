//! Sheet design: the page layout, checked in
//! numbers. Every test below states, in its own comment, what would have to
//! break in `energyml_sheet::layout` for it to turn red (a test that cannot
//! fail proves nothing).

use energyml_domain::color::Rgba;
use energyml_sheet::layout::{
    Board, HeightGoal, Interval, IntervalTrack, LaidTrack, MdTrack, ModeRegistry, ModeResult, Params,
    ProportionMode, Refusal, SectionSpan, Track, TrackRegistry, compute, compute_with,
};

fn interval(top: f64, base: f64, text: Option<&str>, colour: Option<Rgba>) -> Interval {
    Interval {
        md_top: top,
        md_base: base,
        text: text.map(str::to_string),
        colour,
        uri: None,
        row: None,
    }
}

fn red() -> Rgba {
    Rgba {
        r: 1.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    }
}

fn params(proportion: &str, height: HeightGoal) -> Params {
    Params {
        proportion: proportion.to_string(),
        height,
        page_width_mm: 100.0,
        text_size_pt: 8.0,
    }
}

#[test]
fn to_scale_height_is_proportional_to_md_thickness() {
    // What would break this: `ToScale::positions` starting to account for a
    // section's minimum content height, the way `Readable` does.
    let track = IntervalTrack::new(
        "unit",
        vec![
            interval(0.0, 10.0, None, Some(red())),
            interval(10.0, 40.0, None, Some(red())),
        ],
        vec!["fill".to_string()],
    );
    let board = Board {
        title: None,
        tracks: vec![Track::Intervals(track)],
    };
    let layout = compute(&board, &params("to-scale", HeightGoal::Total(100.0))).expect("a valid board");

    let h0 = layout.sections[0].y_base_mm - layout.sections[0].y_top_mm;
    let h1 = layout.sections[1].y_base_mm - layout.sections[1].y_top_mm;
    assert!(
        (h1 / h0 - 3.0).abs() < 1e-6,
        "the second section is 3x thicker in MD, so it must be 3x taller: h0={h0} h1={h1}"
    );
    assert!(
        (h0 + h1 - 100.0).abs() < 1e-6,
        "the two heights must sum to the requested total"
    );
    assert!(!layout.grown);
}

#[test]
fn readable_each_section_gets_at_least_its_minimum_order_is_kept_and_the_sum_is_the_target() {
    // A long description, a very thin section that still needs its own
    // minimum, and a blank one - three different reasons a section might
    // need help. What would break this: the level-filling loop in
    // `Readable::positions` freezing the wrong sections, or not
    // distributing the whole of what is left over the ones that stay
    // active (the algorithm's invariants rule that out).
    let track = IntervalTrack::new(
        "lithology",
        vec![
            interval(
                0.0,
                100.0,
                Some("a rather long description that will need several wrapped lines to show in full"),
                None,
            ),
            interval(100.0, 100.5, Some("thin"), None),
            interval(100.5, 200.0, None, None),
        ],
        vec!["text".to_string()],
    )
    .with_max_chars(16);
    let board = Board {
        title: None,
        tracks: vec![Track::Intervals(track)],
    };
    let layout = compute(&board, &params("readable", HeightGoal::Total(300.0))).expect("a valid board");

    assert_eq!(layout.sections.len(), 3);
    let mut previous_top = f64::NEG_INFINITY;
    for section in &layout.sections {
        assert!(
            section.md_top > previous_top,
            "the sections must stay in MD order"
        );
        previous_top = section.md_top;
        assert!(section.y_base_mm >= section.y_top_mm);
        // The promise of the test's name, which it did not check at first:
        // with the minimums ignored (the to-scale share), the 0.5 m
        // section got 0.75 mm and this test still passed.
        assert!(
            section.y_base_mm - section.y_top_mm >= section.min_mm - 1e-9,
            "section {}-{} m got {} mm, below its minimum {} mm",
            section.md_top,
            section.md_base,
            section.y_base_mm - section.y_top_mm,
            section.min_mm
        );
    }
    // And the thin section **did** need the help - otherwise the check above
    // proves nothing: its to-scale share is under its minimum.
    let thin = &layout.sections[1];
    assert!(
        300.0 * 0.5 / 200.0 < thin.min_mm,
        "the fixture must hold a section the scale alone would squash"
    );
    assert!(!layout.grown, "300 mm is ample for this content");
    let total: f64 = layout
        .sections
        .iter()
        .map(|section| section.y_base_mm - section.y_top_mm)
        .sum();
    assert!(
        (total - 300.0).abs() < 1e-6,
        "the heights must sum to the requested 300 mm, not to something less"
    );
    assert!((layout.body_height_mm - 300.0).abs() < 1e-6);
}

#[test]
fn readable_grows_and_the_note_says_so_when_the_minimums_exceed_the_target() {
    // What would break this: the "grown" branch of `Readable::positions`
    // silently clipping to the target instead of reporting that it needed
    // more.
    let long_text = "word ".repeat(50);
    let track = IntervalTrack::new(
        "lithology",
        vec![interval(0.0, 1.0, Some(&long_text), None)],
        vec!["text".to_string()],
    )
    .with_max_chars(8);
    let board = Board {
        title: None,
        tracks: vec![Track::Intervals(track)],
    };
    let layout = compute(&board, &params("readable", HeightGoal::Total(1.0))).expect("a valid board");

    assert!(layout.grown);
    assert!(
        layout.body_height_mm > 1.0,
        "the body must actually be taller than the 1 mm asked for"
    );
    assert!(
        layout.notes.iter().any(|note| note.contains("grows")),
        "the growth must be said, not just done"
    );
    let total: f64 = layout
        .sections
        .iter()
        .map(|section| section.y_base_mm - section.y_top_mm)
        .sum();
    assert!((total - layout.body_height_mm).abs() < 1e-6);
}

#[test]
fn the_position_function_is_monotone_in_both_modes() {
    // What would break this: a negative section height anywhere in either
    // mode's arithmetic.
    for proportion in ["to-scale", "readable"] {
        let track = IntervalTrack::new(
            "u",
            vec![
                interval(0.0, 5.0, Some("x"), Some(red())),
                interval(5.0, 6.0, Some("y"), Some(red())),
                interval(6.0, 50.0, Some("z"), Some(red())),
            ],
            vec!["fill".to_string(), "text".to_string()],
        )
        .with_max_chars(16);
        let board = Board {
            title: None,
            tracks: vec![Track::Intervals(track)],
        };
        let layout = compute(&board, &params(proportion, HeightGoal::Total(80.0))).expect("a valid board");

        let mut previous = 0.0f64;
        for section in &layout.sections {
            assert!(
                section.y_top_mm + 1e-9 >= previous,
                "{proportion}: y_top went backwards"
            );
            assert!(
                section.y_base_mm + 1e-9 >= section.y_top_mm,
                "{proportion}: y_base before y_top"
            );
            previous = section.y_base_mm;
        }
    }
}

#[test]
fn max_chars_below_min_chars_is_refused_by_name() {
    // What would break this: the check moving after something that already
    // uses `max_chars` to size a column, or the refusal losing its variant
    // name.
    let track = IntervalTrack::new(
        "t",
        vec![interval(0.0, 1.0, Some("x"), None)],
        vec!["text".to_string()],
    )
    .with_max_chars(4)
    .with_min_chars(8);
    let board = Board {
        title: None,
        tracks: vec![Track::Intervals(track)],
    };
    let error = compute(&board, &params("readable", HeightGoal::Total(10.0))).expect_err("must be refused");
    match error {
        Refusal::MaxCharsBelowMinChars {
            track,
            max_chars,
            min_chars,
        } => {
            assert_eq!(track, "t");
            assert_eq!(max_chars, 4);
            assert_eq!(min_chars, 8);
        }
        other => panic!("wrong refusal: {other:?}"),
    }
}

#[test]
fn a_track_by_node_creates_no_section() {
    // The MD column is a "track by node" - one value per node, not per
    // interval - and it carries no MD list of its own for exactly that
    // reason: `compute` derives its ticks from the sections, never the
    // other way around. What would break this: a future change that gives
    // `MdTrack` data of its own and wires it into the boundary collection
    // the way `Track::Intervals` is wired in - the section count would then
    // grow past what the one interval track alone states.
    let interval_track = IntervalTrack::new(
        "unit",
        vec![interval(0.0, 10.0, None, Some(red()))],
        vec!["fill".to_string()],
    );
    let board = Board {
        title: None,
        tracks: vec![
            Track::Md(MdTrack {
                header: "MD".to_string(),
            }),
            Track::Intervals(interval_track),
        ],
    };
    let layout = compute(&board, &params("readable", HeightGoal::Total(50.0))).expect("a valid board");
    assert_eq!(
        layout.sections.len(),
        1,
        "the MD track must not add a boundary of its own"
    );

    // And with no interval track at all, there is nothing to derive a
    // section from.
    let md_only = Board {
        title: None,
        tracks: vec![Track::Md(MdTrack {
            header: "MD".to_string(),
        })],
    };
    let layout = compute(&md_only, &params("readable", HeightGoal::Total(50.0))).expect("a valid board");
    assert!(layout.sections.is_empty());
}

#[test]
fn two_interval_tracks_with_different_bounds_give_the_union() {
    // What would break this: `compute` taking one track's boundaries
    // instead of merging both.
    let a = IntervalTrack::new(
        "a",
        vec![interval(0.0, 20.0, None, Some(red()))],
        vec!["fill".to_string()],
    );
    let b = IntervalTrack::new(
        "b",
        vec![
            interval(0.0, 10.0, None, Some(red())),
            interval(10.0, 20.0, None, Some(red())),
        ],
        vec!["fill".to_string()],
    );
    let board = Board {
        title: None,
        tracks: vec![Track::Intervals(a), Track::Intervals(b)],
    };
    let layout = compute(&board, &params("to-scale", HeightGoal::Total(100.0))).expect("a valid board");

    assert_eq!(
        layout.sections.len(),
        2,
        "track a's [0,20] and track b's [0,10],[10,20] union to two sections"
    );
    assert_eq!(layout.sections[0].md_top, 0.0);
    assert_eq!(layout.sections[0].md_base, 10.0);
    assert_eq!(layout.sections[1].md_top, 10.0);
    assert_eq!(layout.sections[1].md_base, 20.0);
}

#[test]
fn an_empty_interval_is_a_stated_blank() {
    // What would break this: `LaidCell::blank` no longer being recomputed
    // from the cell's own fields, or the note that counts blanks silently
    // dropped.
    let track = IntervalTrack::new(
        "unit",
        vec![
            interval(0.0, 10.0, None, None),
            interval(10.0, 20.0, Some("named"), Some(red())),
        ],
        vec!["fill".to_string(), "text".to_string()],
    );
    let board = Board {
        title: None,
        tracks: vec![Track::Intervals(track)],
    };
    let layout = compute(&board, &params("to-scale", HeightGoal::Total(100.0))).expect("a valid board");

    let LaidTrack::Intervals { cells, .. } = &layout.tracks[0] else {
        panic!("expected an interval track")
    };
    assert!(cells[0].blank, "no colour and no text: a blank, per the design");
    assert!(cells[0].colour.is_none() && cells[0].text.is_none());
    assert!(!cells[1].blank);
    assert!(
        layout.notes.iter().any(|note| note.contains("blank")),
        "the blank must be said, not just drawn empty"
    );
}

#[test]
fn readable_never_has_to_truncate_its_own_text() {
    // A consistency check on the crate's own invariant (documented
    // in the crate): `Readable`'s minimum height is computed from the exact
    // same line count the truncation pass later measures against, so it
    // should never trigger. What would break this: the two computations -
    // the minimum height in `compute`'s first pass, and the truncation
    // check in its second - drifting apart (a different line-height
    // constant, an off-by-one in the margin).
    let long = "word ".repeat(60);
    let track = IntervalTrack::new(
        "u",
        vec![
            interval(0.0, 1.0, Some(&long), None),
            interval(1.0, 2.0, Some("short"), None),
        ],
        vec!["text".to_string()],
    )
    .with_max_chars(10);
    let board = Board {
        title: None,
        tracks: vec![Track::Intervals(track)],
    };
    let layout = compute(&board, &params("readable", HeightGoal::Total(500.0))).expect("a valid board");

    let LaidTrack::Intervals { cells, .. } = &layout.tracks[0] else {
        panic!("expected an interval track")
    };
    for cell in cells {
        if let Some(text) = &cell.text {
            assert!(
                !text.truncated,
                "readable's own minimum should always fit what it measured"
            );
        }
    }
}

#[test]
fn a_caller_s_mode_is_tried_before_the_builtin_of_the_same_id() {
    // The registry's whole point: a caller's addition is tried
    // **first**, so it can replace a built-in rather than merely sit beside
    // it. What would break this: `ModeRegistry::register` appending instead
    // of prepending.
    struct AlwaysFlat;
    impl ProportionMode for AlwaysFlat {
        fn id(&self) -> &'static str {
            "readable"
        }
        fn axis_label(&self) -> &'static str {
            "flat"
        }
        fn deformed(&self) -> bool {
            false
        }
        fn positions(&self, sections: &[SectionSpan], target_mm: f64) -> ModeResult {
            let n = sections.len();
            let each = if n > 0 { target_mm / n as f64 } else { 0.0 };
            let y_mm: Vec<f64> = (0..=n).map(|index| each * index as f64).collect();
            ModeResult {
                y_mm,
                total_mm: target_mm,
                grown: false,
                notes: vec!["flat override".to_string()],
            }
        }
    }

    let track = IntervalTrack::new(
        "u",
        vec![
            interval(0.0, 10.0, None, Some(red())),
            interval(10.0, 90.0, None, Some(red())),
        ],
        vec!["fill".to_string()],
    );
    let board = Board {
        title: None,
        tracks: vec![Track::Intervals(track)],
    };
    let mut modes = ModeRegistry::default();
    modes.register(Box::new(AlwaysFlat));
    let layout = compute_with(
        &board,
        &params("readable", HeightGoal::Total(100.0)),
        &modes,
        &TrackRegistry::default(),
    )
    .expect("a valid board");

    assert!(
        layout.notes.iter().any(|note| note == "flat override"),
        "the overriding mode must have run"
    );
    assert_eq!(layout.axis_label, "flat");
    let h0 = layout.sections[0].y_base_mm - layout.sections[0].y_top_mm;
    let h1 = layout.sections[1].y_base_mm - layout.sections[1].y_top_mm;
    assert!(
        (h0 - h1).abs() < 1e-9,
        "the override splits evenly, ignoring MD thickness entirely"
    );
}
