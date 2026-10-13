//! The colour of an object **per target**, walked once, resumed, kept and
//! invalidated — `energyml_domain::color_walks`.
//!
//! ADR-0009. The requirement (quoted in full in the module): the colour
//! inheritance is computed once and each sub-part is looked up in the complete
//! result (or the walk goes on deeper into the fallbacks when it lacks the
//! information for a given cell type), and only the objects linked to a modified
//! object are searched, so as to do only what is necessary.
//!
//! Every package is **built in memory** (the pieces of `inherit.rs`), and the
//! shape of the assertions is the design's: the order of ranks, the closest
//! tier, the exact target before the whole object within a tier. What is counted
//! is **work** — tiers read, objects read, walks begun — never time.

use std::collections::HashMap;

use energyml_domain::color::{
    Cascade, ElementInfo, Entry, GraphicalSet, Hsv, Information, Level, Paint, Request, Scope,
};
use energyml_domain::color_walks::{Ask, ColourCache, Scoped, changed_targets, drawn_by};
use energyml_resqml_2_2 as resqml22;
use energyml_storage::ObjectSummary;

use crate::inherit::{Built, Package, chain, citation, dor, u};

fn hsv(hue: f64) -> Hsv {
    Hsv {
        hue,
        saturation: 1.0,
        value: 1.0,
        alpha: 1.0,
        title: None,
    }
}

/// The paint a colour named by `hue` gives.
fn paint(hue: f64) -> Paint {
    Paint::Constant(hsv(hue).to_rgba())
}

/// A set where object `n` carries, in one `DefaultGraphicalInformation`, a
/// constant colour for each `(target, hue)`.
fn painted(entries: &[(u32, &[(Scope, f64)])]) -> GraphicalSet {
    GraphicalSet {
        entries: entries
            .iter()
            .enumerate()
            .map(|(position, (n, scopes))| Entry {
                position,
                targets: vec![u(*n)],
                target_uris: Vec::new(),
                map_uri: None,
                information: Information::Default {
                    viewer_kind: None,
                    viewer_id: None,
                    elements: scopes
                        .iter()
                        .enumerate()
                        .map(|(at, (scope, hue))| ElementInfo {
                            at,
                            scope: *scope,
                            visible: true,
                            constant_color: Some(hsv(*hue)),
                            constant_alpha: None,
                            overwrite_color_alpha: false,
                            active_color: None,
                        })
                        .collect(),
                },
            })
            .collect(),
        notes: Vec::new(),
    }
}

/// A triangulated surface `n` of `interpretation` — a kind whose geometry draws
/// nodes and faces.
fn surface(package: &mut Package, n: u32, interpretation: Option<u32>) {
    let id = u(n);
    package.add(
        &id,
        "resqml22.TriangulatedSetRepresentation",
        Box::new(resqml22::TriangulatedSetRepresentation {
            uuid: id.clone(),
            citation: citation(&format!("surface {n}")),
            represented_object: interpretation.map(|i| dor(&u(i), "resqml22.WellboreInterpretation")),
            ..Default::default()
        }),
    );
}

fn ask(cache: &mut ColourCache, built: &Built, set: &GraphicalSet, n: u32, scopes: &[Scope]) -> Vec<Scoped> {
    let cascade = Cascade::new(set);
    let uuid = u(n);
    cache
        .resolve(
            &cascade,
            &built.index,
            &built.source,
            &[Ask { uri: &uuid, scopes }],
        )
        .remove(0)
}

fn carrier(scoped: &Scoped) -> Option<String> {
    scoped.walked.resolution.as_ref().map(|r| r.carrier.clone())
}

fn rank(scoped: &Scoped) -> Option<u8> {
    scoped.walked.resolution.as_ref().map(|r| r.rank)
}

fn level(scoped: &Scoped) -> Option<Level> {
    scoped.decision().map(|d| d.level)
}

// -- the table ---------------------------------------------------------------

/// A polyline draws nodes and edges and **nothing else**; a surface its nodes
/// and faces, its edges following the faces; a grid its volumes too.
///
/// To break it: put `Scope::Faces` in the polylines' row — the test goes red.
#[test]
fn a_polyline_never_asks_for_faces_or_volumes() {
    for kind in [
        "PolylineRepresentation",
        "PolylineSetRepresentation",
        "WellboreTrajectoryRepresentation",
        "obj_Trajectory",
    ] {
        let drawn = drawn_by(kind);
        assert_eq!(drawn.scopes, &[Scope::Nodes, Scope::Edges], "{kind}");
        assert!(
            !drawn.edges_follow_faces,
            "{kind}: a polyline's edges are its own"
        );
    }
    let surface = drawn_by("TriangulatedSetRepresentation");
    assert_eq!(surface.scopes, &[Scope::Nodes, Scope::Faces]);
    assert!(surface.edges_follow_faces);
    let grid = drawn_by("IjkGridRepresentation");
    assert_eq!(grid.scopes, &[Scope::Nodes, Scope::Faces, Scope::Volumes]);
    // What draws no target of its own is asked for the whole object only.
    assert!(drawn_by("WellboreFeature").scopes.is_empty());
    assert!(drawn_by("").scopes.is_empty());
}

// -- per target --------------------------------------------------------------

/// A colour for the **faces only**, on a collection, colours the faces of its
/// members and not their nodes.
///
/// To break it: put all the targets in the "whole object" slot of
/// `default_information_all` — the nodes would receive the faces' colour.
#[test]
fn a_faces_only_colour_on_a_collection_paints_the_faces_of_a_member_not_its_nodes() {
    let mut package = Package::default();
    surface(&mut package, 1, None);
    package.folder(10).set(20, &[(10, &[1])]);
    let built = package.built();
    let set = painted(&[(10, &[(Scope::Faces, 30.0)])]);

    let answers = ask(
        &mut ColourCache::new(),
        &built,
        &set,
        1,
        &[Scope::Nodes, Scope::Faces],
    );
    let (nodes, faces) = (&answers[0], &answers[1]);
    assert!(
        nodes.walked.resolution.is_none(),
        "nothing says anything about the nodes: {nodes:?}"
    );
    assert_eq!(carrier(faces), Some(u(10)));
    assert_eq!(rank(faces), Some(2));
    assert_eq!(level(faces), Some(Level::Collection));
    assert_eq!(faces.decision().map(|d| d.paint.clone()), Some(paint(30.0)));
    assert!(faces.exact, "the faces are what the collection says");
}

/// A colour for the **nodes only**, on the object itself.
///
/// To break it: make the requested target answered by any slot.
#[test]
fn a_nodes_only_colour_on_the_object_paints_its_nodes_and_not_its_faces() {
    let mut package = Package::default();
    surface(&mut package, 1, None);
    let built = package.built();
    let set = painted(&[(1, &[(Scope::Nodes, 200.0)])]);

    let answers = ask(
        &mut ColourCache::new(),
        &built,
        &set,
        1,
        &[Scope::Nodes, Scope::Faces],
    );
    assert_eq!(carrier(&answers[0]), Some(u(1)));
    assert_eq!(rank(&answers[0]), Some(1));
    assert_eq!(answers[0].decision().map(|d| d.paint.clone()), Some(paint(200.0)));
    assert!(answers[1].walked.resolution.is_none());
}

/// The whole object stands for a target nobody distinguished — **and the
/// closest wins**: the whole-object colour of the object itself (rank 1) beats
/// the faces colour of its collection (rank 2).
///
/// To break it: look for the exact target along the whole walk before falling back
/// on the whole object — the collection would win.
#[test]
fn the_whole_object_falls_back_for_a_target_and_the_closest_tier_wins() {
    let mut package = Package::default();
    surface(&mut package, 1, None);
    package.folder(10).set(20, &[(10, &[1])]);
    let built = package.built();
    let set = painted(&[(1, &[(Scope::WholeObject, 100.0)]), (10, &[(Scope::Faces, 30.0)])]);

    let answers = ask(
        &mut ColourCache::new(),
        &built,
        &set,
        1,
        &[Scope::Nodes, Scope::Faces],
    );
    for one in &answers {
        assert_eq!(carrier(one), Some(u(1)), "{:?}", one.scope);
        assert_eq!(one.decision().map(|d| d.paint.clone()), Some(paint(100.0)));
        assert!(!one.exact, "the whole object stands for {:?}", one.scope);
        assert!(
            one.decision()
                .unwrap()
                .notes
                .iter()
                .any(|n| n.contains("used what it says for the whole object")),
            "the fallback is said"
        );
    }
}

/// **Within one tier**, the exact target comes before the whole object.
///
/// To break it: test the whole object before the exact target within a tier.
#[test]
fn within_a_tier_the_exact_target_comes_before_the_whole_object() {
    let mut package = Package::default();
    surface(&mut package, 1, None);
    let built = package.built();
    let set = painted(&[(1, &[(Scope::WholeObject, 100.0), (Scope::Faces, 10.0)])]);

    let answers = ask(
        &mut ColourCache::new(),
        &built,
        &set,
        1,
        &[Scope::Nodes, Scope::Faces],
    );
    assert_eq!(
        answers[0].decision().map(|d| d.paint.clone()),
        Some(paint(100.0)),
        "nodes: the whole object"
    );
    assert_eq!(
        answers[1].decision().map(|d| d.paint.clone()),
        Some(paint(10.0)),
        "faces: their own"
    );
    assert!(answers[1].exact && !answers[0].exact);
}

/// The whole-object answer of the cache is the answer the walk always gave.
///
/// To break it: (see `color_inherit.rs`) — the `WholeObject` target must answer
/// only for the whole object, never for another target.
#[test]
fn the_whole_object_is_never_answered_by_another_target() {
    let mut package = Package::default();
    surface(&mut package, 1, None);
    let built = package.built();
    let set = painted(&[(1, &[(Scope::Faces, 10.0)])]);
    let answers = ask(
        &mut ColourCache::new(),
        &built,
        &set,
        1,
        &[Scope::WholeObject, Scope::Faces],
    );
    assert!(answers[0].walked.resolution.is_none());
    assert!(answers[1].walked.resolution.is_some());
}

// -- the walk, resumed ---------------------------------------------------------

/// A walk that went deep for one target **answers the next from what it read**,
/// and one that stopped early **resumes** for the next — and only that far.
///
/// The object (rank 1), its interpretation (rank 3) and its feature (rank 5) are
/// three tiers. The faces colour is on the feature, the nodes colour on the
/// interpretation.
///
/// To break it: re-read the walk for each target (`cursor` reset and `tiers`
/// emptied in `answer`) — the counts go up; or do not resume
/// (`read_next` without effect) — the second target would stay unanswered.
#[test]
fn a_walk_is_resumed_not_redone() {
    let mut package = chain();
    surface(&mut package, 4, Some(2));
    let built = package.built();
    let set = painted(&[(2, &[(Scope::Nodes, 200.0)]), (3, &[(Scope::Faces, 30.0)])]);

    // Faces first: the walk goes all the way to the feature…
    let mut deep = ColourCache::new();
    let faces = ask(&mut deep, &built, &set, 4, &[Scope::Faces]);
    assert_eq!(carrier(&faces[0]), Some(u(3)));
    let after_faces = deep.stats();
    assert_eq!(
        after_faces.tiers_read, 3,
        "the object, its interpretation, its feature"
    );
    // …and the nodes, found on the interpretation, are **already there**.
    let nodes = ask(&mut deep, &built, &set, 4, &[Scope::Nodes]);
    assert_eq!(carrier(&nodes[0]), Some(u(2)));
    assert_eq!(
        deep.stats(),
        after_faces,
        "nothing was read for the second target"
    );

    // Nodes first: the walk stops at the interpretation…
    let mut shallow = ColourCache::new();
    ask(&mut shallow, &built, &set, 4, &[Scope::Nodes]);
    let after_nodes = shallow.stats();
    assert_eq!(after_nodes.tiers_read, 2, "the object and its interpretation");
    // …and the faces **resume it**: one tier more, one object more.
    let faces = ask(&mut shallow, &built, &set, 4, &[Scope::Faces]);
    assert_eq!(carrier(&faces[0]), Some(u(3)));
    let after_both = shallow.stats();
    assert_eq!(after_both.tiers_read, 3);
    assert_eq!(
        after_both.objects_read,
        after_nodes.objects_read + 1,
        "only the feature was read"
    );
    assert_eq!(after_both.walks_begun, 1, "one walk, resumed");

    // The control: the same two questions with no memory read the first tiers twice.
    let mut forgetful = ColourCache::new();
    ask(&mut forgetful, &built, &set, 4, &[Scope::Nodes]);
    forgetful.clear();
    ask(&mut forgetful, &built, &set, 4, &[Scope::Faces]);
    assert!(
        forgetful.stats().tiers_read > after_both.tiers_read,
        "{:?}",
        forgetful.stats()
    );

    // Same answers whichever order they are asked in.
    let both = ask(
        &mut ColourCache::new(),
        &built,
        &set,
        4,
        &[Scope::Nodes, Scope::Faces],
    );
    assert_eq!(both[0], nodes[0]);
    assert_eq!(both[1], faces[0]);
}

/// **A polyline does not read what a surface needs**: with a faces colour on the
/// feature and a whole-object colour on the polyline itself, the polyline's walk
/// stops at the first tier; the surface's goes to the feature.
///
/// To break it: ask every object for the five targets (the `drawn_by` table
/// ignored) — the polyline's count goes up to three.
#[test]
fn a_polyline_stops_where_its_targets_are_answered() {
    let mut package = chain();
    surface(&mut package, 4, Some(2));
    let built = package.built();
    let set = painted(&[(1, &[(Scope::WholeObject, 100.0)]), (3, &[(Scope::Faces, 30.0)])]);

    let cascade = Cascade::new(&set);
    let one = Request {
        representation: &u(1),
        property: None,
        kind_chain: &[],
        element: Scope::WholeObject,
    };
    let mut polyline = ColourCache::new();
    let ladders = cascade
        .ladders_cached(&mut polyline, &built.index, &built.source, &[one])
        .expect("reads");
    let scopes: Vec<Scope> = ladders[0].targets.iter().map(|t| t.scope).collect();
    assert_eq!(scopes, vec![Scope::Nodes, Scope::Edges], "no faces, no volumes");
    assert_eq!(
        polyline.stats().tiers_read,
        1,
        "answered on the object itself: nothing above it was read"
    );

    let two = Request {
        representation: &u(4),
        property: None,
        kind_chain: &[],
        element: Scope::WholeObject,
    };
    let mut skin = ColourCache::new();
    let ladders = cascade
        .ladders_cached(&mut skin, &built.index, &built.source, &[two])
        .expect("reads");
    let scopes: Vec<Scope> = ladders[0].targets.iter().map(|t| t.scope).collect();
    assert_eq!(
        scopes,
        vec![Scope::Nodes, Scope::Faces, Scope::Edges],
        "the edges follow the faces"
    );
    assert!(skin.stats().tiers_read >= 3, "the faces went up to the feature");
}

/// A surface's edges are drawn with its faces — and the answer **says** when the
/// file gave them another colour.
///
/// To break it: copy the edges' colour instead of the faces' (the paintings
/// test goes red) or silence the note (the note test).
#[test]
fn the_edges_of_a_surface_take_the_faces_colour_and_say_so() {
    let mut package = Package::default();
    surface(&mut package, 1, None);
    let built = package.built();
    let set = painted(&[(1, &[(Scope::Faces, 10.0), (Scope::Edges, 250.0)])]);
    let cascade = Cascade::new(&set);
    let request = Request {
        representation: &u(1),
        property: None,
        kind_chain: &[],
        element: Scope::WholeObject,
    };
    let ladders = cascade
        .ladders(&built.index, &built.source, &[request])
        .expect("reads");
    let edges = ladders[0]
        .targets
        .iter()
        .find(|t| t.scope == Scope::Edges)
        .expect("the edges are answered");
    assert_eq!(edges.merged_into, Some(Scope::Faces));
    assert_eq!(
        edges.decision().map(|d| d.paint.clone()),
        Some(paint(10.0)),
        "the faces' colour, not 250"
    );
    let note = edges
        .merged_note
        .as_deref()
        .expect("the file's own edge colour is said");
    assert!(note.contains(&u(1)) && note.contains("edges"), "{note}");

    // Nothing to say when the file gives them nothing of their own.
    let quiet = painted(&[(1, &[(Scope::Faces, 10.0)])]);
    let cascade = Cascade::new(&quiet);
    let request = Request {
        representation: &u(1),
        property: None,
        kind_chain: &[],
        element: Scope::WholeObject,
    };
    let ladders = cascade
        .ladders(&built.index, &built.source, &[request])
        .expect("reads");
    let edges = ladders[0]
        .targets
        .iter()
        .find(|t| t.scope == Scope::Edges)
        .unwrap();
    assert!(edges.merged_note.is_none());
}

/// The batch says what the unit says, target by target.
#[test]
fn the_batch_says_what_one_object_says_per_target() {
    let mut package = chain();
    surface(&mut package, 4, Some(2));
    package.folder(10).set(20, &[(10, &[1, 4])]);
    let built = package.built();
    let set = painted(&[(10, &[(Scope::Faces, 30.0)]), (2, &[(Scope::WholeObject, 60.0)])]);
    let cascade = Cascade::new(&set);
    let scopes = [Scope::Nodes, Scope::Edges, Scope::Faces, Scope::Volumes];
    let (a, b) = (u(1), u(4));
    let asks = [
        Ask {
            uri: &a,
            scopes: &scopes,
        },
        Ask {
            uri: &b,
            scopes: &scopes,
        },
    ];
    let batch = ColourCache::new().resolve(&cascade, &built.index, &built.source, &asks);
    for (n, from_batch) in [(1u32, &batch[0]), (4, &batch[1])] {
        let alone = ask(&mut ColourCache::new(), &built, &set, n, &scopes);
        assert_eq!(&alone, from_batch, "object {n}");
    }
    assert!(
        batch[0].iter().any(|one| one.decision().is_some()),
        "the comparison proves something"
    );
}

// -- invalidation --------------------------------------------------------------

/// Six objects, no more: `1` and `2` are members of the folder `10`, itself in
/// the folder `12`; `5` is in the folder `11`; `6` has no link at all.
fn linked() -> Package {
    let mut package = Package::default();
    surface(&mut package, 1, None);
    surface(&mut package, 2, None);
    surface(&mut package, 5, None);
    surface(&mut package, 6, None);
    package.folder(10).folder(11).folder(12);
    package
        .set(20, &[(10, &[1, 2])])
        .set(21, &[(11, &[5])])
        .set(22, &[(12, &[10])]);
    package
}

fn resolve_all(cache: &mut ColourCache, built: &Built, set: &GraphicalSet) {
    for n in [1, 2, 5, 6] {
        ask(cache, built, set, n, &[Scope::Faces]);
    }
}

/// Changing the colour of a collection throws away the walks of **its members
/// and of the members of its sub-collections**, and of nobody else.
///
/// To break it: a `disturb` that throws everything away — `5` and `6` lose their
/// walk; or that throws nothing away — `1` and `2` keep the old colour.
#[test]
fn changing_a_collections_colour_drops_only_the_walks_that_went_through_it() {
    let built = linked().built();
    let before = painted(&[(12, &[(Scope::Faces, 30.0)]), (11, &[(Scope::Faces, 200.0)])]);
    let mut cache = ColourCache::new();
    resolve_all(&mut cache, &built, &before);
    assert_eq!(cache.stats().walks_begun, 4);
    for n in [1, 2, 5, 6] {
        assert!(cache.holds(&u(n)));
    }

    // The folder 12 changes colour: the set says so, the cache hears of it.
    let after = painted(&[(12, &[(Scope::Faces, 100.0)]), (11, &[(Scope::Faces, 200.0)])]);
    let changed = changed_targets(&before, &after);
    assert_eq!(changed, vec![u(12)], "one entry moved: the targets of that entry");
    let dropped = cache.disturb(&changed);
    assert_eq!(dropped, 2, "the members of 10, which is in 12");
    assert!(!cache.holds(&u(1)) && !cache.holds(&u(2)));
    assert!(
        cache.holds(&u(5)) && cache.holds(&u(6)),
        "unrelated walks are exact still"
    );

    // Everything asked again: two walks are made again, not four.
    let begun = cache.stats().walks_begun;
    resolve_all(&mut cache, &built, &after);
    assert_eq!(cache.stats().walks_begun, begun + 2, "{:?}", cache.stats());
    let one = ask(&mut cache, &built, &after, 1, &[Scope::Faces]);
    assert_eq!(
        one[0].decision().map(|d| d.paint.clone()),
        Some(paint(100.0)),
        "the new colour"
    );
    let five = ask(&mut cache, &built, &after, 5, &[Scope::Faces]);
    assert_eq!(five[0].decision().map(|d| d.paint.clone()), Some(paint(200.0)));
}

/// A collection that **gains a member** disturbs that member's walk and nobody
/// else's — not the collection's other members.
///
/// The change is played through the index (`PackageIndex::update_with`), which
/// says what it disturbed; the package after is built again.
///
/// To break it: make `disturbed_by` disturb the association set's neighbours (all
/// the members) — `5` would lose its walk.
#[test]
fn a_collection_that_gains_a_member_disturbs_only_that_member() {
    let before = linked();
    let mut after = linked();
    // The set 21 of `after` also holds 6.
    after.objects.retain(|(summary, _)| summary.uuid != u(21));
    after.set(21, &[(11, &[5, 6])]);
    let summaries: HashMap<String, ObjectSummary> = after
        .objects
        .iter()
        .map(|(summary, _)| (summary.uuid.to_ascii_lowercase(), summary.clone()))
        .collect();

    let mut built = before.built();
    let after = after.built();
    let set = painted(&[(11, &[(Scope::Faces, 200.0)])]);
    let mut cache = ColourCache::new();
    resolve_all(&mut cache, &built, &set);
    assert!(
        ask(&mut cache, &built, &set, 6, &[Scope::Faces])[0]
            .walked
            .resolution
            .is_none()
    );

    let disturbed = built
        .index
        .update_with(&after.source, &|uuid| summaries.get(uuid).cloned(), &[u(21)], 2);
    assert!(disturbed.contains(&u(6)), "the new member: {disturbed:?}");
    assert!(
        !disturbed.contains(&u(5)),
        "the other member did not change: {disturbed:?}"
    );
    assert!(
        !disturbed.contains(&u(11)),
        "nor did the collection: {disturbed:?}"
    );
    cache.disturb(&disturbed);
    assert!(!cache.holds(&u(6)));
    for n in [1, 2, 5] {
        assert!(cache.holds(&u(n)), "{n} is exact still");
    }

    // Asked again, 6 now answers from the collection; one walk was begun.
    let begun = cache.stats().walks_begun;
    let six = ask(&mut cache, &built, &set, 6, &[Scope::Faces]);
    assert_eq!(carrier(&six[0]), Some(u(11)));
    assert_eq!(cache.stats().walks_begun, begun + 1);
}

/// The set moves the walks of the objects whose entries moved — **and the
/// positions count**: a set that grows shifts every later entry.
///
/// To break it: compare the entries without their position — the shift test goes red.
#[test]
fn the_positions_of_the_entries_are_part_of_what_changed() {
    let before = painted(&[(1, &[(Scope::Faces, 10.0)]), (2, &[(Scope::Faces, 20.0)])]);
    // Same two entries, a new one in front: every position moved.
    let mut after = painted(&[
        (9, &[(Scope::Faces, 90.0)]),
        (1, &[(Scope::Faces, 10.0)]),
        (2, &[(Scope::Faces, 20.0)]),
    ]);
    assert_eq!(changed_targets(&before, &after), vec![u(1), u(2), u(9)]);
    // Same set: nothing moved.
    after = before.clone();
    assert!(changed_targets(&before, &after).is_empty());
    // One entry appended: only its targets.
    let appended = painted(&[
        (1, &[(Scope::Faces, 10.0)]),
        (2, &[(Scope::Faces, 20.0)]),
        (7, &[(Scope::Nodes, 1.0)]),
    ]);
    assert_eq!(changed_targets(&before, &appended), vec![u(7)]);
}
