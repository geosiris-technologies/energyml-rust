//! `contacts-from-frames` — the tops and the bases of the units the frames'
//! intervals name, and what the
//! wells' properties hold there.
//!
//! The shape of the assertions comes from the operation's specification and from TRG 2.2 §5.34
//! (one unit per interval); the frames and their properties are written here.
//! The control that carries the properties is
//! `a_point_takes_what_its_wellbore_holds_at_that_node`: it finds where each
//! point came from **by its coordinates**, not by the rank the operation kept,
//! and reads the copy back through the generic readers — two sources of one
//! fact, neither of them the bookkeeping under test.

use serde_json::{Value, json};

use energyml_build::operation::{Choices, Identity, Numbers, Plan, Registry, When};
use energyml_build::pieces::{Known, KnownProperty, Pieces, Ref, Source};
use energyml_domain::property::IndexableElement;
use energyml_domain::values::Values;
use energyml_geometry::geometry::{Geometry, Patch, Topology};

/// The frames' system — a real uuid, because a copy is read back through the
/// generated types here.
const CRS: &str = "44444444-4444-4444-8444-444444444444";
const PHI_KIND: &str = "11111111-1111-4111-8111-111111111111";
const FACIES_KIND: &str = "22222222-2222-4222-8222-222222222222";
const FACIES_TABLE: &str = "33333333-3333-4333-8333-333333333333";

fn when() -> When {
    When {
        creation: "2026-09-24T00:00:00Z".into(),
        originator: "test".into(),
    }
}

fn unit(name: &str) -> Option<Ref> {
    Some(Ref::new(
        format!("uuid-{name}"),
        "resqml22.StratigraphicUnitInterpretation",
        name,
    ))
}

/// A frame of vertical nodes at depths 0, 1, 2, … and its intervals' units:
/// frame `at` stands at x = 10·at, its node k at z = −k.
fn frame(bag: Pieces, at: usize, title: &str, units: Vec<Option<Ref>>) -> Pieces {
    let name = if at == 0 {
        "frames".to_string()
    } else {
        format!("frames {}", at + 1)
    };
    let points: Vec<[f64; 3]> = (0..=units.len())
        .map(|k| [at as f64 * 10.0, 0.0, -(k as f64)])
        .collect();
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
            geometry,
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
            interval_units: Some(units),
            ..Known::default()
        }),
    )
}

/// Which frame and which node a point is, **from its coordinates alone** —
/// the construction of [`frame`], and nothing the operation kept.
fn where_is(point: &[f64; 3]) -> (usize, usize) {
    ((point[0] / 10.0).round() as usize, (-point[2]).round() as usize)
}

fn plan(pieces: &Pieces) -> Result<Plan, String> {
    plan_with(pieces, Choices::new())
}

fn plan_with(pieces: &Pieces, choices: Choices) -> Result<Plan, String> {
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

/// The points of a point set, by its title.
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

#[test]
fn a_contact_is_the_node_between_two_units_and_nothing_else() {
    // Frame 1: A, A, B — A's base and B's top at the third node (depth 2).
    // Frame 2: nothing, A, B — no top for A: what lies above is no unit, so
    // the limit with it is no contact; A's base and B's top at depth 2.
    let pieces = frame(Pieces::empty(), 0, "F1", vec![unit("A"), unit("A"), unit("B")]);
    let pieces = frame(pieces, 1, "F2", vec![None, unit("A"), unit("B")]);
    let made = plan(&pieces).expect("a plan");

    assert_eq!(
        points_of(&made, "A — base"),
        Some(vec![vec![0.0, 0.0, -2.0], vec![10.0, 0.0, -2.0]])
    );
    // **Two point sets, even with the same points**: the base of A and the
    // top of B part where erosion took something away.
    assert_eq!(
        points_of(&made, "B — top"),
        Some(vec![vec![0.0, 0.0, -2.0], vec![10.0, 0.0, -2.0]])
    );
    assert_eq!(
        points_of(&made, "A — top"),
        None,
        "the top of a log is not a contact, nor a limit with nothing"
    );
    assert_eq!(
        points_of(&made, "B — base"),
        None,
        "the bottom of a log is not a contact"
    );

    // Each set represents a horizon of its own boundary.
    let set = made
        .objects
        .iter()
        .find(|one| one.title == "B — top" && one.qualified_type.ends_with("PointSetRepresentation"))
        .unwrap();
    assert_eq!(
        set.json["RepresentedObject"]["QualifiedType"],
        Value::from("resqml22.HorizonInterpretation")
    );
}

#[test]
fn a_unit_that_comes_back_in_one_hole_is_refused() {
    let pieces = frame(Pieces::empty(), 0, "F1", vec![unit("A"), unit("B"), unit("A")]);
    let why = plan(&pieces).expect_err("two tops in one hole");
    assert!(why.contains("F1") && why.contains("twice"), "{why}");
}

#[test]
fn frames_that_map_no_unit_are_refused_towards_the_import() {
    let pieces = Pieces::empty().with(
        "frames",
        Source::Shape {
            geometry: Geometry {
                kind: "resqml22.WellboreFrameRepresentation",
                patches: vec![Patch {
                    index: 0,
                    node_offset: 0,
                    points: vec![[0.0, 0.0, 0.0], [0.0, 0.0, -1.0]],
                    topology: Topology::Points,
                    closed: Vec::new(),
                }],
            },
            closed: None,
        },
    );
    let why = plan(&pieces).expect_err("nothing to extract");
    assert!(why.contains("wells-and-properties-from-table"), "{why}");
}

// -- what the wells' properties hold at a contact (24/09) --------------------

/// A property of the frame titled `frame`, as the boundary resolves it
/// (`read::known_of_object`): titled `<frame> — <measure>`, as
/// `properties-from-table` titles them.
fn property(frame: &str, measure: &str, class: &str, on: &str, values: Values) -> Known {
    let discrete = class == "DiscreteProperty";
    Known {
        reference: Some(Ref::new(
            format!("uuid-{frame}-{measure}-{on}"),
            format!("resqml22.{class}"),
            format!("{frame} — {measure}"),
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
            kind: Some(if discrete {
                Ref::new(FACIES_KIND, "eml23.PropertyKind", "facies")
            } else {
                Ref::new(PHI_KIND, "eml23.PropertyKind", "porosity")
            }),
            lookup: discrete.then(|| Ref::new(FACIES_TABLE, "eml23.ColumnBasedTable", "facies")),
            null_value: discrete.then_some(-1),
            facets: Vec::new(),
            values: Ok(values),
        }),
        ..Known::default()
    }
}

/// A porosity log.
fn phi(frame: &str, on: &str, values: &[f64]) -> Known {
    property(
        frame,
        "PHI",
        "ContinuousProperty",
        on,
        Values::Floating(values.to_vec()),
    )
}

/// A facies log, whose codes are ranks in [`FACIES_TABLE`] and whose null is -1.
fn facies(frame: &str, codes: &[i64]) -> Known {
    property(
        frame,
        "FACIES",
        "DiscreteProperty",
        "nodes",
        Values::Integer(codes.to_vec()),
    )
}

/// The bag, with these properties in the slot `properties`.
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

/// A copy's values, by its title.
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

/// A copy, read back **by the generic readers** — never by the conventions it
/// was written with: its JSON through `energyml_core::json`, the path `apply`
/// takes; the property through `energyml_domain::property::read`; its array,
/// and its point set's, where `energyml_storage::array::locate` finds them.
struct ReadBack {
    indexable: IndexableElement,
    per_element: u64,
    kind: Option<String>,
    lookup: Option<String>,
    values: Numbers,
    points: Vec<[f64; 3]>,
}

fn read_back(made: &Plan, title: &str) -> ReadBack {
    let registry = energyml_schemas::all();
    let object = made
        .objects
        .iter()
        .find(|one| one.title == title && one.qualified_type.ends_with("Property"))
        .unwrap_or_else(|| panic!("no copy titled `{title}`"));
    let document = energyml_core::json::read_value(&object.json, &registry)
        .unwrap_or_else(|why| panic!("`{title}` does not read back: {why}"));
    let property = energyml_domain::property::read(document.root.as_reflect())
        .expect("a property, to the generic reader");
    let dataset = |object: &dyn energyml_core::value::Reflect| -> Numbers {
        let located = energyml_storage::array::locate(object);
        assert_eq!(located.len(), 1, "one array: {located:?}");
        made.datasets
            .iter()
            .find(|one| one.path == located[0].path_in_file)
            .unwrap_or_else(|| panic!("no dataset at `{}`", located[0].path_in_file))
            .values
            .clone()
    };
    let values = dataset(document.root.as_reflect());
    let set_uuid = property
        .supporting_representation
        .clone()
        .expect("it names what it hangs on");
    let set = made
        .objects
        .iter()
        .find(|one| one.uuid == set_uuid)
        .expect("the point set it names is in the plan");
    let set = energyml_core::json::read_value(&set.json, &registry).expect("the point set reads back");
    let Numbers::F64(flat) = dataset(set.root.as_reflect()) else {
        panic!("points are doubles")
    };
    ReadBack {
        indexable: property.indexable,
        per_element: property.values_per_element(),
        kind: match &property.kind {
            Some(energyml_domain::property::PropertyKindRef::Object { uuid, .. }) => Some(uuid.clone()),
            _ => None,
        },
        lookup: property.lookup.clone(),
        values,
        points: flat.chunks(3).map(|p| [p[0], p[1], p[2]]).collect(),
    }
}

#[test]
fn a_point_takes_what_its_wellbore_holds_at_that_node() {
    // F1: A, A, B — its contact is node 2. F2: A, B, B — node 1.
    let pieces = frame(Pieces::empty(), 0, "F1", vec![unit("A"), unit("A"), unit("B")]);
    let pieces = frame(pieces, 1, "F2", vec![unit("A"), unit("B"), unit("B")]);
    let f1 = [0.10, 0.11, 0.12, 0.13];
    let f2 = [0.20, 0.21, 0.22, 0.23];
    let pieces = with_properties(pieces, vec![phi("F1", "nodes", &f1), phi("F2", "nodes", &f2)]);
    let made = plan(&pieces).expect("a plan");

    // **The rank the contact was found at**, never a depth looked up again:
    // F1's node 2, F2's node 1, in the order of the frames.
    assert_eq!(
        copied(&made, "A — base — PHI"),
        Some(Numbers::F64(vec![0.12, 0.21]))
    );
    // A node has one value, whichever side of it a point set is.
    assert_eq!(
        copied(&made, "B — top — PHI"),
        Some(Numbers::F64(vec![0.12, 0.21]))
    );

    // **Two sources.** Where each point came from is found by where it is,
    // and the value expected there is the frame's own. Taking the neighbouring
    // rank, or mixing two frames up, would put a plausible porosity on every
    // point — the numbers above would still be numbers.
    let frames: [&[f64]; 2] = [&f1, &f2];
    for title in ["A — base — PHI", "B — top — PHI"] {
        let back = read_back(&made, title);
        // On a point set a point **is** a node, whatever the source hung on.
        assert_eq!(back.indexable, IndexableElement::Nodes);
        assert_eq!(back.per_element, 1);
        assert_eq!(
            back.kind.as_deref(),
            Some(PHI_KIND),
            "the sources' kind, by reference"
        );
        let Numbers::F64(values) = &back.values else {
            panic!("a continuous copy holds doubles")
        };
        assert_eq!(back.points.len(), values.len(), "one value per point");
        for (point, value) in back.points.iter().zip(values) {
            let (well, node) = where_is(point);
            assert_eq!(*value, frames[well][node], "{title}, the point at {point:?}");
        }
    }
}

#[test]
fn an_interval_property_is_read_on_the_units_own_side() {
    // F1: A, A, B — node 2 is where interval 1 (A's last) ends and interval 2
    // (B's first) begins. F2: A, B, B — node 1, between intervals 0 and 1.
    let pieces = frame(Pieces::empty(), 0, "F1", vec![unit("A"), unit("A"), unit("B")]);
    let pieces = frame(pieces, 1, "F2", vec![unit("A"), unit("B"), unit("B")]);
    let f1 = [0.1, 0.2, 0.3];
    let f2 = [0.5, 0.6, 0.7];
    let pieces = with_properties(
        pieces,
        vec![phi("F1", "intervals", &f1), phi("F2", "intervals", &f2)],
    );
    let made = plan(&pieces).expect("a plan");

    // **The base of A takes A's last interval, the top of B its first.** The
    // same node, two values — what two point sets were written for. Reading
    // the node's rank as an interval would give the base of A B's value.
    assert_eq!(
        copied(&made, "A — base — PHI"),
        Some(Numbers::F64(vec![0.2, 0.5]))
    );
    assert_eq!(copied(&made, "B — top — PHI"), Some(Numbers::F64(vec![0.3, 0.6])));

    // Two sources again, and the rule in intervals: node k is where interval
    // k − 1 ends and interval k begins.
    let frames: [&[f64]; 2] = [&f1, &f2];
    for (title, below) in [("A — base — PHI", false), ("B — top — PHI", true)] {
        let back = read_back(&made, title);
        assert_eq!(
            back.indexable,
            IndexableElement::Nodes,
            "a copy hangs on the point set's nodes"
        );
        let Numbers::F64(values) = &back.values else {
            panic!("doubles")
        };
        for (point, value) in back.points.iter().zip(values) {
            let (well, node) = where_is(point);
            let interval = if below { node } else { node - 1 };
            assert_eq!(*value, frames[well][interval], "{title}, the point at {point:?}");
        }
    }
}

#[test]
fn a_wellbore_without_the_property_gives_its_points_a_null() {
    // Three wells; PHI on F1 and F3 only, and F3's own value is null at its
    // contact.
    let pieces = frame(Pieces::empty(), 0, "F1", vec![unit("A"), unit("A"), unit("B")]);
    let pieces = frame(pieces, 1, "F2", vec![unit("A"), unit("B"), unit("B")]);
    let pieces = frame(pieces, 2, "F3", vec![unit("A"), unit("A"), unit("B")]);
    let pieces = with_properties(
        pieces,
        vec![
            phi("F1", "nodes", &[0.10, 0.11, 0.12, 0.13]),
            phi("F3", "nodes", &[0.3, 0.3, f64::NAN, 0.3]),
        ],
    );
    let made = plan(&pieces).expect("a plan");

    // **Anticipated over every well**: the copy holds a value per point, F2's
    // included — null, since F2 carries no PHI.
    let Some(Numbers::F64(base)) = copied(&made, "A — base — PHI") else {
        panic!("the copy: {:?}", made.says)
    };
    assert_eq!(base.len(), 3, "one value per point");
    assert_eq!(base[0], 0.12);
    assert!(base[1].is_nan(), "F2 carries no PHI");
    assert!(base[2].is_nan(), "F3's own value is null at its contact");

    // The two nulls are told apart, and counted over both point sets.
    let note = made
        .says
        .iter()
        .find(|n| n.key == "properties-null")
        .expect("the nulls are counted");
    assert!(note.detail.starts_with("4 "), "{}", note.detail);
    assert!(
        note.detail.contains("2 where the wellbore does not carry")
            && note.detail.contains("2 where its own value is null"),
        "{}",
        note.detail
    );
}

#[test]
fn a_discrete_copy_names_the_same_table_and_states_the_runs_null() {
    // F1's code at its contact is its own null (-1); F2's is 4.
    let pieces = frame(Pieces::empty(), 0, "F1", vec![unit("A"), unit("A"), unit("B")]);
    let pieces = frame(pieces, 1, "F2", vec![unit("A"), unit("B"), unit("B")]);
    let pieces = with_properties(
        pieces,
        vec![facies("F1", &[0, 1, -1, 2]), facies("F2", &[3, 4, 4, 4])],
    );
    // `wellOrigin` off: its table and property are written, not copied, and
    // this test is about what a copy carries (`tests/contacts_well_origin.rs`).
    let made = plan_with(&pieces, Choices::new().set("wellOrigin", false)).expect("a plan");

    assert_eq!(
        copied(&made, "A — base — FACIES"),
        Some(Numbers::I64(vec![-9999, 4])),
        "a source's null becomes the run's"
    );
    let object = made
        .objects
        .iter()
        .find(|one| one.title == "A — base — FACIES")
        .expect("the copy");
    assert_eq!(object.qualified_type, "resqml22.DiscreteProperty");
    // **The same table, by reference**: a code is read against the very rows
    // it was a rank in.
    assert_eq!(object.json["CategoryLookup"]["Uuid"], json!(FACIES_TABLE));
    assert_eq!(
        object.json["CategoryLookup"]["QualifiedType"],
        json!("eml23.ColumnBasedTable")
    );
    assert_eq!(object.json["PropertyKind"]["Uuid"], json!(FACIES_KIND));
    // The null is on the array, where RESQML 2.2 puts it.
    assert_eq!(object.json["ValuesForPatch"][0]["NullValue"], json!(-9999));
    // …and nothing is copied: no table, no kind is written.
    assert!(
        made.objects
            .iter()
            .all(|one| one.qualified_type != "eml23.ColumnBasedTable"
                && one.qualified_type != "eml23.PropertyKind"),
        "a reference, not a copy"
    );
    // The generic reader agrees on the table.
    assert_eq!(
        read_back(&made, "A — base — FACIES").lookup.as_deref(),
        Some(FACIES_TABLE)
    );

    // The null is a setting, and the one given is the one written.
    let other = plan_with(&pieces, Choices::new().set("nullValue", -5)).expect("a plan");
    assert_eq!(
        copied(&other, "A — base — FACIES"),
        Some(Numbers::I64(vec![-5, 4]))
    );
}

#[test]
fn a_code_equal_to_the_declared_null_is_refused_by_name() {
    // 4 is a real code of F1 here; declared as the null, it would read as
    // one.
    let pieces = frame(Pieces::empty(), 0, "F1", vec![unit("A"), unit("B"), unit("B")]);
    let pieces = with_properties(pieces, vec![facies("F1", &[3, 4, 4, 4])]);
    let made = plan_with(&pieces, Choices::new().set("nullValue", 4))
        .expect("a plan: the contacts do not depend on it");
    assert_eq!(copied(&made, "A — base — FACIES"), None);
    assert!(
        made.dropped.iter().any(|one| one.why.contains("nullValue")),
        "{:?}",
        made.dropped
    );
}

#[test]
fn what_cannot_be_copied_is_set_aside_by_name() {
    let pieces = frame(Pieces::empty(), 0, "F1", vec![unit("A"), unit("A"), unit("B")]);
    let mut vector = phi("F1", "nodes", &[0.0; 12]);
    vector.reference = Some(Ref::new("uuid-vector", "resqml22.ContinuousProperty", "F1 — DIP"));
    vector.property.as_mut().expect("a property").value_count = vec![3];
    let mut timed = phi("F1", "nodes", &[0.0; 4]);
    timed.reference = Some(Ref::new(
        "uuid-timed",
        "resqml22.ContinuousProperty",
        "F1 — PRESSURE",
    ));
    timed.property.as_mut().expect("a property").timed = true;
    let flag = property(
        "F1",
        "OK",
        "BooleanProperty",
        "nodes",
        Values::Boolean(vec![true; 4]),
    );
    let elsewhere = phi("F9", "nodes", &[1.0; 4]);
    let pieces = with_properties(
        pieces,
        vec![
            phi("F1", "nodes", &[0.1, 0.2, 0.3, 0.4]),
            vector,
            timed,
            flag,
            elsewhere,
        ],
    );
    let made = plan(&pieces).expect("a plan: what cannot be copied does not stop the contacts");

    assert!(copied(&made, "A — base — PHI").is_some(), "what can be copied is");
    // **Named, with its reason** — a vector read as a scalar, a time series
    // read at one step, a flag given a value nobody stated would each look
    // exactly like a right copy.
    for (name, measure, cause) in [
        ("F1 — DIP", "DIP", "component"),
        ("F1 — PRESSURE", "PRESSURE", "time"),
        ("F1 — OK", "OK", "null"),
    ] {
        assert!(
            made.dropped
                .iter()
                .any(|one| one.why.contains(name) && one.why.contains(cause)),
            "`{name}` is set aside with its reason: {:?}",
            made.dropped
        );
        assert!(
            made.objects.iter().all(|one| !one.title.ends_with(measure)),
            "`{name}` is not written"
        );
    }
    // A property of a frame this run did not extract from is passed over,
    // and counted.
    let note = made
        .says
        .iter()
        .find(|n| n.key == "properties-elsewhere")
        .expect("counted");
    assert!(note.detail.starts_with("1 "), "{}", note.detail);
}

#[test]
fn two_units_for_one_measure_are_named_not_converted() {
    let pieces = frame(Pieces::empty(), 0, "F1", vec![unit("A"), unit("A"), unit("B")]);
    let pieces = frame(pieces, 1, "F2", vec![unit("A"), unit("B"), unit("B")]);
    let mut percent = phi("F2", "nodes", &[20.0, 21.0, 22.0, 23.0]);
    percent.property.as_mut().expect("a property").uom = Some("%".into());
    let pieces = with_properties(
        pieces,
        vec![phi("F1", "nodes", &[0.10, 0.11, 0.12, 0.13]), percent],
    );
    let made = plan(&pieces).expect("a plan: the contacts do not depend on it");

    // One property holds one unit, and converting would choose it for
    // somebody.
    assert_eq!(copied(&made, "A — base — PHI"), None);
    let refused = made
        .dropped
        .iter()
        .find(|one| one.why.contains("m3/m3") && one.why.contains('%'))
        .unwrap_or_else(|| panic!("the two units are named: {:?}", made.dropped));
    assert_eq!(refused.count, 2, "both sources are counted");
    assert!(
        refused.why.contains("F1 — PHI") && refused.why.contains("F2 — PHI"),
        "{}",
        refused.why
    );
    assert!(
        points_of(&made, "A — base").is_some(),
        "the point sets are still written"
    );
}

#[test]
fn a_wellbore_with_two_properties_of_one_measure_is_not_guessed() {
    // What two runs of `properties-from-table` leave behind when the values
    // changed (§15): two `F1 — PHI` on one frame.
    let pieces = frame(Pieces::empty(), 0, "F1", vec![unit("A"), unit("A"), unit("B")]);
    let mut second = phi("F1", "nodes", &[0.5, 0.5, 0.5, 0.5]);
    second.reference = Some(Ref::new(
        "uuid-F1-PHI-second",
        "resqml22.ContinuousProperty",
        "F1 — PHI",
    ));
    let both = with_properties(
        pieces.clone(),
        vec![phi("F1", "nodes", &[0.1, 0.2, 0.3, 0.4]), second],
    );
    let made = plan(&both).expect("a plan");
    assert_eq!(
        copied(&made, "A — base — PHI"),
        None,
        "which of the two is the well's cannot be guessed"
    );
    assert!(
        made.dropped
            .iter()
            .any(|one| one.why.contains("carries two") && one.why.contains("`properties`")),
        "the refusal names the remedy: {:?}",
        made.dropped
    );

    // The remedy it names: the one given in `properties` is copied.
    let one = with_properties(pieces, vec![phi("F1", "nodes", &[0.1, 0.2, 0.3, 0.4])]);
    let made = plan(&one).expect("a plan");
    assert_eq!(copied(&made, "A — base — PHI"), Some(Numbers::F64(vec![0.3])));
}

#[test]
fn a_copy_that_would_be_all_null_is_not_written_and_is_counted() {
    // An array of nulls looks exactly like data.
    let pieces = frame(Pieces::empty(), 0, "F1", vec![unit("A"), unit("A"), unit("B")]);
    let pieces = with_properties(pieces, vec![phi("F1", "nodes", &[0.1, 0.2, f64::NAN, 0.4])]);
    let made = plan(&pieces).expect("a plan");
    assert_eq!(copied(&made, "A — base — PHI"), None);
    assert_eq!(copied(&made, "B — top — PHI"), None);
    let note = made
        .says
        .iter()
        .find(|n| n.key == "properties-empty")
        .expect("counted");
    assert!(note.detail.starts_with("2 "), "{}", note.detail);
}

#[test]
fn copy_properties_off_copies_nothing_and_says_so() {
    let pieces = frame(Pieces::empty(), 0, "F1", vec![unit("A"), unit("A"), unit("B")]);
    let pieces = with_properties(pieces, vec![phi("F1", "nodes", &[0.1, 0.2, 0.3, 0.4])]);
    // `wellOrigin` off too: the well-origin property is written, not copied,
    // and is switched by its own parameter (`tests/contacts_well_origin.rs`).
    let off = plan_with(
        &pieces,
        Choices::new()
            .set("copyProperties", false)
            .set("wellOrigin", false),
    )
    .expect("a plan");
    assert!(
        off.objects
            .iter()
            .all(|one| !one.qualified_type.ends_with("Property")),
        "nothing copied"
    );
    let note = off
        .says
        .iter()
        .find(|n| n.key == "properties-off")
        .expect("said, not silent");
    assert!(note.detail.starts_with("1 "), "{}", note.detail);

    // And the point sets are **the same objects** either way: the switch
    // writes objects beside them, never different ones.
    let on = plan(&pieces).expect("a plan");
    assert!(
        on.objects
            .iter()
            .any(|one| one.qualified_type == "resqml22.ContinuousProperty"),
        "on by default"
    );
    let sets = |made: &Plan| {
        made.objects
            .iter()
            .filter(|one| one.qualified_type == "resqml22.PointSetRepresentation")
            .map(|one| one.uuid.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(sets(&off), sets(&on));
}

#[test]
fn a_crs_answered_in_the_form_is_not_taken_for_a_property() {
    // The binder hands `properties` every object piece no slot is named
    // after, and the front end sends the CRS picked in the form back as one, named
    // after its setting. That is an answer: refusing it would make the form
    // unusable the moment somebody picks a CRS.
    let pieces = frame(Pieces::empty(), 0, "F1", vec![unit("A"), unit("A"), unit("B")]).with(
        "crs",
        Source::Object(Known {
            reference: Some(Ref::new(CRS, "eml23.ProjectedCrs", "EPSG:2154")),
            crs: Some(Ref::new(CRS, "eml23.ProjectedCrs", "EPSG:2154")),
            ..Known::default()
        }),
    );
    let made = plan(&pieces).expect("an answer is no refusal");
    assert!(made.dropped.is_empty(), "{:?}", made.dropped);
    assert!(points_of(&made, "A — base").is_some());
}

// -- the boundary: what a property of the package is known as ----------------

#[test]
fn the_boundary_reads_a_named_property_and_only_a_frames_among_every_one() {
    use energyml_eml_2_3 as eml;
    use energyml_resqml_2_2 as resqml;

    const FRAME: &str = "0a000000-0000-4000-8000-000000000001";
    const TRAJECTORY: &str = "0a000000-0000-4000-8000-000000000002";
    const CLOUD: &str = "0a000000-0000-4000-8000-000000000003";
    const ON_FRAME: &str = "0a000000-0000-4000-8000-000000000004";
    const ON_CLOUD: &str = "0a000000-0000-4000-8000-000000000005";

    let registry = std::sync::Arc::new(energyml_schemas::all());
    let mut store = energyml_build::epc::open(None, "properties.epc", registry).expect("an empty package");
    let dor = |uuid: &str, qualified: &str, title: &str| eml::DataObjectReference {
        uuid: uuid.to_string(),
        qualified_type: qualified.to_string(),
        title: title.to_string(),
        ..Default::default()
    };
    let citation = |title: &str| eml::Citation {
        title: title.to_string(),
        originator: "test".to_string(),
        creation: "2026-09-24T00:00:00Z".to_string(),
        format: "test".to_string(),
        ..Default::default()
    };
    store
        .put(Box::new(resqml::WellboreFrameRepresentation {
            uuid: FRAME.to_string(),
            schema_version: "2.2".to_string(),
            citation: citation("F1 — frame"),
            node_count: 3,
            node_md: Some(eml::AbstractFloatingPointArray::FloatingPointXmlArray(
                eml::FloatingPointXmlArray {
                    count_per_value: 1,
                    values: vec![0.0, 1.0, 2.0],
                    ..Default::default()
                },
            )),
            trajectory: dor(
                TRAJECTORY,
                "resqml22.WellboreTrajectoryRepresentation",
                "F1 — trajectory",
            ),
            ..Default::default()
        }))
        .expect("the frame is a part");
    store
        .put(Box::new(resqml::PointSetRepresentation {
            uuid: CLOUD.to_string(),
            schema_version: "2.2".to_string(),
            citation: citation("cloud"),
            ..Default::default()
        }))
        .expect("the cloud is a part");
    // Inline values: what the boundary reads needs no array backend, so this
    // runs in every build.
    let codes = |uuid: &str, support: (&str, &str), title: &str| resqml::DiscreteProperty {
        uuid: uuid.to_string(),
        schema_version: "2.2".to_string(),
        citation: citation(title),
        indexable_element: eml::IndexableElement::Nodes,
        value_count_per_indexable_element: vec![1],
        property_kind: dor(FACIES_KIND, "eml23.PropertyKind", "facies"),
        supporting_representation: dor(support.0, support.1, "support"),
        category_lookup: Some(dor(FACIES_TABLE, "eml23.ColumnBasedTable", "facies")),
        values_for_patch: vec![resqml::AbstractValueArray::IntegerXmlArray(
            eml::IntegerXmlArray {
                count_per_value: 1,
                null_value: -1,
                values: vec![4, -1, 7],
                ..Default::default()
            },
        )],
        ..Default::default()
    };
    store
        .put(Box::new(codes(
            ON_FRAME,
            (FRAME, "resqml22.WellboreFrameRepresentation"),
            "F1 — FACIES",
        )))
        .expect("a part");
    store
        .put(Box::new(codes(
            ON_CLOUD,
            (CLOUD, "resqml22.PointSetRepresentation"),
            "cloud — FACIES",
        )))
        .expect("a part");

    let known = energyml_build::read::known_of_object(&store, ON_FRAME).expect("what is known");
    assert!(
        known.crs.is_none(),
        "a property is not taken for a coordinate system"
    );
    let facts = known.property.expect("a property is described as one");
    assert_eq!(
        facts.supporting.as_ref().map(|one| one.uuid.as_str()),
        Some(FRAME)
    );
    assert_eq!(facts.indexable, "nodes");
    assert_eq!(facts.value_count, vec![1]);
    assert_eq!(
        facts
            .kind
            .as_ref()
            .map(|one| (one.uuid.as_str(), one.qualified_type.as_str())),
        Some((FACIES_KIND, "eml23.PropertyKind")),
        "the reference the property writes, whole"
    );
    assert_eq!(
        facts.lookup.as_ref().map(|one| one.uuid.as_str()),
        Some(FACIES_TABLE)
    );
    assert_eq!(
        facts.null_value,
        Some(-1),
        "read off the array, where RESQML 2.2 puts it"
    );
    assert_eq!(facts.values, Ok(Values::Integer(vec![4, -1, 7])));

    // **Named**, a property on anything has its values read: one property,
    // chosen — a grid's, for `property-time-series` from a recipe (27/09).
    let other = energyml_build::read::known_of_object(&store, ON_CLOUD).expect("what is known");
    let facts = other.property.expect("still described as a property");
    assert_eq!(
        facts.values,
        Ok(Values::Integer(vec![4, -1, 7])),
        "named, it is read whatever it hangs on"
    );

    // **Met by `every`**, only a frame's are: that slot passes every property of
    // the package, and a grid's can be millions. The other says why, naming
    // what it hangs on, rather than arriving empty.
    let takes = Registry::new()
        .get("contacts-from-frames")
        .expect("registered")
        .takes();
    let bag = Pieces::empty().with("frames", Source::Object(Known::default()));
    let filled = energyml_build::read::every(&store, &takes, bag, energyml_geometry::Placement::Local)
        .expect("the properties fill");
    let met = |uuid: &str| {
        filled
            .iter()
            .find_map(|(name, source)| match source {
                Source::Object(known)
                    if name.starts_with("properties")
                        && known.reference.as_ref().is_some_and(|one| one.uuid == uuid) =>
                {
                    known.property.clone()
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("{uuid} fills `properties`"))
    };
    assert_eq!(
        met(ON_FRAME).values,
        Ok(Values::Integer(vec![4, -1, 7])),
        "a frame's, met by `every`, is read"
    );
    let why = met(ON_CLOUD)
        .values
        .expect_err("a point set's, met by `every`, is not");
    assert!(
        why.contains("PointSetRepresentation"),
        "the reason names what it hangs on: {why}"
    );
}
