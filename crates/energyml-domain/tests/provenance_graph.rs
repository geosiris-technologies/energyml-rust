//! The provenance graph: `energyml_domain::provenance_graph` (WP07 §7).
//!
//! The specification the shapes come from is common TRG 2.3 ch. 3 (the paquet
//! *Activities*, p. 39 to 49): `Activity.Parent` 0..1 (§3.3, p. 42), the parameters
//! and their templates (§3.1, §3.10), and ch. 8 (§8.3) for the collections and their
//! association sets. Every package here is **built in memory**; no file is read.
//!
//! The graph is the place where two writings of the same fact are confronted (the
//! `Parent` of a child and the `subActivity` parameters of its parent; an activity and
//! a verbatim table): the tests put the two writings on two different things and ask
//! which wins. What it would take to make each one fail is in the last line of its
//! comment.

use std::io::Cursor;
use std::sync::Arc;

use energyml_core::registry::DynObject;
use energyml_domain::activity::{
    ActivityParameter, ActivityRecord, ActivityRole, DeclaredReference, ParameterKind, ParameterRole,
    ParameterTemplateRecord, ParameterDirection, ParameterValue, RoleBasis, RoleRules, TemplateRecord,
};
use energyml_domain::package_index::PackageIndex;
use energyml_domain::provenance_graph::{
    CarrierRule, CarrierRules, DeclaredStep, EdgeBasis, ExpandRefusal, FocusRefusal, GraphIssue, GraphOptions,
    InputFileReader, NoInputFiles, NoStepConstraints, NodeKey, OrderBasis, Parts, ProvEdge, ProvEdgeKind,
    ProvNode, ProvenanceGraph, Statement, StepConstraint, StepConstraintReader, TimelineRefusal, TreeRefusal,
    TreeShape, activity_edges, activity_tree, assemble, carriers, focus, of_package, timeline, workflow_steps,
    DEFAULT_SET_ASIDE,
};
use energyml_domain::provenance_verbatim::{VerbatimId, VerbatimSources};
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_2 as resqml22;
use energyml_storage::EpcStore;

use crate::inherit::{citation, dor, u};

// -- building blocks ---------------------------------------------------------

const FEATURE: &str = "resqml22.WellboreFeature";

fn activity_uri(n: u32) -> String {
    format!("eml:///eml23.Activity({})", u(n))
}

fn feature_uri(n: u32) -> String {
    format!("eml:///{FEATURE}({})", u(n))
}

fn reference(address: &str) -> DeclaredReference {
    DeclaredReference {
        address: address.to_string(),
        uuid: energyml_core::identity::parse_address(address).0,
        qualified_type: None,
        title: None,
        version: None,
    }
}

fn parameter(position: usize, title: &str, value: ParameterValue) -> ActivityParameter {
    ActivityParameter {
        position,
        title: title.to_string(),
        index: None,
        selection: None,
        is_uncertain: Some(false),
        keys: Vec::new(),
        value,
        class: "eml23.DataObjectParameter".to_string(),
    }
}

fn object_parameter(position: usize, title: &str, address: &str) -> ActivityParameter {
    parameter(position, title, ParameterValue::Object(reference(address)))
}

fn record(n: u32, parameters: Vec<ActivityParameter>, parent: Option<u32>) -> ActivityRecord {
    ActivityRecord {
        uri: activity_uri(n),
        title: Some(format!("activity {n}")),
        creation: None,
        last_update: None,
        originator: None,
        format: None,
        descriptor: None,
        parent: parent.map(|p| reference(&activity_uri(p))),
        parameters,
    }
}

fn role(position: usize, role: ActivityRole) -> ParameterRole {
    ParameterRole {
        position,
        role,
        basis: RoleBasis::Template,
    }
}

fn has_edge(edges: &[ProvEdge], from: &NodeKey, to: &NodeKey, kind: &ProvEdgeKind) -> bool {
    edges
        .iter()
        .any(|edge| &edge.from == from && &edge.to == to && &edge.kind == kind)
}

fn object(uri: &str) -> NodeKey {
    NodeKey::Object(uri.to_string())
}

fn node_keys(graph: &ProvenanceGraph) -> Vec<NodeKey> {
    graph.nodes.iter().map(ProvNode::key).collect()
}

fn store() -> EpcStore {
    EpcStore::from_reader(
        Box::new(Cursor::new(
            energyml_storage::extract::empty_package(None).expect("an empty package"),
        )),
        "provenance_graph.epc",
        Arc::new(energyml_schemas::all()),
    )
    .expect("it opens")
}

fn put(store: &mut EpcStore, object: impl DynObject + 'static) {
    store.put(Box::new(object)).expect("it goes in");
}

fn index_of(store: &EpcStore) -> PackageIndex {
    PackageIndex::build(store, store.revision())
}

fn feature(store: &mut EpcStore, n: u32) {
    put(
        store,
        resqml22::WellboreFeature {
            uuid: u(n),
            schema_version: "2.2".into(),
            citation: citation(&format!("feature {n}")),
            ..Default::default()
        },
    );
}

fn interpretation(store: &mut EpcStore, n: u32, of: u32) {
    put(
        store,
        resqml22::WellboreInterpretation {
            uuid: u(n),
            schema_version: "2.2".into(),
            citation: citation(&format!("interpretation {n}")),
            interpreted_feature: dor(&u(of), FEATURE),
            ..Default::default()
        },
    );
}

fn representation(store: &mut EpcStore, n: u32, of: u32) {
    put(
        store,
        resqml22::WellboreTrajectoryRepresentation {
            uuid: u(n),
            schema_version: "2.2".into(),
            citation: citation(&format!("representation {n}")),
            represented_object: Some(dor(&u(of), "resqml22.WellboreInterpretation")),
            ..Default::default()
        },
    );
}

fn property(store: &mut EpcStore, n: u32, on: u32) {
    put(
        store,
        resqml22::ContinuousProperty {
            uuid: u(n),
            schema_version: "2.2".into(),
            citation: citation(&format!("property {n}")),
            property_kind: dor(&u(900), "eml23.PropertyKind"),
            supporting_representation: dor(&u(on), "resqml22.WellboreTrajectoryRepresentation"),
            ..Default::default()
        },
    );
}

fn collection(store: &mut EpcStore, n: u32) {
    put(
        store,
        eml23::DataobjectCollection {
            uuid: u(n),
            schema_version: "2.3".into(),
            citation: citation(&format!("collection {n}")),
            kind: Some(eml23::CollectionKindExt::Folder),
            ..Default::default()
        },
    );
}

/// One association set: `(collection, members)` per association, in order.
fn set(store: &mut EpcStore, n: u32, associations: &[(u32, &[u32])]) {
    put(
        store,
        eml23::CollectionsToDataobjectsAssociationSet {
            uuid: u(n),
            schema_version: "2.3".into(),
            citation: citation(&format!("set {n}")),
            single_collection_association: associations
                .iter()
                .map(|(collection, members)| eml23::SingleCollectionAssociation {
                    collection: dor(&u(*collection), "eml23.DataobjectCollection"),
                    dataobject: members.iter().map(|member| dor(&u(*member), FEATURE)).collect(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        },
    );
}

fn uri_in(index: &PackageIndex, n: u32) -> String {
    index.uri_of(&u(n)).expect("the package holds it").to_string()
}

/// A reader that reads the `held: <reason>` form, for the tests: the domain knows no convention.
struct HeldReader;

impl StepConstraintReader for HeldReader {
    fn read(&self, constraint: &str) -> Option<StepConstraint> {
        let reason = constraint.strip_prefix("held:")?;
        Some(StepConstraint {
            operation: "test".to_string(),
            template_uuid: None,
            serves: Vec::new(),
            held: Some(reason.trim().to_string()),
        })
    }
}

/// A reader that says every text parameter named `file` is a file path.
struct FileTitled;

impl InputFileReader for FileTitled {
    fn id(&self) -> &'static str {
        "test:file"
    }

    fn file(&self, _activity: &ActivityRecord, parameter: &ActivityParameter) -> Option<String> {
        match (&parameter.value, parameter.title.as_str()) {
            (ParameterValue::Text(path), "file") => Some(path.clone()),
            _ => None,
        }
    }
}

// -- activity_edges ----------------------------------------------------------

#[test]
fn inputs_point_in_outputs_point_out() {
    // common TRG 2.3 §3.10, p. 49: IsInput / IsOutput, and an object that is both takes two parameters.
    let a = record(
        1,
        vec![
            object_parameter(0, "input", &feature_uri(1)),
            object_parameter(1, "output", &feature_uri(2)),
            object_parameter(2, "both", &feature_uri(3)),
        ],
        None,
    );
    let roles = [
        role(0, ActivityRole::Input),
        role(1, ActivityRole::Output),
        role(2, ActivityRole::Both),
    ];
    let edges = activity_edges(&a, &roles, &NoInputFiles);
    let me = object(&activity_uri(1));
    assert!(has_edge(&edges, &object(&feature_uri(1)), &me, &ProvEdgeKind::Consumed));
    assert!(has_edge(&edges, &me, &object(&feature_uri(2)), &ProvEdgeKind::Produced));
    assert!(has_edge(&edges, &object(&feature_uri(3)), &me, &ProvEdgeKind::Consumed));
    assert!(has_edge(&edges, &me, &object(&feature_uri(3)), &ProvEdgeKind::Produced));
    assert_eq!(edges.len(), 4, "one edge for an input, one for an output, two for both");
    assert!(
        edges
            .iter()
            .all(|edge| edge.basis == EdgeBasis::Activity(RoleBasis::Template))
    );
    // Break it: swap `from` and `to` of the `Consumed` edge, and the first assertion fails.
}

#[test]
fn an_undeclared_parameter_is_a_participant_without_direction() {
    // A role nobody declares is not guessed from the title (WP07 §5).
    let a = record(1, vec![object_parameter(0, "input", &feature_uri(1))], None);
    let roles = [ParameterRole {
        position: 0,
        role: ActivityRole::Undeclared,
        basis: RoleBasis::None,
    }];
    let edges = activity_edges(&a, &roles, &NoInputFiles);
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].kind, ProvEdgeKind::Participant);
    // A parameter the roles do not mention at all is just as undeclared.
    let edges = activity_edges(&a, &[], &NoInputFiles);
    assert_eq!(edges[0].kind, ProvEdgeKind::Participant);
    // Break it: make `Undeclared` a `Consumed` edge.
}

#[test]
fn parent_and_sub_activity_give_parent_edges() {
    let a = record(
        1,
        vec![object_parameter(0, "step-1", &activity_uri(5))],
        Some(9),
    );
    let edges = activity_edges(&a, &[role(0, ActivityRole::SubActivity)], &NoInputFiles);
    let me = object(&activity_uri(1));
    assert!(
        has_edge(&edges, &me, &object(&activity_uri(5)), &ProvEdgeKind::Parent),
        "a sub-activity hangs under the activity that names it"
    );
    let from_parent = edges
        .iter()
        .find(|edge| edge.from == object(&activity_uri(9)))
        .expect("the Parent field gives an edge");
    assert_eq!(from_parent.to, me);
    assert_eq!(from_parent.kind, ProvEdgeKind::Parent);
    assert_eq!(from_parent.basis, EdgeBasis::Reference);
    // Break it: forget the `Parent` field, and `from_parent` is not found.
}

#[test]
fn a_text_parameter_is_a_file_only_when_a_reader_says_so() {
    let a = record(
        1,
        vec![parameter(0, "file", ParameterValue::Text("./data/a.las".to_string()))],
        None,
    );
    assert!(
        activity_edges(&a, &[], &NoInputFiles).is_empty(),
        "no convention known: a text is not a file"
    );
    let edges = activity_edges(&a, &[], &FileTitled);
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].from, NodeKey::File("./data/a.las".to_string()));
    assert_eq!(edges[0].kind, ProvEdgeKind::Consumed);
    assert_eq!(edges[0].basis, EdgeBasis::Activity(RoleBasis::Rule("test:file")));
    // Break it: make every text a file, and the first assertion fails.
}

// -- activity_tree -----------------------------------------------------------

#[test]
fn no_parent_anywhere_is_flat() {
    // common TRG 2.3 §3.3, p. 42: `Parent` is 0..1.
    let activities = [record(1, vec![], None), record(2, vec![], None), record(3, vec![], None)];
    let tree = activity_tree(&activities, &[vec![], vec![], vec![]]);
    assert_eq!(tree.shape, TreeShape::Flat);
    assert_eq!(tree.roots.len(), 3);
    assert!(tree.children.is_empty());
    assert!(tree.refusals.is_empty() && tree.disagreements.is_empty());
    // Break it: answer `Tree` whatever the links.
}

#[test]
fn a_root_and_three_children_written_both_ways_is_a_tree() {
    // The root names its children 4, 2, 3 in that order; each child names the root as `Parent`.
    let root = record(
        1,
        vec![
            object_parameter(0, "step-1", &activity_uri(4)),
            object_parameter(1, "step-2", &activity_uri(2)),
            object_parameter(2, "step-3", &activity_uri(3)),
        ],
        None,
    );
    let activities = [
        root,
        record(2, vec![], Some(1)),
        record(3, vec![], Some(1)),
        record(4, vec![], Some(1)),
    ];
    let roles = [
        vec![
            role(0, ActivityRole::SubActivity),
            role(1, ActivityRole::SubActivity),
            role(2, ActivityRole::SubActivity),
        ],
        vec![],
        vec![],
        vec![],
    ];
    let tree = activity_tree(&activities, &roles);
    assert_eq!(tree.shape, TreeShape::Tree);
    assert_eq!(tree.roots, vec![activity_uri(1)]);
    assert!(tree.disagreements.is_empty(), "both writings agree");
    assert_eq!(
        tree.children[&activity_uri(1)],
        vec![activity_uri(4), activity_uri(2), activity_uri(3)],
        "the order of the parent's parameters, not the input order"
    );
    // Break it: order the children by input order only, and the last assertion fails.
}

#[test]
fn a_root_naming_a_child_that_does_not_name_it_disagrees() {
    // The parent's parameter names 2; 2 has no `Parent`. And 3 names the parent while the parent does not name 3.
    let activities = [
        record(1, vec![object_parameter(0, "step-1", &activity_uri(2))], None),
        record(2, vec![], None),
        record(3, vec![], Some(1)),
    ];
    let roles = [vec![role(0, ActivityRole::SubActivity)], vec![], vec![]];
    let tree = activity_tree(&activities, &roles);
    assert_eq!(
        tree.disagreements,
        vec![
            GraphIssue::TreeDisagreement {
                parent: activity_uri(1),
                child: activity_uri(2),
                stated_by: Statement::ParameterOnly,
            },
            GraphIssue::TreeDisagreement {
                parent: activity_uri(1),
                child: activity_uri(3),
                stated_by: Statement::ParentOnly,
            },
        ]
    );
    // A parent that names no sub-activity states the tree once: nothing to confront.
    let activities = [record(1, vec![], None), record(2, vec![], Some(1))];
    let tree = activity_tree(&activities, &[vec![], vec![]]);
    assert!(tree.disagreements.is_empty());
    // Break it: read one direction only, and the first assertion loses an entry.
}

#[test]
fn two_activities_naming_each_other_are_a_cycle_shown_flat() {
    let activities = [record(1, vec![], Some(2)), record(2, vec![], Some(1))];
    let tree = activity_tree(&activities, &[vec![], vec![]]);
    assert_eq!(tree.shape, TreeShape::Flat);
    assert_eq!(tree.roots, vec![activity_uri(1), activity_uri(2)], "both are shown, flat");
    assert!(tree.children.is_empty(), "a loop is never given children");
    assert_eq!(tree.refusals.len(), 1);
    assert!(matches!(&tree.refusals[0], TreeRefusal::ParentCycle { uris } if uris.len() == 2));
    // Break it: cut the loop at its first node, and `refusals` is empty.
}

#[test]
fn a_parent_the_package_lacks_makes_a_marked_root() {
    let activities = [record(1, vec![], Some(77)), record(2, vec![], Some(1))];
    let tree = activity_tree(&activities, &[vec![], vec![]]);
    assert_eq!(tree.roots, vec![activity_uri(1)], "the orphan is a root");
    assert_eq!(tree.children[&activity_uri(1)], vec![activity_uri(2)]);
    assert_eq!(tree.shape, TreeShape::Tree);
    assert_eq!(
        tree.refusals,
        vec![TreeRefusal::ParentAbsent {
            child: activity_uri(1),
            parent: activity_uri(77),
        }]
    );
    // Break it: drop the orphan silently, and `refusals` is empty.
}

// -- workflow_steps ----------------------------------------------------------

fn step_template(title: &str, min: Option<i64>, constraint: Option<&str>, position: usize) -> ParameterTemplateRecord {
    ParameterTemplateRecord {
        position,
        title: title.to_string(),
        allowed: vec![ParameterKind::SubActivity],
        direction: ParameterDirection::Neither,
        min_occurs: min,
        max_occurs: Some(1),
        constraint: constraint.map(str::to_string),
        content_type: None,
        key_constraints: Vec::new(),
        defaults: Vec::new(),
    }
}

fn workflow(parameters: Vec<ParameterTemplateRecord>) -> TemplateRecord {
    TemplateRecord {
        uri: format!("eml:///eml23.ActivityTemplate({})", u(50)),
        title: Some("workflow".to_string()),
        parameters,
    }
}

#[test]
fn a_held_step_without_child_becomes_a_held_step_node() {
    let template = workflow(vec![
        step_template("step-1", Some(1), Some("follows a"), 0),
        step_template("step-2", Some(0), Some("held: why"), 1),
    ]);
    let steps = workflow_steps(&template, &HeldReader);
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0].held(), None);
    assert_eq!(steps[1].held(), Some("why"));
    // `MinOccurs` is part of it: the same words on a step that must occur hold nothing.
    let must_occur = workflow_steps(
        &workflow(vec![step_template("step-2", Some(1), Some("held: why"), 0)]),
        &HeldReader,
    );
    assert_eq!(must_occur[0].held(), None);

    // The root names only the first step; the held one hangs under it as a node of its own.
    let root = record(1, vec![object_parameter(0, "step-1", &activity_uri(2))], None);
    let held_key = NodeKey::HeldStep {
        workflow: template.uri.clone(),
        step: "step-2".to_string(),
    };
    let edges = vec![ProvEdge {
        from: object(&activity_uri(1)),
        to: held_key.clone(),
        kind: ProvEdgeKind::Parent,
        basis: EdgeBasis::Reference,
    }];
    let graph = assemble(Parts {
        activities: &[root],
        edges,
        steps: &steps,
        index: None,
        set_aside: DEFAULT_SET_ASIDE,
        expanded: &[],
        issues: Vec::new(),
    });
    let at = graph.position(&held_key).expect("a node for the held step");
    assert_eq!(
        graph.nodes[at],
        ProvNode::HeldStep {
            workflow: template.uri.clone(),
            step: "step-2".to_string(),
            reason: "why".to_string(),
        }
    );
    // Break it: do not look at `MinOccurs`, and `must_occur[0].held()` is `Some("why")`.
}

#[test]
fn a_template_without_sub_activity_declares_no_step() {
    let mut plain = step_template("input", Some(1), None, 0);
    plain.allowed = vec![ParameterKind::DataObject];
    assert!(workflow_steps(&workflow(vec![plain]), &NoStepConstraints).is_empty());
    // Without a reader, the constraint stays as written and nothing is read of it.
    let steps = workflow_steps(
        &workflow(vec![step_template("step-1", Some(0), Some("held: why"), 0)]),
        &NoStepConstraints,
    );
    assert_eq!(steps[0].constraint.as_deref(), Some("held: why"));
    assert_eq!(steps[0].parsed, None);
    assert_eq!(steps[0].held(), None);
    // Break it: take every parameter as a step, and the first assertion fails.
}

// -- expand_collection -------------------------------------------------------

#[test]
fn a_collection_expands_to_the_members_its_set_lists() {
    // common TRG 2.3 §8.3: a set lists the members of the collection it names.
    // TODO(verify): the page of §8.3 in the PDF.
    let mut store = store();
    for n in [1, 2, 3] {
        feature(&mut store, n);
    }
    collection(&mut store, 10);
    set(&mut store, 11, &[(10, &[1, 2, 3])]);
    let index = index_of(&store);
    let members = index
        .expand_collection(&uri_in(&index, 10), 10)
        .expect("the set names the collection");
    assert_eq!(members, vec![uri_in(&index, 1), uri_in(&index, 2), uri_in(&index, 3)]);
    // A bare uuid is accepted at the entry, like everywhere in the index.
    assert_eq!(index.expand_collection(&u(10), 10).unwrap(), members);
    let refused = index.expand_collection(&uri_in(&index, 10), 2).unwrap_err();
    assert!(
        matches!(refused, ExpandRefusal::TooMany { members: 3, limit: 2, .. }),
        "{refused:?}"
    );
    assert!(matches!(
        index.expand_collection(&uri_in(&index, 1), 10),
        Err(ExpandRefusal::NotACollection { .. })
    ));
    assert!(matches!(
        index.expand_collection(&feature_uri(404), 10),
        Err(ExpandRefusal::NotInIndex { .. })
    ));
    // Break it: ignore `limit`, and the `TooMany` refusal is lost.
}

#[test]
fn a_collection_no_set_names_is_refused() {
    let mut store = store();
    collection(&mut store, 10);
    let index = index_of(&store);
    assert!(matches!(
        index.expand_collection(&uri_in(&index, 10), 10),
        Err(ExpandRefusal::NoAssociationSet { .. })
    ));
    // Break it: answer `Ok(vec![])`, and a collection with no set claims it has zero members.
}

// -- carriers ----------------------------------------------------------------

/// feature 1 <- interpretation 2 <- representation 3 <- property 4.
fn chain() -> EpcStore {
    let mut store = store();
    feature(&mut store, 1);
    interpretation(&mut store, 2, 1);
    representation(&mut store, 3, 2);
    property(&mut store, 4, 3);
    store
}

#[test]
fn carriers_follow_support_then_firp_up_to_the_feature_and_stop_at_depth() {
    let store = chain();
    let index = index_of(&store);
    let seed = uri_in(&index, 4);
    let rules = CarrierRules::new();
    let deep = carriers(&index, std::slice::from_ref(&seed), &rules, 3);
    assert_eq!(deep.len(), 3, "property to representation to interpretation to feature");
    assert!(has_edge(
        &deep,
        &object(&uri_in(&index, 4)),
        &object(&uri_in(&index, 3)),
        &ProvEdgeKind::Carrier {
            path: "SupportingRepresentation".to_string()
        }
    ));
    assert!(deep.iter().all(|edge| edge.basis == EdgeBasis::Reference));
    assert!(has_edge(
        &deep,
        &object(&uri_in(&index, 2)),
        &object(&uri_in(&index, 1)),
        &ProvEdgeKind::Carrier {
            path: "InterpretedFeature".to_string()
        }
    ));
    let shallow = carriers(&index, std::slice::from_ref(&seed), &rules, 1);
    assert_eq!(shallow.len(), 1, "one level: only the support");
    assert!(carriers(&index, std::slice::from_ref(&seed), &rules, 0).is_empty());
    // Break it: ignore `depth`, and `shallow` has three edges.
}

#[test]
fn the_firp_rule_finds_what_the_index_tables_find() {
    // Triangulation: the index lays its own parent rows while it builds (`parent_rows`), from the
    // same three fields but by another road. With no property in the package, the only rule that
    // applies to the three objects is `firp`.
    let mut store = store();
    feature(&mut store, 1);
    interpretation(&mut store, 2, 1);
    representation(&mut store, 3, 2);
    let index = index_of(&store);
    let rules = CarrierRules::new();
    for n in [1, 2, 3] {
        let uri = uri_in(&index, n);
        let mut by_carriers: Vec<String> = carriers(&index, std::slice::from_ref(&uri), &rules, 1)
            .into_iter()
            .filter_map(|edge| match edge.to {
                NodeKey::Object(to) => Some(to),
                _ => None,
            })
            .collect();
        let mut by_index: Vec<String> = index
            .parent_rows(&uri)
            .into_iter()
            .map(|(_, node)| node.uri.to_string())
            .collect();
        by_carriers.sort();
        by_index.sort();
        assert_eq!(by_carriers, by_index, "object {n}");
    }
    // Break it: make `firp` follow one more field (`Citation`), and the two lists differ.
}

#[test]
fn a_callers_carrier_rule_is_tried_first() {
    struct Test;
    impl CarrierRule for Test {
        fn id(&self) -> &'static str {
            "test"
        }
        fn follows(&self, _holder: &str, field: &str, _target: &str) -> bool {
            field == "PropertyKind"
        }
    }
    let mut rules = CarrierRules::new();
    rules.add(Box::new(Test));
    assert_eq!(rules.ids()[0], "test");
    assert_eq!(rules.ids().len(), 5);
    assert_eq!(CarrierRules::empty().ids().len(), 0);
    let store = chain();
    let index = index_of(&store);
    let edges = carriers(&index, &[uri_in(&index, 4)], &rules, 1);
    assert!(
        edges
            .iter()
            .any(|edge| matches!(&edge.kind, ProvEdgeKind::Carrier { path } if path == "PropertyKind")),
        "the caller's rule follows a field no built-in follows"
    );
    // Break it: push the caller's rule last, and `ids()[0]` is `firp`.
}

// -- assemble ----------------------------------------------------------------

fn produced(activity: u32, address: &str) -> ProvEdge {
    ProvEdge {
        from: object(&activity_uri(activity)),
        to: NodeKey::Object(address.to_string()),
        kind: ProvEdgeKind::Produced,
        basis: EdgeBasis::Activity(RoleBasis::Template),
    }
}

#[test]
fn an_object_two_activities_wrote_is_one_node_with_two_produced_edges() {
    let mut store = store();
    feature(&mut store, 1);
    let index = index_of(&store);
    // One activity writes the typed uri, the other a bare uuid: the index names them one object.
    let graph = assemble(Parts {
        activities: &[record(1, vec![], None), record(2, vec![], None)],
        edges: vec![produced(1, &feature_uri(1)), produced(2, &u(1))],
        steps: &[],
        index: Some(&index),
        set_aside: DEFAULT_SET_ASIDE,
        expanded: &[],
        issues: Vec::new(),
    });
    assert_eq!(graph.nodes.len(), 3, "two activities and one object: {:?}", graph.nodes);
    let wrote = object(&uri_in(&index, 1));
    let produced_edges: Vec<&ProvEdge> = graph
        .edges
        .iter()
        .filter(|edge| edge.kind == ProvEdgeKind::Produced)
        .collect();
    assert_eq!(produced_edges.len(), 2);
    assert!(produced_edges.iter().all(|edge| edge.to == wrote));
    let at = graph.position(&wrote).unwrap();
    assert!(matches!(&graph.nodes[at], ProvNode::Object { present: true, .. }));
    assert!(graph.issues.is_empty(), "the package holds it: {:?}", graph.issues);
    // Break it: key the nodes by the text of the address, and there are four nodes.
}

#[test]
fn a_crs_set_aside_is_no_node_but_is_listed() {
    let crs = format!("eml:///eml23.LocalEngineeringCompoundCrs({})", u(7));
    let graph = assemble(Parts {
        activities: &[record(1, vec![], None)],
        edges: vec![produced(1, &crs), produced(1, &feature_uri(2))],
        steps: &[],
        index: None,
        set_aside: DEFAULT_SET_ASIDE,
        expanded: &[],
        issues: Vec::new(),
    });
    assert!(!node_keys(&graph).contains(&object(&crs)));
    assert_eq!(graph.edges.len(), 1, "the edge to the CRS is taken out too");
    assert_eq!(graph.set_aside.len(), 1);
    assert_eq!(graph.set_aside[0].uri, crs);
    assert_eq!(graph.set_aside[0].qualified_type, "eml23.LocalEngineeringCompoundCrs");
    assert_eq!(graph.set_aside[0].named_by, activity_uri(1));
    // Break it: keep it as a node, and the first assertion fails.
}

#[test]
fn an_edge_both_sources_state_is_both() {
    let id = VerbatimId("test-source");
    let consumed = |basis: EdgeBasis| ProvEdge {
        from: object(&feature_uri(1)),
        to: object(&activity_uri(1)),
        kind: ProvEdgeKind::Consumed,
        basis,
    };
    let graph = assemble(Parts {
        activities: &[record(1, vec![], None)],
        edges: vec![
            consumed(EdgeBasis::Activity(RoleBasis::Template)),
            consumed(EdgeBasis::Verbatim(id)),
        ],
        steps: &[],
        index: None,
        set_aside: DEFAULT_SET_ASIDE,
        expanded: &[],
        issues: Vec::new(),
    });
    assert_eq!(graph.edges.len(), 1);
    assert_eq!(graph.edges[0].basis, EdgeBasis::Both(id));
    // Break it: keep both edges, and `len()` is 2.
}

#[test]
fn an_output_the_package_lacks_is_signalled() {
    let mut store = store();
    feature(&mut store, 1);
    let index = index_of(&store);
    let asked = |address: &str| {
        assemble(Parts {
            activities: &[record(1, vec![], None)],
            edges: vec![produced(1, address)],
            steps: &[],
            index: Some(&index),
            set_aside: DEFAULT_SET_ASIDE,
            expanded: &[],
            issues: Vec::new(),
        })
    };
    let lacking = asked(&feature_uri(404));
    assert_eq!(
        lacking.issues,
        vec![GraphIssue::OutputAbsent {
            activity: activity_uri(1),
            object: feature_uri(404),
        }]
    );
    assert!(asked(&feature_uri(1)).issues.is_empty(), "an object the package holds is not signalled");
    // Break it: say nothing, and the first assertion fails.
}

// -- focus -------------------------------------------------------------------

/// `f -> A -> o1 -> B -> o2`, written the way the activities state it.
fn chain_graph() -> ProvenanceGraph {
    let flow = |from: NodeKey, to: NodeKey, kind: ProvEdgeKind| ProvEdge {
        from,
        to,
        kind,
        basis: EdgeBasis::Activity(RoleBasis::Template),
    };
    assemble(Parts {
        activities: &[record(1, vec![], None), record(2, vec![], None)],
        edges: vec![
            flow(NodeKey::File("f.las".to_string()), object(&activity_uri(1)), ProvEdgeKind::Consumed),
            flow(object(&activity_uri(1)), object(&feature_uri(1)), ProvEdgeKind::Produced),
            flow(object(&feature_uri(1)), object(&activity_uri(2)), ProvEdgeKind::Consumed),
            flow(object(&activity_uri(2)), object(&feature_uri(2)), ProvEdgeKind::Produced),
        ],
        steps: &[],
        index: None,
        set_aside: DEFAULT_SET_ASIDE,
        expanded: &[],
        issues: Vec::new(),
    })
}

#[test]
fn focus_upstream_one_downstream_zero_keeps_only_the_producers() {
    let graph = chain_graph();
    assert_eq!(graph.nodes.len(), 5);
    let on = object(&feature_uri(2));
    let one = focus(&graph, &on, 1, 0).unwrap();
    assert_eq!(
        node_keys(&one),
        vec![object(&activity_uri(2)), object(&feature_uri(2))],
        "one edge upstream of o2 is B"
    );
    let two = focus(&graph, &on, 2, 0).unwrap();
    assert_eq!(two.nodes.len(), 3);
    assert!(node_keys(&two).contains(&object(&feature_uri(1))));
    assert_eq!(two.edges.len(), 2);
    let down = focus(&graph, &object(&feature_uri(1)), 0, 1).unwrap();
    assert_eq!(
        node_keys(&down),
        vec![object(&activity_uri(2)), object(&feature_uri(1))]
    );
    // Break it: count nodes instead of edges, and `one` also keeps `o1`.
}

#[test]
fn focus_refuses_what_it_cannot_centre_on() {
    let graph = chain_graph();
    let missing = object(&feature_uri(404));
    assert_eq!(
        focus(&graph, &missing, 1, 1).unwrap_err(),
        FocusRefusal::NotInGraph(missing)
    );
    assert_eq!(
        focus(&graph, &object(&feature_uri(1)), 65, 0).unwrap_err(),
        FocusRefusal::DepthTooLarge { asked: 65, limit: 64 }
    );
    // Break it: skip the bound, and the second assertion fails.
}

// -- timeline ----------------------------------------------------------------

fn at(n: u32, seconds: u32) -> ActivityRecord {
    let mut activity = record(n, vec![], None);
    activity.creation = Some(format!("2026-09-24T00:00:{seconds:02}Z"));
    activity
}

#[test]
fn all_instants_equal_gives_no_measured_order_and_the_declared_order() {
    // A recipe writes one `Creation` for the whole package. The root names C, A, B in that order.
    let mut root = record(
        1,
        vec![
            object_parameter(0, "step-1", &activity_uri(4)),
            object_parameter(1, "step-2", &activity_uri(2)),
            object_parameter(2, "step-3", &activity_uri(3)),
        ],
        None,
    );
    root.creation = Some("2026-09-24T00:00:00Z".to_string());
    let mut activities = vec![root];
    for n in [2, 3, 4] {
        let mut child = record(n, vec![], Some(1));
        child.creation = Some("2026-09-24T00:00:00Z".to_string());
        activities.push(child);
    }
    let roles = vec![
        vec![
            role(0, ActivityRole::SubActivity),
            role(1, ActivityRole::SubActivity),
            role(2, ActivityRole::SubActivity),
        ],
        vec![],
        vec![],
        vec![],
    ];
    let tree = activity_tree(&activities, &roles);
    let line = timeline(&activities, &[], &tree, &ProvenanceGraph::default(), OrderBasis::Auto);
    assert_eq!(line.basis_used, OrderBasis::Declared);
    assert_eq!(line.note, Some(TimelineRefusal::NoMeasuredOrder(OrderBasis::Declared)));
    let order: Vec<&str> = line.rows.iter().map(|row| row.activity.as_str()).collect();
    assert_eq!(
        order,
        vec![
            activity_uri(1).as_str(),
            activity_uri(4).as_str(),
            activity_uri(2).as_str(),
            activity_uri(3).as_str()
        ],
        "the root, then its children in the order it names them"
    );
    assert!(line.rows.iter().all(|row| row.offset_ms.is_none()));
    assert_eq!(line.rows[1].rank, 1);
    // Break it: sort by instant without checking they differ, and the order is the input order.
}

#[test]
fn different_instants_give_the_measured_order_ties_in_declared_order() {
    let activities = [at(1, 0), at(2, 1), at(3, 0)];
    let tree = activity_tree(&activities, &[vec![], vec![], vec![]]);
    let line = timeline(&activities, &[], &tree, &ProvenanceGraph::default(), OrderBasis::Auto);
    assert_eq!(line.basis_used, OrderBasis::Measured);
    assert_eq!(line.note, None);
    let order: Vec<&str> = line.rows.iter().map(|row| row.activity.as_str()).collect();
    assert_eq!(
        order,
        vec![
            activity_uri(1).as_str(),
            activity_uri(3).as_str(),
            activity_uri(2).as_str()
        ],
        "the tie keeps the declared order"
    );
    let offsets: Vec<Option<i64>> = line.rows.iter().map(|row| row.offset_ms).collect();
    assert_eq!(offsets, vec![Some(0), Some(0), Some(1000)]);
    // Asked for a measured order and given one that is impossible: the fallback, with the note.
    let same = [at(1, 5), at(2, 5)];
    let tree = activity_tree(&same, &[vec![], vec![]]);
    let line = timeline(&same, &[], &tree, &ProvenanceGraph::default(), OrderBasis::Measured);
    assert_eq!(line.note, Some(TimelineRefusal::NoMeasuredOrder(OrderBasis::Dependency)));
    // Break it: use an unstable sort that ignores the declared rank, and the tie order is arbitrary.
}

#[test]
fn a_flat_package_of_equal_instants_takes_the_dependency_order() {
    // Input order B, A; A wrote `o`, B read it: A must come first.
    let activities = [at(2, 0), at(1, 0)];
    let tree = activity_tree(&activities, &[vec![], vec![]]);
    let o = object(&feature_uri(1));
    let graph = ProvenanceGraph {
        edges: vec![
            ProvEdge {
                from: object(&activity_uri(1)),
                to: o.clone(),
                kind: ProvEdgeKind::Produced,
                basis: EdgeBasis::Activity(RoleBasis::Template),
            },
            ProvEdge {
                from: o,
                to: object(&activity_uri(2)),
                kind: ProvEdgeKind::Consumed,
                basis: EdgeBasis::Activity(RoleBasis::Template),
            },
        ],
        ..Default::default()
    };
    let line = timeline(&activities, &[], &tree, &graph, OrderBasis::Auto);
    assert_eq!(line.basis_used, OrderBasis::Dependency);
    assert_eq!(line.note, Some(TimelineRefusal::NoMeasuredOrder(OrderBasis::Dependency)));
    let order: Vec<&str> = line.rows.iter().map(|row| row.activity.as_str()).collect();
    assert_eq!(order, vec![activity_uri(1).as_str(), activity_uri(2).as_str()]);
    // Asking for the dependency order is not a refusal of anything.
    let asked = timeline(&activities, &[], &tree, &graph, OrderBasis::Dependency);
    assert_eq!(asked.note, None);
    // Break it: take the input order, and the order is B, A.
}

// -- of_package --------------------------------------------------------------

fn template_object(store: &mut EpcStore, n: u32, parameters: &[(&str, bool, bool)]) {
    put(
        store,
        eml23::ActivityTemplate {
            uuid: u(n),
            schema_version: "2.3".into(),
            citation: citation(&format!("template {n}")),
            parameter: parameters
                .iter()
                .map(|(title, is_input, is_output)| eml23::ParameterTemplate {
                    title: title.to_string(),
                    allowed_kind: vec![eml23::ActivityParameterKind::DataObject],
                    is_input: *is_input,
                    is_output: *is_output,
                    min_occurs: 1,
                    max_occurs: 1,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        },
    );
}

fn activity_object(store: &mut EpcStore, n: u32, template: u32, parameters: &[(&str, &str, u32)]) {
    put(
        store,
        eml23::Activity {
            uuid: u(n),
            schema_version: "2.3".into(),
            citation: citation(&format!("activity {n}")),
            activity_descriptor: dor(&u(template), "eml23.ActivityTemplate"),
            parameter: parameters
                .iter()
                .map(|(title, qualified_type, target)| {
                    eml23::AbstractActivityParameter::DataObjectParameter(eml23::DataObjectParameter {
                        title: title.to_string(),
                        data_object: dor(&u(*target), qualified_type),
                        ..Default::default()
                    })
                })
                .collect(),
            ..Default::default()
        },
    );
}

#[test]
fn of_package_reads_a_small_recipe_package_into_one_graph() {
    // A writes `o` (and a property kind, set aside by type); B reads `o`. Both templates declare
    // their parameters, so the roles come from the specification, not from a rule.
    let mut store = store();
    feature(&mut store, 1);
    put(
        &mut store,
        eml23::PropertyKind {
            uuid: u(2),
            schema_version: "2.3".into(),
            citation: citation("kind"),
            ..Default::default()
        },
    );
    template_object(&mut store, 20, &[("output", false, true), ("kind", false, true)]);
    template_object(&mut store, 21, &[("input", true, false)]);
    activity_object(
        &mut store,
        30,
        20,
        &[("output", FEATURE, 1), ("kind", "eml23.PropertyKind", 2)],
    );
    activity_object(&mut store, 31, 21, &[("input", FEATURE, 1)]);
    let index = index_of(&store);

    let roles = RoleRules::new();
    let carriers = CarrierRules::new();
    let verbatim = VerbatimSources::empty();
    let options = GraphOptions {
        roles: &roles,
        carriers: &carriers,
        carrier_depth: 4,
        set_aside: DEFAULT_SET_ASIDE,
        expanded: &[],
        expand_limit: 1000,
        steps: &NoStepConstraints,
        files: &NoInputFiles,
        verbatim: &verbatim,
    };
    let found = of_package(&index, &store, &options);

    assert_eq!(found.activities.len(), 2);
    assert_eq!(found.templates.len(), 2);
    let graph = &found.graph;
    let activities: Vec<&ProvNode> = graph
        .nodes
        .iter()
        .filter(|node| matches!(node, ProvNode::Activity { .. }))
        .collect();
    assert_eq!(activities.len(), 2);
    let o = object(&uri_in(&index, 1));
    assert!(graph.position(&o).is_some(), "one node for `o`");
    let a = object(&uri_in(&index, 30));
    let b = object(&uri_in(&index, 31));
    assert!(has_edge(&graph.edges, &a, &o, &ProvEdgeKind::Produced));
    assert!(has_edge(&graph.edges, &o, &b, &ProvEdgeKind::Consumed));
    assert_eq!(graph.set_aside.len(), 1, "the property kind is listed, not drawn");
    assert_eq!(graph.set_aside[0].named_by, uri_in(&index, 30));
    assert!(
        !graph
            .issues
            .iter()
            .any(|issue| matches!(issue, GraphIssue::Conformance { .. } | GraphIssue::Unreadable { .. })),
        "{:?}",
        graph.issues
    );
    assert_eq!(found.tree.shape, TreeShape::Flat);
    // Break it: forget the roles (step 2) and no edge has a direction: both `has_edge` fail.
}

#[test]
fn a_step_nobody_declared_is_not_invented() {
    // A package with no workflow template declares no step and holds none.
    let steps: Vec<DeclaredStep> = Vec::new();
    let graph = assemble(Parts {
        activities: &[record(1, vec![], None)],
        edges: Vec::new(),
        steps: &steps,
        index: None,
        set_aside: DEFAULT_SET_ASIDE,
        expanded: &[],
        issues: Vec::new(),
    });
    assert_eq!(graph.nodes.len(), 1);
    assert!(graph.edges.is_empty() && graph.issues.is_empty());
    // Break it: add a node per activity template, and `nodes.len()` is not 1.
}
