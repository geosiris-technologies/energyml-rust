//! The shared blocks of every writer, declared as types (`ops/written.rs`) —
//! and `common_slots`, which is now those slots with the bag's suggestions.
//!
//! # What would have to break to make it turn red
//!
//! Re-ordering the slots (the form, `--list` and the templates read them in
//! that order); forgetting to say that the interpretation left empty
//! is **created** when the operation imposes it; making the title unreadable on a
//! geometry plan, where it may be missing.

use energyml_build::declare::Params;
use energyml_build::operation::{Applies, Choices, Otherwise};
use energyml_build::ops::support::common_slots;
use energyml_build::ops::written::{Written, WrittenImposed};
use energyml_build::pieces::{Known, Pieces, Ref, Source};

#[test]
fn the_blocks_return_the_earlier_slots_in_the_earlier_order() {
    let names = |slots: &[energyml_build::operation::Slot]| slots.iter().map(|s| s.name).collect::<Vec<_>>();
    assert_eq!(
        names(&Written::slots()),
        ["crs", "sourceEpsg", "project", "vertical", "interpretation", "title", "dataFile", "featureType", "interpretationType", "featureKind"]
    );
    assert_eq!(
        names(&WrittenImposed::slots()),
        ["crs", "sourceEpsg", "project", "vertical", "interpretation", "title", "dataFile"]
    );
    let slots = Written::slots();
    let of = |name: &str| slots.iter().find(|s| s.name == name).unwrap().clone();
    assert_eq!(of("crs").applies, Applies::Without(&["sourceEpsg"]));
    assert_eq!(of("crs").reads, ["Geometry/LocalCrs", "Patches/0/Geometry/LocalCrs", "LocalCrs"]);
    assert_eq!(of("project").applies, Applies::With(&["sourceEpsg"]));
    // **This slot declared a type that the crate never writes**, and this test
    // pinned it word for word — so it defended it. Measured on 21/09 by
    // counting the types of an import of 936 boreholes: an `eml23.ProjectedCrs`,
    // an `eml23.VerticalCrs`, and not a single compound; the trajectory points at
    // the first. The `LocalEngineeringCompoundCrs` existed only in a
    // comment and in the stand-in reference of a blocked plan.
    //
    // The two horizontals are named because the answer depends on
    // `project`, which a static declaration cannot read. And what really
    // keeps the promise is elsewhere, against a **plan**:
    // `operations.rs::what_a_slot_declares_it_creates_is_what_the_plan_writes`.
    // Pinning a declaration against itself says nothing of what is written
    // — it is what let this one through.
    //
    // **And the compound comes back, first, on 24/09** — because it is written
    // now, with its `LocalEngineering2dCrs`: it is what a geometry
    // names (RESQML TRG 2.2 §3.1 p. 22). The list that did not name it was
    // right about what was written, and what was written was wrong.
    assert_eq!(
        of("sourceEpsg").otherwise,
        Otherwise::Creates {
            types: vec![
                "eml23.LocalEngineeringCompoundCrs",
                "eml23.LocalEngineering2dCrs",
                "eml23.ProjectedCrs",
                "eml23.VerticalCrs",
                "eml23.Geographic2dCrs",
            ]
        }
    );
    assert_eq!((of("title").least, of("title").group), (1, "naming"), "required for writing");
    assert_eq!(of("interpretationType").least, 0);
    assert_eq!(of("interpretationType").suggested, Some(serde_json::json!("resqml22.GenericFeatureInterpretation")));
}

/// **An operation that imposes what it represents says that the empty
/// interpretation is created** — and the inputs' suggestions land on the typed
/// slots, with where they came from.
#[test]
fn the_bag_suggests_and_the_imposition_is_said() {
    let crs = Ref::new("c-1", "eml23.LocalEngineeringCompoundCrs", "The frame");
    let bag = Pieces::empty().with("shape", Source::Object(Known { crs: Some(crs.clone()), crs_epsg: Some(2154), ..Default::default() }));
    let slots = common_slots(&bag, Some(("resqml22.CulturalFeature", "resqml22.GenericFeatureInterpretation")));
    let of = |name: &str| slots.iter().find(|s| s.name == name).unwrap().clone();
    assert_eq!(
        of("interpretation").otherwise,
        Otherwise::Creates { types: vec!["resqml22.CulturalFeature", "resqml22.GenericFeatureInterpretation"] }
    );
    assert!(of("interpretation").about.contains("created here"));
    assert_eq!(of("crs").suggested, Some(crs.dor()));
    assert_eq!(of("project").suggested, Some(serde_json::json!(2154)));
    assert!(slots.iter().all(|s| s.name != "featureType"), "imposed: no feature to choose");
    // Imposing nothing, the empty interpretation creates nothing.
    let free = common_slots(&bag, None);
    assert_eq!(free.iter().find(|s| s.name == "interpretation").unwrap().otherwise, Otherwise::Nothing);
}

/// A geometry plan may leave the title out: the block still reads, and writes
/// back only what was given.
#[test]
fn an_absent_title_is_read_and_not_written() {
    let written = Written::read(&Choices::new().set("crs", "c-1")).expect("read without a title");
    assert!(written.naming.title.is_none());
    let back = written.write(Choices::new());
    assert_eq!(back.text("crs"), Some("c-1"));
    assert!(back.get("title").is_none());
    assert_eq!(back.text("dataFile"), Some("data.h5"), "the default, written once");
}
