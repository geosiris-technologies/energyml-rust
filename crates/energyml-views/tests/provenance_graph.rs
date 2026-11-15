//! `provenance-graph`: the boxes, the columns, the arrows, the notes, the focus, the refusals.
//!
//! Every package is built in memory, from the shapes of common TRG 2.3 ch. 3 (an `Activity`, its
//! `ActivityTemplate`, its parameters). What the graph holds is the domain's (`of_package`, `focus`): the tests
//! confront what is drawn with what the domain says, and check the layout in numbers on the display list.

use std::io::Cursor;
use std::sync::Arc;

use energyml_core::registry::DynObject;
use energyml_domain::activity::RoleRules;
use energyml_domain::color::{Paint as FilePaint, Rgba};
use energyml_domain::package_index::PackageIndex;
use energyml_domain::provenance_graph::{
    CarrierRules, DEFAULT_SET_ASIDE, GraphOptions, NoInputFiles, NoStepConstraints, NodeKey, ProvNode,
    ProvenanceGraph, focus, of_package,
};
use energyml_domain::provenance_verbatim::VerbatimSources;
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_2 as resqml22;
use energyml_sheet::draw::{Colour, DisplayList, Element, Item, Paint, Rect, Segment};
use energyml_storage::EpcStore;
use energyml_views::provenance_graph::ProvenanceView;
use energyml_views::{
    ColourDoor, Drawn, ViewAsk, ViewKinds, ViewRefusal, draw_list, draw_svg, redraw, style,
};
use serde_json::json;

use crate::fixtures::{NO_MAPS, Opened};

const ID: &str = "provenance-graph";
const FEATURE: &str = "resqml22.WellboreFeature";

fn kinds() -> ViewKinds {
    let mut kinds = ViewKinds::empty();
    kinds.register(Box::new(ProvenanceView));
    kinds
}

fn u(n: u32) -> String {
    format!("00000000-0000-4000-8000-{n:012x}")
}

fn citation(title: &str) -> eml23::Citation {
    eml23::Citation {
        title: title.to_string(),
        ..Default::default()
    }
}

fn dor(uuid: &str, qualified_type: &str) -> eml23::DataObjectReference {
    eml23::DataObjectReference {
        uuid: uuid.to_string(),
        qualified_type: qualified_type.to_string(),
        title: format!("t-{uuid}"),
        ..Default::default()
    }
}

fn empty_store() -> EpcStore {
    EpcStore::from_reader(
        Box::new(Cursor::new(
            energyml_storage::extract::empty_package(None).expect("an empty package"),
        )),
        "provenance.epc",
        Arc::new(energyml_schemas::all()),
    )
    .expect("it opens")
}

fn put(store: &mut EpcStore, object: impl DynObject + 'static) {
    store.put(Box::new(object)).expect("it goes in");
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

fn template(store: &mut EpcStore, n: u32, parameters: &[(&str, bool, bool)]) {
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

fn activity(store: &mut EpcStore, n: u32, template: u32, parameters: &[(&str, &str, u32)]) {
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

/// A package where activity 30 writes a feature (1), a property kind (2, set aside by type) and a property (5)
/// that hangs on a trajectory (4), which represents an interpretation (3) of the feature; activity 31 reads the
/// feature. The roles come from the templates (20 and 21), not from a rule.
fn recipe_store() -> EpcStore {
    let mut store = empty_store();
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
    put(
        &mut store,
        resqml22::WellboreInterpretation {
            uuid: u(3),
            schema_version: "2.2".into(),
            citation: citation("interpretation 3"),
            interpreted_feature: dor(&u(1), FEATURE),
            ..Default::default()
        },
    );
    put(
        &mut store,
        resqml22::WellboreTrajectoryRepresentation {
            uuid: u(4),
            schema_version: "2.2".into(),
            citation: citation("trajectory 4"),
            represented_object: Some(dor(&u(3), "resqml22.WellboreInterpretation")),
            ..Default::default()
        },
    );
    put(
        &mut store,
        resqml22::ContinuousProperty {
            uuid: u(5),
            schema_version: "2.2".into(),
            citation: citation("property 5"),
            property_kind: dor(&u(900), "eml23.PropertyKind"),
            supporting_representation: dor(&u(4), "resqml22.WellboreTrajectoryRepresentation"),
            ..Default::default()
        },
    );
    template(
        &mut store,
        20,
        &[
            ("output", false, true),
            ("kind", false, true),
            ("curve", false, true),
        ],
    );
    template(&mut store, 21, &[("input", true, false)]);
    activity(
        &mut store,
        30,
        20,
        &[
            ("output", FEATURE, 1),
            ("kind", "eml23.PropertyKind", 2),
            ("curve", "resqml22.ContinuousProperty", 5),
        ],
    );
    activity(&mut store, 31, 21, &[("input", FEATURE, 1)]);
    store
}

fn recipe() -> Opened {
    Opened::of(recipe_store())
}

/// The recipe, and an activity that names a parameter its template does not have (a conformance issue) and another
/// that says it wrote an object the package does not hold.
fn recipe_with_trouble() -> Opened {
    let mut store = recipe_store();
    activity(
        &mut store,
        32,
        21,
        &[("input", FEATURE, 1), ("stray", FEATURE, 1)],
    );
    activity(&mut store, 33, 20, &[("output", FEATURE, 98)]);
    Opened::of(store)
}

fn uri_in(index: &PackageIndex, n: u32) -> String {
    index.uri_of(&u(n)).expect("the package holds it").to_string()
}

/// The graph as the domain makes it, with the options the view uses.
fn domain_graph(opened: &Opened) -> ProvenanceGraph {
    let roles = RoleRules::new();
    let carriers = CarrierRules::new();
    let verbatim = VerbatimSources::new();
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
    of_package(&opened.index, &opened.store, &options).graph
}

fn drawn(opened: &Opened, ask: ViewAsk) -> Drawn {
    draw_list(&kinds(), &ask, &opened.context(&NO_MAPS))
        .expect("drawn")
        .0
}

fn refused(opened: &Opened, ask: ViewAsk) -> ViewRefusal {
    draw_list(&kinds(), &ask, &opened.context(&NO_MAPS)).expect_err("refused")
}

fn elements<'a>(list: &'a DisplayList, layer: &str) -> &'a [Element] {
    &list.layer(layer).expect("the layer").elements
}

fn element<'a>(list: &'a DisplayList, layer: &str, key: &str) -> &'a Element {
    elements(list, layer)
        .iter()
        .find(|element| element.key.as_deref() == Some(key))
        .unwrap_or_else(|| panic!("no element `{key}` in the layer `{layer}`"))
}

fn rect_of(element: &Element) -> Rect {
    element
        .items
        .iter()
        .find_map(|item| match item {
            Item::Rect { rect, .. } => Some(*rect),
            _ => None,
        })
        .expect("a rectangle")
}

fn outline_of(element: &Element) -> (Colour, Vec<f64>) {
    element
        .items
        .iter()
        .find_map(|item| match item {
            Item::Rect {
                stroke: Some(stroke), ..
            } => Some((stroke.colour, stroke.dash.clone())),
            _ => None,
        })
        .expect("an outlined rectangle")
}

fn line_dash(element: &Element) -> Vec<f64> {
    element
        .items
        .iter()
        .find_map(|item| match item {
            Item::Path {
                stroke: Some(stroke), ..
            } => Some(stroke.dash.clone()),
            _ => None,
        })
        .expect("a stroked path")
}

fn notes_with(drawn: &Drawn, key: &str) -> Vec<String> {
    drawn
        .notes
        .iter()
        .filter(|note| note.key == key)
        .map(|note| note.says.clone())
        .collect()
}

#[test]
fn a_package_with_activities_is_offered_once_and_one_without_is_not() {
    let opened = recipe();
    let offers = kinds().offers(&opened.index);
    assert_eq!(offers.len(), 1);
    assert_eq!(offers[0].view, ID);
    assert!(offers[0].objects.is_empty() && offers[0].missing.is_empty());
    assert!(offers[0].title.contains("2 activities"), "{}", offers[0].title);

    let mut store = empty_store();
    feature(&mut store, 1);
    assert!(kinds().offers(&Opened::of(store).index).is_empty());
    // Break it: offer the view whatever the package holds.
}

#[test]
fn a_box_is_drawn_for_every_node_of_the_graph_and_an_arrow_for_every_edge() {
    let opened = recipe();
    let graph = domain_graph(&opened);
    let list = drawn(&opened, ViewAsk::new(ID)).list;
    assert_eq!(elements(&list, "nodes").len(), graph.nodes.len());
    assert_eq!(elements(&list, "edges").len(), graph.edges.len());
    // The key of an object is its full uri, and every object node of the graph is there under it.
    for node in &graph.nodes {
        if let ProvNode::Activity { uri, .. } | ProvNode::Object { uri, .. } = node {
            element(&list, "nodes", uri);
        }
    }
    for n in [1, 3, 4, 5, 30, 31] {
        element(&list, "nodes", &uri_in(&opened.index, n));
    }
    // Break it: skip the activities when drawing, or a node the graph holds only once.
}

#[test]
fn the_columns_run_from_the_activities_to_the_objects_then_to_what_carries_them() {
    let opened = recipe();
    let list = drawn(&opened, ViewAsk::new(ID)).list;
    let x = |n: u32| rect_of(element(&list, "nodes", &uri_in(&opened.index, n))).x;
    // The activity, the property it wrote, then the chain of carriers: trajectory, interpretation, feature.
    assert!(x(30) < x(5), "an activity before what it wrote");
    assert!(
        x(5) < x(4) && x(4) < x(3) && x(3) < x(1),
        "each carrier one column after what it carries"
    );
    assert!((x(30) - x(31)).abs() < 1.0e-6, "the activities share a column");

    // Top down, the columns are rows.
    let down = drawn(&opened, ViewAsk::new(ID).set("flow", json!("top_down"))).list;
    let y = |n: u32| rect_of(element(&down, "nodes", &uri_in(&opened.index, n))).y;
    assert!(y(30) < y(5) && y(5) < y(4) && y(4) < y(3) && y(3) < y(1));
    // Break it: put every object in one column, or ignore the carrier edges when choosing the columns.
}

#[test]
fn an_edge_against_the_columns_is_dashed_and_one_with_them_is_not() {
    let opened = recipe();
    let list = drawn(&opened, ViewAsk::new(ID)).list;
    let key = |from: u32, to: u32, kind: &str| {
        format!(
            "{}>{}#{kind}",
            uri_in(&opened.index, from),
            uri_in(&opened.index, to)
        )
    };
    // Activity 31 reads the feature, which sits in a later column than the activities: drawn against the flow.
    let read = element(&list, "edges", &key(1, 31, "consumed"));
    assert_eq!(line_dash(read), vec![5.0, 4.0]);
    // Activity 30 wrote the property, one column on: with the flow, solid, in the ink of what is written.
    let wrote = element(&list, "edges", &key(30, 5, "produced"));
    assert!(line_dash(wrote).is_empty());
    let ink = wrote.items.iter().find_map(|item| match item {
        Item::Path { stroke: Some(s), .. } => Some(s.colour),
        _ => None,
    });
    assert_eq!(ink, Some(style::SUBJECT));
    // The legend says what the dashes are.
    let said: Vec<String> = elements(&list, "legend")
        .iter()
        .flat_map(|element| element.items.iter())
        .filter_map(|item| match item {
            Item::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert!(
        said.iter().any(|text| text.contains("against the flow")),
        "{said:?}"
    );
    // Break it: dash the edges by their kind alone, and the read edge is solid.
}

#[test]
fn an_arrowhead_stands_on_the_box_it_points_at() {
    let opened = recipe();
    let list = drawn(&opened, ViewAsk::new(ID)).list;
    let from = uri_in(&opened.index, 30);
    let to = uri_in(&opened.index, 5);
    let edge = element(&list, "edges", &format!("{from}>{to}#produced"));
    let target = rect_of(element(&list, "nodes", &to));
    let tip = edge
        .items
        .iter()
        .find_map(|item| match item {
            Item::Polygon { points, .. } => Some(points[0]),
            _ => None,
        })
        .expect("an arrowhead");
    // With the flow, the edge arrives at the middle of the left side of its target.
    assert!((tip[0] - target.x).abs() < 1.0e-9, "{tip:?} {target:?}");
    assert!(
        (tip[1] - (target.y + target.h / 2.0)).abs() < 1.0e-9,
        "{tip:?} {target:?}"
    );
    // The curve starts at the middle of the right side of its source.
    let source = rect_of(element(&list, "nodes", &from));
    let start = edge
        .items
        .iter()
        .find_map(|item| match item {
            Item::Path { path, .. } => match path.segments.first() {
                Some(Segment::MoveTo(p)) => Some(*p),
                _ => None,
            },
            _ => None,
        })
        .expect("a path");
    assert!((start[0] - (source.x + source.w)).abs() < 1.0e-9);
    assert!((start[1] - (source.y + source.h / 2.0)).abs() < 1.0e-9);
    // Break it: end the curve at the box's centre, or point the head at the source.
}

#[test]
fn the_objects_set_aside_by_type_are_counted_in_a_note_and_not_drawn() {
    let opened = recipe();
    let graph = domain_graph(&opened);
    assert_eq!(graph.set_aside.len(), 1, "the property kind, by the domain");
    let found = drawn(&opened, ViewAsk::new(ID));
    let notes = notes_with(&found, "set-aside");
    assert_eq!(notes.len(), 1);
    assert!(
        notes[0].contains("1 objects") && notes[0].contains("PropertyKind"),
        "{}",
        notes[0]
    );
    assert!(!found.list.keys().contains(&uri_in(&opened.index, 2).as_str()));

    // Set aside nothing: the kind is no longer a note, and the graph is the domain's own, bigger.
    let all = drawn(&opened, ViewAsk::new(ID).set("set_aside", json!([])));
    assert!(notes_with(&all, "set-aside").is_empty());
    assert!(all.list.keys().contains(&uri_in(&opened.index, 2).as_str()));
    // Break it: hand the default list to the domain whatever the setting says.
}

#[test]
fn every_issue_the_graph_signals_is_a_note() {
    let opened = recipe_with_trouble();
    let graph = domain_graph(&opened);
    assert!(!graph.issues.is_empty(), "the package is troubled");
    let found = drawn(&opened, ViewAsk::new(ID));
    let issues = found
        .notes
        .iter()
        .filter(|note| {
            !matches!(
                note.key,
                "set-aside" | "colour-not-read" | "edge-without-node" | "more-notes"
            )
        })
        .count();
    assert_eq!(issues, graph.issues.len(), "{:?}", found.notes);
    assert!(!notes_with(&found, "conformance").is_empty());
    let absent = notes_with(&found, "output-absent");
    assert_eq!(absent.len(), 1, "{absent:?}");
    assert!(absent[0].contains(&uri_in(&opened.index, 33)), "{}", absent[0]);
    // Break it: log the issues the domain finds and draw none of them.
}

#[test]
fn an_object_the_package_does_not_hold_is_a_dashed_box() {
    let opened = recipe_with_trouble();
    let graph = domain_graph(&opened);
    let ghost = graph
        .nodes
        .iter()
        .find_map(|node| match node {
            ProvNode::Object {
                uri, present: false, ..
            } => Some(uri.clone()),
            _ => None,
        })
        .expect("a node for the object activity 33 says it wrote");
    let list = drawn(&opened, ViewAsk::new(ID)).list;
    let (_, dash) = outline_of(element(&list, "nodes", &ghost));
    assert!(!dash.is_empty());
    // An object the package holds is solid.
    let (_, solid) = outline_of(element(&list, "nodes", &uri_in(&opened.index, 1)));
    assert!(solid.is_empty());
    // Break it: draw an absent object like a held one.
}

#[test]
fn a_focus_keeps_what_the_domain_keeps_and_marks_its_subject() {
    let opened = recipe();
    let graph = domain_graph(&opened);
    let reader = uri_in(&opened.index, 31);
    let key = NodeKey::Object(reader.clone());
    let around = focus(&graph, &key, 0, 0).expect("a node of the graph");
    assert!(around.nodes.len() < graph.nodes.len());

    let ask = ViewAsk::new(ID)
        .set("focus", json!(reader))
        .set("up", json!(0))
        .set("down", json!(0));
    let list = drawn(&opened, ask).list;
    assert_eq!(elements(&list, "nodes").len(), around.nodes.len());
    assert_eq!(elements(&list, "edges").len(), around.edges.len());
    // The subject has the ink of the subject; the others have the plain one.
    let (ink, _) = outline_of(element(&list, "nodes", &reader));
    assert_eq!(ink, style::SUBJECT);
    // One step back reaches the feature the activity read, and the carriers the domain follows from it.
    let back = domain_graph(&opened);
    let step = focus(&back, &key, 1, 0).expect("a node of the graph");
    let one = drawn(
        &opened,
        ViewAsk::new(ID)
            .set("focus", json!(reader))
            .set("up", json!(1))
            .set("down", json!(0)),
    )
    .list;
    assert_eq!(elements(&one, "nodes").len(), step.nodes.len());
    assert!(step.nodes.len() > around.nodes.len());
    // Break it: draw the whole graph and only mark the focus.
}

#[test]
fn a_focus_the_graph_does_not_hold_or_the_package_does_not_is_refused() {
    let mut store = recipe_store();
    feature(&mut store, 9);
    let opened = Opened::of(store);
    // A feature nothing refers to and no activity names: in the package, not in the graph.
    let alone = uri_in(&opened.index, 9);
    let why = refused(&opened, ViewAsk::new(ID).set("focus", json!(alone)));
    assert!(matches!(&why, ViewRefusal::Computing { .. }), "{why}");
    assert!(why.to_string().contains("not a node of the graph"), "{why}");

    let missing = format!("eml:///{FEATURE}({})", u(777));
    let why = refused(&opened, ViewAsk::new(ID).set("focus", json!(missing)));
    assert!(matches!(&why, ViewRefusal::NotInPackage { .. }), "{why}");
    // Break it: focus on the whole graph when the node is unknown.
}

#[test]
fn a_package_that_states_no_provenance_is_refused_not_drawn_empty() {
    let mut store = empty_store();
    feature(&mut store, 1);
    let opened = Opened::of(store);
    let why = refused(&opened, ViewAsk::new(ID));
    assert!(matches!(&why, ViewRefusal::Computing { .. }), "{why}");
    assert!(why.to_string().contains("no provenance"), "{why}");
    // Break it: return the empty display list.
}

#[test]
fn a_setting_out_of_its_bounds_or_of_the_wrong_shape_is_refused_by_its_name() {
    let opened = recipe();
    for (name, value) in [
        ("up", json!(65)),
        ("down", json!(-1)),
        ("layering", json!("nope")),
        ("set_aside", json!("PropertyKind")),
        ("set_aside", json!([1, 2])),
    ] {
        let why = refused(&opened, ViewAsk::new(ID).set(name, value.clone()));
        assert!(
            matches!(&why, ViewRefusal::BadSetting { name: named, .. } if named == name),
            "{name} = {value}: {why}"
        );
    }
    // Break it: clamp the depth, or take any list as the set aside.
}

#[test]
fn the_colour_a_file_gives_an_object_is_a_stripe_and_never_invented() {
    struct Red(String);
    impl ColourDoor for Red {
        fn paint(&self, object_uri: &str) -> Result<Option<FilePaint>, String> {
            Ok((object_uri == self.0).then_some(FilePaint::Constant(Rgba {
                r: 1.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            })))
        }
    }
    let opened = recipe();
    let feature_uri = uri_in(&opened.index, 1);
    let stripe = |list: &DisplayList, uri: &str| -> Option<Colour> {
        element(list, "nodes", uri)
            .items
            .iter()
            .find_map(|item| match item {
                Item::Rect {
                    fill: Some(Paint::Solid(colour)),
                    stroke: None,
                    ..
                } => Some(*colour),
                _ => None,
            })
    };
    let door = Red(feature_uri.clone());
    let ask = ViewAsk::new(ID);
    let coloured = draw_list(&kinds(), &ask, &opened.context(&door))
        .expect("drawn")
        .0
        .list;
    assert_eq!(stripe(&coloured, &feature_uri), Some(Colour::rgb(255, 0, 0)));
    // An object the file gives no colour has no stripe: nothing is invented.
    assert_eq!(stripe(&coloured, &uri_in(&opened.index, 5)), None);
    // Asked off, or with no map in the file at all, there is none.
    let off = draw_list(
        &kinds(),
        &ask.clone().set("accent", json!(false)),
        &opened.context(&door),
    )
    .expect("drawn")
    .0
    .list;
    assert_eq!(stripe(&off, &feature_uri), None);
    let none = drawn(&opened, ask).list;
    assert_eq!(stripe(&none, &feature_uri), None);
    // Break it: fall back to a palette colour for an object the file does not colour.
}

#[test]
fn the_same_request_draws_the_same_bytes_and_a_redraw_agrees() {
    let opened = recipe_with_trouble();
    let kinds = kinds();
    let cx = opened.context(&NO_MAPS);
    let ask = ViewAsk::new(ID).set("ordering", json!("median"));
    let first = draw_svg(&kinds, &ask, &cx).expect("drawn");
    let second = draw_svg(&kinds, &ask, &cx).expect("drawn");
    assert_eq!(first.svg, second.svg);
    let again = redraw(&kinds, &first.svg, &cx).expect("redrawn");
    assert_eq!(again.svg, first.svg);
    // Break it: iterate a hash map when placing the boxes.
}
