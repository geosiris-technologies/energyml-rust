//! `provenance-graph`: where each object of a package comes from, and what was done with it.
//!
//! The graph is `energyml_domain::provenance_graph::of_package` (and `focus` around one node): activities with what
//! they read and wrote, the files they read, the representations that carry an object, what a verbatim source states
//! beside them. The boxes are placed by `energyml_sheet::draw::layered` in four columns, left to right: the files,
//! the activities, the objects, then the objects that carry them (a property on its frame, on its trajectory, on its
//! interpretation, on its feature). This module only lays that out through [`crate::graph`]; what the graph signals
//! ([`GraphIssue`]) is written as notes, never left out.

use std::collections::{BTreeSet, HashMap};

use energyml_domain::activity::{ParameterIssue, RoleBasis, RoleRules};
use energyml_domain::color::Paint as FilePaint;
use energyml_domain::package_index::{NodeState, PackageIndex};
use energyml_domain::provenance_graph::{
    CarrierRules, DEFAULT_SET_ASIDE, EdgeBasis, GraphIssue, GraphOptions, MAX_DEPTH, NoInputFiles,
    NoStepConstraints, NodeKey, ProvEdge, ProvEdgeKind, ProvNode, TreeRefusal, focus, of_package,
};
use energyml_domain::provenance_verbatim::VerbatimSources;
use energyml_sheet::draw::Colour;
use energyml_sheet::draw::layered::LayerInput;
use serde_json::Value;

use crate::graph::{self, BoxLook, EdgeLook, Frame, LegendEntry, Line, Tone};
use crate::kind::{Drawn, ObjectSlot, Offer, Request, ViewContext, ViewKind, ViewNote, local_type};
use crate::refusal::ViewRefusal;
use crate::setting::ViewSetting;
use crate::style;

/// The id of the view.
pub const ID: &str = "provenance-graph";

/// The layerings it offers, the first the default: the four columns, or the longest path from a source (the
/// columns then only a minimum).
pub const LAYERINGS: &[&str] = &["fixed-columns", "longest-path"];

/// How far the carriers of an object are followed: a property, its frame, its trajectory, its interpretation, its
/// feature.
const CARRIER_DEPTH: u8 = 4;

/// The most members a collection is expanded to: collections are not expanded here.
const EXPAND_LIMIT: usize = 1000;

const COLUMN_FILES: usize = 0;
const COLUMN_ACTIVITIES: usize = 1;
const COLUMN_OBJECTS: usize = 2;

/// The view.
pub struct ProvenanceView;

impl ViewKind for ProvenanceView {
    fn id(&self) -> &'static str {
        ID
    }

    fn about(&self) -> &'static str {
        "where each object of a package comes from: the activities that read and wrote it, the files they read, the representations that carry it; the whole package, or the graph around one object"
    }

    fn objects(&self) -> Vec<ObjectSlot> {
        Vec::new()
    }

    fn settings(&self) -> Vec<ViewSetting> {
        let mut settings = vec![
            ViewSetting::text(
                "focus",
                "the uri of an object or an activity to centre the graph on; none: the whole package",
            ),
            ViewSetting::integer(
                "up",
                "with a focus: how many steps back towards what produced it (0 to 64)",
            )
            .bounds(Some(0.0), Some(f64::from(MAX_DEPTH)))
            .default(Value::from(i64::from(MAX_DEPTH))),
            ViewSetting::integer(
                "down",
                "with a focus: how many steps forward towards what depends on it (0 to 64)",
            )
            .bounds(Some(0.0), Some(f64::from(MAX_DEPTH)))
            .default(Value::from(1)),
            ViewSetting::json(
                "set_aside",
                "the types left out of the graph and counted in a note, as a list of type names (a CRS, a property kind)",
            )
            .default(Value::from(DEFAULT_SET_ASIDE.to_vec())),
            ViewSetting::flag(
                "accent",
                "a stripe of the colour the file gives an object, on its box; a file with no colour for it gets none",
            )
            .default(Value::Bool(true)),
            ViewSetting::choice(
                "layering",
                LAYERINGS,
                "fixed-columns: files, activities, objects, carriers; longest-path: every box after what it follows",
            )
            .default(Value::from(LAYERINGS[0])),
        ];
        settings.extend(graph::layout_settings());
        settings
    }

    fn size(&self) -> [f64; 2] {
        [900.0, 600.0]
    }

    fn offers(&self, index: &PackageIndex) -> Vec<Offer> {
        let activities = index
            .nodes()
            .filter(|node| node.state == NodeState::Present && local_type(&node.qualified_type) == "Activity")
            .count();
        if activities == 0 {
            return Vec::new();
        }
        vec![Offer {
            view: ID,
            objects: Vec::new(),
            title: format!("the provenance of the package ({activities} activities)"),
            missing: Vec::new(),
        }]
    }

    fn draw(&self, request: &Request, cx: &ViewContext<'_>) -> Result<Drawn, ViewRefusal> {
        let refuse = |why: String| ViewRefusal::computing(ID, why);
        let settings = &request.settings;
        let set_aside = set_aside_of(settings.get("set_aside"))?;
        let aside: Vec<&str> = set_aside.iter().map(String::as_str).collect();

        // The graph: one composition, then the part around the focus when one is asked.
        let roles = RoleRules::new();
        let carriers = CarrierRules::new();
        let verbatim = VerbatimSources::new();
        let options = GraphOptions {
            roles: &roles,
            carriers: &carriers,
            carrier_depth: CARRIER_DEPTH,
            set_aside: &aside,
            expanded: &[],
            expand_limit: EXPAND_LIMIT,
            steps: &NoStepConstraints,
            files: &NoInputFiles,
            verbatim: &verbatim,
        };
        let mut graph = of_package(cx.index, cx.source, &options).graph;
        let mut subject: Option<NodeKey> = None;
        let mut title = "provenance of the package".to_string();
        if let Some(address) = settings
            .text("focus")
            .map(str::trim)
            .filter(|text| !text.is_empty())
        {
            let node = cx.index.node(address).ok_or_else(|| ViewRefusal::NotInPackage {
                object: address.to_string(),
            })?;
            let key = NodeKey::Object(node.uri.to_string());
            let up =
                u8::try_from(settings.integer("up").unwrap_or(i64::from(MAX_DEPTH))).unwrap_or(MAX_DEPTH);
            let down = u8::try_from(settings.integer("down").unwrap_or(1)).unwrap_or(MAX_DEPTH);
            graph = focus(&graph, &key, up, down).map_err(|why| refuse(why.to_string()))?;
            title = format!("provenance around {}", node.title.as_deref().unwrap_or(&node.uri));
            subject = Some(key);
        }
        if graph.nodes.is_empty() {
            return Err(refuse(
                "the package states no provenance: no activity, and no table a verbatim source reads"
                    .to_string(),
            ));
        }

        // The notes: what the graph signals, what was set aside.
        let mut notes: Vec<ViewNote> = graph.issues.iter().map(note_of).collect();
        if !graph.set_aside.is_empty() {
            let types: BTreeSet<&str> = graph
                .set_aside
                .iter()
                .map(|aside| local_type(&aside.qualified_type))
                .collect();
            notes.push(ViewNote {
                key: "set-aside",
                says: format!(
                    "{} objects set aside by type, not drawn: {}",
                    graph.set_aside.len(),
                    types.into_iter().collect::<Vec<_>>().join(", ")
                ),
            });
        }

        // The nodes, in the columns, and the edges between them.
        let at: HashMap<NodeKey, usize> = graph
            .nodes
            .iter()
            .enumerate()
            .map(|(place, node)| (node.key(), place))
            .collect();
        let columns = columns_of(&graph.nodes, &graph.edges, &at);
        let mut accent_error: Option<String> = None;
        let boxes: Vec<BoxLook> = graph
            .nodes
            .iter()
            .map(|node| {
                let mut look = look_of(node);
                if subject.as_ref() == Some(&node.key()) {
                    look.frame = Frame::Subject;
                }
                if settings.flag("accent")
                    && let Some(uri) = coloured_uri(node)
                {
                    match cx.colours.paint(uri) {
                        Ok(Some(FilePaint::Constant(rgba))) => look.accent = Some(colour_of(rgba)),
                        // A map colours values, and nothing here is a value.
                        Ok(_) => {}
                        Err(why) => {
                            accent_error.get_or_insert(why);
                        }
                    }
                }
                look
            })
            .collect();
        if let Some(why) = accent_error {
            notes.push(ViewNote {
                key: "colour-not-read",
                says: format!("the colours the file gives its objects were not read: {why}"),
            });
        }
        let mut kept: Vec<&ProvEdge> = Vec::new();
        let mut ends: Vec<(usize, usize)> = Vec::new();
        let mut dropped = 0usize;
        for edge in &graph.edges {
            match (at.get(&edge.from), at.get(&edge.to)) {
                (Some(from), Some(to)) => {
                    kept.push(edge);
                    ends.push((*from, *to));
                }
                _ => dropped += 1,
            }
        }
        if dropped > 0 {
            notes.push(ViewNote {
                key: "edge-without-node",
                says: format!("{dropped} edges name a node the graph does not hold and are not drawn"),
            });
        }
        let looks: Vec<EdgeLook> = kept
            .iter()
            .zip(&ends)
            .map(|(edge, (from, to))| edge_look(edge, &boxes[*from].key, &boxes[*to].key))
            .collect();

        // The placement.
        let input = LayerInput {
            nodes: boxes.iter().map(BoxLook::spec).collect(),
            edges: ends,
            fixed_layer: columns.iter().map(|column| Some(*column)).collect(),
            pins: Vec::new(),
        };
        let layering = settings.text("layering").unwrap_or(LAYERINGS[0]);
        let placed = graph::place(ID, &input, &graph::layered_options(settings, Some(layering)))?;
        let legend = legend_of(&kept, &placed.back);
        let list = graph::draw(ID, &title, &placed, &boxes, &looks, &legend)?;
        Ok(Drawn {
            list,
            notes: graph::capped(notes),
        })
    }
}

/// The types set aside: a list of names, nothing else.
fn set_aside_of(value: Option<&Value>) -> Result<Vec<String>, ViewRefusal> {
    let bad = |why: String| ViewRefusal::BadSetting {
        name: "set_aside".to_string(),
        why,
    };
    let Some(value) = value else {
        return Ok(DEFAULT_SET_ASIDE.iter().map(|name| name.to_string()).collect());
    };
    let Some(list) = value.as_array() else {
        return Err(bad(format!("`{value}` is not a list of type names")));
    };
    list.iter()
        .map(|item| {
            item.as_str()
                .map(str::to_string)
                .ok_or_else(|| bad(format!("`{item}` is not a type name")))
        })
        .collect()
}

/// The column of every node: files, activities, objects, then each carrier one column after what it carries.
fn columns_of(nodes: &[ProvNode], edges: &[ProvEdge], at: &HashMap<NodeKey, usize>) -> Vec<usize> {
    let mut column: Vec<usize> = nodes
        .iter()
        .map(|node| match node {
            ProvNode::File { .. } => COLUMN_FILES,
            ProvNode::Activity { .. } | ProvNode::HeldStep { .. } => COLUMN_ACTIVITIES,
            ProvNode::Object { .. } | ProvNode::Collection { .. } | ProvNode::Unresolved { .. } => {
                COLUMN_OBJECTS
            }
        })
        .collect();
    let carries: Vec<(usize, usize)> = edges
        .iter()
        .filter(|edge| matches!(edge.kind, ProvEdgeKind::Carrier { .. }))
        .filter_map(|edge| Some((*at.get(&edge.from)?, *at.get(&edge.to)?)))
        .collect();
    // One pass per node is enough for a chain; a loop of carriers stops there instead of growing for ever.
    for _ in 0..nodes.len() {
        let mut moved = false;
        for (holder, carrier) in &carries {
            if column[*carrier] <= column[*holder] {
                column[*carrier] = column[*holder] + 1;
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
    column
}

/// The uri of a node whose colour the file may give: an object the package holds.
fn coloured_uri(node: &ProvNode) -> Option<&str> {
    match node {
        ProvNode::Object {
            uri, present: true, ..
        } => Some(uri),
        ProvNode::Collection { uri, .. } => Some(uri),
        _ => None,
    }
}

fn colour_of(rgba: energyml_domain::color::Rgba) -> Colour {
    let channel = |value: f64| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    Colour::rgb(channel(rgba.r), channel(rgba.g), channel(rgba.b))
}

/// What a user points at on a node: a uri, or the words that tell a file, a step or a value nothing resolves apart.
pub fn key_of(key: &NodeKey) -> String {
    match key {
        NodeKey::Object(uri) => uri.clone(),
        NodeKey::File(path) => format!("file:{path}"),
        NodeKey::HeldStep { workflow, step } => format!("{workflow}#step={step}"),
        NodeKey::Unresolved { source, label } => format!("unresolved:{}:{label}", source.0),
    }
}

fn last_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn look_of(node: &ProvNode) -> BoxLook {
    let key = key_of(&node.key());
    match node {
        ProvNode::File { path } => {
            let mut look = BoxLook::new(
                key,
                "node",
                vec![
                    Line::new("file", Tone::Caption),
                    Line::new(last_name(path), Tone::Body),
                ],
            );
            look.tip = path.clone();
            look.data.push(("family".to_string(), "file".to_string()));
            look
        }
        ProvNode::Activity { uri, title, template } => {
            let name = title
                .clone()
                .unwrap_or_else(|| energyml_core::identity::parse_address(uri).0);
            let mut look = BoxLook::new(
                key,
                "node",
                vec![Line::new("Activity", Tone::Caption), Line::new(name, Tone::Body)],
            );
            look.tip = match template {
                Some(template) => format!("{uri} (template {template})"),
                None => uri.clone(),
            };
            look.data.push(("family".to_string(), "activity".to_string()));
            look
        }
        ProvNode::HeldStep { step, reason, .. } => {
            let mut look = BoxLook::new(
                key,
                "node",
                vec![
                    Line::new("held step", Tone::Caption),
                    Line::new(step.clone(), Tone::Body),
                ],
            );
            look.frame = Frame::Ghost;
            look.tip = format!("held: {reason}");
            look.data.push(("family".to_string(), "step".to_string()));
            look
        }
        ProvNode::Object {
            uri,
            qualified_type,
            title,
            present,
        } => {
            let kind = match local_type(qualified_type) {
                "" => "object",
                known => known,
            };
            let name = title
                .clone()
                .unwrap_or_else(|| energyml_core::identity::parse_address(uri).0);
            let caption = if *present {
                kind.to_string()
            } else {
                format!("{kind} (not in the package)")
            };
            let mut look = BoxLook::new(
                key,
                "node",
                vec![Line::new(caption, Tone::Caption), Line::new(name, Tone::Body)],
            );
            if !present {
                look.frame = Frame::Ghost;
            }
            look.tip = uri.clone();
            look.data.push(("family".to_string(), "object".to_string()));
            look
        }
        ProvNode::Collection {
            uri, title, members, ..
        } => {
            let name = title
                .clone()
                .unwrap_or_else(|| energyml_core::identity::parse_address(uri).0);
            let caption = match members {
                Some(count) => format!("collection ({count} objects)"),
                None => "collection".to_string(),
            };
            let mut look = BoxLook::new(
                key,
                "node",
                vec![Line::new(caption, Tone::Caption), Line::new(name, Tone::Body)],
            );
            look.tip = uri.clone();
            look.data.push(("family".to_string(), "collection".to_string()));
            look
        }
        ProvNode::Unresolved { label, source } => {
            let mut look = BoxLook::new(
                key,
                "node",
                vec![
                    Line::new(format!("unresolved ({})", source.0), Tone::Caption),
                    Line::new(label.clone(), Tone::Body),
                ],
            );
            look.frame = Frame::Ghost;
            look.tip = format!(
                "{label}: written by {}, and nothing in the package is it",
                source.0
            );
            look.data.push(("family".to_string(), "unresolved".to_string()));
            look
        }
    }
}

/// The word of an edge's kind, in the data of its element and the legend.
fn kind_word(kind: &ProvEdgeKind) -> &'static str {
    match kind {
        ProvEdgeKind::Consumed => "consumed",
        ProvEdgeKind::Produced => "produced",
        ProvEdgeKind::Participant => "participant",
        ProvEdgeKind::Parent => "parent",
        ProvEdgeKind::Carrier { .. } => "carrier",
        ProvEdgeKind::Member => "member",
    }
}

/// The kinds in the order the legend lists them, with what each says.
const KINDS: &[(&str, &str)] = &[
    ("consumed", "read by an activity"),
    ("produced", "written by an activity"),
    ("participant", "takes part in an activity, no direction stated"),
    ("parent", "an activity under its parent"),
    (
        "carrier",
        "carried by a representation, an interpretation, a feature",
    ),
    ("member", "a member of a collection"),
];

fn basis_word(basis: &EdgeBasis) -> &'static str {
    match basis {
        EdgeBasis::Activity(_) => "activity",
        EdgeBasis::Verbatim(_) => "verbatim",
        EdgeBasis::Reference => "reference",
        EdgeBasis::Both(_) => "both",
    }
}

fn basis_says(basis: &EdgeBasis) -> String {
    match basis {
        EdgeBasis::Activity(RoleBasis::Template) => {
            "stated by an activity, its role by the template".to_string()
        }
        EdgeBasis::Activity(RoleBasis::Rule(rule)) => {
            format!("stated by an activity, its role by the rule {rule}")
        }
        EdgeBasis::Activity(RoleBasis::None) => "stated by an activity, its role not declared".to_string(),
        EdgeBasis::Verbatim(source) => format!("stated by the verbatim source {}", source.0),
        EdgeBasis::Reference => "a reference written in the object".to_string(),
        EdgeBasis::Both(source) => format!("stated by an activity and by the verbatim source {}", source.0),
    }
}

/// The ink, the dash and the width of an edge: the kind says the colour and the dash, the basis the width, and a
/// verbatim statement alone is brought forward in the highlight ink.
fn stroke_of(kind: &ProvEdgeKind, basis: &EdgeBasis) -> (Colour, Vec<f64>, f64) {
    let (ink, dash) = match kind {
        ProvEdgeKind::Consumed => (style::INK, Vec::new()),
        ProvEdgeKind::Produced => (style::SUBJECT, Vec::new()),
        ProvEdgeKind::Participant => (style::MUTED, vec![2.0, 3.0]),
        ProvEdgeKind::Parent => (style::MUTED, Vec::new()),
        ProvEdgeKind::Carrier { .. } => (style::NEUTRAL, Vec::new()),
        ProvEdgeKind::Member => (style::NEUTRAL, vec![2.0, 3.0]),
    };
    match basis {
        EdgeBasis::Verbatim(_) => (style::HIGHLIGHT, dash, 1.5),
        EdgeBasis::Both(_) => (ink, dash, 3.0),
        EdgeBasis::Activity(_) | EdgeBasis::Reference => (ink, dash, 1.5),
    }
}

fn edge_look(edge: &ProvEdge, from: &str, to: &str) -> EdgeLook {
    let (colour, dash, width) = stroke_of(&edge.kind, &edge.basis);
    let word = kind_word(&edge.kind);
    EdgeLook {
        key: format!("{from}>{to}#{word}"),
        tip: format!("{word}: {}", basis_says(&edge.basis)),
        colour,
        width,
        dash,
        label: None,
        data: vec![
            ("edge".to_string(), word.to_string()),
            ("basis".to_string(), basis_word(&edge.basis).to_string()),
        ],
    }
}

/// The legend: the kinds of edge there are, then what the line style says beyond the kind.
fn legend_of(edges: &[&ProvEdge], back: &[usize]) -> Vec<LegendEntry> {
    let mut legend = Vec::new();
    for (word, says) in KINDS {
        if let Some(edge) = edges.iter().find(|edge| kind_word(&edge.kind) == *word) {
            let (colour, dash, width) = stroke_of(&edge.kind, &EdgeBasis::Reference);
            legend.push(LegendEntry {
                label: (*says).to_string(),
                colour,
                width,
                dash,
            });
        }
    }
    if !back.is_empty() {
        legend.push(LegendEntry {
            label: "dashed, long dashes: drawn against the flow of the columns".to_string(),
            colour: style::MUTED,
            width: 1.5,
            dash: graph::BACK_DASH.to_vec(),
        });
    }
    if edges
        .iter()
        .any(|edge| matches!(edge.basis, EdgeBasis::Verbatim(_)))
    {
        legend.push(LegendEntry {
            label: "stated by a verbatim source only".to_string(),
            colour: style::HIGHLIGHT,
            width: 1.5,
            dash: Vec::new(),
        });
    }
    if edges.iter().any(|edge| matches!(edge.basis, EdgeBasis::Both(_))) {
        legend.push(LegendEntry {
            label: "thick: stated twice, by an activity and by a verbatim source".to_string(),
            colour: style::INK,
            width: 3.0,
            dash: Vec::new(),
        });
    }
    legend
}

fn issue_says(issue: &ParameterIssue) -> String {
    match issue {
        ParameterIssue::NotInTemplate { title, .. } => {
            format!("parameter `{title}` has no equivalent in the template")
        }
        ParameterIssue::KindDiffers { title, written, .. } => {
            format!("parameter `{title}` is written as {written:?}, which the template does not allow")
        }
        ParameterIssue::TooFew { title, min, found } => {
            format!("parameter `{title}` is given {found} times, the template asks at least {min}")
        }
        ParameterIssue::TooMany { title, max, found } => {
            format!("parameter `{title}` is given {found} times, the template allows at most {max}")
        }
        ParameterIssue::TemplateAbsent => "the activity names no template the package holds".to_string(),
    }
}

/// What the graph signals, as a note: a stable key per kind of signal, the sentence in English.
fn note_of(issue: &GraphIssue) -> ViewNote {
    let (key, says) = match issue {
        GraphIssue::Tree(TreeRefusal::ParentCycle { uris }) => (
            "tree-refused",
            format!(
                "the Parent links of {} activities loop; they are drawn flat",
                uris.len()
            ),
        ),
        GraphIssue::Tree(refusal @ TreeRefusal::ParentAbsent { .. }) => ("tree-refused", refusal.to_string()),
        GraphIssue::TreeDisagreement { parent, child, .. } => (
            "tree-disagreement",
            format!("the Parent of {child} and the sub-activities of {parent} do not state the same link"),
        ),
        GraphIssue::HeldButPlayed { workflow, step } => (
            "held-but-played",
            format!("the step `{step}` of {workflow} is declared held and ran anyway"),
        ),
        GraphIssue::OutputAbsent { activity, object } => (
            "output-absent",
            format!("{activity} says it wrote {object}, which the package does not hold"),
        ),
        GraphIssue::Conformance { activity, issue } => {
            ("conformance", format!("{activity}: {}", issue_says(issue)))
        }
        GraphIssue::ActivityLinkDiffers {
            source,
            process,
            by_column,
            by_mapping,
        } => (
            "activity-link-differs",
            format!(
                "{}: the process `{process}` is tied to {} by its table and to {} by the mapping",
                source.0,
                by_column.as_deref().unwrap_or("no activity"),
                by_mapping.as_deref().unwrap_or("no activity")
            ),
        ),
        GraphIssue::ProcessWithoutActivity { source, process } => (
            "process-without-activity",
            format!(
                "{}: the process `{process}` names no activity, its edges are left out",
                source.0
            ),
        ),
        GraphIssue::Verbatim(refusal) => (
            "verbatim-refused",
            format!("a verbatim source refused: {refusal}"),
        ),
        GraphIssue::Unreadable { uri, why } => ("unreadable", format!("{uri} could not be read: {why}")),
        GraphIssue::Expand(refusal) => (
            "expand-refused",
            format!("a collection was not expanded: {refusal}"),
        ),
    };
    ViewNote { key, says }
}
