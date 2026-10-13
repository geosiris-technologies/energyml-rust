//! `assemble::assemble` over contributions built by hand (ADR-0015).

use energyml_section::assemble::assemble;
use energyml_section::model::{
    CrossWidths, Crosses, Contribution, CutShape, FORMAT, Inputs, Item, Mark, ObjectId, Outcome,
    Refused, RowKind, SectionKey, Side, SurfaceCut, SurfaceRead, TraceRole, WellKind, WellRead,
};

fn oid(uuid: &str, title: &str, qualified_type: &str) -> ObjectId {
    ObjectId {
        store: "s1".to_string(),
        uri: format!("eml:///{qualified_type}({uuid})"),
        uuid: uuid.to_string(),
        title: title.to_string(),
        qualified_type: qualified_type.to_string(),
    }
}

fn well(
    uuid: &str,
    kind: WellKind,
    wellbore: Option<&str>,
    head: [f64; 3],
    path: Vec<[f64; 3]>,
) -> WellRead {
    WellRead {
        object: oid(
            uuid,
            uuid,
            match kind {
                WellKind::Frame => "resqml22.WellboreFrameRepresentation",
                WellKind::Trajectory => "resqml22.WellboreTrajectoryRepresentation",
            },
        ),
        kind,
        wellbore: wellbore.map(str::to_string),
        head,
        path,
        md: None,
        intervals: Vec::new(),
        strata: None,
        borrowed: false,
        head_only: false,
    }
}

fn inputs(trace: Vec<[f64; 2]>) -> Inputs {
    Inputs {
        trace,
        system: None,
        left: 20.0,
        right: 20.0,
        crosses: Crosses::None,
        objects: Vec::new(),
        time_factor: None,
    }
}

fn wells(wells: Vec<WellRead>) -> Vec<Contribution> {
    vec![Contribution { store: "s1".to_string(), wells, ..Contribution::default() }]
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn well_items(items: &[Item]) -> Vec<&energyml_section::model::WellItem> {
    items
        .iter()
        .filter_map(|i| if let Item::Well(w) = i { Some(w) } else { None })
        .collect()
}

#[test]
fn a_well_on_a_segment_is_placed_at_its_abscissa_with_its_nodes_shifted() {
    let i = inputs(vec![[0.0, 0.0], [100.0, 0.0]]);
    let w = well(
        "w1",
        WellKind::Frame,
        Some("wb"),
        [30.0, 5.0, 10.0],
        vec![[30.0, 5.0, 10.0], [40.0, 5.0, -90.0], [55.0, 5.0, -190.0]],
    );
    let cat = assemble(&i, &wells(vec![w])).expect("assembles");
    assert_eq!(cat.format, FORMAT);
    assert_eq!(cat.inputs, i);
    let items = well_items(&cat.long.items);
    assert_eq!(items.len(), 1);
    let item = items[0];
    assert!(near(item.s, 30.0) && near(item.d, 5.0), "{item:?}");
    assert_eq!(item.side, Side::Left);
    assert_eq!(item.head, [30.0, 10.0]);
    // Each node at its own abscissa, with its own z.
    let expected = [[30.0, 10.0], [40.0, -90.0], [55.0, -190.0]];
    for (got, want) in item.path.iter().zip(expected) {
        assert!(near(got[0], want[0]) && near(got[1], want[1]), "{:?}", item.path);
    }
    assert_eq!(cat.objects.len(), 1);
    assert_eq!(cat.objects[0].kind, RowKind::Well);
    assert_eq!(cat.objects[0].outcome, Outcome::Drawn { sections: vec![SectionKey::Long] });
    assert_eq!(cat.long.z_range, Some([-190.0, 10.0]));
}

#[test]
fn the_side_and_the_nodes_follow_the_direction_of_the_trace() {
    // The trace runs along +y: its left is -x, so a head at x = 5 is on the right.
    let i = inputs(vec![[0.0, 0.0], [0.0, 100.0]]);
    let w = well(
        "w1",
        WellKind::Trajectory,
        None,
        [5.0, 30.0, 0.0],
        vec![[5.0, 30.0, 0.0], [5.0, 45.0, -100.0]],
    );
    let cat = assemble(&i, &wells(vec![w])).unwrap();
    let item = well_items(&cat.long.items)[0].clone();
    assert_eq!(item.side, Side::Right);
    assert!(near(item.s, 30.0));
    // Node 2 is 15 further along +y: s = 45, not the 0 an x-only reading gives.
    assert!(near(item.path[1][0], 45.0), "{:?}", item.path);
}

#[test]
fn a_head_in_the_outer_wedge_of_a_turn_places_its_nodes_along_the_bisector() {
    // Turn at (100, 0): the head (110, -10) projects on the vertex (s = 100).
    // The direction kept is the bisector (1, 1) / sqrt 2: the node at (120, 0)
    // is (10, 10) from the head, so 10 sqrt 2 further.
    let i = inputs(vec![[0.0, 0.0], [100.0, 0.0], [100.0, 100.0]]);
    let w = well(
        "w1",
        WellKind::Frame,
        Some("wb"),
        [110.0, -10.0, 0.0],
        vec![[110.0, -10.0, 0.0], [120.0, 0.0, -50.0]],
    );
    let cat = assemble(&i, &wells(vec![w])).unwrap();
    let item = well_items(&cat.long.items)[0].clone();
    assert!(near(item.s, 100.0), "{item:?}");
    assert!(near(item.path[1][0], 100.0 + 10.0 * 2f64.sqrt()), "{:?}", item.path);
}

#[test]
fn a_frame_and_its_trajectory_are_one_line_and_the_frame_is_kept() {
    let i = inputs(vec![[0.0, 0.0], [100.0, 0.0]]);
    let traj = well("t", WellKind::Trajectory, Some("wb"), [30.0, 5.0, 0.0], vec![[30.0, 5.0, 0.0]]);
    let frame = well("f", WellKind::Frame, Some("wb"), [30.0, 5.0, 0.0], vec![[30.0, 5.0, 0.0]]);
    let alone = well("a", WellKind::Trajectory, None, [60.0, 5.0, 0.0], vec![[60.0, 5.0, 0.0]]);
    let other = well("b", WellKind::Trajectory, None, [70.0, 5.0, 0.0], vec![[70.0, 5.0, 0.0]]);
    let cat = assemble(&i, &wells(vec![traj.clone(), alone, other, frame.clone()])).unwrap();

    let items = well_items(&cat.long.items);
    assert_eq!(items.len(), 3, "the wellbore is one line, the two without one stand alone");
    let line = items.iter().find(|w| w.object.uuid == "f").expect("the frame is drawn");
    assert_eq!(line.merged, vec![traj.object.clone()]);
    assert!(items.iter().all(|w| w.object.uuid != "t"));

    let row = |uuid: &str| cat.objects.iter().find(|r| r.object.uuid == uuid).unwrap();
    assert_eq!(row("t").outcome, Outcome::Merged { into: frame.object.clone() });
    assert_eq!(row("f").outcome, Outcome::Drawn { sections: vec![SectionKey::Long] });
    assert!(matches!(row("a").outcome, Outcome::Drawn { .. }));
    assert!(matches!(row("b").outcome, Outcome::Drawn { .. }));
    assert_eq!(cat.objects.len(), 4);
}

#[test]
fn of_two_frames_of_a_wellbore_the_one_that_knows_its_strata_is_kept() {
    // Lutetia gives each borehole a frame for the geometry and one for the
    // stratigraphy: keeping the first met drew the wells grey (03/10).
    let i = inputs(vec![[0.0, 0.0], [100.0, 0.0]]);
    let silent = well("silent", WellKind::Frame, Some("wb"), [30.0, 5.0, 0.0], vec![[30.0, 5.0, 0.0]]);
    let mut strata = well("strata", WellKind::Frame, Some("wb"), [30.0, 5.0, 0.0], vec![[30.0, 5.0, 0.0]]);
    strata.intervals = vec![energyml_section::model::Interval {
        from: 0.0,
        to: 10.0,
        colour: Some("#ffcc00".to_string()),
        unit: "Holocene".to_string(),
        unit_uri: None,
    }];
    let traj = well("t", WellKind::Trajectory, Some("wb"), [30.0, 5.0, 0.0], vec![[30.0, 5.0, 0.0]]);
    let cat = assemble(&i, &wells(vec![silent.clone(), traj, strata])).unwrap();
    let items = well_items(&cat.long.items);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].object.uuid, "strata");
    assert_eq!(items[0].intervals.len(), 1);
    assert!(items[0].merged.contains(&silent.object));
}

/// A member painted from the wellbore's frames (`borrowed`) has the stated
/// frame's units on its own nodes, and can count more intervals than it — the
/// frame that **states** them keeps the line all the same, whatever the
/// order the members come in. What happened on Lutetia once the faces gave
/// `frames_of` (03/10): the line went to a trajectory, then to the frame
/// prolonged below the described units.
///
/// To make it fail: rank on the count before the provenance.
#[test]
fn the_frame_that_states_the_strata_keeps_the_line_over_members_that_borrow_them() {
    let i = inputs(vec![[0.0, 0.0], [100.0, 0.0]]);
    let interval = |from: f64, to: f64, unit: &str| energyml_section::model::Interval {
        from,
        to,
        colour: Some("#ffcc00".to_string()),
        unit: unit.to_string(),
        unit_uri: None,
    };
    let mut own = well("own", WellKind::Frame, Some("wb"), [30.0, 5.0, 0.0], vec![[30.0, 5.0, 0.0]]);
    own.intervals = vec![interval(0.0, 10.0, "Holocene")];
    let mut prolonged = well("prolonged", WellKind::Frame, Some("wb"), [30.0, 5.0, 0.0], vec![[30.0, 5.0, 0.0]]);
    prolonged.intervals = vec![interval(0.0, 5.0, "Holocene"), interval(5.0, 10.0, "Holocene"), interval(10.0, 10.0, "Holocene")];
    prolonged.borrowed = true;
    let mut traj = well("t", WellKind::Trajectory, Some("wb"), [30.0, 5.0, 0.0], vec![[30.0, 5.0, 0.0]]);
    traj.intervals = prolonged.intervals.clone();
    traj.borrowed = true;
    for order in [vec![prolonged.clone(), traj.clone(), own.clone()], vec![own.clone(), traj.clone(), prolonged.clone()]] {
        let cat = assemble(&i, &wells(order)).unwrap();
        let items = well_items(&cat.long.items);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].object.uuid, "own");
    }
}

#[test]
fn the_line_takes_its_strata_from_one_frame_and_its_depth_from_the_deepest() {
    // The stratigraphic frame stops at -20, the geometric one runs to -200: the
    // line is drawn to -200, coloured by the units of the first (03/10).
    let i = inputs(vec![[0.0, 0.0], [100.0, 0.0]]);
    let mut strata = well("strata", WellKind::Frame, Some("wb"), [30.0, 5.0, 0.0], vec![[30.0, 5.0, 0.0], [30.0, 5.0, -20.0]]);
    strata.md = Some(vec![0.0, 20.0]);
    strata.intervals = vec![energyml_section::model::Interval {
        from: 0.0,
        to: 20.0,
        colour: Some("#ffcc00".to_string()),
        unit: "Holocene".to_string(),
        unit_uri: None,
    }];
    let mut deep = well("deep", WellKind::Frame, Some("wb"), [30.0, 5.0, 0.0], vec![[30.0, 5.0, 0.0], [30.0, 5.0, -200.0]]);
    deep.md = Some(vec![0.0, 200.0]);
    let cat = assemble(&i, &wells(vec![deep, strata])).unwrap();
    let item = well_items(&cat.long.items)[0].clone();
    assert_eq!(item.object.uuid, "strata");
    assert_eq!(item.intervals.len(), 1);
    assert_eq!(item.path.last().map(|n| n[1]), Some(-200.0));
    assert_eq!(item.md, Some(vec![0.0, 200.0]));
}

#[test]
fn a_trajectory_alone_is_drawn_and_folds_the_wellbores_other_trajectory() {
    let i = inputs(vec![[0.0, 0.0], [100.0, 0.0]]);
    let t1 = well("t1", WellKind::Trajectory, Some("wb"), [30.0, 5.0, 0.0], vec![[30.0, 5.0, 0.0]]);
    let t2 = well("t2", WellKind::Trajectory, Some("wb"), [30.0, 5.0, 0.0], vec![[30.0, 5.0, 0.0]]);
    let cat = assemble(&i, &wells(vec![t1.clone(), t2.clone()])).unwrap();
    let items = well_items(&cat.long.items);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].object.uuid, "t1");
    assert_eq!(items[0].merged, vec![t2.object.clone()]);
}

#[test]
fn a_well_beyond_an_end_is_refused_with_its_reason() {
    let i = inputs(vec![[0.0, 0.0], [100.0, 0.0]]);
    let beyond = well("past", WellKind::Frame, Some("w1"), [110.0, 0.0, 0.0], vec![[110.0, 0.0, 0.0]]);
    let far = well("far", WellKind::Frame, Some("w2"), [50.0, 500.0, 0.0], vec![[50.0, 500.0, 0.0]]);
    let cat = assemble(&i, &wells(vec![beyond, far])).unwrap();
    assert!(well_items(&cat.long.items).is_empty());
    let row = |uuid: &str| cat.objects.iter().find(|r| r.object.uuid == uuid).unwrap();
    let Outcome::Refused { reason } = &row("past").outcome else { panic!("{:?}", row("past")) };
    assert!(reason.contains("past") && reason.contains("beyond an end"), "{reason}");
    let Outcome::Refused { reason } = &row("far").outcome else { panic!("{:?}", row("far")) };
    assert!(reason.contains("farther than 20"), "{reason}");
    // The plan keeps the well and says where it went: nowhere.
    let pw = cat.plan.wells.iter().find(|w| w.object.uuid == "past").unwrap();
    assert!(pw.feet.is_empty());
    assert!(pw.rejected.as_deref().unwrap().contains("beyond an end"));
    assert!(cat.notes.iter().any(|n| n.code == "beyondAnEnd"));
}

#[test]
fn a_surface_is_a_trace_item_where_it_is_cut_and_missed_where_it_is_not() {
    let i = inputs(vec![[0.0, 0.0], [100.0, 0.0]]);
    let spans = vec![vec![[0.0, -10.0], [100.0, -20.0]]];
    let hit = SurfaceRead {
        object: oid("h", "Top", "resqml22.TriangulatedSetRepresentation"),
        role: TraceRole::Horizon,
        time: false,
        colour: Some("#3366cc".to_string()),
        age: None,
        feature: None,
        cuts: vec![SurfaceCut {
            section: SectionKey::Long,
            shape: CutShape::Trace { spans: spans.clone() },
            notes: vec!["a folded span was dropped".to_string()],
            sources: Vec::new(),
        }],
    };
    let miss = SurfaceRead {
        object: oid("m", "Elsewhere", "resqml22.TriangulatedSetRepresentation"),
        role: TraceRole::Other,
        time: false,
        colour: None,
        age: None,
        feature: None,
        cuts: Vec::new(),
    };
    // Cut along a cross section that does not exist in these inputs.
    let stale = SurfaceRead {
        object: oid("x", "Stale", "resqml22.TriangulatedSetRepresentation"),
        role: TraceRole::Other,
        time: false,
        colour: None,
        age: None,
        feature: None,
        cuts: vec![SurfaceCut {
            section: SectionKey::Cross { index: 4 },
            shape: CutShape::Trace { spans: spans.clone() },
            notes: Vec::new(),
            sources: Vec::new(),
        }],
    };
    let grid = SurfaceRead {
        object: oid("g", "Grid", "resqml22.IjkGridRepresentation"),
        role: TraceRole::Other,
        time: true,
        colour: None,
        age: None,
        feature: None,
        cuts: vec![SurfaceCut {
            section: SectionKey::Long,
            shape: CutShape::Envelope {
                top: vec![vec![[0.0, 5.0], [100.0, 6.0]]],
                bottom: vec![vec![[0.0, -50.0], [100.0, -60.0]]],
                fills: Vec::new(),
                faults: Vec::new(),
            },
            notes: Vec::new(),
            sources: Vec::new(),
        }],
    };
    let c = Contribution {
        store: "s1".to_string(),
        surfaces: vec![hit, miss, stale, grid],
        ..Contribution::default()
    };
    let cat = assemble(&i, &[c]).unwrap();

    let traces: Vec<_> = cat
        .long
        .items
        .iter()
        .filter_map(|i| if let Item::Trace(t) = i { Some(t) } else { None })
        .collect();
    assert_eq!(traces.len(), 1);
    assert_eq!(traces[0].spans, spans);
    assert_eq!(traces[0].role, TraceRole::Horizon);
    assert_eq!(traces[0].notes, vec!["a folded span was dropped".to_string()]);
    // The colour the file gives the surface travels with it to the drawing.
    assert_eq!(traces[0].colour.as_deref(), Some("#3366cc"));
    assert!(cat.long.items.iter().any(|i| matches!(i, Item::Envelope(e) if e.time)));
    assert_eq!(cat.long.z_range, Some([-60.0, 6.0]));

    let row = |uuid: &str| cat.objects.iter().find(|r| r.object.uuid == uuid).unwrap();
    assert_eq!(row("h").outcome, Outcome::Drawn { sections: vec![SectionKey::Long] });
    assert_eq!(row("h").kind, RowKind::Surface);
    assert_eq!(row("m").outcome, Outcome::Missed);
    assert_eq!(row("x").outcome, Outcome::Missed);
    assert_eq!(row("g").kind, RowKind::Grid);
}

#[test]
fn what_a_package_refused_becomes_a_row() {
    let i = inputs(vec![[0.0, 0.0], [100.0, 0.0]]);
    let c = Contribution {
        store: "s1".to_string(),
        refused: vec![Refused {
            object: oid("r", "Bad", "resqml22.WellboreFrameRepresentation"),
            reason: "no CRS reachable".to_string(),
        }],
        ..Contribution::default()
    };
    let cat = assemble(&i, &[c]).unwrap();
    assert_eq!(cat.objects.len(), 1);
    assert_eq!(cat.objects[0].kind, RowKind::Well);
    assert_eq!(
        cat.objects[0].outcome,
        Outcome::Refused { reason: "no CRS reachable".to_string() }
    );
    assert_eq!(cat.long.z_range, None);
}

#[test]
fn the_plan_is_local_with_its_origin_at_the_first_point() {
    let o = [651_000.0, 6_860_000.0];
    let i = inputs(vec![o, [o[0] + 100.0, o[1]], [o[0] + 100.0, o[1] + 50.0]]);
    let w = well(
        "w1",
        WellKind::Frame,
        Some("wb"),
        [o[0] + 30.0, o[1] + 5.0, 0.0],
        vec![[o[0] + 30.0, o[1] + 5.0, 0.0]],
    );
    let cat = assemble(&i, &wells(vec![w])).unwrap();
    assert_eq!(cat.plan.origin, o);
    let (key, trace) = &cat.plan.sections[0];
    assert_eq!(*key, SectionKey::Long);
    assert_eq!(trace[0], [0.0, 0.0]);
    assert_eq!(trace[1], [100.0, 0.0]);
    // Nothing of the plan carries a six-figure coordinate.
    for (_, ring) in &cat.plan.bands {
        assert!(!ring.is_empty());
        assert!(ring.iter().flatten().all(|v| v.abs() < 1000.0), "{ring:?}");
    }
    let pw = &cat.plan.wells[0];
    assert!(near(pw.head[0], 30.0) && near(pw.head[1], 5.0));
    assert_eq!(pw.feet.len(), 1);
    assert_eq!(pw.feet[0].0, SectionKey::Long);
    assert!(near(pw.feet[0].1[0], 30.0) && near(pw.feet[0].1[1], 0.0), "{:?}", pw.feet);
    assert!(pw.rejected.is_none());
    // The section itself keeps the scene's coordinates.
    assert_eq!(cat.long.trace[0], o);
    assert!(near(cat.long.length, 150.0));
}

#[test]
fn the_cross_sections_project_their_own_wells_and_every_section_knows_its_marks() {
    let mut i = inputs(vec![[0.0, 0.0], [100.0, 0.0]]);
    i.crosses = Crosses::Every {
        spacing: 50.0,
        common: CrossWidths { before: 40.0, after: 40.0, left: 5.0, right: 5.0 },
        own: Vec::new(),
    };
    let w = well("w1", WellKind::Frame, Some("wb"), [50.0, 3.0, -7.0], vec![[50.0, 3.0, -7.0]]);
    let cat = assemble(&i, &wells(vec![w])).unwrap();
    assert_eq!(cat.crosses.len(), 3);
    assert_eq!(
        cat.crosses.iter().map(|c| c.key).collect::<Vec<_>>(),
        (0..3).map(|index| SectionKey::Cross { index }).collect::<Vec<_>>()
    );

    // On the long section (d = 3) and on cross 1, which runs from y = 40 to y = -40.
    let on_cross = well_items(&cat.crosses[1].items);
    assert_eq!(on_cross.len(), 1);
    assert!(near(on_cross[0].s, 37.0) && near(on_cross[0].d, 0.0), "{:?}", on_cross[0]);
    assert!(well_items(&cat.crosses[0].items).is_empty());
    assert_eq!(
        cat.objects[0].outcome,
        Outcome::Drawn { sections: vec![SectionKey::Long, SectionKey::Cross { index: 1 }] }
    );
    assert_eq!(cat.plan.wells[0].feet.len(), 2);

    // Marks: the long section's vertices and the three crossings, in order of s.
    let crossings: Vec<_> = cat
        .long
        .marks
        .iter()
        .filter_map(|m| if let Mark::Crossing { s, other } = m { Some((*s, *other)) } else { None })
        .collect();
    assert_eq!(
        crossings,
        vec![
            (0.0, SectionKey::Cross { index: 0 }),
            (50.0, SectionKey::Cross { index: 1 }),
            (100.0, SectionKey::Cross { index: 2 }),
        ]
    );
    let vertices = cat.long.marks.iter().filter(|m| matches!(m, Mark::Vertex { .. })).count();
    assert_eq!(vertices, 2);
    // A cross says where the long section crosses it: at its `before` length.
    assert!(cat.crosses[1]
        .marks
        .iter()
        .any(|m| matches!(m, Mark::Crossing { s, other: SectionKey::Long } if near(*s, 40.0))));
    // The plan holds every section and every band.
    assert_eq!(cat.plan.sections.len(), 4);
    assert_eq!(cat.plan.bands.len(), 4);
    assert!(cat.crosses[1].length > 79.9 && cat.crosses[1].length < 80.1);
}

#[test]
fn cross_sections_running_through_each_other_leave_a_note_and_a_mark() {
    let mut i = inputs(vec![[0.0, 0.0], [100.0, 0.0], [100.0, 100.0]]);
    i.crosses = Crosses::Every {
        spacing: 50.0,
        common: CrossWidths { before: 80.0, after: 80.0, left: 5.0, right: 5.0 },
        own: Vec::new(),
    };
    let cat = assemble(&i, &[]).unwrap();
    assert!(cat.notes.iter().any(|n| n.code == "crossesCross"));
    // Cross 1 knows where cross 2 runs through it.
    assert!(cat
        .crosses[1]
        .marks
        .iter()
        .any(|m| matches!(m, Mark::Crossing { other: SectionKey::Cross { index: 2 }, .. })));
}

#[test]
fn bad_inputs_are_refused_before_anything_is_assembled() {
    let mut i = inputs(vec![[0.0, 0.0]]);
    assert!(assemble(&i, &[]).is_err());
    i.trace = vec![[0.0, 0.0], [1.0, 0.0]];
    i.left = -1.0;
    assert!(assemble(&i, &[]).is_err());
}

/// What the front end may fill is decided here: a horizon or a plain surface,
/// triangulated or a 2D grid alike — never a fault, an IJK grid or a well. And
/// a 2D grid is a `Surface`: it is cut in a line like a triangulated one, and
/// `RowKind::Grid` is the IJK grid its doc says.
#[test]
fn a_2d_grid_is_a_fillable_surface_a_fault_and_an_ijk_grid_are_not() {
    let i = inputs(vec![[0.0, 0.0], [100.0, 0.0]]);
    let line = |uuid: &str, qualified_type: &str, role: TraceRole, feature: Option<&str>| SurfaceRead {
        object: oid(uuid, uuid, qualified_type),
        role,
        time: false,
        colour: None,
        age: None,
        feature: feature.map(str::to_string),
        cuts: vec![SurfaceCut {
            section: SectionKey::Long,
            shape: CutShape::Trace { spans: vec![vec![[0.0, -10.0], [100.0, -20.0]]] },
            notes: Vec::new(),
            sources: Vec::new(),
        }],
    };
    let ijk = SurfaceRead {
        cuts: vec![SurfaceCut {
            section: SectionKey::Long,
            shape: CutShape::Envelope {
                top: vec![vec![[0.0, 5.0], [100.0, 6.0]]],
                bottom: vec![vec![[0.0, -50.0], [100.0, -60.0]]],
                fills: Vec::new(),
                faults: Vec::new(),
            },
            notes: Vec::new(),
            sources: Vec::new(),
        }],
        ..line("ijk", "resqml22.IjkGridRepresentation", TraceRole::Horizon, None)
    };
    let c = Contribution {
        store: "s1".to_string(),
        surfaces: vec![
            line("grid2d", "resqml22.Grid2dRepresentation", TraceRole::Horizon, Some("feature-1")),
            line("mnt", "resqml22.Grid2dRepresentation", TraceRole::Other, None),
            line("mesh", "resqml22.TriangulatedSetRepresentation", TraceRole::Horizon, None),
            line("fault", "resqml22.TriangulatedSetRepresentation", TraceRole::Fault, None),
            line("fault2d", "resqml22.Grid2dRepresentation", TraceRole::Fault, None),
            ijk,
        ],
        refused: vec![Refused {
            object: oid("bad2d", "bad2d", "resqml22.Grid2dRepresentation"),
            reason: "unreadable".to_string(),
        }],
        ..Contribution::default()
    };
    let cat = assemble(&i, &[c]).unwrap();
    let row = |uuid: &str| cat.objects.iter().find(|r| r.object.uuid == uuid).unwrap();
    for (uuid, kind, fillable) in [
        ("grid2d", RowKind::Surface, true),
        ("mnt", RowKind::Surface, true),
        ("mesh", RowKind::Surface, true),
        ("fault", RowKind::Surface, false),
        ("fault2d", RowKind::Surface, false),
        ("ijk", RowKind::Grid, false),
        ("bad2d", RowKind::Surface, false),
    ] {
        assert_eq!((row(uuid).kind, row(uuid).fillable), (kind, fillable), "{uuid}");
    }
    // The feature rides to the trace item, where the fill reads it.
    let feature = cat.long.items.iter().find_map(|i| match i {
        Item::Trace(t) if t.object.uuid == "grid2d" => Some(t.feature.clone()),
        _ => None,
    });
    assert_eq!(feature, Some(Some("feature-1".to_string())));
}
