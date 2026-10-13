//! Cutting a package to a chosen set — `energyml_storage::extract`.
//!
//! The writing half needs a real package and is covered by the corpus run. What
//! is here is the **planning** half, which is where the decisions are: what is
//! kept, what is left, how much of what is left is clean, and — the one that
//! earns the feature its keep — which references the cut would break, told apart
//! from the ones the source was already breaking.

use energyml_storage::extract::{Broken, plan};
use energyml_storage::graph::{Edge, Graph, Node};
use energyml_storage::resource::ObjectSummary;

fn summary(uri: &str) -> ObjectSummary {
    ObjectSummary {
        uri: uri.to_string(),
        uuid: uri.to_string(),
        qualified_type: "resqml22.Thing".to_string(),
        content_type: "application/x-resqml+xml;version=2.2;type=Thing".to_string(),
        locator: uri.to_string(),
    }
}

fn node(uri: &str, present: bool) -> Node {
    Node {
        uri: uri.to_string(),
        uuid: uri.to_string(),
        qualified_type: "resqml22.Thing".to_string(),
        type_key: "Thing".to_string(),
        title: None,
        present,
        externally_defined: false,
    }
}

fn edge(from: &str, to: &str) -> Edge {
    Edge {
        from: from.to_string(),
        to: to.to_string(),
        path: "Ref".to_string(),
    }
}

/// A representation, its interpretation, that one's feature, and a spare object
/// nothing kept will point at.
fn package() -> (Vec<ObjectSummary>, Graph) {
    let resources = vec![
        summary("rep"),
        summary("interp"),
        summary("feature"),
        summary("spare"),
    ];
    let graph = Graph {
        nodes: vec![
            node("rep", true),
            node("interp", true),
            node("feature", true),
            node("spare", true),
            // Named and never held: a well-known property kind is the ordinary
            // case, and the package is not broken for it.
            node("kind", false),
        ],
        edges: vec![
            edge("rep", "interp"),
            edge("interp", "feature"),
            edge("rep", "kind"),
        ],
    };
    (resources, graph)
}

#[test]
fn a_closed_choice_breaks_nothing_and_leaves_a_clean_cut() {
    let (resources, graph) = package();
    let made = plan(
        &resources,
        &graph,
        &["rep".into(), "interp".into(), "feature".into()],
    );
    assert_eq!(made.kept, vec!["rep", "interp", "feature"]);
    assert_eq!(made.left, 1, "the spare object stays behind");
    // Nothing kept points at it, so the cut is clean where it was made.
    assert_eq!(made.clean, 1);
    assert!(made.is_clean(), "the well-known reference is not the cut's doing");
}

#[test]
fn a_choice_that_is_not_closed_names_every_reference_it_would_break() {
    // The whole reason this is planned rather than simply written: a
    // representation without its interpretation produces a package that does
    // not open, and nothing downstream would say which reference did it.
    let (resources, graph) = package();
    let made = plan(&resources, &graph, &["rep".into()]);
    assert!(!made.is_clean());
    let broke: Vec<&Broken> = made.broken.iter().filter(|one| one.in_source).collect();
    assert_eq!(broke.len(), 1);
    assert_eq!(broke[0].from, "rep");
    assert_eq!(broke[0].to, "interp");
    // With the path inside the source, so somebody can go and look.
    assert_eq!(broke[0].path, "Ref");
}

#[test]
fn a_reference_the_source_never_held_is_not_the_cut_s_fault() {
    // A well-known kind, a cross-package model boundary. Reporting it as broken
    // would blame the extraction for the package's own state — and a report that
    // cries wolf on every property is a report nobody reads.
    let (resources, graph) = package();
    let made = plan(
        &resources,
        &graph,
        &["rep".into(), "interp".into(), "feature".into()],
    );
    let carried: Vec<&Broken> = made.broken.iter().filter(|one| !one.in_source).collect();
    assert_eq!(carried.len(), 1, "it is still reported");
    assert_eq!(carried[0].to, "kind");
    assert!(made.is_clean(), "but it does not make the cut dirty");
}

#[test]
fn what_is_left_and_still_pointed_at_is_counted_apart() {
    // `left == clean` is what says a cut is self-contained. Here the spare is
    // pointed at by something kept, so it is left *and* not clean — which is the
    // signal that the choice was not closed.
    let mut resources = vec![summary("rep"), summary("spare")];
    resources.push(summary("interp"));
    let graph = Graph {
        nodes: vec![node("rep", true), node("spare", true), node("interp", true)],
        edges: vec![edge("rep", "spare"), edge("rep", "interp")],
    };
    let made = plan(&resources, &graph, &["rep".into(), "interp".into()]);
    assert_eq!(made.left, 1);
    assert_eq!(made.clean, 0, "the one left behind is still referred to");
}

#[test]
fn an_object_asked_for_and_not_held_is_named_rather_than_dropped() {
    // A decision replayed on a newer package names objects that have gone. A
    // shorter list with no word would be a decision nobody can trust twice.
    let (resources, graph) = package();
    let made = plan(&resources, &graph, &["rep".into(), "ghost".into()]);
    assert_eq!(made.unknown, vec!["ghost"]);
    assert!(!made.kept.contains(&"ghost".to_string()));
}

#[test]
fn the_kept_order_is_the_package_s_own() {
    // A package lists its parts in an order somebody chose; a cut has no reason
    // to shuffle them, and a stable order is what makes two extractions of the
    // same decision comparable.
    let (resources, graph) = package();
    let made = plan(
        &resources,
        &graph,
        &["feature".into(), "rep".into(), "interp".into()],
    );
    assert_eq!(made.kept, vec!["rep", "interp", "feature"]);
}

#[test]
fn asking_twice_for_the_same_object_keeps_it_once() {
    let (resources, graph) = package();
    let made = plan(&resources, &graph, &["rep".into(), "rep".into()]);
    assert_eq!(made.kept, vec!["rep"]);
}

/// The proxy relationship is a declaration, and the checker now says so.
///
/// Not a unit test of `plan` — it guards the fix that cutting a real package
/// uncovered. The library's own writer emits `mlToExternalPartProxy` for a
/// reference to an HDF5 proxy, which is what EPC §3.4.1 asks for; the checker
/// only accepted the generic `destinationObject`, so **every package this
/// library saved complained about a link it had itself written correctly**.
///
/// Two components of the house disagreeing about one fact is the shape the
/// rules forbid, and nothing synthetic would have found it: it took cutting a
/// package and running `check` on the result.
#[test]
fn the_proxy_relationship_counts_as_a_declaration() {
    use energyml_storage::resource::Relationship;

    let proxy = Relationship {
        id: "x".into(),
        kind: "http://schemas.energistics.org/package/2012/relationships/mlToExternalPartProxy".into(),
        target: "obj_EpcExternalPartReference_a.xml".into(),
        external: false,
    };
    assert_eq!(proxy.short_kind(), "mlToExternalPartProxy");
    let back = Relationship {
        kind: "http://schemas.energistics.org/package/2012/relationships/externalPartProxyToMl".into(),
        ..proxy.clone()
    };
    assert_eq!(back.short_kind(), "externalPartProxyToMl");
}
