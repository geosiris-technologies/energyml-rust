//! **Who a wellbore has**: its trajectories, frames, marker frames, and the
//! properties on them - from the inventory and the references alone, no array,
//! no geometry. The walk the wells grid and the depth coverage share.
//!
//! The port of the studio's `wellMatrix` (`matrix.ts`), seeded from both ends:
//! a well with no representation is a row, and a representation with no well
//! makes its own. It lives here and not in the cache because the depth coverage
//! of the domain needs it and the domain does not depend on the cache (ADR 0040).
//!
//! Nothing here judges a member: whether a representation can be drawn is the
//! business of the face that draws it.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::package_index::{NodeState, PackageIndex};

/// What a well has of a kind of object.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MemberKind {
    /// The path of the well: a RESQML trajectory, a deviation survey, a WITSML trajectory.
    Trajectory,
    /// What samples along the well: the supports of the logs.
    Frame,
    /// The tops: a marker frame.
    Marker,
}

/// The types of each kind, by local name: a table, not a `match`.
pub const MEMBER_KINDS: &[(MemberKind, &[&str])] = &[
    (
        MemberKind::Trajectory,
        &[
            "WellboreTrajectoryRepresentation",
            "DeviationSurveyRepresentation",
            "Trajectory",
        ],
    ),
    (
        MemberKind::Frame,
        &[
            "WellboreFrameRepresentation",
            "SeismicWellboreFrameRepresentation",
            "BlockedWellboreRepresentation",
        ],
    ),
    (MemberKind::Marker, &["WellboreMarkerFrameRepresentation"]),
];

/// The markers of a table of categories in a type name (`StringTableLookup`,
/// `ColumnBasedTable`). TODO(verify): carried over with the doubt of the wells
/// grid it comes from (that a continuous or discrete property never references
/// a `ColumnBasedTable`).
pub const CATEGORY_TABLE_MARKERS: &[&str] = &["Lookup", "ColumnBasedTable"];

/// The local name of the type that names a well.
const FEATURE: &str = "WellboreFeature";
/// The local name of the type that interprets a well.
const INTERPRETATION: &str = "WellboreInterpretation";
/// The WITSML wellbore: how a WITSML trajectory names its well.
const WELLBORE: &str = "Wellbore";

/// The local name of a qualified type: after the last `.`, without the `obj_`
/// of release 2.0.1.
fn local(qualified: &str) -> &str {
    let after = qualified.rsplit('.').next().unwrap_or(qualified);
    after.strip_prefix("obj_").unwrap_or(after)
}

fn is_property(local: &str) -> bool {
    local.ends_with("Property")
}

fn is_category_table(local: &str) -> bool {
    CATEGORY_TABLE_MARKERS.iter().any(|marker| local.contains(marker))
}

/// The kind of a type, by its local name.
pub fn member_kind_of(local: &str) -> Option<MemberKind> {
    MEMBER_KINDS
        .iter()
        .find(|(_, names)| names.contains(&local))
        .map(|(kind, _)| *kind)
}

/// Whether an object of this local type takes part in the walk.
pub fn relevant(local: &str) -> bool {
    member_kind_of(local).is_some()
        || matches!(local, FEATURE | INTERPRETATION | WELLBORE)
        || is_property(local)
        || is_category_table(local)
}

/// One object of the inventory, as the walk needs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectFact {
    /// Its complete uri.
    pub uri: String,
    /// `resqml22.WellboreFeature`, `resqml20.obj_WellboreFeature`, `witsml21.Trajectory`.
    pub qualified_type: String,
    /// Its title; empty when not read (a property: one of thousands).
    pub title: String,
}

/// One object a well holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    /// Its complete uri.
    pub uri: String,
    /// Its title, empty when none was read.
    pub title: String,
}

/// One well, and what the package holds for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WellMembers {
    /// The uri of the feature, else of the interpretation, else of the
    /// representation that reaches no well.
    pub key: String,
    /// Its title, or its uri when it has none (never told apart here).
    pub title: String,
    /// The prefixes of the types met (`resqml22`, `witsml21`), sorted.
    pub versions: Vec<String>,
    /// Its interpretations' uris, sorted.
    pub interpretations: Vec<String>,
    /// Its trajectories, in the order of the inventory.
    pub trajectories: Vec<Member>,
    /// Its frames, in the order of the inventory.
    pub frames: Vec<Member>,
    /// Its marker frames, in the order of the inventory.
    pub markers: Vec<Member>,
    /// The properties that reference one of its representations.
    pub properties: Vec<Member>,
    /// Those of them that also reference a table of categories.
    pub categorical: Vec<Member>,
}

#[derive(Default)]
struct Draft {
    title: String,
    versions: BTreeSet<String>,
    interpretations: BTreeSet<String>,
    trajectories: Vec<Member>,
    frames: Vec<Member>,
    markers: Vec<Member>,
}

/// The objects by uri and what each references, asked once per relevant object.
struct Graph<'a> {
    by_uri: HashMap<&'a str, &'a ObjectFact>,
    out: HashMap<&'a str, Vec<String>>,
}

impl<'a> Graph<'a> {
    /// The objects the list holds that `uri` references, in order.
    fn targets(&self, uri: &str) -> impl Iterator<Item = &'a ObjectFact> + '_ {
        self.out
            .get(uri)
            .into_iter()
            .flatten()
            .filter_map(|target| self.by_uri.get(target.as_str()).copied())
    }

    /// The first target of `uri` that has this local type.
    fn target_of(&self, uri: &str, wanted: &str) -> Option<&'a ObjectFact> {
        self.targets(uri)
            .find(|found| local(&found.qualified_type) == wanted)
    }
}

/// The title a well is shown by: its own, else its uri.
fn label_of(fact: &ObjectFact) -> String {
    if fact.title.is_empty() {
        fact.uri.clone()
    } else {
        fact.title.clone()
    }
}

fn draft<'r>(rows: &'r mut BTreeMap<String, Draft>, owner: &ObjectFact) -> &'r mut Draft {
    rows.entry(owner.uri.clone()).or_insert_with(|| Draft {
        title: label_of(owner),
        ..Draft::default()
    })
}

/// The walk: one entry per well, sorted by lower-case title then key.
///
/// `objects` are the objects of the package that [`relevant`] keeps (others are
/// ignored); `outgoing(uri)` gives the uris an object references (the index's,
/// or a dataspace's edges) and is asked **once per relevant object**. A
/// reference to an object the list does not hold is ignored.
///
/// Two wells of one title are not told apart here: that is presentation.
pub fn well_members(objects: &[ObjectFact], outgoing: &dyn Fn(&str) -> Vec<String>) -> Vec<WellMembers> {
    let relevant_objects: Vec<&ObjectFact> = objects
        .iter()
        .filter(|fact| relevant(local(&fact.qualified_type)))
        .collect();
    let mut out: HashMap<&str, Vec<String>> = HashMap::new();
    for fact in &relevant_objects {
        out.entry(fact.uri.as_str())
            .or_insert_with(|| outgoing(&fact.uri));
    }
    let graph = Graph {
        by_uri: relevant_objects
            .iter()
            .map(|fact| (fact.uri.as_str(), *fact))
            .collect(),
        out,
    };
    let kind_of = |fact: &ObjectFact| member_kind_of(local(&fact.qualified_type));

    // The feature that names a WITSML wellbore (RESQML 2.2 `WellboreFeature.WitsmlWellbore`).
    let mut naming: HashMap<&str, &ObjectFact> = HashMap::new();
    for fact in relevant_objects
        .iter()
        .filter(|fact| local(&fact.qualified_type) == FEATURE)
    {
        for target in graph
            .targets(&fact.uri)
            .filter(|found| local(&found.qualified_type) == WELLBORE)
        {
            naming.entry(target.uri.as_str()).or_insert(fact);
        }
    }

    let mut rows: BTreeMap<String, Draft> = BTreeMap::new();

    // Seeded from the wells themselves.
    for fact in relevant_objects
        .iter()
        .filter(|fact| local(&fact.qualified_type) == FEATURE)
    {
        draft(&mut rows, fact);
    }
    for fact in relevant_objects
        .iter()
        .filter(|fact| local(&fact.qualified_type) == INTERPRETATION)
    {
        let owner = graph.target_of(&fact.uri, FEATURE).unwrap_or(fact);
        draft(&mut rows, owner).interpretations.insert(fact.uri.clone());
    }

    // A representation's row key: its well, otherwise itself. A WITSML
    // trajectory's well is its wellbore, or the feature that names that
    // wellbore, so the two halves of one well share a row.
    let mut well_reps: HashMap<&str, String> = HashMap::new();
    for fact in &relevant_objects {
        let Some(kind) = kind_of(fact) else {
            continue;
        };
        let key = if let Some(interpretation) = graph.target_of(&fact.uri, INTERPRETATION) {
            let owner = graph
                .target_of(&interpretation.uri, FEATURE)
                .unwrap_or(interpretation);
            draft(&mut rows, owner)
                .interpretations
                .insert(interpretation.uri.clone());
            owner.uri.clone()
        } else if let Some(wellbore) = graph.target_of(&fact.uri, WELLBORE) {
            let owner = naming.get(wellbore.uri.as_str()).copied().unwrap_or(wellbore);
            draft(&mut rows, owner);
            owner.uri.clone()
        } else {
            draft(&mut rows, fact);
            fact.uri.clone()
        };
        let member = Member {
            uri: fact.uri.clone(),
            title: fact.title.clone(),
        };
        if let Some(found) = rows.get_mut(&key) {
            match kind {
                MemberKind::Trajectory => found.trajectories.push(member),
                MemberKind::Frame => found.frames.push(member),
                MemberKind::Marker => found.markers.push(member),
            }
            found.versions.insert(
                fact.qualified_type
                    .split('.')
                    .next()
                    .unwrap_or_default()
                    .to_string(),
            );
        }
        well_reps.insert(fact.uri.as_str(), key);
    }

    // The properties that point at one of the well's representations; a table
    // of categories among their targets makes the property categorical.
    let mut properties: HashMap<String, Vec<Member>> = HashMap::new();
    let mut categorical: HashMap<String, Vec<Member>> = HashMap::new();
    for fact in relevant_objects
        .iter()
        .filter(|fact| is_property(local(&fact.qualified_type)))
    {
        let Some(key) = graph
            .out
            .get(fact.uri.as_str())
            .into_iter()
            .flatten()
            .find_map(|target| well_reps.get(target.as_str()))
        else {
            continue;
        };
        let member = Member {
            uri: fact.uri.clone(),
            title: fact.title.clone(),
        };
        if graph
            .targets(&fact.uri)
            .any(|found| is_category_table(local(&found.qualified_type)))
        {
            categorical.entry(key.clone()).or_default().push(member.clone());
        }
        properties.entry(key.clone()).or_default().push(member);
    }

    let mut wells: Vec<WellMembers> = rows
        .into_iter()
        .map(|(key, draft)| WellMembers {
            trajectories: draft.trajectories,
            frames: draft.frames,
            markers: draft.markers,
            properties: properties.remove(&key).unwrap_or_default(),
            categorical: categorical.remove(&key).unwrap_or_default(),
            versions: draft.versions.into_iter().collect(),
            interpretations: draft.interpretations.into_iter().collect(),
            title: draft.title,
            key,
        })
        .collect();
    wells.sort_by(|a, b| (a.title.to_lowercase(), &a.key).cmp(&(b.title.to_lowercase(), &b.key)));
    wells
}

/// The relevant objects the index holds (`NodeState::Present`), titles as indexed.
pub fn facts_of_index(index: &PackageIndex) -> Vec<ObjectFact> {
    index
        .nodes()
        .filter(|node| node.state == NodeState::Present && relevant(local(&node.qualified_type)))
        .map(|node| ObjectFact {
            uri: node.uri.to_string(),
            qualified_type: node.qualified_type.to_string(),
            title: node.title.as_deref().unwrap_or_default().to_string(),
        })
        .collect()
}

/// [`well_members`] over an index: `outgoing` is `references_of`, by the uuid
/// the node itself carries (the index keys by uuid; the uri stays the identity).
pub fn well_members_of_index(index: &PackageIndex) -> Vec<WellMembers> {
    let outgoing = |uri: &str| -> Vec<String> {
        match index.node(uri) {
            Some(node) => index
                .references_of(&node.uuid)
                .into_iter()
                .map(|pointer| pointer.uri)
                .collect(),
            None => Vec::new(),
        }
    };
    well_members(&facts_of_index(index), &outgoing)
}
