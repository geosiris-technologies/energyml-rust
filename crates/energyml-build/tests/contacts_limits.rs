//! `contacts-from-frames` read through `energyml_domain::limits` (ADR 0053): a class that comes back in one hole,
//! a categorical property, a threshold on a continuous one, a well the reading cannot treat.
//!
//! The frames are written here: vertical holes, node `k` of frame `at` at `(10 * at, 0, -md[k])`, so a contact's
//! place is read **from its coordinates**, never from anything the operation kept. The shape of the assertions
//! is the specification's (RESQML TRG 2.2 §7.1: the values of a property hang on nodes or intervals; common TRG 2.3
//! for the properties' kinds); the numbers are the tests' own.
//!
//! Each test says what to break at its end.

use std::sync::Arc;

use serde_json::{Value, json};

use energyml_build::operation::{Choices, Identity, Numbers, Plan, Registry, When};
use energyml_build::pieces::{Known, KnownPath, KnownProperty, Pieces, Ref, Source};
use energyml_domain::values::Values;
use energyml_geometry::geometry::{Geometry, Patch, Topology};
use energyml_geometry::parametric::{Path, Vertical};
use energyml_geometry::well::AlongHole;

const CRS: &str = "44444444-4444-4444-8444-444444444444";
const PHI_KIND: &str = "11111111-1111-4111-8111-111111111111";
const FACIES_KIND: &str = "22222222-2222-4222-8222-222222222222";
const FACIES_TABLE: &str = "33333333-3333-4333-8333-333333333333";

fn when() -> When {
    When {
        creation: "2026-11-26T00:00:00Z".into(),
        originator: "test".into(),
    }
}

/// A frame whose nodes are at these measured depths, standing at x = 10 * `at`, on a straight hole.
fn frame(bag: Pieces, at: usize, title: &str, mds: &[f64]) -> Pieces {
    let points: Vec<[f64; 3]> = mds.iter().map(|md| [at as f64 * 10.0, 0.0, -md]).collect();
    let line = Path::new(1, points.clone(), mds.to_vec(), None, Vertical::Down).expect("a straight line");
    frame_on(bag, at, title, mds, points, Some(AlongHole::Line(line)))
}

/// A frame at these nodes, on this line - or on none: the trajectory it names is not in the package.
fn frame_on(
    bag: Pieces,
    at: usize,
    title: &str,
    mds: &[f64],
    points: Vec<[f64; 3]>,
    line: Option<AlongHole>,
) -> Pieces {
    let name = if at == 0 {
        "frames".to_string()
    } else {
        format!("frames {}", at + 1)
    };
    let geometry = Geometry {
        kind: "resqml22.WellboreFrameRepresentation",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points,
            topology: Topology::Points,
            closed: Vec::new(),
        }],
    };
    bag.with(
        name.clone(),
        Source::Shape {
            geometry: Arc::new(geometry),
            closed: None,
        },
    )
    .with(
        format!("{name} (object)"),
        Source::Object(Known {
            reference: Some(Ref::new(
                format!("uuid-{title}"),
                "resqml22.WellboreFrameRepresentation",
                title,
            )),
            crs: Some(Ref::new(CRS, "eml23.ProjectedCrs", "EPSG:2154")),
            node_mds: Some(mds.to_vec()),
            path: Some(KnownPath {
                trajectory: Ref::new(
                    format!("uuid-{title}-trajectory"),
                    "resqml22.WellboreTrajectoryRepresentation",
                    format!("{title} trajectory"),
                ),
                path: match &line {
                    Some(AlongHole::Line(path)) => Ok(path.clone()),
                    _ => Err("the trajectory the frame names has no line".to_string()),
                },
                along: line.ok_or_else(|| "the trajectory the frame names is not in the package".to_string()),
                md_uom: Some("m".to_string()),
                datum: None,
            }),
            ..Known::default()
        }),
    )
}

/// A property of the frame, as the boundary resolves it.
fn property(
    frame: &str,
    measure: &str,
    kind: (&str, &str),
    discrete: bool,
    on: &str,
    values: Values,
) -> Known {
    Known {
        reference: Some(Ref::new(
            format!("uuid-{frame}-{measure}"),
            if discrete {
                "resqml22.DiscreteProperty"
            } else {
                "resqml22.ContinuousProperty"
            },
            format!("{frame} - {measure}"),
        )),
        property: Some(KnownProperty {
            supporting: Some(Ref::new(
                format!("uuid-{frame}"),
                "resqml22.WellboreFrameRepresentation",
                frame,
            )),
            indexable: on.to_string(),
            value_count: vec![1],
            timed: false,
            realizations: false,
            uom: (!discrete).then(|| "m3/m3".to_string()),
            kind: Some(Ref::new(kind.0, "eml23.PropertyKind", kind.1)),
            lookup: discrete.then(|| Ref::new(FACIES_TABLE, "eml23.ColumnBasedTable", "facies")),
            null_value: discrete.then_some(-1),
            facets: Vec::new(),
            values: Ok(values),
        }),
        ..Known::default()
    }
}

fn facies(frame: &str, codes: &[i64]) -> Known {
    property(
        frame,
        "FACIES",
        (FACIES_KIND, "facies"),
        true,
        "nodes",
        Values::Integer(codes.to_vec()),
    )
}

fn phi(frame: &str, values: &[f64]) -> Known {
    property(
        frame,
        "PHI",
        (PHI_KIND, "porosity"),
        false,
        "nodes",
        Values::Floating(values.to_vec()),
    )
}

fn with_properties(mut bag: Pieces, all: Vec<Known>) -> Pieces {
    for (at, known) in all.into_iter().enumerate() {
        let name = if at == 0 {
            "properties".to_string()
        } else {
            format!("properties {}", at + 1)
        };
        bag = bag.with(name, Source::Object(known));
    }
    bag
}

fn contacts(pieces: &Pieces, answers: Value) -> Result<Plan, String> {
    let mut choices = Choices::new();
    for (name, value) in answers.as_object().expect("an object") {
        choices = choices.set(name, value.clone());
    }
    Registry::new()
        .plan(
            "contacts-from-frames",
            pieces,
            &choices,
            &Identity::derived(),
            &when(),
        )
        .map_err(|why| why.to_string())
}

fn points_of(made: &Plan, title: &str) -> Option<Vec<Vec<f64>>> {
    let set = made
        .objects
        .iter()
        .find(|one| one.qualified_type == "resqml22.PointSetRepresentation" && one.title == title)?;
    let path = format!("/resqml22/{}/points", set.uuid);
    let dataset = made.datasets.iter().find(|one| one.path == path)?;
    let Numbers::F64(values) = &dataset.values else {
        return None;
    };
    Some(values.chunks(3).map(<[f64]>::to_vec).collect())
}

fn copied(made: &Plan, title: &str) -> Option<Numbers> {
    let object = made
        .objects
        .iter()
        .find(|one| one.title == title && one.qualified_type.ends_with("Property"))?;
    let path = format!("/resqml22/{}/values", object.uuid);
    made.datasets
        .iter()
        .find(|one| one.path == path)
        .map(|one| one.values.clone())
}

/// A hole of eight nodes, 10 m apart: facies 1, 1, 2, 2, 1, 1, 2, 2 - the second facies comes back.
fn abab() -> Pieces {
    let mds: Vec<f64> = (0..8).map(|k| k as f64 * 10.0).collect();
    with_properties(
        frame(Pieces::empty(), 0, "F1", &mds),
        vec![facies("F1", &[1, 1, 2, 2, 1, 1, 2, 2])],
    )
}

// -- a class that comes back --------------------------------------------------------------------

/// A class that comes back in one hole is not refused: the top of facies 2 is met twice, and the point set
/// of that top holds **two points**, ranks 1 and 2, in the order down the hole, with the rank in a property.
///
/// Break it: in `contacts_of`, set `rank: 1` on every origin.
#[test]
fn a_class_that_comes_back_gives_two_points_of_ranks_one_and_two_in_one_cloud() {
    let made = contacts(&abab(), json!({ "categoryKind": FACIES_KIND })).expect("a hole A, B, A, B");
    // Facies 2 begins at node 2 (20 m) and again at node 6 (60 m).
    assert_eq!(
        points_of(&made, "facies 2 - top"),
        Some(vec![vec![0.0, 0.0, -20.0], vec![0.0, 0.0, -60.0]])
    );
    assert_eq!(
        copied(&made, "facies 2 - top - rank"),
        Some(Numbers::I64(vec![1, 2]))
    );
    // Facies 1 has one top - the second run's, rank 2: the first run begins where the log does - and two bases.
    assert_eq!(
        points_of(&made, "facies 1 - top"),
        Some(vec![vec![0.0, 0.0, -40.0]])
    );
    assert_eq!(
        copied(&made, "facies 1 - top - rank"),
        Some(Numbers::I64(vec![2]))
    );
    assert_eq!(
        points_of(&made, "facies 1 - base"),
        Some(vec![vec![0.0, 0.0, -10.0], vec![0.0, 0.0, -50.0]])
    );
    assert_eq!(
        copied(&made, "facies 1 - base - rank"),
        Some(Numbers::I64(vec![1, 2]))
    );
    assert_eq!(
        points_of(&made, "facies 2 - base"),
        Some(vec![vec![0.0, 0.0, -30.0]]),
        "the last base of a hole is where the log ends: no contact"
    );
    assert!(
        made.says.iter().any(|note| note.key == "contact-ranks"),
        "the ranks are said"
    );
}

/// The person chooses which of a well's contacts are written: all, the first or the last of each class and side.
///
/// Break it: in `contacts_of`, let `Which::Last` keep `first` instead of `last`.
#[test]
fn the_person_chooses_the_first_or_the_last() {
    let first = contacts(&abab(), json!({ "categoryKind": FACIES_KIND, "which": "first" })).expect("a plan");
    assert_eq!(
        points_of(&first, "facies 2 - top"),
        Some(vec![vec![0.0, 0.0, -20.0]])
    );
    let last = contacts(&abab(), json!({ "categoryKind": FACIES_KIND, "which": "last" })).expect("a plan");
    assert_eq!(
        points_of(&last, "facies 2 - top"),
        Some(vec![vec![0.0, 0.0, -60.0]])
    );
    assert_eq!(
        copied(&last, "facies 2 - top - rank"),
        Some(Numbers::I64(vec![2]))
    );
}

/// With nothing repeated, no rank is written: the output is the one a run always gave.
///
/// Break it: in `rank_properties`, delete the `repeated == 0` return.
#[test]
fn a_hole_with_no_repeated_crossing_writes_no_rank() {
    let mds: Vec<f64> = (0..6).map(|k| k as f64 * 10.0).collect();
    let pieces = with_properties(
        frame(Pieces::empty(), 0, "F1", &mds),
        vec![facies("F1", &[1, 1, 2, 2, 3, 3])],
    );
    let made = contacts(&pieces, json!({ "categoryKind": FACIES_KIND })).expect("a plan");
    assert!(points_of(&made, "facies 2 - top").is_some());
    assert!(
        made.objects.iter().all(|one| !one.title.ends_with(" - rank")),
        "{:?}",
        made.objects.iter().map(|one| &one.title).collect::<Vec<_>>()
    );
}

/// The codes named are the ones whose contacts are written; the others are still classes, so the limit with a
/// kept code is still a contact.
///
/// Break it: in `property_suites`, keep every code whatever `codes` says.
#[test]
fn only_the_codes_named_have_contacts() {
    let made = contacts(&abab(), json!({ "categoryKind": FACIES_KIND, "codes": [2] })).expect("a plan");
    assert!(points_of(&made, "facies 2 - top").is_some());
    assert_eq!(points_of(&made, "facies 1 - top"), None);
}

// -- a well the reading cannot treat --------------------------------------------------------------

/// A well that carries no property of the kind is refused **alone**, with its reason; the operation goes on
/// with the others.
///
/// Break it: in `plan`, return the refusal of the frame instead of setting it aside.
#[test]
fn a_well_the_reading_cannot_treat_is_refused_alone_and_the_others_are_read() {
    let mds: Vec<f64> = (0..8).map(|k| k as f64 * 10.0).collect();
    let pieces = frame(abab(), 1, "F2", &mds);
    let made = contacts(&pieces, json!({ "categoryKind": FACIES_KIND })).expect("F1 is read");
    assert_eq!(
        points_of(&made, "facies 2 - top"),
        Some(vec![vec![0.0, 0.0, -20.0], vec![0.0, 0.0, -60.0]]),
        "F1's contacts, and only its"
    );
    assert!(
        made.dropped
            .iter()
            .any(|one| one.why.contains("`F2`") && one.why.contains("no property of the kind")),
        "{:?}",
        made.dropped.iter().map(|one| &one.why).collect::<Vec<_>>()
    );
    assert!(made.says.iter().any(|note| note.key == "frames-refused"));
}

/// Two readings at once, and an interpolation of what is no threshold, are refused on the setting at fault.
///
/// Break it: in `Mode::of`, delete the `place == Interpolated` test.
#[test]
fn settings_that_contradict_each_other_are_refused_by_name() {
    let both = contacts(
        &abab(),
        json!({ "categoryKind": FACIES_KIND, "thresholdKind": PHI_KIND, "threshold": 0.2 }),
    )
    .expect_err("one reading at a time");
    assert!(both.contains("one thing at a time"), "{both}");
    let lone = contacts(
        &abab(),
        json!({ "categoryKind": FACIES_KIND, "place": "interpolated" }),
    )
    .expect_err("a code has no crossing");
    assert!(lone.contains("thresholdKind"), "{lone}");
    let flat =
        contacts(&abab(), json!({ "thresholdKind": PHI_KIND })).expect_err("a threshold needs its value");
    assert!(flat.contains("threshold"), "{flat}");
}

// -- a threshold on a continuous property -------------------------------------------------------------

/// A porosity 0.1, 0.1, 0.3, 0.3 at 0, 100, 200 and 300 m, a threshold of 0.2.
fn porosity() -> Pieces {
    let mds = [0.0, 100.0, 200.0, 300.0];
    with_properties(
        frame(Pieces::empty(), 0, "F1", &mds),
        vec![
            phi("F1", &[0.1, 0.1, 0.3, 0.3]),
            property(
                "F1",
                "PERM",
                ("55555555-5555-4555-8555-555555555555", "permeability"),
                false,
                "nodes",
                Values::Floating(vec![10.0, 10.0, 30.0, 30.0]),
            ),
        ],
    )
}

fn close(point: &[f64], want: [f64; 3]) -> bool {
    point.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-9)
}

/// Interpolated, the contact is where the threshold is crossed between two nodes - half way from 0.1 to 0.3 -, the
/// point is on the frame's leg there, and the properties copied onto it are read **at its MD**: a continuous
/// one interpolated, not the value of either node.
///
/// Break it: in `contacts_of`, build the point with `nodes[lower]` instead of `between(..)`; in `at_md_cell`,
/// answer `cells.at(lower)` for a property on nodes.
#[test]
fn an_interpolated_contact_is_between_two_nodes_and_its_properties_are_read_at_its_md() {
    let made = contacts(
        &porosity(),
        json!({ "thresholdKind": PHI_KIND, "threshold": 0.2, "place": "interpolated" }),
    )
    .expect("a plan");
    let top = points_of(&made, "porosity at or above 0.2 - top").expect("the top of the stretch at or above");
    assert_eq!(top.len(), 1);
    assert!(close(&top[0], [0.0, 0.0, -150.0]), "{top:?}");

    let Some(Numbers::F64(perm)) = copied(&made, "porosity at or above 0.2 - top - PERM") else {
        panic!("the permeability is copied")
    };
    assert!(
        (perm[0] - 20.0).abs() < 1e-9,
        "10 at 100 m, 30 at 200 m: 20 at 150 m, not either: {perm:?}"
    );
    let Some(Numbers::F64(own)) = copied(&made, "porosity at or above 0.2 - top - PHI") else {
        panic!("the porosity is copied")
    };
    assert!((own[0] - 0.2).abs() < 1e-9, "the threshold itself: {own:?}");
}

/// Inside, a top is on the first node of the stretch; outside, on the node above it.
///
/// Break it: in `Place::anchor`, answer `Anchor::Outside` for `Inside`.
#[test]
fn inside_and_outside_name_the_node() {
    let top_at = |place: &str| {
        let made = contacts(
            &porosity(),
            json!({ "thresholdKind": PHI_KIND, "threshold": 0.2, "place": place }),
        )
        .expect("a plan");
        points_of(&made, "porosity at or above 0.2 - top").expect("a top")[0][2]
    };
    assert_eq!(top_at("inside"), -200.0, "the first node at or above 0.2");
    assert_eq!(top_at("outside"), -100.0, "the last node below it");
}

/// The side kept is the one whose contacts are written: `below` gives the base of the stretch below the threshold.
///
/// Break it: in `property_suites`, class a value `>= threshold` as `Below`.
#[test]
fn the_side_kept_is_the_one_written() {
    let made = contacts(
        &porosity(),
        json!({ "thresholdKind": PHI_KIND, "threshold": 0.2, "thresholdSide": "below" }),
    )
    .expect("a plan");
    let base = points_of(&made, "porosity below 0.2 - base").expect("the base of the stretch below");
    assert_eq!(base[0][2], -100.0, "the last node below 0.2, placed inside");
    assert_eq!(points_of(&made, "porosity at or above 0.2 - top"), None);
}

// -- a contact between two nodes is on the trajectory ----------------------------------------------

/// A deviated hole: its trajectory is a natural cubic through four stations, 100 m apart, and its frame's nodes
/// are the line at those depths.
fn deviated(at: usize, title: &str, bag: Pieces) -> (Pieces, Path) {
    let stations = [
        [0.0, 0.0, 0.0],
        [30.0, 0.0, -100.0],
        [30.0, 0.0, -200.0],
        [0.0, 0.0, -300.0],
    ];
    let mds = [0.0, 100.0, 200.0, 300.0];
    let line = Path::new(2, stations.to_vec(), mds.to_vec(), None, Vertical::Down).expect("a curved line");
    let nodes: Vec<[f64; 3]> = mds.iter().map(|md| line.at(*md)).collect();
    (
        frame_on(bag, at, title, &mds, nodes, Some(AlongHole::Line(line.clone()))),
        line,
    )
}

/// On a deviated hole the point interpolated between two nodes lies **on the trajectory** at the measured depth, and
/// not on the chord between the nodes, which is beside the well.
///
/// Break it: in `contacts_of`, place the point on the chord again (`nodes[lower] + (nodes[upper] - nodes[lower]) * t`).
#[test]
fn an_interpolated_point_is_on_the_trajectory_and_not_on_the_chord() {
    let (pieces, line) = deviated(0, "F1", Pieces::empty());
    let pieces = with_properties(pieces, vec![phi("F1", &[0.1, 0.1, 0.3, 0.3])]);
    let made = contacts(
        &pieces,
        json!({ "thresholdKind": PHI_KIND, "threshold": 0.2, "place": "interpolated" }),
    )
    .expect("a plan");
    let top = points_of(&made, "porosity at or above 0.2 - top").expect("a top");
    let on_the_line = line.at(150.0);
    assert!(close(&top[0], on_the_line), "{top:?} against {on_the_line:?}");
    // The two nodes it lies between are both at x = 30: the chord is at 30, the well is not.
    assert!(
        (top[0][0] - 30.0).abs() > 1.0,
        "the point is at {} where the chord is at 30",
        top[0][0]
    );
}

/// A frame whose trajectory is missing, or is drawn elsewhere than its line, is refused alone in an interpolated
/// reading - never placed on the chord; the others are read. Inside and outside need no line.
///
/// Break it: in `property_suites`, give `path: None` instead of the refusal and fall back on the nodes.
#[test]
fn an_interpolated_frame_without_its_trajectory_is_refused_alone_and_named() {
    let (pieces, _) = deviated(0, "F1", Pieces::empty());
    let mds = [0.0, 100.0, 200.0, 300.0];
    let nodes: Vec<[f64; 3]> = mds.iter().map(|md| [10.0, 0.0, -md]).collect();
    // F2 names a trajectory the package does not hold; both frames carry the same log.
    let pieces = frame_on(pieces, 1, "F2", &mds, nodes, None);
    let pieces = with_properties(
        pieces,
        vec![phi("F1", &[0.1, 0.1, 0.3, 0.3]), phi("F2", &[0.1, 0.1, 0.3, 0.3])],
    );
    let made = contacts(
        &pieces,
        json!({ "thresholdKind": PHI_KIND, "threshold": 0.2, "place": "interpolated" }),
    )
    .expect("F1 is read");
    assert_eq!(
        points_of(&made, "porosity at or above 0.2 - top").map(|points| points.len()),
        Some(1),
        "F1's contact, and only its"
    );
    assert!(
        made.dropped
            .iter()
            .any(|one| one.why.contains("`F2`") && one.why.contains("trajectory")),
        "{:?}",
        made.dropped.iter().map(|one| &one.why).collect::<Vec<_>>()
    );
    // Inside, F2 is read: a node needs no line.
    let inside = contacts(&pieces, json!({ "thresholdKind": PHI_KIND, "threshold": 0.2 })).expect("a plan");
    assert_eq!(
        points_of(&inside, "porosity at or above 0.2 - top").map(|points| points.len()),
        Some(2)
    );
}

// -- classes chosen and named by their labels ---------------------------------------------------

/// The facies log of the hole of eight nodes, with the labels its table gives: 1 is `shale`, 2 is `sand`.
fn labelled() -> Pieces {
    let mds: Vec<f64> = (0..8).map(|k| k as f64 * 10.0).collect();
    let property = Known {
        labels: vec![(1.0, "shale".to_string()), (2.0, "sand".to_string())],
        ..facies("F1", &[1, 1, 2, 2, 1, 1, 2, 2])
    };
    with_properties(frame(Pieces::empty(), 0, "F1", &mds), vec![property])
}

/// A class is chosen by its label and named by it: the point sets of `sand` are titled `sand`, and those of the
/// code the table does not name by the kind and the code.
///
/// Break it: in `property_suites`, never test `labels` (keep the class whatever the labels say).
#[test]
fn a_class_is_chosen_and_named_by_its_label() {
    let made = contacts(
        &labelled(),
        json!({ "categoryKind": FACIES_KIND, "labels": ["sand"] }),
    )
    .expect("a plan");
    assert_eq!(
        points_of(&made, "sand - top"),
        Some(vec![vec![0.0, 0.0, -20.0], vec![0.0, 0.0, -60.0]])
    );
    assert_eq!(points_of(&made, "shale - top"), None, "shale was not chosen");
    assert_eq!(
        points_of(&made, "facies 2 - top"),
        None,
        "named by its label, not by its code"
    );
}

/// Codes and labels together are their **union**: a class is kept when its code is named or its label is.
///
/// Break it: in `property_suites`, keep a class only when both name it (`&&` for the `||`).
#[test]
fn codes_and_labels_together_are_their_union() {
    let made = contacts(
        &labelled(),
        json!({ "categoryKind": FACIES_KIND, "codes": [1], "labels": ["sand"] }),
    )
    .expect("a plan");
    assert!(points_of(&made, "shale - top").is_some() && points_of(&made, "sand - top").is_some());
}

/// A label no row of the table bears is refused by its name - on the frame, alone, with the labels the table has.
///
/// Break it: in `property_suites`, delete the check that a label names a row.
#[test]
fn a_label_the_table_does_not_hold_is_refused_by_name() {
    let why = contacts(
        &labelled(),
        json!({ "categoryKind": FACIES_KIND, "labels": ["gravel"] }),
    )
    .expect_err("no frame can be read");
    assert!(
        why.contains("gravel") && why.contains("`shale`") && why.contains("`sand`"),
        "{why}"
    );
    // A property whose table names nothing cannot be chosen by label.
    let nameless = contacts(
        &abab(),
        json!({ "categoryKind": FACIES_KIND, "labels": ["sand"] }),
    )
    .expect_err("the table gives no label");
    assert!(nameless.contains("gives no label"), "{nameless}");
}

// -- a trajectory with no geometry: the vertical read from its datum ----------------------------------------------

mod vertical {
    //! The trajectory states no `Geometry`: its only placement is the vertical below its datum, which
    //! `well::frame` draws the nodes with. The interpolated contacts use that same door.

    use energyml_core::registry::DynObject;
    use energyml_domain::source::{ArraySource, ObjectSource};
    use energyml_eml_2_3::{
        AbstractFloatingPointArray, AbstractHorizontalCoordinates, AbstractVerticalCrs, DataObjectReference,
        FloatingPointXmlArray, LengthUomExt, MdInterval, ProjectedCoordinates, ReferencePointInACrs,
        VerticalCrs, VerticalDirection, VerticalEpsgCrs,
    };
    use energyml_resqml_2_2::{WellboreFrameRepresentation, WellboreTrajectoryRepresentation};

    use super::*;

    const DATUM: &str = "11111111-1111-4111-8111-aaaaaaaaaaaa";
    const VCRS: &str = "22222222-2222-4222-8222-aaaaaaaaaaaa";
    const TRAJECTORY: &str = "33333333-3333-4333-8333-aaaaaaaaaaaa";

    fn dor(uuid: &str) -> DataObjectReference {
        DataObjectReference {
            uuid: uuid.to_string(),
            ..Default::default()
        }
    }

    fn datum() -> ReferencePointInACrs {
        ReferencePointInACrs {
            uuid: DATUM.to_string(),
            vertical_crs: Some(dor(VCRS)),
            vertical_coordinate: Some(40.0),
            horizontal_coordinates: Some(AbstractHorizontalCoordinates::ProjectedCoordinates(
                ProjectedCoordinates {
                    coordinate1: 1000.0,
                    coordinate2: 2000.0,
                    crs: dor(CRS),
                },
            )),
            ..Default::default()
        }
    }

    fn vertical_crs() -> VerticalCrs {
        VerticalCrs {
            uuid: VCRS.to_string(),
            direction: VerticalDirection::Up,
            abstract_vertical_crs: Some(AbstractVerticalCrs::VerticalEpsgCrs(VerticalEpsgCrs {
                epsg_code: 5714,
            })),
            uom: LengthUomExt::M,
            ..Default::default()
        }
    }

    /// A trajectory with a datum and depth bounds, and no `Geometry`.
    fn trajectory() -> WellboreTrajectoryRepresentation {
        WellboreTrajectoryRepresentation {
            uuid: TRAJECTORY.to_string(),
            md_interval: MdInterval {
                md_min: 0.0,
                md_max: 400.0,
                uom: LengthUomExt::M,
                datum: Some(dor(DATUM)),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// A store of exactly the objects a well without geometry needs.
    struct Site;

    impl ObjectSource for Site {
        fn object(&self, uuid: &str) -> Option<Box<dyn DynObject>> {
            match uuid {
                DATUM => Some(Box::new(datum())),
                VCRS => Some(Box::new(vertical_crs())),
                TRAJECTORY => Some(Box::new(trajectory())),
                _ => None,
            }
        }
    }

    impl ArraySource for Site {
        fn locate(
            &self,
            _object: &dyn energyml_core::value::Reflect,
        ) -> Vec<energyml_storage::ArrayLocation> {
            Vec::new()
        }
        fn metadata(
            &self,
            location: &energyml_storage::ArrayLocation,
        ) -> energyml_domain::Result<energyml_storage::ArrayMetadata> {
            Err(energyml_domain::Error::Missing(location.field_path.clone()))
        }
        fn read(
            &self,
            location: &energyml_storage::ArrayLocation,
        ) -> energyml_domain::Result<energyml_storage::ArrayData> {
            Err(energyml_domain::Error::Missing(location.field_path.clone()))
        }
        fn read_into(
            &self,
            location: &energyml_storage::ArrayLocation,
            _out: &mut energyml_storage::array::ArrayBuffer<'_>,
        ) -> energyml_domain::Result<energyml_storage::ArrayMetadata> {
            Err(energyml_domain::Error::Missing(location.field_path.clone()))
        }
    }

    /// A frame of that trajectory with these node depths.
    fn frame_of(mds: &[f64]) -> WellboreFrameRepresentation {
        WellboreFrameRepresentation {
            uuid: "66666666-6666-4666-8666-aaaaaaaaaaaa".to_string(),
            node_md: Some(AbstractFloatingPointArray::FloatingPointXmlArray(
                FloatingPointXmlArray {
                    count_per_value: 1,
                    values: mds.to_vec(),
                    ..Default::default()
                },
            )),
            trajectory: dor(TRAJECTORY),
            ..Default::default()
        }
    }

    /// The points `well::frame` places for a frame at these depths - through the public reader, `geometry::of`.
    fn drawn(mds: &[f64]) -> Vec<[f64; 3]> {
        let found =
            energyml_geometry::geometry::of(&frame_of(mds), &Site).expect("a frame with no geometry draws");
        found.patches[0].points.clone()
    }

    /// An interpolated threshold contact of a frame without geometry is on the datum's vertical at its depth,
    /// **the point `well::frame` gives for a node at that depth** - both asked, none copied -, and the plan says that
    /// the placement is the vertical read from the datum.
    ///
    /// Break it: in `property_suites`, refuse `along.is_assumed()` as the line used to be refused (the frame is then
    /// read by nobody, and the unwrapped point set is `None`).
    #[test]
    fn a_frame_without_geometry_is_placed_on_the_datums_vertical_as_well_frame_places_it() {
        let mds = [0.0, 100.0, 200.0, 300.0];
        let along = energyml_geometry::well::along_hole(&trajectory(), &Site).expect("the vertical reads");
        assert!(
            along.is_assumed(),
            "no `Geometry`: the placement is the datum's vertical"
        );
        let pieces = frame_on(Pieces::empty(), 0, "F1", &mds, drawn(&mds), Some(along));
        let pieces = with_properties(pieces, vec![phi("F1", &[0.1, 0.1, 0.3, 0.3])]);
        let made = contacts(
            &pieces,
            json!({ "thresholdKind": PHI_KIND, "threshold": 0.2, "place": "interpolated" }),
        )
        .expect("a plan: the frame is no longer refused for want of a geometry");
        let top = points_of(&made, "porosity at or above 0.2 - top").expect("a top");
        // 0.2 is half way from 0.1 (MD 100) to 0.3 (MD 200).
        let expected = drawn(&[150.0])[0];
        assert!(
            close(&top[0], expected),
            "{:?} against what `well::frame` places at MD 150: {expected:?}",
            top[0]
        );
        // On the vertical of the datum: x and y are the datum's, z is its elevation less the depth.
        assert!(
            (top[0][0] - 1000.0).abs() < 1e-9 && (top[0][1] - 2000.0).abs() < 1e-9,
            "{:?}",
            top[0]
        );
        assert!(
            made.says
                .iter()
                .any(|note| note.key == "contacts-on-datum-vertical"),
            "{:?}",
            made.says.iter().map(|note| note.key).collect::<Vec<_>>()
        );
    }
}
