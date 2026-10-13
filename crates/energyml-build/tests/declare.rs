//! The declaration derived from types - (ADR 0017).
//! Every type here is declared **in the test**: a fake operation in
//! the registry would be an entry the front end offers to somebody.
//!
//! # What would have to break to make it turn red
//!
//! Taking the whole doc of a field as its label; reading the absence of a field
//! without its default value; declaring twice a setting that two cases of a
//! union share; ordering `surfaces 2` before `surfaces`; binding a piece without a
//! name to a slot when two slots of its kind could take it;
//! silently losing a suggestion set on a slot that does not exist.

use energyml_build::declare::{Choice, Epsg, Inputs, Params, Shape, Suggestions};
use energyml_build::operation::{
    About, Applies, Category, Choices, Context, Fitness, Need, Operation, Plan, Refusal, Registry, typed,
};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_geometry::geometry::{Geometry, Patch, Topology};

// ---------------------------------------------------------------- a block of parameters

#[derive(Debug, Clone, Copy, PartialEq, Eq, Choice)]
enum Kernel {
    ThinPlate,
    #[choice(name = "mq")]
    Multiquadric,
}

/// A struct of every shape a field can take.
#[derive(Debug, Params)]
struct Settings {
    /// the cell size
    ///
    /// This second paragraph is for whoever reads the code - not a label.
    #[param(min = 0.0, uom = "m")]
    cell_size: f64,
    /// how many neighbours
    #[param(default = 8usize, optional)]
    neighbours: usize,
    /// a code, when there is one
    source_epsg: Option<Epsg>,
    /// several words
    #[param(least = 1, ordered)]
    words: Vec<String>,
    /// the kernel
    #[param(default = Kernel::ThinPlate, advanced)]
    kernel: Kernel,
}

#[test]
fn a_field_is_declared_by_its_type_its_doc_and_its_attributes() {
    let slots = Settings::slots();
    let names: Vec<&str> = slots.iter().map(|s| s.name).collect();
    assert_eq!(names, ["cellSize", "neighbours", "sourceEpsg", "words", "kernel"]);

    let cell = &slots[0];
    assert_eq!(cell.about, "the cell size", "the first paragraph only");
    assert_eq!(
        cell.need,
        Need::Number {
            uom: Some("m"),
            least: Some(0.0),
            most: None
        }
    );
    assert_eq!((cell.least, cell.most), (1, Some(1)), "a plain type is required");

    assert_eq!(
        slots[1].least, 0,
        "`optional` with a default: may be left out, and then takes it"
    );
    assert_eq!(slots[1].suggested, Some(serde_json::json!(8)));
    assert_eq!(slots[2].need, Need::Epsg);
    assert_eq!(
        (slots[2].least, slots[2].most),
        (0, Some(1)),
        "an `Option` may be absent"
    );
    assert_eq!(
        (slots[3].least, slots[3].most, slots[3].ordered),
        (1, None, true),
        "a `Vec` takes several"
    );
    assert_eq!(slots[4].need, Need::Choice(&["thin-plate", "mq"]));
    assert_eq!(slots[4].suggested, Some(serde_json::json!("thin-plate")));
    assert!(slots[4].advanced);
}

#[test]
fn the_default_value_is_the_suggestion_and_the_reading_of_the_absence() {
    // **One expression, two readers.** Settled or not, an absent field reads as
    // its default - the value the form suggested.
    let read = Settings::read(
        &Choices::new()
            .set("cellSize", 25.0)
            .set("words", serde_json::json!(["a"])),
    )
    .unwrap();
    assert_eq!(read.cell_size, 25.0);
    assert_eq!(read.neighbours, 8);
    assert_eq!(read.kernel, Kernel::ThinPlate);
    assert_eq!(read.source_epsg, None);
    assert_eq!(read.words, ["a"]);

    // A wrong value is refused **on its slot**, by name.
    let why = Settings::read(&Choices::new().set("cellSize", "wide")).unwrap_err();
    assert_eq!(why.slot.as_deref(), Some("cellSize"));
    assert!(why.message.contains("`cellSize`"), "{}", why.message);
    let why = Settings::read(&Choices::new().set("cellSize", 1.0).set("kernel", "cubic")).unwrap_err();
    assert!(why.message.contains("thin-plate, mq"), "{}", why.message);
}

// ---------------------------------------------------------------- a union

/// the variogram model
#[derive(Debug, PartialEq, Params)]
#[params(tag = "model")]
enum Model {
    #[case(default)]
    Fitted,
    Spherical(Stated),
    Exponential(Stated),
}

#[derive(Debug, PartialEq, Params)]
struct Stated {
    /// the sill
    sill: f64,
}

#[derive(Debug, PartialEq, Params)]
struct Idw {
    /// the exponent
    #[param(default = 2.0, optional)]
    power: f64,
    /// the search radius
    radius: Option<f64>,
}

#[derive(Debug, PartialEq, Params)]
struct Kriging {
    /// the search radius
    radius: Option<f64>,
    #[param(flatten)]
    model: Model,
}

/// which interpolator
#[derive(Debug, PartialEq, Params)]
#[params(tag = "method")]
enum Method {
    #[case(default)]
    Idw(Idw),
    Kriging(Kriging),
}

#[test]
fn a_union_flattens_into_conditioned_slots_and_its_cases_share() {
    let slots = Method::slots();
    let names: Vec<&str> = slots.iter().map(|s| s.name).collect();
    assert_eq!(
        names,
        ["method", "power", "radius", "model", "sill"],
        "one `radius` for two cases"
    );
    assert_eq!(slots[0].need, Need::Choice(&["idw", "kriging"]));
    assert_eq!(slots[0].suggested, Some(serde_json::json!("idw")));
    assert_eq!(
        slots[1].applies,
        Applies::When {
            slot: "method",
            is: vec!["idw"]
        }
    );
    assert_eq!(
        slots[2].applies,
        Applies::When {
            slot: "method",
            is: vec!["idw", "kriging"]
        }
    );
    assert_eq!(
        slots[3].applies,
        Applies::When {
            slot: "method",
            is: vec!["kriging"]
        }
    );
    // The sub-union keeps its own condition: it nests through `model`.
    assert_eq!(
        slots[4].applies,
        Applies::When {
            slot: "model",
            is: vec!["spherical", "exponential"]
        }
    );
}

#[test]
fn a_union_is_read_by_its_case_and_what_is_not_of_the_case_does_not_exist() {
    let slots = Method::slots();
    // Settled like any declaration: IDW's power is filled, kriging's model not.
    let settled = Choices::new().set("method", "idw").settle(&slots).unwrap();
    assert_eq!(
        Method::read(&settled).unwrap(),
        Method::Idw(Idw {
            power: 2.0,
            radius: None
        })
    );

    let settled = Choices::new()
        .set("method", "kriging")
        .set("model", "spherical")
        .set("sill", 3.0)
        .settle(&slots)
        .unwrap();
    assert_eq!(
        Method::read(&settled).unwrap(),
        Method::Kriging(Kriging {
            radius: None,
            model: Model::Spherical(Stated { sill: 3.0 })
        })
    );
    // **Written back** - the inverse of the reading: the case's word, then its
    // own fields; an absent value left out.
    let kriged = Method::Kriging(Kriging {
        radius: Some(5.0),
        model: Model::Spherical(Stated { sill: 3.0 }),
    });
    let written = kriged.write(Choices::new());
    assert_eq!(written.text("method"), Some("kriging"));
    assert_eq!(written.text("model"), Some("spherical"));
    assert_eq!(
        (written.number("radius"), written.number("sill")),
        (Some(5.0), Some(3.0))
    );
    assert_eq!(
        Method::read(&written).unwrap(),
        kriged,
        "read ∘ write is the identity"
    );
    assert!(
        Idw {
            power: 2.0,
            radius: None
        }
        .write(Choices::new())
        .get("radius")
        .is_none()
    );

    // Unsettled, an absent choice reads as the default case - and its own
    // fields as theirs: the rule of a field's default, for a union.
    assert_eq!(
        Method::read(&Choices::new()).unwrap(),
        Method::Idw(Idw {
            power: 2.0,
            radius: None
        })
    );
    let why = Method::read(&Choices::new().set("method", "cubic")).unwrap_err();
    assert_eq!(why.slot.as_deref(), Some("method"));
}

// ---------------------------------------------------------------- inputs

#[derive(Debug, Inputs)]
struct TwoRoles {
    /// the upper surface
    top: Shape,
    /// the lower one
    base: Shape,
}

#[derive(Debug, Inputs)]
struct Many {
    /// the surfaces, top to bottom
    #[input(least = 2, ordered)]
    surfaces: Vec<Shape>,
}

#[derive(Debug, Inputs)]
struct One {
    /// the one shape
    shape: Shape,
}

/// The shape of `interpolate`: samples required, values and traces optional.
#[derive(Debug, Inputs)]
struct Samples {
    /// the samples
    shape: Shape,
    /// the values
    field: Option<energyml_build::declare::Field>,
    /// the traces
    faults: Vec<Shape>,
}

fn shape(z: f64) -> Source {
    Source::Shape {
        geometry: Geometry {
            kind: "x",
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points: vec![[0.0, 0.0, z]],
                topology: Topology::Points,
                closed: Vec::new(),
            }],
        },
        closed: None,
    }
}

fn known(title: &str) -> Source {
    Source::Object(Known {
        reference: Some(Ref::new("u", "resqml22.X", title)),
        ..Default::default()
    })
}

fn z_of(shape: &Shape) -> f64 {
    shape.geometry.patches[0].points[0][2]
}

#[test]
fn a_piece_binds_to_its_slot_by_name_with_what_is_known_of_it() {
    let bag = Pieces::empty()
        .with("base", shape(2.0))
        .with("top", shape(1.0))
        .with("top (object)", known("Toit"));
    let bound = TwoRoles::bind(&bag).unwrap();
    assert_eq!(
        (z_of(&bound.top), z_of(&bound.base)),
        (1.0, 2.0),
        "by name, whatever the bag's order"
    );
    assert_eq!(bound.top.known.unwrap().reference.unwrap().title, "Toit");
    assert!(bound.base.known.is_none());

    let slots = TwoRoles::slots();
    assert_eq!(
        slots[0].need,
        Need::Input(energyml_build::operation::Takes::shape())
    );
}

#[test]
fn a_multiple_slot_takes_its_pieces_in_the_order_of_their_rank() {
    let bag = Pieces::empty()
        .with("surfaces 3", shape(3.0))
        .with("surfaces", shape(1.0))
        .with("surfaces 2", shape(2.0));
    let bound = Many::bind(&bag).unwrap();
    assert_eq!(
        bound.surfaces.iter().map(z_of).collect::<Vec<_>>(),
        [1.0, 2.0, 3.0]
    );
    // A piece named after nothing joins its one candidate slot, **after** the
    // named ones - a writer takes every shape it is given.
    let bound = Many::bind(&bag.clone().with("Horizon", shape(4.0))).unwrap();
    assert_eq!(
        bound.surfaces.iter().map(z_of).collect::<Vec<_>>(),
        [1.0, 2.0, 3.0, 4.0]
    );
    let why = Many::bind(&Pieces::empty().with("surfaces", shape(1.0))).unwrap_err();
    assert!(why.message.contains("at least 2"), "{}", why.message);
}

#[test]
fn a_piece_without_a_name_goes_to_the_only_slot_of_its_kind_and_otherwise_is_a_refusal() {
    // The caller that names nothing - the CLI without `slot=`, a piece named by
    // its title - and the older pairing convention, `object`.
    let bag = Pieces::empty()
        .with("Horizon 1", shape(5.0))
        .with("object", known("Horizon 1"));
    let bound = One::bind(&bag).unwrap();
    assert_eq!(z_of(&bound.shape), 5.0);
    assert!(
        bound.shape.known.is_some(),
        "the lone `object` pairs with the lone piece"
    );

    // One **required** slot of that kind and an optional one: the unnamed
    // piece is the samples - the CLI passes them so - and nothing becomes a
    // fault that was not named one.
    let bound = Samples::bind(
        &Pieces::empty()
            .with("Horizon 1", shape(5.0))
            .with("faults", shape(9.0)),
    )
    .unwrap();
    assert_eq!(z_of(&bound.shape), 5.0);
    assert_eq!(bound.faults.iter().map(z_of).collect::<Vec<_>>(), [9.0]);
    // And with no fault named, the unnamed samples are not **also** a fault -
    // the case the first version of this test did not reach, and let a
    // looser rule through.
    let bound = Samples::bind(&Pieces::empty().with("Horizon 1", shape(5.0))).unwrap();
    assert_eq!(z_of(&bound.shape), 5.0);
    assert!(bound.faults.is_empty(), "an unnamed piece became a fault");
    // No slot of kind `field` is required: the only one takes the unnamed field.
    let phi = Source::Field {
        over: "Horizon 1".into(),
        on: "nodes".into(),
        values: vec![0.5],
        uom: None,
    };
    let bound = Samples::bind(&Pieces::empty().with("Horizon 1", shape(5.0)).with("phi", phi)).unwrap();
    assert_eq!(bound.field.map(|f| f.values), Some(vec![0.5]));

    // Two required slots of that kind: which one is the top is exactly what
    // must not be guessed.
    let why = TwoRoles::bind(&Pieces::empty().with("a", shape(1.0)).with("b", shape(2.0))).unwrap_err();
    assert!(
        why.message.contains("`top`") && why.message.contains("`base`"),
        "{}",
        why.message
    );

    // And the wrong kind is refused on its slot.
    let lattice = Source::Lattice(energyml_build::pieces::Lattice {
        origin: [0.0, 0.0],
        spacing: [1.0, 1.0],
        counts: [2, 2],
        z: vec![0.0; 4],
    });
    let why = One::bind(&Pieces::empty().with("shape", lattice)).unwrap_err();
    assert_eq!(why.slot.as_deref(), Some("shape"));
    assert!(why.message.contains("lattice"), "{}", why.message);
}

// ---------------------------------------------------------------- the adapter

/// An operation declared here, never registered in the built-ins.
struct Probe {
    suggest_for: &'static str,
}

#[derive(Debug, Params)]
struct ProbeParams {
    /// a word
    word: String,
}

impl Operation for Probe {
    type Params = ProbeParams;
    type Inputs = One;

    fn about(&self) -> About {
        About {
            name: "probe",
            describe: "a test",
            category: Category::Other,
            makes: &[],
            recipe: "probe",
            represents: None,
        }
    }

    fn suggest(&self, _bag: &Pieces, slots: &mut Suggestions<'_>) {
        slots.set(self.suggest_for, serde_json::json!("hello"));
    }

    fn plan(&self, _inputs: One, params: ProbeParams, _cx: &Context<'_>) -> Result<Plan, Refusal> {
        Err(Refusal::new(
            "refused",
            format!("a probe plans nothing, not even `{}`", params.word),
        ))
    }
}

#[test]
fn the_adapter_returns_the_typed_operation_through_the_erased_interface() {
    let mut registry = Registry::bare();
    registry.register(typed(Probe { suggest_for: "word" }));
    let op = registry.get("probe").unwrap();
    assert_eq!(op.takes()[0].name, "shape");
    assert_eq!(
        op.needs(&Pieces::empty())[0].suggested,
        Some(serde_json::json!("hello"))
    );
    match op.fitness(&Pieces::empty()) {
        Fitness::Unfit(why) => assert!(why.message.contains("`shape`"), "{why}"),
        Fitness::Fit => panic!("no shape, unfit"),
    }
    assert_eq!(op.fitness(&Pieces::empty().with("x", shape(0.0))), Fitness::Fit);
}

#[test]
#[should_panic(expected = "suggests for slots it does not declare")]
fn a_suggestion_on_an_unknown_slot_fails_a_test() {
    let op = typed(Probe { suggest_for: "wrod" });
    let _ = op.needs(&Pieces::empty());
}
