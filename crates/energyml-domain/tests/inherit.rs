//! What an object inherits, and by which path — `energyml_domain::inherit`.
//!
//! See ADR-0009. Every
//! package is **built in memory** after the specifications — common TRG 2.3
//! §8.3 and §8.5 for the collections, RESQML TRG 2.2 §2.7 and §6.19 for the
//! feature / interpretation / representation chain and the earth model — and
//! never read from a file.
//!
//! The walk knows nothing about colours: the predicate here says *"this object
//! carries a label"*, and the labels are plain strings.
//!
//! # What it would take to make each rank test fail
//!
//! Each test puts the information on **two** places that two adjacent ranks
//! would answer differently, and asks which wins: swap the two ranks in the
//! walk and the test goes red. The sabotages are listed in the report of the
//! change; each guard was broken once on purpose.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use energyml_core::registry::DynObject;
use energyml_core::value::Reflect;
use energyml_domain::color::{Cascade, GraphicalSet, Request, Scope};
use energyml_domain::inherit::{self, Resolution, Via, Walked, Walker};
use energyml_domain::package_index::{IndexBuilder, PackageIndex};
use energyml_domain::source::Prefetched;
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_2 as resqml22;
use energyml_storage::ObjectSummary;
use energyml_storage::array::ArrayLocation;

// -- building blocks ---------------------------------------------------------

pub(crate) fn u(n: u32) -> String {
    format!("00000000-0000-4000-8000-{n:012x}")
}

/// The uuid inside the identity (the uri) a walk hands its predicate — through the
/// library's one reading of an address.
pub(crate) fn uuid_of(identity: &str) -> String {
    energyml_core::identity::parse_address(identity).0
}

pub(crate) fn nowhere(_: &dyn Reflect) -> Vec<ArrayLocation> {
    Vec::new()
}

pub(crate) fn dor(uuid: &str, qualified_type: &str) -> eml23::DataObjectReference {
    eml23::DataObjectReference {
        uuid: uuid.to_string(),
        qualified_type: qualified_type.to_string(),
        title: format!("t-{uuid}"),
        ..Default::default()
    }
}

pub(crate) fn citation(title: &str) -> eml23::Citation {
    eml23::Citation {
        title: title.to_string(),
        ..Default::default()
    }
}

/// A package under construction: the objects, in the order they are written.
#[derive(Default)]
pub(crate) struct Package {
    pub(crate) objects: Vec<(ObjectSummary, Box<dyn DynObject>)>,
}

impl Package {
    pub(crate) fn add(&mut self, uuid: &str, qualified_type: &str, object: Box<dyn DynObject>) {
        let summary = ObjectSummary {
            uri: format!("eml:///{qualified_type}({uuid})"),
            uuid: uuid.to_string(),
            qualified_type: qualified_type.to_string(),
            content_type: String::new(),
            locator: String::new(),
        };
        self.objects.push((summary, object));
    }

    /// A representation of `interpretation` (none: of nothing).
    pub(crate) fn representation(&mut self, uuid: u32, interpretation: Option<u32>) -> &mut Self {
        let id = u(uuid);
        self.add(
            &id,
            "resqml22.WellboreTrajectoryRepresentation",
            Box::new(resqml22::WellboreTrajectoryRepresentation {
                uuid: id.clone(),
                citation: citation(&format!("rep {uuid}")),
                represented_object: interpretation.map(|n| dor(&u(n), "resqml22.WellboreInterpretation")),
                ..Default::default()
            }),
        );
        self
    }

    pub(crate) fn interpretation(&mut self, uuid: u32, feature: u32) -> &mut Self {
        let id = u(uuid);
        self.add(
            &id,
            "resqml22.WellboreInterpretation",
            Box::new(resqml22::WellboreInterpretation {
                uuid: id.clone(),
                citation: citation(&format!("interpretation {uuid}")),
                interpreted_feature: dor(&u(feature), "resqml22.WellboreFeature"),
                ..Default::default()
            }),
        );
        self
    }

    pub(crate) fn feature(&mut self, uuid: u32) -> &mut Self {
        let id = u(uuid);
        self.add(
            &id,
            "resqml22.WellboreFeature",
            Box::new(resqml22::WellboreFeature {
                uuid: id.clone(),
                citation: citation(&format!("feature {uuid}")),
                ..Default::default()
            }),
        );
        self
    }

    pub(crate) fn model(&mut self, uuid: u32) -> &mut Self {
        let id = u(uuid);
        self.add(
            &id,
            "resqml22.Model",
            Box::new(resqml22::Model {
                uuid: id.clone(),
                citation: citation("the model"),
                ..Default::default()
            }),
        );
        self
    }

    /// An earth model interpretation of `model`, naming `wells` — the one
    /// object that points the interpretations (a `Model` points nothing).
    pub(crate) fn earth_model(&mut self, uuid: u32, model: u32, wells: &[u32]) -> &mut Self {
        let id = u(uuid);
        self.add(
            &id,
            "resqml22.EarthModelInterpretation",
            Box::new(resqml22::EarthModelInterpretation {
                uuid: id.clone(),
                citation: citation("the earth model"),
                interpreted_feature: dor(&u(model), "resqml22.Model"),
                wellbore_interpretation_set: wells
                    .iter()
                    .map(|n| dor(&u(*n), "resqml22.WellboreInterpretation"))
                    .collect(),
                ..Default::default()
            }),
        );
        self
    }

    pub(crate) fn collection(&mut self, uuid: u32, kind: eml23::CollectionKindExt) -> &mut Self {
        let id = u(uuid);
        self.add(
            &id,
            "eml23.DataobjectCollection",
            Box::new(eml23::DataobjectCollection {
                uuid: id.clone(),
                citation: citation(&format!("collection {uuid}")),
                kind: Some(kind),
                ..Default::default()
            }),
        );
        self
    }

    pub(crate) fn folder(&mut self, uuid: u32) -> &mut Self {
        self.collection(uuid, eml23::CollectionKindExt::Folder)
    }

    pub(crate) fn project(&mut self, uuid: u32) -> &mut Self {
        self.collection(uuid, eml23::CollectionKindExt::Project)
    }

    /// An association set: one `(collection, members)` per association, in
    /// this order.
    pub(crate) fn set(&mut self, uuid: u32, associations: &[(u32, &[u32])]) -> &mut Self {
        let id = u(uuid);
        self.add(
            &id,
            "eml23.CollectionsToDataobjectsAssociationSet",
            Box::new(eml23::CollectionsToDataobjectsAssociationSet {
                uuid: id.clone(),
                citation: citation(&format!("set {uuid}")),
                single_collection_association: associations
                    .iter()
                    .map(|(collection, members)| eml23::SingleCollectionAssociation {
                        collection: dor(&u(*collection), "eml23.DataobjectCollection"),
                        dataobject: members.iter().map(|m| dor(&u(*m), "resqml22.Anything")).collect(),
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            }),
        );
        self
    }

    /// A continuous property of `representation`, whose kind is `kind`.
    pub(crate) fn property(&mut self, uuid: u32, representation: u32, kind: u32) -> &mut Self {
        let id = u(uuid);
        self.add(
            &id,
            "resqml22.ContinuousProperty",
            Box::new(resqml22::ContinuousProperty {
                uuid: id.clone(),
                citation: citation(&format!("property {uuid}")),
                property_kind: dor(&u(kind), "eml23.PropertyKind"),
                supporting_representation: dor(
                    &u(representation),
                    "resqml22.WellboreTrajectoryRepresentation",
                ),
                ..Default::default()
            }),
        );
        self
    }

    pub(crate) fn property_kind(&mut self, uuid: u32, parent: Option<u32>) -> &mut Self {
        let id = u(uuid);
        self.add(
            &id,
            "eml23.PropertyKind",
            Box::new(eml23::PropertyKind {
                uuid: id.clone(),
                citation: citation(&format!("kind {uuid}")),
                parent: parent.map(|p| dor(&u(p), "eml23.PropertyKind")),
                ..Default::default()
            }),
        );
        self
    }

    /// The package as a source, and its index.
    pub(crate) fn built(self) -> Built {
        let mut source = Prefetched::new();
        let mut summaries = Vec::new();
        for (summary, object) in self.objects {
            summaries.push(summary);
            source.insert(object, &nowhere);
        }
        let mut builder = IndexBuilder::new(summaries, 1);
        let total = builder.total();
        builder.step(&source, total);
        Built {
            source,
            index: builder.finish(),
        }
    }
}

pub(crate) struct Built {
    pub(crate) source: Prefetched,
    pub(crate) index: PackageIndex,
}

/// Run a walk where the information sits on exactly `carriers`.
fn walk(built: &Built, from: u32, carriers: &[(u32, &'static str)]) -> Walked<&'static str> {
    let map: HashMap<String, &'static str> = carriers.iter().map(|(n, l)| (u(*n), *l)).collect();
    let has = |identity: &str| map.get(&uuid_of(identity)).copied();
    inherit::resolve(&built.index, &built.source, &u(from), &has)
}

fn hit<'a>(walked: &'a Walked<&'static str>) -> &'a Resolution<&'static str> {
    walked
        .resolution
        .as_ref()
        .unwrap_or_else(|| panic!("nothing found; notes: {:?}", walked.notes))
}

fn route(resolution: &Resolution<&'static str>) -> Vec<(String, Via)> {
    resolution.path.iter().map(|s| (s.to.clone(), s.via)).collect()
}

/// The chain every rank test starts from: a representation 1, its
/// interpretation 2, its feature 3.
pub(crate) fn chain() -> Package {
    let mut package = Package::default();
    package.representation(1, Some(2)).interpretation(2, 3).feature(3);
    package
}

// -- rank 1 ------------------------------------------------------------------

/// Rank 1: what the object carries beats everything above it.
///
/// To break it: test the collections before the object — the collection (which
/// also carries something) would win.
#[test]
fn the_object_itself_comes_first() {
    let mut package = chain();
    package.folder(10).set(20, &[(10, &[1])]);
    let built = package.built();
    let walked = walk(
        &built,
        1,
        &[(1, "own"), (10, "collection"), (2, "interpretation")],
    );
    let found = hit(&walked);
    assert_eq!(
        (found.value, found.rank, found.carrier.as_str()),
        ("own", 1, u(1).as_str())
    );
    assert!(found.path.is_empty());
    assert!(found.ties.is_empty());
}

// -- rank 2 and 2 bis ----------------------------------------------------------

/// Rank 2 before rank 3: a collection of the representation beats the
/// interpretation. The path is one `collection` step and names the association
/// that wrote it.
///
/// To break it: put the parents before the collections — the interpretation would win.
#[test]
fn a_collection_of_the_object_beats_its_interpretation() {
    let mut package = chain();
    package.folder(10).set(20, &[(10, &[1])]);
    let built = package.built();
    let walked = walk(&built, 1, &[(2, "interpretation"), (10, "collection")]);
    let found = hit(&walked);
    assert_eq!((found.value, found.rank, found.distance()), ("collection", 2, 1));
    assert_eq!(found.carrier, u(10));
    let step = &found.path[0];
    assert_eq!(
        (step.from.as_str(), step.to.as_str(), step.via),
        (u(1).as_str(), u(10).as_str(), Via::Collection)
    );
    assert_eq!(step.field, "SingleCollectionAssociation[0].Dataobject[0]");
    assert_eq!(step.held_by.as_deref(), Some(u(20).as_str()));
}

/// Rank 2 bis: a collection of a collection is one step further — and still
/// before the parents.
///
/// To break it: do not descend into collections of collections (None at the
/// first level), or rank them after the parents.
#[test]
fn a_collection_of_a_collection_is_one_step_further() {
    let mut package = chain();
    package.folder(10).folder(11).set(20, &[(10, &[1]), (11, &[10])]);
    let built = package.built();
    let walked = walk(&built, 1, &[(11, "outer"), (2, "interpretation")]);
    let found = hit(&walked);
    assert_eq!((found.value, found.rank, found.distance()), ("outer", 2, 2));
    assert_eq!(
        route(found),
        vec![(u(10), Via::Collection), (u(11), Via::Collection)]
    );
}

// -- rank 3, 4, 4 bis ----------------------------------------------------------

/// Rank 3 before rank 4: the interpretation beats a collection of the
/// interpretation.
///
/// To break it: test the parent's collections before the parent.
#[test]
fn the_parent_beats_the_collection_of_the_parent() {
    let mut package = chain();
    package.folder(10).set(20, &[(10, &[2])]);
    let built = package.built();
    let walked = walk(
        &built,
        1,
        &[(2, "interpretation"), (10, "collection of the interpretation")],
    );
    let found = hit(&walked);
    assert_eq!(
        (found.value, found.rank, found.distance()),
        ("interpretation", 3, 1)
    );
    assert_eq!(route(found), vec![(u(2), Via::Parent)]);
    assert_eq!(found.path[0].field, "RepresentedObject");
}

/// Rank 4 before rank 5: a collection of the interpretation beats the feature.
///
/// To break it: climb up to the feature before looking at the interpretation's
/// collections.
#[test]
fn a_collection_of_the_interpretation_beats_the_feature() {
    let mut package = chain();
    package.folder(10).set(20, &[(10, &[2])]);
    let built = package.built();
    let walked = walk(
        &built,
        1,
        &[(3, "feature"), (10, "collection of the interpretation")],
    );
    let found = hit(&walked);
    assert_eq!(
        (found.value, found.rank, found.distance()),
        ("collection of the interpretation", 4, 2)
    );
    assert_eq!(route(found), vec![(u(2), Via::Parent), (u(10), Via::Collection)]);
}

/// Rank 4 bis: the collections of the interpretation's collections, distance 3.
///
/// To break it: do not nest at rank 4.
#[test]
fn nested_collections_of_the_parent_are_followed() {
    let mut package = chain();
    package.folder(10).folder(11).set(20, &[(10, &[2]), (11, &[10])]);
    let built = package.built();
    let walked = walk(&built, 1, &[(3, "feature"), (11, "outer")]);
    let found = hit(&walked);
    assert_eq!((found.value, found.rank, found.distance()), ("outer", 4, 3));
}

// -- rank 5 --------------------------------------------------------------------

/// Rank 5: the walk goes up — the feature, then its collections, and a
/// collection of the **feature** is one stage above the interpretation's.
///
/// To break it: stop at the interpretation, or mix up the stages (a
/// grandparent's collection against a great-grandparent).
#[test]
fn the_walk_goes_up_and_the_stages_are_read_in_order() {
    let mut package = chain();
    package.folder(10).set(20, &[(10, &[3])]);
    let built = package.built();

    let on_the_feature = walk(&built, 1, &[(3, "feature")]);
    let found_feature = hit(&on_the_feature);
    assert_eq!((found_feature.rank, found_feature.distance()), (5, 2));
    assert_eq!(
        route(found_feature),
        vec![(u(2), Via::Parent), (u(3), Via::Parent)]
    );
    assert_eq!(found_feature.path[1].field, "InterpretedFeature");

    // The feature itself is before the feature's collections.
    let both = walk(&built, 1, &[(3, "feature"), (10, "collection of the feature")]);
    assert_eq!(hit(&both).value, "feature");

    let only_collection = walk(&built, 1, &[(10, "collection of the feature")]);
    let found_collection = hit(&only_collection);
    assert_eq!((found_collection.rank, found_collection.distance()), (5, 3));
}

// -- rank 6 --------------------------------------------------------------------

pub(crate) fn with_model() -> Package {
    let mut package = chain();
    package.model(30).earth_model(31, 30, &[2]);
    package
}

/// Rank 6: the model that references the object's parent answers last — after
/// the feature.
///
/// To break it: put the model before the climb (the feature and the model both
/// carry: the feature must win).
#[test]
fn the_model_comes_after_the_whole_chain() {
    let built = with_model().built();
    let walked = walk(&built, 1, &[(3, "feature"), (31, "earth model")]);
    assert_eq!(hit(&walked).value, "feature");
    assert_eq!(hit(&walked).rank, 5);
}

/// Rank 6 itself: the earth model that names the interpretation, and the
/// `Model` it interprets one step beyond — the nearer first, the other not a
/// tie (it is further).
///
/// To break it: do not read the parents' **incoming** references (the model
/// would never be reached), or do not follow the earth model's
/// `InterpretedFeature` to the `Model`.
#[test]
fn the_model_that_references_a_parent_is_reached() {
    let built = with_model().built();

    let walked = walk(&built, 1, &[(31, "earth model"), (30, "model")]);
    let found = hit(&walked);
    assert_eq!((found.value, found.rank, found.distance()), ("earth model", 6, 2));
    assert_eq!(route(found), vec![(u(2), Via::Parent), (u(31), Via::Model)]);
    assert!(found.ties.is_empty(), "the Model is further, not a tie");

    let on_the_model = walk(&built, 1, &[(30, "model")]);
    let found = hit(&on_the_model);
    assert_eq!((found.value, found.rank, found.distance()), ("model", 6, 3));
    assert_eq!(route(found).last(), Some(&(u(30), Via::Model)));
}

// -- rank 7 and the project ----------------------------------------------------

/// A `project` collection is **not followed**, and the walk says so — whatever
/// the information on it. The control is the same package with a `folder`,
/// which is followed.
///
/// To break it: do not filter on `Kind` (the project would answer); or filter it
/// without noting it (the note disappears).
#[test]
fn a_project_collection_is_not_followed_and_is_noted() {
    let mut project = chain();
    project.project(10).set(20, &[(10, &[1, 2, 3])]);
    let built = project.built();
    let walked = walk(&built, 1, &[(10, "project colour")]);
    assert!(walked.resolution.is_none(), "{walked:?}");
    assert!(
        walked
            .notes
            .iter()
            .any(|n| n.contains("project") && n.contains("not followed")),
        "{:?}",
        walked.notes
    );

    // The project does not pass in front of the interpretation, and the note stays.
    let walked = walk(&built, 1, &[(10, "project colour"), (2, "interpretation")]);
    assert_eq!(hit(&walked).value, "interpretation");
    assert!(
        walked.notes.iter().any(|n| n.contains("project")),
        "{:?}",
        walked.notes
    );

    let mut folder = chain();
    folder.folder(10).set(20, &[(10, &[1, 2, 3])]);
    let built = folder.built();
    let walked = walk(&built, 1, &[(10, "folder colour")]);
    assert_eq!(hit(&walked).value, "folder colour");
}

/// The collections of the **model** are not followed either (§2.1, rank 7),
/// and the walk says it when it would have answered.
///
/// To break it: follow the model's collections at rank 6.
#[test]
fn the_collections_of_the_model_are_not_followed_and_are_noted() {
    let mut package = with_model();
    package.folder(10).set(20, &[(10, &[31])]);
    let built = package.built();
    let walked = walk(&built, 1, &[(10, "collection of the earth model")]);
    assert!(walked.resolution.is_none(), "{walked:?}");
    assert!(
        walked
            .notes
            .iter()
            .any(|n| n.contains("collections of the model are not followed")),
        "{:?}",
        walked.notes
    );
}

// -- ties and document order ---------------------------------------------------

/// Two collections of the object at the same distance: the one written first
/// wins and the other is kept in `ties`; reverse the association order and the
/// other wins. **The order is the document's**, not the uuid's: the winner here
/// has the larger uuid.
///
/// To break it: sort by uuid (the reversal test fails), or drop the losers
/// (`ties` empty).
#[test]
fn a_tie_takes_the_first_in_the_document_and_keeps_the_others() {
    let mut package = chain();
    package.folder(10).folder(11).set(20, &[(11, &[1]), (10, &[1])]);
    let built = package.built();
    let walked = walk(&built, 1, &[(10, "ten"), (11, "eleven")]);
    let found = hit(&walked);
    assert_eq!((found.value, found.carrier.as_str()), ("eleven", u(11).as_str()));
    assert_eq!(found.ties.len(), 1);
    assert_eq!(
        (found.ties[0].carrier.as_str(), found.ties[0].value),
        (u(10).as_str(), "ten")
    );
    assert_eq!(
        found.ties[0].path[0].field,
        "SingleCollectionAssociation[1].Dataobject[0]"
    );

    let mut reversed = chain();
    reversed.folder(10).folder(11).set(20, &[(10, &[1]), (11, &[1])]);
    let built = reversed.built();
    let walked = walk(&built, 1, &[(10, "ten"), (11, "eleven")]);
    assert_eq!(hit(&walked).value, "ten");
    assert_eq!(hit(&walked).ties[0].value, "eleven");
}

/// The shortest path wins over the document order: a collection reached in one
/// step beats one reached in two, though it is written later — and the loser
/// is **not** a tie (it is further).
///
/// To break it: break ties by the order before the distance.
#[test]
fn the_shortest_path_wins_before_the_order() {
    let mut package = chain();
    // 10 holds 1 and is itself held by 12 (distance 2 for 12); 11 holds 1 directly, written after.
    package
        .folder(10)
        .folder(11)
        .folder(12)
        .set(20, &[(10, &[1]), (12, &[10]), (11, &[1])]);
    let built = package.built();
    let walked = walk(&built, 1, &[(12, "far"), (11, "near")]);
    let found = hit(&walked);
    assert_eq!((found.value, found.distance()), ("near", 1));
    assert!(found.ties.is_empty());
}

// -- cycles and duplicates -----------------------------------------------------

/// A collection that holds itself, near or far, is a cycle **named**, and the
/// walk ends: no loop, no answer invented.
///
/// To break it: remove the visited set (the test does not terminate) or do not
/// name the cycle.
#[test]
fn a_cycle_of_collections_is_named_and_the_walk_ends() {
    let mut package = chain();
    package
        .folder(10)
        .folder(11)
        .set(20, &[(10, &[1]), (11, &[10])])
        .set(21, &[(10, &[11])]);
    let built = package.built();
    let walked = walk(&built, 1, &[]);
    assert!(walked.resolution.is_none());
    assert!(
        walked
            .notes
            .iter()
            .any(|n| n.starts_with("cycle in the collections")),
        "{:?}",
        walked.notes
    );

    // A cycle on the way does not forbid an answer found beyond it.
    let walked = walk(&built, 1, &[(11, "outer")]);
    // (the edge that closes it is only met when the walk goes past the answer, so
    // it is not named here: a cycle is named where the walk meets it.)
    assert_eq!(hit(&walked).value, "outer");

    // A collection that holds itself.
    let mut itself = chain();
    itself.folder(10).set(20, &[(10, &[1])]).set(21, &[(10, &[10])]);
    let built = itself.built();
    let walked = walk(&built, 1, &[]);
    assert!(walked.resolution.is_none());
    assert!(
        walked
            .notes
            .iter()
            .any(|n| n.starts_with("cycle in the collections")),
        "{:?}",
        walked.notes
    );
}

/// An object listed twice in one association counts once (common TRG 2.3
/// §8.5) — one carrier, no tie with itself — and the doubling is said.
///
/// To break it: do not deduplicate (`ties` would contain the collection).
#[test]
fn an_object_listed_twice_counts_once() {
    let mut package = chain();
    package.folder(10).set(20, &[(10, &[1, 1])]);
    let built = package.built();
    let walked = walk(&built, 1, &[(10, "collection")]);
    let found = hit(&walked);
    assert_eq!(found.carrier, u(10));
    assert!(found.ties.is_empty());
    assert!(
        found.notes.iter().any(|n| n.contains("§8.5")),
        "{:?}",
        found.notes
    );
    assert_eq!(
        found.path[0].field, "SingleCollectionAssociation[0].Dataobject[0]",
        "the first listing"
    );
}

/// A collection named in two associations of one set counts once — the first
/// (§8.3) — and the second association's members are **not** members: the
/// repeat is ignored, said.
///
/// To break it: take all the associations (object 1 would be in the
/// collection).
#[test]
fn a_collection_named_twice_in_a_set_counts_once() {
    let mut package = Package::default();
    package.representation(1, None).representation(2, None);
    package.folder(10).set(20, &[(10, &[2]), (10, &[1])]);
    let built = package.built();
    let walked = walk(&built, 1, &[(10, "collection")]);
    assert!(walked.resolution.is_none(), "{walked:?}");
    assert!(
        walked.notes.iter().any(|n| n.contains("§8.3")),
        "{:?}",
        walked.notes
    );
    // The first association stands: object 2 is in the collection.
    let walked = walk(&built, 2, &[(10, "collection")]);
    assert_eq!(hit(&walked).value, "collection");
}

/// A reference to a collection that is not a membership — the collection
/// naming an object as its own parent, here the association set pointing a
/// collection in `Collection` only — makes no member.
///
/// To break it: read every incoming edge of a set as a membership.
#[test]
fn only_the_paired_paths_are_memberships() {
    let mut package = chain();
    // Association 0 names collection 10 and no member: object 1 is not in 10.
    package.folder(10).set(20, &[(10, &[])]);
    let built = package.built();
    let walked = walk(&built, 1, &[(10, "collection")]);
    assert!(walked.resolution.is_none(), "{walked:?}");
}

/// The `Collection` path of an association is **not** a membership: a collection
/// is not a member of itself, and the walk must not invent a cycle out of the
/// path that names it. With nothing carrying anything the walk explores the
/// collection's own memberships — and finds none.
///
/// This is the filter of the paired paths (`Collection` and `Dataobject[j]` of
/// one rank): `only_the_paired_paths_are_memberships` did not exercise it — a
/// collection with no member is never asked for its own memberships — and
/// reading the `.Collection` path as `Dataobject[0]` left every test of this
/// file green (sabotaged on 02/10).
///
/// To break it: accept the `…Collection` path in `links_of` as a member
/// (collection 10 would have itself as a membership, and the walk would name a
/// cycle that does not exist).
#[test]
fn a_collection_is_not_a_member_of_itself() {
    let mut package = chain();
    package.folder(10).set(20, &[(10, &[1])]);
    let built = package.built();
    let walked = walk(&built, 1, &[]);
    assert!(walked.resolution.is_none(), "{walked:?}");
    assert!(
        walked.notes.iter().all(|n| !n.starts_with("cycle")),
        "a cycle was invented out of a `Collection` path: {:?}",
        walked.notes
    );

    // And a collection that does hold a collection is still followed one step
    // further — the control, so the check above is not an empty walk.
    let mut package = chain();
    package.folder(10).folder(11).set(20, &[(10, &[1]), (11, &[10])]);
    let built = package.built();
    let walked = walk(&built, 1, &[(11, "outer")]);
    let found = hit(&walked);
    assert_eq!(
        (found.carrier.clone(), found.rank, found.distance()),
        (u(11), 2, 2)
    );
}

/// Only an **association set** writes a membership (common TRG 2.3 §8.3): the
/// same document registered as another type holds nobody.
///
/// To break it: remove the type filter from `links_of` (object 1 would enter
/// folder 10 through an object that is not an association set).
#[test]
fn only_an_association_set_writes_a_membership() {
    let mut package = chain();
    package.folder(10).set(20, &[(10, &[1])]);
    // The control: registered as what it is, the membership is read.
    let built = package.built();
    assert_eq!(hit(&walk(&built, 1, &[(10, "collection")])).carrier, u(10));

    let mut package = chain();
    package.folder(10).set(20, &[(10, &[1])]);
    for (summary, _) in package.objects.iter_mut() {
        if summary.uuid == u(20) {
            summary.qualified_type = "eml23.SomethingElse".to_string();
            summary.uri = format!("eml:///eml23.SomethingElse({})", u(20));
        }
    }
    let built = package.built();
    let walked = walk(&built, 1, &[(10, "collection")]);
    assert!(walked.resolution.is_none(), "{walked:?}");
}

// -- the shorter property walk -------------------------------------------------

/// A property climbs to its kind and the kind's parents, a collection at each
/// step — and **stops there**: nothing on the representation, the
/// interpretation, the feature or the model answers (§2.2).
///
/// To break it: let the property descend to the representation (the test
/// would find `representation`).
#[test]
fn a_property_does_not_climb_to_its_representation() {
    let mut package = chain();
    package
        .property_kind(40, Some(41))
        .property_kind(41, None)
        .property(50, 1, 40);
    package.model(30).earth_model(31, 30, &[2]);
    let built = package.built();
    let kinds = vec![u(40), u(41)];
    let carriers = |c: &[(u32, &'static str)]| {
        let map: HashMap<String, &'static str> = c.iter().map(|(n, l)| (u(*n), *l)).collect();
        move |identity: &str| map.get(&uuid_of(identity)).copied()
    };

    // Everything above is coloured; the property has nothing.
    let has = carriers(&[
        (1, "representation"),
        (2, "interpretation"),
        (3, "feature"),
        (31, "model"),
    ]);
    let walker = Walker::new(&built.index, &built.source, &has);
    let walked = walker.resolve_property(&u(50), &kinds);
    assert!(walked.resolution.is_none(), "{walked:?}");

    // The kind's parent answers at rank 5, two steps up.
    let has = carriers(&[(41, "parent kind"), (1, "representation")]);
    let walker = Walker::new(&built.index, &built.source, &has);
    let walked = walker.resolve_property(&u(50), &kinds);
    let found = hit(&walked);
    assert_eq!((found.value, found.rank, found.distance()), ("parent kind", 5, 2));
    assert_eq!(route(found), vec![(u(40), Via::Parent), (u(41), Via::Parent)]);
    assert_eq!(found.path[0].field, "PropertyKind");
    assert_eq!(found.path[1].field, "Parent");

    // The kind before the kind's parent; the property before its kind.
    let has = carriers(&[(40, "kind"), (41, "parent kind")]);
    let walker = Walker::new(&built.index, &built.source, &has);
    assert_eq!(hit(&walker.resolve_property(&u(50), &kinds)).rank, 3);
    let has = carriers(&[(50, "property"), (40, "kind")]);
    let walker = Walker::new(&built.index, &built.source, &has);
    assert_eq!(hit(&walker.resolve_property(&u(50), &kinds)).rank, 1);
}

/// The collections of the property and of its kind answer at ranks 2 and 4 —
/// the collection of the property before the kind, the kind before the
/// collection of the kind.
///
/// To break it: do not look for collections in a property's walk.
#[test]
fn a_property_has_its_collections_at_each_step() {
    let mut package = chain();
    package.property_kind(40, None).property(50, 1, 40);
    package.folder(10).folder(11).set(20, &[(10, &[50]), (11, &[40])]);
    let built = package.built();
    let kinds = vec![u(40)];
    let run = |c: &[(u32, &'static str)]| {
        let map: HashMap<String, &'static str> = c.iter().map(|(n, l)| (u(*n), *l)).collect();
        let has = move |identity: &str| map.get(&uuid_of(identity)).copied();
        let walker = Walker::new(&built.index, &built.source, &has);
        let walked = walker.resolve_property(&u(50), &kinds);
        let found = hit(&walked);
        (found.value, found.rank, found.distance())
    };
    assert_eq!(
        run(&[(10, "of the property"), (40, "kind")]),
        ("of the property", 2, 1)
    );
    assert_eq!(run(&[(40, "kind"), (11, "of the kind")]), ("kind", 3, 1));
    assert_eq!(run(&[(11, "of the kind")]), ("of the kind", 4, 2));
}

/// `kind_chain` is `property_kind`'s chain, by uuid: the same one the colour
/// cascade walks.
///
/// To break it: read it anywhere but in `property_kind::Resolver` (the parent
/// would disappear from the list).
#[test]
fn the_kind_chain_is_the_resolvers() {
    let mut package = chain();
    package
        .property_kind(40, Some(41))
        .property_kind(41, None)
        .property(50, 1, 40);
    let built = package.built();
    let chain = inherit::kind_chain(&built.index, &built.source, &u(50));
    assert_eq!(chain.uuids.first(), Some(&u(40)), "{chain:?}");
    assert_eq!(chain.uuids.get(1), Some(&u(41)), "{chain:?}");
}

// -- the batch -----------------------------------------------------------------

/// A population: the answers are the one-by-one answers, and an object that
/// several share — a collection — is **asked once**.
///
/// To break it: the memoisation (the counter exceeds 1); or a memoisation per
/// walk instead of per package (same on the shared collection).
#[test]
fn the_batch_equals_the_unit_and_asks_each_object_once() {
    let mut package = Package::default();
    package.folder(10);
    let members: Vec<u32> = (1..=5).collect();
    for n in &members {
        package.representation(*n, None);
    }
    package.set(20, &[(10, &members)]);
    let built = package.built();

    let asked: RefCell<HashMap<String, usize>> = RefCell::default();
    let has = |identity: &str| {
        let uuid = uuid_of(identity);
        *asked.borrow_mut().entry(uuid.to_string()).or_default() += 1;
        (uuid == u(10)).then_some("shared")
    };
    let uuids: Vec<String> = members.iter().map(|n| u(*n)).collect();
    let batch = inherit::resolve_all(&built.index, &built.source, &uuids, &has);
    assert_eq!(batch.len(), 5);
    assert_eq!(
        asked.borrow()[&u(10)],
        1,
        "the shared collection is asked once, not once per object"
    );
    assert!(asked.borrow().values().all(|n| *n == 1), "{:?}", asked.borrow());

    for (uuid, from_batch) in uuids.iter().zip(&batch) {
        let calls = Cell::new(0usize);
        let unit_has = |id: &str| {
            let id = uuid_of(id);
            calls.set(calls.get() + 1);
            (id == u(10)).then_some("shared")
        };
        let alone = inherit::resolve(&built.index, &built.source, uuid, &unit_has);
        assert_eq!(&alone, from_batch);
        assert_eq!(hit(from_batch).carrier, u(10));
    }
}

// -- the parents, against the cascade that already walks them -----------------

/// The parents the index gives are the parents `color::Cascade::chain` walks —
/// two independent readings of the three fields, one from the objects, one from
/// the index's edges.
///
/// To break it: remove `InterpretedFeature` from the list of parent fields (the
/// walk stops at the interpretation, the cascade does not).
#[test]
fn the_parents_are_the_cascades_parents() {
    let built = chain().built();
    let set = GraphicalSet::default();
    let cascade = Cascade::new(&set);
    let request = Request {
        representation: &u(1),
        property: None,
        kind_chain: &[],
        element: Scope::WholeObject,
    };
    let from_cascade: Vec<String> = cascade
        .chain(&request, &built.source)
        .expect("the cascade reads")
        .into_iter()
        .map(|step| step.uuid)
        .collect();
    assert_eq!(from_cascade, vec![u(1), u(2), u(3)]);

    let walked = walk(&built, 1, &[(3, "feature")]);
    let mut from_walk = vec![u(1)];
    from_walk.extend(hit(&walked).path.iter().map(|s| s.to.clone()));
    assert_eq!(from_walk, from_cascade);
}

/// An object that is not in the package, and a parent loop, are answered, not
/// crashed on.
///
/// To break it: remove the parents' cycle guard (the test does not terminate).
#[test]
fn a_loop_of_parents_is_named() {
    let mut package = Package::default();
    // 1 represents 2, 2 represents 1.
    package.representation(1, Some(2)).representation(2, Some(1));
    let built = package.built();
    let walked = walk(&built, 1, &[]);
    assert!(walked.resolution.is_none());
    assert!(
        walked.notes.iter().any(|n| n.starts_with("cycle:")),
        "{:?}",
        walked.notes
    );

    let walked = walk(&built, 99, &[(99, "alone")]);
    assert_eq!(hit(&walked).rank, 1);
    let walked = walk(&built, 99, &[]);
    assert!(walked.resolution.is_none());
}
