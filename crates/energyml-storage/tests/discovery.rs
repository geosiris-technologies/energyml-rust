//! Which objects a context reaches: [`discover`], the default of
//! `EnergymlStorage::resources_in`.
//!
//! The rules come from ETP 1.2: a dataspace URI ignores scope and depth and
//! answers the dataspace's content (§8.2.2, row 9); an object URI walks the
//! graph to the depth, in the direction of the scope, then the secondary
//! expansions, then the type filter, then the edges with either end among what
//! is kept (§8.1.1.1.4, spelled out in §8.3.1 p. 163, 3.c). Sources and targets are those of §8.1.1.1.1: the object holding
//! a reference is its source. The graph here is fabricated - the rules are the
//! specification's, not a package's.
//!
//! To make them fail: walk the other way for `Sources`, filter the types before
//! the walk, or follow edges by kind when no edge states its kind.

use energyml_storage::context::discover;
use energyml_storage::{Context, Edge, EdgeKind, Error, Navigable, ObjectSummary, Scope};

const FEATURE: &str = "eml:///resqml22.BoundaryFeature(7e000000-0000-4000-8000-000000000001)";
const HORIZON: &str = "eml:///resqml22.HorizonInterpretation(7e000000-0000-4000-8000-000000000002)";
const GRID: &str = "eml:///resqml22.Grid2dRepresentation(7e000000-0000-4000-8000-000000000003)";
const PROPERTY: &str = "eml:///resqml22.ContinuousProperty(7e000000-0000-4000-8000-000000000004)";
const CRS: &str = "eml:///eml23.LocalEngineeringCompoundCrs(7e000000-0000-4000-8000-000000000005)";

fn summary(uri: &str) -> ObjectSummary {
    let open = uri.find('(').expect("a uri with a uuid");
    let qualified = &uri["eml:///".len()..open];
    let uuid = &uri[open + 1..uri.len() - 1];
    ObjectSummary::new(uri, uuid, qualified)
}

/// grid -> horizon -> feature; property -> grid; grid -> crs.
fn graph(kinds: bool) -> (Vec<ObjectSummary>, Vec<Edge>) {
    let resources = [FEATURE, HORIZON, GRID, PROPERTY, CRS].map(summary).to_vec();
    let edge = |from: &str, to: &str, kind: EdgeKind| {
        let edge = Edge::new(from, to, "path");
        if kinds { edge.with_kind(kind) } else { edge }
    };
    let edges = vec![
        edge(GRID, HORIZON, EdgeKind::Primary),
        edge(HORIZON, FEATURE, EdgeKind::Primary),
        edge(PROPERTY, GRID, EdgeKind::Primary),
        edge(GRID, CRS, EdgeKind::Secondary),
    ];
    (resources, edges)
}

fn uris(found: &energyml_storage::Found) -> Vec<&str> {
    let mut out: Vec<&str> = found.resources.iter().map(|r| r.uri.as_str()).collect();
    out.sort_unstable();
    out
}

fn sorted(mut uris: Vec<&str>) -> Vec<&str> {
    uris.sort_unstable();
    uris
}

fn ask(context: &Context, kinds: bool) -> energyml_storage::Result<energyml_storage::Found> {
    let (resources, edges) = graph(kinds);
    discover("eml:///", resources, edges, context)
}

#[test]
fn a_dataspace_answers_its_content_whatever_the_scope_and_depth() {
    let context = Context::dataspace("eml:///");
    let found = ask(&context, false).unwrap();
    assert_eq!(found.resources.len(), 5);
    assert_eq!(found.edges.len(), 4, "every edge has both ends kept");

    let mut deep = Context::of("eml:///", Scope::Sources).with_depth(0);
    deep.types = vec!["resqml22.Grid2dRepresentation".into()];
    let found = ask(&deep, false).unwrap();
    assert_eq!(uris(&found), vec![GRID], "scope and depth ignored, the type kept");
    assert_eq!(
        found.edges.len(),
        3,
        "every edge touching the grid, its other end filtered out or not (ETP 1.2 §8.3.1, 3.c)"
    );
}

#[test]
fn another_dataspace_and_an_absent_object_are_not_found() {
    let other = ask(&Context::dataspace("eml:///dataspace('elsewhere')"), false);
    assert!(matches!(other, Err(Error::NotFound(_))), "{other:?}");
    let absent = ask(
        &Context::of(
            "eml:///resqml22.BoundaryFeature(7e000000-0000-4000-8000-0000000000ff)",
            Scope::Targets,
        ),
        false,
    );
    assert!(matches!(absent, Err(Error::NotFound(_))), "{absent:?}");
}

#[test]
fn targets_are_what_an_object_references_to_the_depth() {
    let one = ask(&Context::of(GRID, Scope::Targets), false).unwrap();
    assert_eq!(uris(&one), sorted(vec![HORIZON, CRS]));
    let two = ask(&Context::of(GRID, Scope::Targets).with_depth(2), false).unwrap();
    assert_eq!(uris(&two), sorted(vec![HORIZON, CRS, FEATURE]));
    let with_self = ask(&Context::of(GRID, Scope::TargetsOrSelf), false).unwrap();
    assert_eq!(uris(&with_self), sorted(vec![GRID, HORIZON, CRS]));
}

#[test]
fn sources_are_what_references_an_object() {
    // ETP 1.2 §8.1.1.1.3: the horizon interpretations and 2D grids of a feature
    // are its sources at depth 2.
    let mut context = Context::of(FEATURE, Scope::Sources).with_depth(2);
    context.types = vec![
        "resqml22.HorizonInterpretation".into(),
        "resqml22.Grid2dRepresentation".into(),
    ];
    let found = ask(&context, false).unwrap();
    assert_eq!(uris(&found), sorted(vec![HORIZON, GRID]));
    assert_eq!(
        found.edges.len(),
        4,
        "grid -> horizon, horizon -> feature, property -> grid, grid -> crs: one end found is enough"
    );
}

#[test]
fn the_types_are_filtered_after_the_walk_not_before() {
    // The property reaches the horizon only through the grid: filtering first
    // would cut the walk at the grid and find nothing.
    let context = Context::of(PROPERTY, Scope::Targets)
        .with_depth(2)
        .with_types(["resqml22.HorizonInterpretation"]);
    let found = ask(&context, false).unwrap();
    assert_eq!(uris(&found), vec![HORIZON]);
}

#[test]
fn itself_is_the_object_alone_whatever_the_depth() {
    let found = ask(&Context::of(GRID, Scope::Itself).with_depth(5), false).unwrap();
    assert_eq!(uris(&found), vec![GRID]);
}

#[test]
fn following_by_kind_is_refused_when_no_edge_states_its_kind() {
    let mut context = Context::of(GRID, Scope::Targets);
    context.navigable = Navigable::Primary;
    let refused = ask(&context, false);
    assert!(matches!(refused, Err(Error::UnstatedKind { .. })), "{refused:?}");

    let mut expanded = Context::of(GRID, Scope::Targets);
    expanded.include_secondary_targets = true;
    let refused = ask(&expanded, false);
    assert!(matches!(refused, Err(Error::UnstatedKind { .. })), "{refused:?}");
}

#[test]
fn with_kinds_stated_the_walk_follows_them_and_expands_once() {
    let mut primary = Context::of(GRID, Scope::Targets).with_depth(2);
    primary.navigable = Navigable::Primary;
    assert_eq!(
        uris(&ask(&primary, true).unwrap()),
        sorted(vec![HORIZON, FEATURE])
    );

    let mut secondary = Context::of(GRID, Scope::Targets);
    secondary.navigable = Navigable::Secondary;
    assert_eq!(uris(&ask(&secondary, true).unwrap()), vec![CRS]);

    // Primary from the property reaches the grid; the secondary expansion adds
    // the grid's secondary target, the crs - once, not walked further.
    let mut expanded = Context::of(PROPERTY, Scope::Targets);
    expanded.navigable = Navigable::Primary;
    expanded.include_secondary_targets = true;
    assert_eq!(uris(&ask(&expanded, true).unwrap()), sorted(vec![GRID, CRS]));
}

#[test]
fn a_wildcard_type_is_every_type_of_its_schema_version() {
    // ETP 1.2 §23.34.15: `resqml22.*` means every RESQML 2.2 type.
    let context = Context::dataspace("eml:///").with_types(["resqml22.*"]);
    let found = ask(&context, false).unwrap();
    assert_eq!(
        uris(&found),
        sorted(vec![FEATURE, HORIZON, GRID, PROPERTY]),
        "the eml23 crs is not resqml22"
    );
    let other = Context::dataspace("eml:///").with_types(["eml23.*"]);
    assert_eq!(uris(&ask(&other, false).unwrap()), vec![CRS]);
}

#[test]
fn a_misplaced_star_is_refused_rather_than_read_as_a_name() {
    for odd in ["resqml*", "res*ml22.Grid2dRepresentation", "*.*"] {
        let refused = ask(&Context::dataspace("eml:///").with_types([odd]), false);
        assert!(matches!(refused, Err(Error::BadUri { .. })), "{odd}: {refused:?}");
    }
}

#[test]
fn counts_are_the_distinct_objects_at_the_other_end_of_the_stated_edges() {
    let mut context = Context::dataspace("eml:///");
    context.count_objects = true;
    let found = ask(&context, false).unwrap();
    let counts = |uri: &str| {
        let summary = found.resources.iter().find(|r| r.uri == uri).expect(uri);
        (summary.source_count, summary.target_count)
    };
    assert_eq!(
        counts(GRID),
        (Some(1), Some(2)),
        "the property points at it; it points at horizon and crs"
    );
    assert_eq!(counts(FEATURE), (Some(1), Some(0)));
    assert_eq!(counts(PROPERTY), (Some(0), Some(1)));

    let uncounted = ask(&Context::dataspace("eml:///"), false).unwrap();
    assert!(
        uncounted
            .resources
            .iter()
            .all(|r| r.source_count.is_none() && r.target_count.is_none())
    );
}

#[test]
fn a_status_the_store_does_not_state_is_not_filtered_as_nothing() {
    let mut context = Context::dataspace("eml:///");
    context.active_status = Some(energyml_storage::ActiveStatus::Inactive);
    let refused = ask(&context, false);
    assert!(matches!(refused, Err(Error::Unsupported { .. })), "{refused:?}");
}

#[test]
fn types_of_counts_what_a_context_reached() {
    let found = ask(&Context::of(GRID, Scope::TargetsOrSelf), false).unwrap();
    assert_eq!(
        energyml_storage::context::types_of(&found),
        vec![
            ("eml23.LocalEngineeringCompoundCrs".to_string(), 1),
            ("resqml22.Grid2dRepresentation".to_string(), 1),
            ("resqml22.HorizonInterpretation".to_string(), 1),
        ]
    );
}
