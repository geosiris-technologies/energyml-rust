//! A surface drawn on a long section and on a cross section gives the
//! same elevation where they cross, and when it does not, the catalogue says so
//! (ADR-0015; `assemble::crossing_notes`).
//!
//! The geometry: a long section along +x from 0 to 100, one cross section
//! every 50 (indices 0, 1, 2); cross 1 runs from y = +40 to y = -40 through
//! (50, 0), so the crossing is at `s = 50` on the long section and `s = 40` on
//! the cross. Every surface here is cut by hand.

use energyml_section::assemble::{CROSSING_TOLERANCE_FLOOR, CROSSING_TOLERANCE_SHARE, assemble};
use energyml_section::model::{
    Catalogue, Contribution, CrossWidths, Crosses, CutShape, Inputs, Note, ObjectId, SectionKey, SurfaceCut,
    SurfaceRead, TraceRole,
};

const LONG: SectionKey = SectionKey::Long;
const CROSS: SectionKey = SectionKey::Cross { index: 1 };

fn oid(uuid: &str, title: &str) -> ObjectId {
    ObjectId {
        store: "s1".to_string(),
        uri: format!("eml:///resqml22.TriangulatedSetRepresentation({uuid})"),
        uuid: uuid.to_string(),
        title: title.to_string(),
        qualified_type: "resqml22.TriangulatedSetRepresentation".to_string(),
    }
}

fn trace_cut(section: SectionKey, spans: Vec<Vec<[f64; 2]>>) -> SurfaceCut {
    SurfaceCut {
        section,
        shape: CutShape::Trace { spans },
        notes: Vec::new(),
        sources: Vec::new(),
    }
}

fn surface(title: &str, role: TraceRole, cuts: Vec<SurfaceCut>) -> SurfaceRead {
    SurfaceRead {
        object: oid(title, title),
        role,
        time: false,
        colour: None,
        age: None,
        feature: None,
        cuts,
    }
}

fn catalogue(surfaces: Vec<SurfaceRead>) -> Catalogue {
    let inputs = Inputs {
        trace: vec![[0.0, 0.0], [100.0, 0.0]],
        system: None,
        left: 20.0,
        right: 20.0,
        crosses: Crosses::Every {
            spacing: 50.0,
            common: CrossWidths {
                before: 40.0,
                after: 40.0,
                left: 5.0,
                right: 5.0,
            },
            own: Vec::new(),
        },
        objects: Vec::new(),
        time_factor: None,
    };
    let c = Contribution {
        store: "s1".to_string(),
        surfaces,
        ..Contribution::default()
    };
    assemble(&inputs, &[c]).expect("assembles")
}

/// A horizon that falls from -10 to -20 along the long section (so -15 at the
/// crossing) and lies at `on_cross` along the cross section.
fn horizon(on_cross: f64) -> SurfaceRead {
    surface(
        "Top",
        TraceRole::Horizon,
        vec![
            trace_cut(LONG, vec![vec![[0.0, -10.0], [100.0, -20.0]]]),
            trace_cut(CROSS, vec![vec![[0.0, on_cross], [80.0, on_cross]]]),
        ],
    )
}

fn crossing_notes(cat: &Catalogue) -> Vec<&Note> {
    cat.notes
        .iter()
        .filter(|n| n.code.starts_with("crossing"))
        .collect()
}

#[test]
fn a_surface_that_agrees_at_the_crossing_says_nothing() {
    let cat = catalogue(vec![horizon(-15.0)]);
    assert!(crossing_notes(&cat).is_empty(), "{:?}", cat.notes);
}

#[test]
fn a_surface_offset_on_one_section_is_said_with_both_elevations_and_the_gap() {
    let cat = catalogue(vec![horizon(-18.0)]);
    let notes = crossing_notes(&cat);
    assert_eq!(notes.len(), 1, "{:?}", cat.notes);
    let n = notes[0];
    assert_eq!(n.code, "crossingMismatch");
    assert_eq!(n.sections, vec![LONG, CROSS]);
    assert_eq!(n.objects.len(), 1);
    assert_eq!(n.objects[0].title, "Top");
    for want in [
        "'Top'",
        "long section",
        "cross section 1",
        "-15.00",
        "-18.00",
        "3.00",
    ] {
        assert!(n.said.contains(want), "{want} missing from: {}", n.said);
    }
}

#[test]
fn a_gap_is_judged_against_a_share_of_the_vertical_extent() {
    // The two sections span z from -20 to -10: a tolerance of 0.5 % is 0.05.
    let tolerance = 10.0 * CROSSING_TOLERANCE_SHARE;
    assert!(tolerance > CROSSING_TOLERANCE_FLOOR);
    let under = catalogue(vec![horizon(-15.0 - tolerance * 0.8)]);
    assert!(crossing_notes(&under).is_empty(), "{:?}", under.notes);
    let over = catalogue(vec![horizon(-15.0 - tolerance * 1.2)]);
    assert_eq!(crossing_notes(&over).len(), 1, "{:?}", over.notes);
}

#[test]
fn a_surface_cut_on_both_but_absent_at_the_crossing_on_one_is_said_as_one_sided() {
    // The cross section's cut stops at s = 30, before the crossing at s = 40.
    let one_sided = surface(
        "Top",
        TraceRole::Horizon,
        vec![
            trace_cut(LONG, vec![vec![[0.0, -10.0], [100.0, -20.0]]]),
            trace_cut(CROSS, vec![vec![[0.0, -15.0], [30.0, -15.0]]]),
        ],
    );
    let cat = catalogue(vec![one_sided]);
    let notes = crossing_notes(&cat);
    assert_eq!(notes.len(), 1, "{:?}", cat.notes);
    assert_eq!(notes[0].code, "crossingOneSided");
    assert!(notes[0].said.contains("-15.00") && notes[0].said.contains("long section"));
    assert!(notes[0].said.contains("nothing there"));
}

#[test]
fn a_surface_cut_along_one_section_only_says_nothing_here() {
    let alone = surface(
        "Top",
        TraceRole::Horizon,
        vec![trace_cut(LONG, vec![vec![[0.0, -10.0], [100.0, -20.0]]])],
    );
    assert!(crossing_notes(&catalogue(vec![alone])).is_empty());
}

#[test]
fn a_fault_is_left_out_since_a_steep_surface_has_no_elevation_at_an_abscissa() {
    let mut fault = horizon(-18.0);
    fault.role = TraceRole::Fault;
    assert!(crossing_notes(&catalogue(vec![fault])).is_empty());
}

#[test]
fn each_surface_is_judged_on_its_own() {
    let mut base = horizon(-30.0);
    base.object = oid("Base", "Base");
    let cat = catalogue(vec![horizon(-15.0), base]);
    let notes = crossing_notes(&cat);
    assert_eq!(notes.len(), 1, "{:?}", cat.notes);
    assert_eq!(notes[0].objects[0].title, "Base");
}

fn envelope(top: f64, bottom: f64, to: f64) -> CutShape {
    CutShape::Envelope {
        top: vec![vec![[0.0, top], [to, top]]],
        bottom: vec![vec![[0.0, bottom], [to, bottom]]],
        fills: Vec::new(),
        faults: Vec::new(),
    }
}

fn grid(cuts: Vec<SurfaceCut>) -> SurfaceRead {
    SurfaceRead {
        object: oid("g", "Grid"),
        role: TraceRole::Other,
        time: false,
        colour: None,
        age: None,
        feature: None,
        cuts,
    }
}

#[test]
fn a_grid_is_compared_by_its_top_and_its_bottom() {
    // The top agrees, the bottom is 5 deeper on the cross section.
    let cat = catalogue(vec![grid(vec![
        SurfaceCut {
            section: LONG,
            shape: envelope(-10.0, -50.0, 100.0),
            notes: Vec::new(),
            sources: Vec::new(),
        },
        SurfaceCut {
            section: CROSS,
            shape: envelope(-10.0, -55.0, 80.0),
            notes: Vec::new(),
            sources: Vec::new(),
        },
    ])]);
    let notes = crossing_notes(&cat);
    assert_eq!(notes.len(), 1, "{:?}", cat.notes);
    assert_eq!(notes[0].code, "crossingMismatch");
    assert!(
        notes[0].said.contains("bottom of grid 'Grid'"),
        "{}",
        notes[0].said
    );
    assert!(notes[0].said.contains("5.00"), "{}", notes[0].said);
}

#[test]
fn a_step_in_a_grid_at_the_crossing_is_compared_as_two_sets() {
    let top_cut = |section, top: Vec<Vec<[f64; 2]>>| SurfaceCut {
        section,
        shape: CutShape::Envelope {
            top,
            bottom: Vec::new(),
            fills: Vec::new(),
            faults: Vec::new(),
        },
        notes: Vec::new(),
        sources: Vec::new(),
    };
    let cross_top = vec![vec![[0.0, -10.0], [80.0, -10.0]]];
    // The long section's top steps from -10 to -20 at s = 50 (a fault): two
    // elevations at the crossing. The cross section sees only -10, so the -20
    // has no match, and that is said.
    let stepped = vec![vec![[0.0, -10.0], [50.0, -10.0], [50.0, -20.0], [100.0, -20.0]]];
    let cat = catalogue(vec![grid(vec![
        top_cut(LONG, stepped),
        top_cut(CROSS, cross_top.clone()),
    ])]);
    assert_eq!(crossing_notes(&cat).len(), 1, "{:?}", cat.notes);
    assert!(
        cat.notes.iter().any(|n| n.said.contains("-20.00")),
        "{:?}",
        cat.notes
    );
    // Without the step, the same cross section agrees.
    let flat = vec![vec![[0.0, -10.0], [100.0, -10.0]]];
    let cat = catalogue(vec![grid(vec![top_cut(LONG, flat), top_cut(CROSS, cross_top)])]);
    assert!(crossing_notes(&cat).is_empty(), "{:?}", cat.notes);
}
