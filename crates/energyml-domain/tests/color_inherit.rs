//! The colour cascade under the inheritance order — `energyml_domain::color_inherit`.
//!
//! See ADR-0009. The
//! packages are **built in memory** (the same pieces as `inherit.rs`, which
//! holds the rank tests of the walk itself): what is tested here is what the
//! colour adds to the walk — the level an answer is told by, the property's
//! shorter order, the ladder's rungs — never a file.
//!
//! The shape of the assertions is the design's: *the object, its collections, its
//! parents, …, the model; a property climbs only to its kind*. A colour is
//! named by its hue so two can be told apart in a failure.

use energyml_domain::color::{
    Cascade, ElementInfo, Entry, GraphicalSet, Hsv, Information, Level, Paint, Request, Scaling, Scope,
};
use energyml_domain::inherit::{self, Via};
use energyml_resqml_2_2 as resqml22;

use crate::inherit::{Built, Package, chain, citation, u, with_model};

fn hsv(hue: f64) -> Hsv {
    Hsv { hue, saturation: 1.0, value: 1.0, alpha: 1.0, title: None }
}

/// The `#rrggbbaa` of the colour named by `hue`.
fn hex(hue: f64) -> String {
    hsv(hue).to_rgba().to_hex()
}

/// A set where object `n` carries a constant colour of the given hue for the
/// whole object (`DefaultGraphicalInformation`).
fn painted(entries: &[(u32, f64)]) -> GraphicalSet {
    GraphicalSet {
        entries: entries
            .iter()
            .enumerate()
            .map(|(position, (n, hue))| Entry {
                position,
                targets: vec![u(*n)],
                target_uris: Vec::new(),
                map_uri: None,
                information: Information::Default {
                    viewer_kind: None,
                    viewer_id: None,
                    elements: vec![ElementInfo {
                        at: 0,
                        scope: Scope::WholeObject,
                        visible: true,
                        constant_color: Some(hsv(*hue)),
                        constant_alpha: None,
                        overwrite_color_alpha: false,
                        active_color: None,
                    }],
                },
            })
            .collect(),
        notes: Vec::new(),
    }
}

/// A set where object `n` carries a `ColorInformation` naming the map `map`.
fn mapped(entries: &[(u32, u32)]) -> GraphicalSet {
    GraphicalSet {
        entries: entries
            .iter()
            .enumerate()
            .map(|(position, (n, map))| Entry {
                position,
                targets: vec![u(*n)],
                target_uris: Vec::new(),
                map_uri: None,
                information: Information::Color {
                    scaling: Scaling::default(),
                    map: Some(u(*map)),
                    value_vector_index: None,
                },
            })
            .collect(),
        notes: Vec::new(),
    }
}

fn colour_map(package: &mut Package, uuid: u32) {
    let id = u(uuid);
    package.add(
        &id,
        "resqml22.ContinuousColorMap",
        Box::new(resqml22::ContinuousColorMap {
            uuid: id.clone(),
            citation: citation("map"),
            entry: [0.0, 1.0]
                .iter()
                .map(|index| resqml22::ContinuousColorMapEntry {
                    index: *index,
                    hsv: resqml22::HsvColor { hue: 120.0, saturation: 1.0, value: 1.0, alpha: 1.0, ..Default::default() },
                })
                .collect(),
            ..Default::default()
        }),
    );
}

fn hex_of(paint: &Paint) -> String {
    match paint {
        Paint::Constant(rgba) => rgba.to_hex(),
        Paint::Mapped { .. } => "<mapped>".to_string(),
    }
}

fn inherited(built: &Built, set: &GraphicalSet, from: u32) -> inherit::Walked<energyml_domain::color::Decision> {
    Cascade::new(set).inherited(&built.index, &built.source, &u(from)).expect("the walk reads")
}

/// The collection of the object answers before the interpretation, and the
/// answer says it came **by a collection**: the level, the rank, the one step
/// and the association that wrote it.
///
/// To break it: read `parent_of` instead of the walker (the interpretation would win),
/// or name the level by the rank (rank 2 → "interpretation").
#[test]
fn a_collection_answers_before_the_interpretation_and_says_so() {
    let mut package = chain();
    package.folder(10).set(20, &[(10, &[1])]);
    let built = package.built();
    let set = painted(&[(2, 120.0), (10, 240.0)]);

    let walked = inherited(&built, &set, 1);
    let found = walked.resolution.expect("a colour");
    assert_eq!(found.carrier, u(10));
    assert_eq!(found.rank, 2);
    assert_eq!(found.value.level, Level::Collection);
    assert_eq!(found.value.found_on, u(10));
    assert_eq!(hex_of(&found.value.paint), hex(240.0), "the collection's colour, not the interpretation's");
    assert_eq!(found.path.len(), 1);
    assert_eq!(found.path[0].via, Via::Collection);
    assert_eq!(found.path[0].field, "SingleCollectionAssociation[0].Dataobject[0]");
    assert_eq!(found.path[0].held_by.as_deref(), Some(u(20).as_str()));
}

/// Without a collection the parents answer, and the level is the type the
/// package holds for the parent — an interpretation, then a feature — whatever
/// the number of steps.
///
/// To break it: name the level by the number of steps (the feature, at rank 5, two
/// steps, would be "interpretation" if the index did not say its type).
#[test]
fn the_parents_answer_at_the_level_of_their_type() {
    let built = chain().built();

    let walked = inherited(&built, &painted(&[(2, 120.0), (3, 30.0)]), 1);
    let found = walked.resolution.expect("a colour");
    assert_eq!((found.value.level, found.rank, found.path.len()), (Level::Interpretation, 3, 1));
    assert_eq!(hex_of(&found.value.paint), hex(120.0));

    let walked = inherited(&built, &painted(&[(3, 30.0)]), 1);
    let found = walked.resolution.expect("a colour");
    assert_eq!((found.value.level, found.rank, found.path.len()), (Level::Feature, 5, 2));
    assert_eq!(found.path.iter().map(|s| s.via).collect::<Vec<_>>(), vec![Via::Parent, Via::Parent]);

    // The object's own colour: no path, the representation.
    let walked = inherited(&built, &painted(&[(1, 10.0), (2, 120.0)]), 1);
    let found = walked.resolution.expect("a colour");
    assert_eq!((found.value.level, found.rank, found.path.len()), (Level::Representation, 1, 0));
}

/// The model closes the walk — after the whole chain — and says so.
///
/// To break it: do not raise the level of a `Via::Model` step (it would say
/// "feature").
#[test]
fn the_model_answers_last_and_says_so() {
    let built = with_model().built();
    let set = painted(&[(31, 200.0)]);
    let found = inherited(&built, &set, 1).resolution.expect("a colour");
    assert_eq!((found.value.level, found.rank), (Level::Model, 6));
    assert_eq!(found.carrier, u(31));
    assert_eq!(found.path.last().map(|s| s.via), Some(Via::Model));

    // The feature, further up the chain, wins over the model.
    let set = painted(&[(31, 200.0), (3, 30.0)]);
    let found = inherited(&built, &set, 1).resolution.expect("a colour");
    assert_eq!(found.value.level, Level::Feature);
}

/// Nothing found is `None` — the front end keeps the hue of the uuid — and a colour
/// that sits on the model's collection is **said**, not applied.
///
/// To break it: follow the model's collections (the project's hue would win).
#[test]
fn nothing_found_is_none_and_a_project_colour_is_noted() {
    let mut package = chain();
    package.project(12).set(20, &[(12, &[1])]);
    let built = package.built();

    let walked = inherited(&built, &painted(&[]), 1);
    assert!(walked.resolution.is_none());

    let walked = inherited(&built, &painted(&[(12, 99.0)]), 1);
    assert!(walked.resolution.is_none(), "a project colour is applied to nothing: {walked:?}");
    assert!(walked.notes.iter().any(|n| n.contains("not followed")), "{:?}", walked.notes);
}

/// §2.2: a property goes to its kind and **stops** — a colour on its
/// representation, interpretation or feature is not its colour (painting a
/// property is a view). The old order fell through to the representation.
///
/// To break it: pass `Request.representation` to the property's walker (the
/// representation would answer).
#[test]
fn a_property_does_not_inherit_what_its_representation_carries() {
    let mut package = chain();
    package.property_kind(40, Some(41)).property_kind(41, None).property(50, 1, 40);
    colour_map(&mut package, 60);
    let built = package.built();
    let kinds = inherit::kind_chain(&built.index, &built.source, &u(50)).uuids;
    assert_eq!(kinds, vec![u(40), u(41)]);

    // Everything above the property carries a default colour; it has none.
    let set = painted(&[(1, 10.0), (2, 120.0), (3, 30.0)]);
    let cascade = Cascade::new(&set);
    let walked = cascade.inherited_property(&built.index, &built.source, &u(50), &kinds).expect("reads");
    assert!(walked.resolution.is_none(), "{walked:?}");
    // And the order it replaces does fall through, which is what made it differ.
    let old = cascade
        .resolve(
            &Request { representation: &u(1), property: Some(&u(50)), kind_chain: &kinds, element: Scope::WholeObject },
            &built.source,
        )
        .expect("the old order reads");
    assert!(old.is_some(), "the order of 05/09 reaches the representation");

    // The kind's parent carries a map: it answers, as a property kind.
    let set = mapped(&[(41, 60)]);
    let cascade = Cascade::new(&set);
    let found = cascade
        .inherited_property(&built.index, &built.source, &u(50), &kinds)
        .expect("reads")
        .resolution
        .expect("the parent kind answers");
    assert_eq!((found.value.level, found.rank, found.path.len()), (Level::PropertyKind, 5, 2));
    assert_eq!(found.carrier, u(41));
    assert!(matches!(found.value.paint, Paint::Mapped { .. }));
}

/// A collection of the property answers at rank 2, as a collection.
///
/// To break it: do not look for collections in a property's walk.
#[test]
fn a_collection_of_the_property_answers_for_it() {
    let mut package = chain();
    package.property_kind(40, None).property(50, 1, 40).folder(10).set(20, &[(10, &[50])]);
    colour_map(&mut package, 60);
    let built = package.built();
    let kinds = vec![u(40)];
    let set = mapped(&[(10, 60)]);
    let found = Cascade::new(&set)
        .inherited_property(&built.index, &built.source, &u(50), &kinds)
        .expect("reads")
        .resolution
        .expect("the collection answers");
    assert_eq!((found.value.level, found.rank), (Level::Collection, 2));
}

/// Two collections at the same distance: the first of the document answers and
/// the other is kept, **with its own level**.
///
/// To break it: drop the losers, or do not raise their level.
#[test]
fn a_tie_is_kept_with_its_level() {
    let mut package = chain();
    package.folder(10).folder(11).set(20, &[(11, &[1]), (10, &[1])]);
    let built = package.built();
    let set = painted(&[(10, 10.0), (11, 220.0)]);
    let found = inherited(&built, &set, 1).resolution.expect("a colour");
    assert_eq!(found.carrier, u(11), "the first association of the document");
    assert_eq!(found.ties.len(), 1);
    assert_eq!(found.ties[0].carrier, u(10));
    assert_eq!(found.ties[0].value.level, Level::Collection);
    assert_eq!(hex_of(&found.ties[0].value.paint), hex(10.0));
}

/// A member is paired with the collection of **its own** association — the
/// `Collection` and `Dataobject[j]` of the same rank `i` — and an association
/// that names no collection makes no member.
///
/// The membership filter of the paired paths, the one `inherit.rs` did not
/// test: `only_the_paired_paths_are_memberships` puts a collection without a
/// member, which no pairing could get wrong.
///
/// To break it: pair a member with `associations.first()` instead of the collection
/// of the same rank (object 1 would be in folder 10, and not 11), or read a
/// member without a collection as a member of the first.
#[test]
fn a_member_is_paired_with_the_collection_of_its_own_association() {
    use energyml_eml_2_3 as eml23;

    let mut package = Package::default();
    package.representation(1, None).representation(2, None).representation(3, None);
    package.folder(10).folder(11);
    let id = u(20);
    let association = |collection: Option<u32>, members: &[u32]| eml23::SingleCollectionAssociation {
        collection: match collection {
            Some(n) => crate::inherit::dor(&u(n), "eml23.DataobjectCollection"),
            None => eml23::DataObjectReference::default(),
        },
        dataobject: members.iter().map(|m| crate::inherit::dor(&u(*m), "resqml22.Anything")).collect(),
        ..Default::default()
    };
    package.add(
        &id,
        "eml23.CollectionsToDataobjectsAssociationSet",
        Box::new(eml23::CollectionsToDataobjectsAssociationSet {
            uuid: id.clone(),
            citation: citation("set"),
            single_collection_association: vec![
                association(Some(10), &[2]),
                association(Some(11), &[1]),
                // Names no collection: object 3 is a member of nothing.
                association(None, &[3]),
            ],
            ..Default::default()
        }),
    );
    let built = package.built();
    let set = painted(&[(10, 10.0), (11, 220.0)]);

    let one = inherited(&built, &set, 1).resolution.expect("object 1 is in the second association's collection");
    assert_eq!(one.carrier, u(11), "paired with association 1, not with the first");
    assert_eq!(one.path[0].field, "SingleCollectionAssociation[1].Dataobject[0]");

    let two = inherited(&built, &set, 2).resolution.expect("object 2 is in the first");
    assert_eq!(two.carrier, u(10));

    let three = inherited(&built, &set, 3);
    assert!(three.resolution.is_none(), "an association with no collection holds nobody: {three:?}");
}

/// A population answers what each object answers alone.
///
/// To break it: memoise the answer per package instead of per object.
#[test]
fn the_batch_says_what_the_unit_says() {
    let mut package = chain();
    package.representation(4, Some(2)).folder(10).set(20, &[(10, &[1])]);
    let built = package.built();
    let set = painted(&[(2, 120.0), (10, 240.0)]);
    let cascade = Cascade::new(&set);
    let uuids = vec![u(1), u(4), u(2), u(3)];
    let batch = cascade.inherited_all(&built.index, &built.source, &uuids).expect("reads");
    assert_eq!(batch.len(), 4);
    for (uuid, from_batch) in uuids.iter().zip(&batch) {
        let alone = cascade.inherited(&built.index, &built.source, uuid).expect("reads");
        assert_eq!(&alone, from_batch, "{uuid}");
    }
    // 1 has the collection, 4 the interpretation, 2 itself, 3 nothing.
    assert_eq!(batch[0].resolution.as_ref().map(|r| r.value.level), Some(Level::Collection));
    assert_eq!(batch[1].resolution.as_ref().map(|r| r.value.level), Some(Level::Interpretation));
    assert_eq!(batch[2].resolution.as_ref().map(|r| r.value.level), Some(Level::Representation));
    assert!(batch[3].resolution.is_none());
}

/// The ladder lists the object, then the collection **that answered**, right
/// after the rung it was reached from, then the parents; `chosen` is the carrier
/// and says the level the decision says.
///
/// To break it: do not insert the path's nodes (`chosen` would be `None` or
/// would point at the interpretation).
#[test]
fn the_ladder_puts_the_answering_collection_after_the_object() {
    let mut package = chain();
    package.folder(10).set(20, &[(10, &[1])]);
    let built = package.built();
    let set = painted(&[(2, 120.0), (10, 240.0)]);
    let cascade = Cascade::new(&set);
    let request = Request { representation: &u(1), property: None, kind_chain: &[], element: Scope::WholeObject };
    let ladders = cascade.ladders(&built.index, &built.source, &[request]).expect("reads");
    let ladder = &ladders[0];

    let order: Vec<&str> = ladder.steps.iter().map(|rung| rung.uuid.as_str()).collect();
    assert_eq!(order, vec![u(1).as_str(), u(10).as_str(), u(2).as_str(), u(3).as_str()]);
    assert_eq!(ladder.chosen, Some(1));
    assert_eq!(ladder.steps[1].level, Level::Collection);
    assert!(ladder.steps[1].carries.is_some());
    // The parents stay listed, and the one that carries can still be chosen.
    assert!(ladder.steps[2].carries.is_some());
    assert!(ladder.steps[3].carries.is_none());
    let decision = &ladder.walked.resolution.as_ref().expect("a walk").value;
    assert_eq!(decision.level, ladder.steps[1].level);
    assert_eq!(decision.paint, ladder.steps[1].carries.as_ref().expect("carries").paint);
}

/// A property's ladder is the property and its kinds — no representation
/// rung, since the property does not inherit from it.
///
/// To break it: keep the representation's rungs on a property's ladder.
#[test]
fn a_property_ladder_has_no_representation_rung() {
    let mut package = chain();
    package.property_kind(40, None).property(50, 1, 40);
    colour_map(&mut package, 60);
    let built = package.built();
    let set = mapped(&[(40, 60)]);
    let cascade = Cascade::new(&set);
    let kinds = vec![u(40)];
    let request =
        Request { representation: &u(1), property: Some(&u(50)), kind_chain: &kinds, element: Scope::WholeObject };
    let ladder = cascade.ladders(&built.index, &built.source, &[request]).expect("reads").remove(0);
    let levels: Vec<Level> = ladder.steps.iter().map(|rung| rung.level).collect();
    assert_eq!(levels, vec![Level::Property, Level::PropertyKind]);
    assert_eq!(ladder.chosen, Some(1));
}

/// `inherit = false` is the object alone, and says that the rest was not read.
///
/// To break it: ignore the switch (the collection would answer).
#[test]
fn with_inheritance_off_only_the_object_answers() {
    let mut package = chain();
    package.folder(10).set(20, &[(10, &[1])]);
    let built = package.built();
    let set = painted(&[(2, 120.0), (10, 240.0)]);
    let mut cascade = Cascade::new(&set);
    cascade.inherit = false;
    let walked = cascade.inherited(&built.index, &built.source, &u(1)).expect("reads");
    assert!(walked.resolution.is_none());
    assert!(walked.notes.iter().any(|n| n.contains("switched off")), "{:?}", walked.notes);
}

/// **The whole order, one rung at a time.** One object carries every place a
/// colour can sit — itself, a collection, a collection of that collection, its
/// interpretation, a collection of the interpretation (and of that), its
/// feature, a collection of the feature, the earth model — and the colour is
/// taken away from the winner until nothing is left. Each answer is the next
/// rung of the ADR-0009 order, with its level and its rank: 1. directly on the
/// object; 2. a collection that references the object; 2 bis. collections of
/// collections, recursively; 3. parents directly referenced; 4. parents
/// referenced by a collection; 4 bis. nested collections; 5. go up with the same
/// rules on the parents of the parents; 6. if nothing is found, apply to the
/// model that references the object or its parents.
///
/// This is the statement the Lutetia fallback demo stands on: there, each rung is shown on a
/// different borehole, and this is the proof that the rungs are in that order.
///
/// To break it: swap two ranks in `Walker::run` (the collection before the object,
/// the interpretation before the nested collection, the model before the feature) —
/// the walk that follows the winner's removal stops at the wrong rung.
#[test]
fn every_rung_answers_in_the_order_of_the_design() {
    let mut package = with_model();
    package.folder(10).folder(11).folder(12).folder(13).folder(14);
    // 10 holds the object, 11 holds 10; 12 holds the interpretation, 13 holds 12; 14 holds the feature.
    package.set(20, &[(10, &[1]), (11, &[10]), (12, &[2]), (13, &[12]), (14, &[3])]);
    let built = package.built();

    // (carrier, level, rank, distance) in the order they must answer.
    let rungs = [
        (1, Level::Representation, 1, 0),
        (10, Level::Collection, 2, 1),
        (11, Level::Collection, 2, 2),
        (2, Level::Interpretation, 3, 1),
        (12, Level::Collection, 4, 2),
        (13, Level::Collection, 4, 3),
        (3, Level::Feature, 5, 2),
        (14, Level::Collection, 5, 3),
        (31, Level::Model, 6, 2),
    ];
    let mut standing: Vec<u32> = rungs.iter().map(|rung| rung.0).collect();
    for (carrier, level, rank, distance) in rungs {
        let set = painted(&standing.iter().map(|n| (*n, f64::from(*n) * 7.0)).collect::<Vec<_>>());
        let found = inherited(&built, &set, 1)
            .resolution
            .unwrap_or_else(|| panic!("a colour is left on {standing:?}"));
        assert_eq!(
            (found.carrier.as_str(), found.value.level, found.rank, found.distance()),
            (u(carrier).as_str(), level, rank, distance),
            "with {standing:?} standing"
        );
        assert_eq!(hex_of(&found.value.paint), hex(f64::from(carrier) * 7.0), "the colour is the carrier's");
        standing.retain(|n| *n != carrier);
    }
    // Nothing left: the hue of the uuid, and no colour invented.
    assert!(inherited(&built, &painted(&[]), 1).resolution.is_none());
}

/// A collection that holds **the model** is the model's collection — rank 7,
/// which the house leaves out (§2.1) — and the walk **says** so instead of
/// applying it. The same colour on the model itself does answer, at rank 6:
/// that is the control, so the silence is the collection's and not the model's.
///
/// To break it: follow the model's collections (the folder's hue would win),
/// or follow them silently (the note disappears).
#[test]
fn a_collection_holding_the_model_is_not_followed_and_is_noted() {
    let mut package = with_model();
    package.folder(10).set(20, &[(10, &[31])]);
    let built = package.built();

    let walked = inherited(&built, &painted(&[(10, 240.0)]), 1);
    assert!(walked.resolution.is_none(), "the model's collection is applied to nothing: {walked:?}");
    assert!(
        walked.notes.iter().any(|n| n.contains("collections of the model are not followed")),
        "{:?}",
        walked.notes
    );

    let on_the_model = inherited(&built, &painted(&[(31, 240.0)]), 1).resolution.expect("the model answers");
    assert_eq!((on_the_model.value.level, on_the_model.rank), (Level::Model, 6));
}
