//! `Applies::When` — a slot that acts only for a value of another, and the
//! conditions nesting through it (ADR 0017).
//!
//! # What would have to break to make it turn red
//!
//! Reading the condition on the raw answer rather than on the settled value (a
//! `nugget` would survive the removal of `model`); letting a suggestion of an
//! inapplicable slot enter the settled choices (an IDW run would record
//! `kernel: thin-plate` again); projecting a conditional slot as
//! `MinOccurs 1`; forgetting the category of an operation.

use energyml_build::operation::{Applies, Category, Choices, Identity, Need, Purpose, Registry, Slot, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_domain::provenance::Parameter;
use energyml_geometry::geometry::{Geometry, Patch, Topology};

fn when() -> When {
    When {
        creation: "2026-09-12T00:00:00Z".into(),
        originator: "test".into(),
    }
}

/// Samples on a plane, in a frame of the package.
fn samples() -> Pieces {
    let points: Vec<[f64; 3]> = (0..4)
        .flat_map(|j| (0..4).map(move |i| [i as f64 * 30.0, j as f64 * 30.0, 100.0 + i as f64 + j as f64]))
        .collect();
    Pieces::empty()
        .with(
            "shape",
            Source::Shape {
                geometry: Geometry {
                    kind: "cloud",
                    patches: vec![Patch {
                        index: 0,
                        node_offset: 0,
                        points,
                        topology: Topology::Points,
                        closed: Vec::new(),
                    }],
                },
                closed: None,
            },
        )
        .with(
            "object",
            Source::Object(Known {
                reference: Some(Ref::new(
                    "bbbbbbbb-0000-4000-8000-000000000002",
                    "resqml22.PointSetRepresentation",
                    "Semis",
                )),
                // A compound: a `LocalEngineering2dCrs` is half a system, and
                // since 24/09 naming one as the frame blocks the writing.
                crs: Some(Ref::new(
                    "aaaaaaaa-0000-4000-8000-000000000001",
                    "eml23.LocalEngineeringCompoundCrs",
                    "The frame",
                )),
                vertical_uom: Some("m".into()),
                ..Default::default()
            }),
        )
}

fn titles(parameters: &[Parameter]) -> Vec<&str> {
    parameters
        .iter()
        .map(|p| match p {
            Parameter::Double { title, .. }
            | Parameter::Text { title, .. }
            | Parameter::Object { title, .. } => title.as_str(),
        })
        .collect()
}

/// **An activity records what the run used, and only that.** The defect this
/// fixes: an IDW run recorded `kernel: thin-plate` and `model: fitted`,
/// because those slots had a suggestion and always applied — so they were
/// settled, and whatever is settled is recorded.
#[test]
fn an_activity_only_records_the_settings_of_its_method() {
    let registry = Registry::new();
    let run = |method: &str| {
        let choices = Choices::new()
            .set("method", method)
            .set("spacing", 30.0)
            .set("title", "socle");
        registry
            .plan("interpolate", &samples(), &choices, &Identity::derived(), &when())
            .expect("planned")
    };
    let idw = run("idw");
    let recorded = titles(&idw.activity.parameters);
    assert!(
        recorded.contains(&"power"),
        "IDW's own setting is recorded: {recorded:?}"
    );
    for foreign in ["kernel", "model", "nugget", "sill", "range", "epsilon"] {
        assert!(
            !recorded.contains(&foreign),
            "an IDW run recorded `{foreign}`: {recorded:?}"
        );
    }
    let kriging = run("kriging");
    let recorded = titles(&kriging.activity.parameters);
    assert!(
        recorded.contains(&"model"),
        "kriging's own setting is recorded: {recorded:?}"
    );
    for foreign in ["power", "kernel", "epsilon"] {
        assert!(
            !recorded.contains(&foreign),
            "a kriging run recorded `{foreign}`: {recorded:?}"
        );
    }
}

/// **The conditions nest**: `nugget` acts under `model`, which acts under
/// `method`. Answering a sub-setting of a method that is not chosen is refused
/// naming its controller; and one level down, the same.
#[test]
fn conditions_nest_and_a_setting_outside_its_method_is_refused() {
    let registry = Registry::new();
    let slots = registry.get("interpolate").unwrap().needs(&samples());
    let settle = |choices: Choices| choices.settle_for(&slots, Purpose::Geometry);

    let why = settle(
        Choices::new()
            .set("method", "idw")
            .set("spacing", 30.0)
            .set("kernel", "linear"),
    )
    .unwrap_err();
    assert!(
        why.message()
            .contains("`kernel` acts only when `method` is `rbf`"),
        "{}",
        why.message()
    );

    // Kriging with the fitted model: `nugget` has no place, one level down.
    let why = settle(
        Choices::new()
            .set("method", "kriging")
            .set("spacing", 30.0)
            .set("nugget", 0.1),
    )
    .unwrap_err();
    assert!(
        why.message().contains("`nugget` acts only when `model`"),
        "{}",
        why.message()
    );

    // **Transitively**: a model answered under IDW is out, and so is its
    // `nugget` — read on the model *as settled*, not as answered. Reading the
    // raw answer would let `nugget` stand on a model that was just removed.
    let why = settle(
        Choices::new()
            .set("method", "idw")
            .set("spacing", 30.0)
            .set("model", "spherical")
            .set("nugget", 0.1),
    )
    .unwrap_err();
    assert!(
        why.message().contains("`model` acts only when `method`"),
        "{}",
        why.message()
    );
    assert!(
        why.message().contains("`nugget` acts only when `model`"),
        "{}",
        why.message()
    );

    // Stated model: its three numbers act — and, since the method is a union,
    // **are required** there, refused by name at settling rather than by the
    // plan once the work has started. And the method switched away takes the
    // model **and** its numbers with it, suggestions included.
    let why = settle(
        Choices::new()
            .set("method", "kriging")
            .set("model", "spherical")
            .set("spacing", 30.0)
            .set("nugget", 0.1),
    )
    .unwrap_err();
    assert!(
        why.message().contains("`sill` is required") && why.message().contains("`range` is required"),
        "{}",
        why.message()
    );
    let kriged = settle(
        Choices::new()
            .set("method", "kriging")
            .set("model", "spherical")
            .set("spacing", 30.0)
            .set("nugget", 0.1)
            .set("sill", 1.0)
            .set("range", 50.0),
    )
    .expect("settled");
    assert_eq!(kriged.number("nugget"), Some(0.1));
    let idw = settle(Choices::new().set("method", "idw").set("spacing", 30.0)).expect("settled");
    for gone in ["model", "kernel", "nugget"] {
        assert!(idw.get(gone).is_none(), "`{gone}` settled for IDW: {idw:?}");
    }
    assert_eq!(idw.number("power"), Some(2.0), "IDW's suggestion is settled");
}

/// A controller declared after what it controls is a declaration fault,
/// refused rather than read in an order nobody can see.
#[test]
fn a_controller_is_declared_before_what_it_controls() {
    let slots = vec![
        Slot::optional("kernel", Need::Choice(&["a", "b"]), "").only_when("method", &["rbf"]),
        Slot::required("method", Need::Choice(&["idw", "rbf"]), ""),
    ];
    let why = Choices::new().set("method", "rbf").settle(&slots).unwrap_err();
    assert!(
        why.message().contains("not declared before it"),
        "{}",
        why.message()
    );
}

/// The wire and the template: the form reads `when`, and a conditional slot
/// is `MinOccurs 0` in the template, the condition said in words.
#[test]
fn the_wire_and_the_template_state_the_condition() {
    let registry = Registry::new();
    let slots = registry.get("interpolate").unwrap().needs(&Pieces::empty());
    let nugget = slots.iter().find(|s| s.name == "nugget").unwrap();
    assert_eq!(
        nugget.applies,
        Applies::When {
            slot: "model",
            is: vec!["spherical", "exponential"]
        }
    );
    let wire = nugget.json();
    assert_eq!(wire["applies"]["kind"], "when");
    assert_eq!(wire["applies"]["slot"], "model");
    assert_eq!(wire["advanced"], false);
    // The neighbourhood is advanced — folded away in a form — and says so on
    // the wire, the one slot of two cases keeping the flag.
    let radius = slots.iter().find(|s| s.name == "radius").unwrap();
    assert!(radius.advanced && radius.json()["advanced"] == true);

    let power = slots.iter().find(|s| s.name == "power").unwrap();
    let projected = energyml_build::template::parameter_template(power, true);
    assert_eq!(projected["MinOccurs"], 0);
    assert!(
        projected["Constraint"]
            .as_str()
            .unwrap()
            .contains("only when method is idw"),
        "{projected}"
    );

    // A required slot under a condition is still `MinOccurs 0`: a run that
    // did not meet the condition does not carry it.
    let required_under = Slot::required("x", Need::Text, "").only_when("method", &["idw"]);
    assert_eq!(
        energyml_build::template::parameter_template(&required_under, true)["MinOccurs"],
        0
    );
}

/// Every built-in says where it is ranged; `Other` is for a caller's own.
#[test]
fn every_built_in_operation_has_its_category() {
    let registry = Registry::new();
    let unranged: Vec<&str> = registry
        .iter()
        .filter(|op| op.category() == Category::Other)
        .map(|op| op.name())
        .collect();
    assert!(unranged.is_empty(), "built-ins without a category: {unranged:?}");
    for op in registry.iter() {
        assert_eq!(op.json()["category"], op.category().key());
        assert!(Category::ALL.contains(&op.category()));
    }
}

#[test]
fn a_secondary_category_is_neither_the_main_one_nor_other() {
    // The decision of 26/09: an operation may be filed “also” elsewhere —
    // the cloud of wellheads under “Wells”. A secondary category that
    // repeated the main one would show it twice in the same list, and
    // “other” is not a filing.
    let registry = Registry::new();
    let mut declared = 0;
    for op in registry.iter() {
        for also in op.also() {
            declared += 1;
            assert_ne!(*also, op.category(), "{} repeats its own category", op.name());
            assert_ne!(*also, Category::Other, "{}", op.name());
        }
        let json = op.json();
        let listed: Vec<&str> = json["also"]
            .as_array()
            .expect("also is listed")
            .iter()
            .filter_map(|one| one.as_str())
            .collect();
        assert_eq!(
            listed,
            op.also().iter().map(|one| one.key()).collect::<Vec<_>>(),
            "{}",
            op.name()
        );
    }
    assert!(
        declared > 0,
        "at least one operation declares another category — else this proves nothing"
    );
    let wellheads = registry.get("pointset-from-wellheads").expect("registered");
    assert!(wellheads.also().contains(&Category::Wells));
}

/// The operations that invent their values are filed apart, and only
/// there.
///
/// The need: let the operations of
/// lutetia into the operation list only if the operation allows useful
/// geometric operations for other, more generic use cases. […] Or
/// else a separate category would be needed for these lutetia operations.
///
/// The criterion is read in what the operation declares: an operation that
/// invents **only on request** carries a `synthetic` parameter (`fault-throw`,
/// `witsml-wells`…) and stays in its category; an operation that always
/// invents has no such parameter — it is not a choice for it — and goes into
/// `Synthetic`. An operation of `Synthetic` is filed “also” nowhere,
/// and no operation is filed “also” under `Synthetic`.
#[test]
fn operations_that_invent_are_filed_apart() {
    let registry = Registry::new();
    let empty = Pieces::empty();
    let asks_whether = |op: &dyn energyml_build::operation::AnyOperation| {
        op.needs(&empty).iter().any(|slot| slot.name == "synthetic")
    };
    let mut apart = Vec::new();
    for op in registry.iter() {
        assert!(
            !op.also().contains(&Category::Synthetic),
            "{} is ranged under synthetic as a second home",
            op.name()
        );
        if op.category() == Category::Synthetic {
            apart.push(op.name());
            assert!(
                op.also().is_empty(),
                "{} invents its values and is also listed elsewhere",
                op.name()
            );
            assert!(
                !asks_whether(op),
                "{} asks whether to invent: it is not always synthetic",
                op.name()
            );
        }
    }
    assert!(
        !apart.is_empty(),
        "no operation ranged apart — this proves nothing"
    );
    // The criterion is real: the two that invent on demand do declare it.
    for on_demand in ["fault-throw", "witsml-wells"] {
        assert!(
            asks_whether(registry.get(on_demand).expect("registered")),
            "{on_demand} asks whether to invent"
        );
    }
    for general in [
        "fault-throw",
        "witsml-wells",
        "time-series",
        "well-from-plan",
        "business-associate",
    ] {
        let op = registry.get(general).expect("registered");
        assert_ne!(
            op.category(),
            Category::Synthetic,
            "{general} is general, written for Lutetia or not"
        );
    }
    for invents in [
        "horizon-from-law",
        "well-logs",
        "prodml-doublet",
        "injection-test",
    ] {
        let op = registry.get(invents).expect("registered");
        assert_eq!(
            op.category(),
            Category::Synthetic,
            "{invents} invents every value it writes"
        );
    }
}

/// “Measure” writes what is measured **on** a shape — a property, a
/// table —, never a new shape. The report: why is a grid MBA
/// categorised as “measure” when it creates an object — `interpolate`,
/// `estimate` and `resurface` write a representation and were filed there.
/// To break: put `Category::Analysis` back on `interpolate`.
#[test]
fn measuring_makes_no_representation() {
    let registry = Registry::new();
    let mut measured = 0;
    for op in registry.iter().filter(|op| op.category() == Category::Analysis) {
        measured += 1;
        let shapes: Vec<&str> = op
            .makes()
            .iter()
            .copied()
            .filter(|one| one.ends_with("Representation"))
            .collect();
        assert!(
            shapes.is_empty(),
            "{} is ranged under analysis and makes {shapes:?}",
            op.name()
        );
    }
    assert!(measured > 0, "no operation under analysis — this proves nothing");
    for makes_a_shape in ["interpolate", "estimate", "resurface"] {
        assert_eq!(
            registry.get(makes_a_shape).expect("registered").category(),
            Category::Geometry,
            "{makes_a_shape}"
        );
    }
}
