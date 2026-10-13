//! What an object inherits — a colour, a symbol, any information it does not
//! carry itself: **the order, and the path it was found by**.
//!
//! Design: ADR-0009. The walk knows nothing about colours: it takes *"does this object
//! carry I?"* as a function and answers with a [`Resolution`] — the value, the
//! rank it was found at, the object that carries it, **the steps from the
//! object to it**, the candidates that lost a tie, and what was skipped.
//!
//! # The order
//!
//! | rank | where |
//! |---|---|
//! | 1 | on the object |
//! | 2 | on the collections that reference it, then theirs (2 bis) |
//! | 3 | on its parents |
//! | 4 | on the collections that reference those parents, nested (4 bis) |
//! | 5 | the parents of the parents, with the same rules |
//! | 6 | on the model that references the object or one of its parents |
//! | — | **not followed, but noted**: the model's collections, any `project` collection |
//!
//! A **property** is a shorter case (searching for the property kind is accepted): the
//! property, its `PropertyKind`, the kind's parents — a collection at every
//! step — and **nothing beyond**: [`Walker::resolve_property`].
//!
//! Within a rank, **the shortest path wins**; at equal length, **the first in
//! document order** (document order is the accepted tie-break), and the others are kept
//! in [`Resolution::ties`]. Rank 5 is read stage by stage — each stage up is
//! the parents, then their collections — so a collection of a grandparent does
//! not race a great-grandparent.
//!
//! # Where the memberships and the parents are read
//!
//! Only from the **relation tables** the package index lays while it builds —
//! ADR-0010: these relations are built at graph-construction time, not by
//! traversing the whole graph afterwards.
//! The walk used to filter the whole of an object's edges
//! ([`PackageIndex::dependents_of`], [`PackageIndex::references_of`], a `Pointer`
//! of five strings per edge) to find its memberships, its parents and the
//! models that name it, and to open every collection it met to read its `Kind`.
//! It now reads [`PackageIndex::memberships_of`], [`PackageIndex::associations_of`],
//! [`PackageIndex::parent_rows`], [`PackageIndex::models_referencing`] and
//! [`PackageIndex::is_project`]: a row per relation, laid once, kept up to date
//! by [`PackageIndex::update`] at the place it keeps the references. No object
//! is opened by a walk.
//!
//! - **membership** (ADR-0014): a
//!   `CollectionsToDataobjectsAssociationSet` writes, in association `i`,
//!   `SingleCollectionAssociation[i].Collection` (one reference) and
//!   `SingleCollectionAssociation[i].Dataobject[j]` (the members). A member is
//!   **in** the collection of the association of the same `i`. A collection
//!   referenced elsewhere (a parent it names, say) is not a membership;
//! - **parents**: the edges whose path is `RepresentedObject`,
//!   `RepresentedInterpretation` or `InterpretedFeature` — the three fields of
//!   `color::parent_of`, which is private there and read from the object; the
//!   confrontation of the two is in `tests/inherit.rs`;
//! - **the model**: a node whose type is `Model` or `EarthModelInterpretation`
//!   that **references** the object or one of its parents (a
//!   `Model` points nothing, an `EarthModelInterpretation` points the
//!   interpretations), and the `Model` the latter interprets.
//!
//! # What the index cannot give: the file order
//!
//! The index keeps each node's references sorted by target, not in the order
//! they were written. *Document order* is therefore read from what the **path**
//! says — the `[i]` and `[j]` of an association, the field's rank among the three
//! parent fields — and, past that, the uuid. Two collections named in two
//! different association sets tie on position and fall to the holder's uuid.
//!
//! # The rules of the specification that apply
//!
//! common TRG 2.3 §8.3: the same collection named in two associations of one
//! set counts once — the first; §8.5: an object listed twice in an association
//! counts once. Both are counted in the notes. A collection that holds itself,
//! near or far, is a **cycle, named**: the walk does not follow the edge that
//! closes it.
//!
//! # The cost
//!
//! A [`Walker`] memoises what is per package — the memberships of a node, the
//! `Kind` of a collection, the answer of the predicate per object — so a
//! population ([`Walker::resolve_all`]) pays once for a collection shared by two
//! thousand wells: what is per package is paid once per package.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::package_index::PackageIndex;
use crate::property_kind::Resolver;
use crate::source::ObjectSource;

/// The three fields a representation, an interpretation and a feature climb by,
/// in the order `color::parent_of` tries them.
pub(crate) const PARENT_FIELDS: [&str; 3] = [
    "RepresentedObject",
    "RepresentedInterpretation",
    "InterpretedFeature",
];

/// The upper bound on a chain of parents: past it, it is a loop, not a chain.
const MAX_STAGES: usize = 64;

/// How a step goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Via {
    /// To a parent: the representation's interpretation, the property's kind…
    Parent,
    /// To a collection the object is a member of.
    Collection,
    /// To the model that references it (read in the direction of the walk, even
    /// though the reference points the other way).
    Model,
}

impl Via {
    /// `parent`, `collection`, `model`.
    pub fn as_str(self) -> &'static str {
        match self {
            Via::Parent => "parent",
            Via::Collection => "collection",
            Via::Model => "model",
        }
    }
}

/// One link of the path from the object to the object that answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    /// Uuid the step leaves — for a face that shows one; the walk goes by
    /// [`Step::from_uri`].
    pub from: String,
    /// Uuid the step reaches — see [`Step::to_uri`].
    pub to: String,
    /// Uri (the full identity) of the object the step leaves.
    pub from_uri: String,
    /// Uri (the full identity) of the object the step reaches. Two versions of
    /// one object are two uris with one uuid.
    pub to_uri: String,
    /// What kind of link it is.
    pub via: Via,
    /// The path of the reference in the object that carries it:
    /// `SingleCollectionAssociation[2].Dataobject[5]`, `InterpretedFeature`…
    pub field: String,
    /// For a collection step, the uuid of the association set that writes the
    /// membership — the object that carries `field`.
    pub held_by: Option<String>,
    /// The same, by uri.
    pub held_by_uri: Option<String>,
}

/// A candidate that lost a tie: same rank, same distance, later in the document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tie<I> {
    /// The uuid of the object that carries the information.
    pub carrier: String,
    /// Its uri, the identity the walk went by.
    pub carrier_uri: String,
    /// What it carries.
    pub value: I,
    /// How it was reached.
    pub path: Vec<Step>,
}

/// What was found, and how.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolution<I> {
    /// The information.
    pub value: I,
    /// 1 to 6, see the module documentation.
    pub rank: u8,
    /// Uuid of the object that carries it.
    pub carrier: String,
    /// Its uri — the identity the walk went by.
    pub carrier_uri: String,
    /// From the object to the carrier. Empty at rank 1.
    pub path: Vec<Step>,
    /// The other candidates of the same rank and the same distance, in document
    /// order: discarded, and shown.
    pub ties: Vec<Tie<I>>,
    /// What was skipped and why: a cycle, a duplicate, an information on a
    /// collection that is not followed.
    pub notes: Vec<String>,
}

impl<I> Resolution<I> {
    /// How many steps from the object to the carrier.
    pub fn distance(&self) -> usize {
        self.path.len()
    }
}

/// The answer of a walk: the resolution when there is one, and the notes
/// either way — an absence is explained, not silent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Walked<I> {
    /// `None`: nothing in the order carries the information.
    pub resolution: Option<Resolution<I>>,
    /// Everything skipped; the same list as [`Resolution::notes`] when found.
    pub notes: Vec<String>,
}

/// The chain of kinds of a property, as uuids, and what could not be followed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KindChain {
    /// The kind, then each parent. A kind with no uuid (a 2.0.1 standard kind,
    /// known by its name only) cannot carry anything and is left out.
    pub uuids: Vec<String>,
    /// `property_kind::Resolution::notes`, or why the chain is empty.
    pub notes: Vec<String>,
}

/// The kind chain of a property: the sheet's kind, resolved by
/// [`property_kind::Resolver`](crate::property_kind::Resolver) — which the
/// colour cascade uses too, so the two chains are the same one.
pub fn kind_chain(index: &PackageIndex, objects: &dyn ObjectSource, property_uuid: &str) -> KindChain {
    let Some(reference) = index.sheet(property_uuid).and_then(|sheet| sheet.kind_ref()) else {
        return KindChain {
            uuids: Vec::new(),
            notes: vec![format!(
                "`{property_uuid}` has no property sheet in the index, or its sheet names no kind"
            )],
        };
    };
    match Resolver::new(objects).resolve(&reference) {
        Ok(resolution) => KindChain {
            uuids: resolution
                .chain
                .iter()
                .filter(|kind| !kind.uuid.is_empty())
                .map(|kind| kind.uuid.clone())
                .collect(),
            notes: resolution.notes,
        },
        Err(why) => KindChain {
            uuids: Vec::new(),
            notes: vec![why.to_string()],
        },
    }
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

/// Document position of one step: what the path says, then the uuid.
type StepKey = (u64, u64, String);

#[derive(Clone)]
struct Hop {
    /// Uuid of the object reached, and its uri — **the identity the walk goes by**.
    to: String,
    to_uri: String,
    via: Via,
    field: String,
    held_by: Option<String>,
    held_by_uri: Option<String>,
    key: StepKey,
}

/// The uuid inside an identity: the uuid of a uri, the text itself when it is
/// not one (an object the package does not hold, known by a bare uuid).
pub(crate) fn uuid_of_identity(identity: &str) -> String {
    let text = identity.trim();
    let (uuid, _) = energyml_core::identity::parse_address(text);
    // A bare uuid comes back as written, not lowercased.
    if uuid.eq_ignore_ascii_case(text) {
        text.to_string()
    } else {
        uuid
    }
}

/// An object a walk reaches, and the steps it went by.
///
/// What a [`Tier`] holds. The document-order key stays private: a tier is
/// already sorted by it.
#[derive(Clone, Debug)]
pub struct Candidate {
    /// Uuid of the object reached.
    pub uuid: String,
    /// Its **uri**: the identity the walk went by, so that two versions of one
    /// object are two candidates.
    pub uri: String,
    /// From the object the walk started on to this one. Empty at rank 1.
    pub steps: Vec<Step>,
    key: Vec<StepKey>,
}

impl Candidate {
    fn start(identity: &str) -> Candidate {
        Candidate {
            uuid: uuid_of_identity(identity),
            uri: identity.to_string(),
            steps: Vec::new(),
            key: Vec::new(),
        }
    }

    fn then(&self, hop: &Hop) -> Candidate {
        let mut steps = self.steps.clone();
        steps.push(Step {
            from: self.uuid.clone(),
            to: hop.to.clone(),
            from_uri: self.uri.clone(),
            to_uri: hop.to_uri.clone(),
            via: hop.via,
            field: hop.field.clone(),
            held_by: hop.held_by.clone(),
            held_by_uri: hop.held_by_uri.clone(),
        });
        let mut key = self.key.clone();
        key.push(hop.key.clone());
        Candidate {
            uuid: hop.to.clone(),
            uri: hop.to_uri.clone(),
            steps,
            key,
        }
    }
}

#[derive(Default)]
struct Links {
    hops: Vec<Hop>,
    notes: Vec<String>,
}

struct Association {
    index: u64,
    collection: String,
    collection_uri: String,
    /// Set when §8.3 says this one is not to be taken into account.
    repeated_of: Option<u64>,
}

pub(crate) enum Part {
    Collection,
    Member(u64),
}

/// `SingleCollectionAssociation[i].Collection` or `….Dataobject[j]`.
pub(crate) fn parse_association(path: &str) -> Option<(u64, Part)> {
    const HEAD: &str = "SingleCollectionAssociation[";
    let at = path.rfind(HEAD)?;
    if at != 0 && !path[..at].ends_with('.') {
        return None;
    }
    let rest = &path[at + HEAD.len()..];
    let close = rest.find(']')?;
    let i: u64 = rest[..close].parse().ok()?;
    let tail = rest[close + 1..].strip_prefix('.')?;
    if tail == "Collection" {
        return Some((i, Part::Collection));
    }
    let tail = tail.strip_prefix("Dataobject[")?;
    let close = tail.find(']')?;
    let j: u64 = tail[..close].parse().ok()?;
    (close + 1 == tail.len()).then_some((i, Part::Member(j)))
}

/// The first two integers a path carries — its position in the document.
fn numbers(path: &str) -> (u64, u64) {
    let mut found = Vec::new();
    let mut digits = String::new();
    for c in path.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_digit() {
            digits.push(c);
        } else if !digits.is_empty() {
            found.push(digits.parse().unwrap_or(u64::MAX));
            digits.clear();
        }
    }
    (
        found.first().copied().unwrap_or(0),
        found.get(1).copied().unwrap_or(0),
    )
}

fn lower(uuid: &str) -> String {
    uuid.to_ascii_lowercase()
}

/// Where the parents of a node come from.
enum Climb<'k> {
    /// The index: the three parent fields.
    Index,
    /// A property: its kind, then the kinds above it. The property is stage 0.
    Kinds(&'k [String]),
}

// ---------------------------------------------------------------------------
// The resumable walk
// ---------------------------------------------------------------------------

/// The objects the order reaches at one place of it: one rank, one distance.
///
/// Sorted by document order, so the first that carries the information wins and
/// the others that do are its ties.
#[derive(Clone, Debug)]
pub struct Tier {
    /// 1 to 6, see the module documentation.
    pub rank: u8,
    /// The objects of this tier, in document order.
    pub candidates: Vec<Candidate>,
    /// What the walk had skipped, and said, when it reached this tier.
    pub notes: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Subjects,
    Collections,
    Up,
    Models,
    Done,
}

/// The breadth-first reading of the collections of one stage.
#[derive(Clone, Debug)]
struct Collections {
    rank: u8,
    visited: HashSet<String>,
    level: Vec<Candidate>,
    edges: Vec<(String, String)>,
}

/// Where a walk stopped: everything [`Walker::advance`] needs to go on from
/// there, **owned**, so that it outlives the walker that began it.
///
/// A walker borrows the package index; a state does not, which is what lets a
/// session keep the walk of an object between two questions and resume it with a
/// new walker over the (possibly updated) index.
#[derive(Clone, Debug)]
pub struct WalkState {
    start: String,
    with_model: bool,
    phase: Phase,
    depth: usize,
    stage: Vec<Candidate>,
    all: Vec<Candidate>,
    seen: HashSet<String>,
    collections: Option<Collections>,
    models: Vec<Candidate>,
    models_built: bool,
    models_next: usize,
    notes: Vec<String>,
    touched: HashSet<String>,
}

impl WalkState {
    /// The beginning of the walk of `start` — an **identity**: the uri of the
    /// object ([`Walker::identity`] makes one of any address). `with_model`:
    /// whether rank 6 is part of it (an object: yes; a property: no, §2.2).
    pub fn new(start: &str, with_model: bool) -> WalkState {
        WalkState {
            start: start.to_string(),
            with_model,
            phase: Phase::Subjects,
            depth: 0,
            stage: vec![Candidate::start(start)],
            all: Vec::new(),
            seen: HashSet::from([lower(start)]),
            collections: None,
            models: Vec::new(),
            models_built: false,
            models_next: 0,
            notes: Vec::new(),
            touched: HashSet::new(),
        }
    }

    /// Everything said so far: cycles, duplicates, what was not followed.
    pub fn notes(&self) -> &[String] {
        &self.notes
    }

    /// Whether the order is exhausted: [`Walker::advance`] has nothing more.
    pub fn is_done(&self) -> bool {
        self.phase == Phase::Done
    }

    /// **Every object the walk looked at**, by lowercase identity (the uri): the ones it
    /// reached, whose collections and parents it read, and the models it
    /// considered. A change to one of these — or to an object that now
    /// references one — can change what the walk finds beyond where it stopped,
    /// or what it found; a change to anything else cannot. This is what a
    /// cache of walks is invalidated by.
    pub fn touched(&self) -> &HashSet<String> {
        &self.touched
    }
}

// ---------------------------------------------------------------------------
// The walker
// ---------------------------------------------------------------------------

/// One package, one predicate, and everything the walks share.
///
/// Build it once per population and call [`Walker::resolve`] for each object:
/// the memberships, the `Kind` of each collection and the predicate's answers
/// are remembered.
///
/// **A walk goes by identity, and the identity is the uri.** The start is an
/// address — a uri, or a bare uuid at the entry — that [`Walker::identity`]
/// turns into the uri of the node the index holds for it; every step after it
/// is a node of the index, reached by its uri. Two versions of one object are
/// two objects to the walk (ADR-0003: internally, full uris only).
pub struct Walker<'a, I> {
    index: &'a PackageIndex,
    has: &'a dyn Fn(&str) -> Option<I>,
    carried: RefCell<HashMap<String, Option<I>>>,
    links: RefCell<HashMap<String, Rc<Links>>>,
    associations: RefCell<HashMap<String, Rc<Vec<Association>>>>,
    parents: RefCell<HashMap<String, Rc<Vec<Hop>>>>,
}

/// **The identity a walk of this address goes by**: the uri of the node the
/// index holds for it — the address itself when it is a uri it names, the
/// uuid's default node for a bare uuid — or the address, trimmed, when the
/// package holds no such node (an object known elsewhere can still carry
/// information). The one place a bare uuid becomes an identity.
pub fn identity_of(index: &PackageIndex, address: &str) -> String {
    match index.uri_of(address) {
        Some(uri) => uri.to_string(),
        None => address.trim().to_string(),
    }
}

impl<'a, I: Clone> Walker<'a, I> {
    /// A walker over `index`. `has` answers *"does this object carry I?"* for an
    /// identity — **the uri** of the object, or the address itself for one the
    /// index does not hold; it is called **once** per object, whatever the
    /// number of walks.
    ///
    /// `_objects` is not read any more: it was opened for the `Kind` of a
    /// collection, which the index now records while it reads the collection
    /// ([`PackageIndex::is_project`]). The parameter stays so that no caller
    /// changes.
    pub fn new(
        index: &'a PackageIndex,
        _objects: &'a dyn ObjectSource,
        has: &'a dyn Fn(&str) -> Option<I>,
    ) -> Self {
        Self {
            index,
            has,
            carried: RefCell::default(),
            links: RefCell::default(),
            associations: RefCell::default(),
            parents: RefCell::default(),
        }
    }

    /// The identity a walk of `address` goes by — see [`identity_of`].
    pub fn identity(&self, address: &str) -> String {
        identity_of(self.index, address)
    }

    /// What the object inherits, by the order of the module. `address`: the uri
    /// of the object (a bare uuid is adapted, see [`Walker::identity`]).
    pub fn resolve(&self, address: &str) -> Walked<I> {
        self.run(&self.identity(address), &Climb::Index, true)
    }

    /// [`Walker::resolve`], and **every object the walk looked at** (lowercase
    /// identities, [`WalkState::touched`]): what a cache of the answer is dropped by.
    pub fn resolve_traced(&self, address: &str) -> (Walked<I>, HashSet<String>) {
        let mut state = WalkState::new(&self.identity(address), true);
        let walked = self.run_state(&mut state, &Climb::Index);
        (walked, state.touched)
    }

    /// [`Walker::resolve`] for a population, sharing what is common.
    pub fn resolve_all<S: AsRef<str>>(&self, addresses: &[S]) -> Vec<Walked<I>> {
        addresses
            .iter()
            .map(|address| self.resolve(address.as_ref()))
            .collect()
    }

    /// What a property inherits: itself, its kind, the kind's parents, a
    /// collection at each step, and nothing beyond (§2.2). `kinds` is the kind,
    /// then its parents — [`kind_chain`] reads it.
    pub fn resolve_property(&self, property: &str, kinds: &[String]) -> Walked<I> {
        let kinds: Vec<String> = kinds.iter().map(|kind| self.identity(kind)).collect();
        self.run(&self.identity(property), &Climb::Kinds(&kinds), false)
    }

    // -- memoised readings ---------------------------------------------------

    fn carried(&self, identity: &str) -> Option<I> {
        let key = lower(identity);
        if let Some(known) = self.carried.borrow().get(&key) {
            return known.clone();
        }
        let answer = (self.has)(identity);
        self.carried.borrow_mut().insert(key, answer.clone());
        answer
    }

    /// A `project` is told apart by the `Kind` of the collection; the index took
    /// it while it held the object.
    fn is_project(&self, identity: &str) -> bool {
        self.index.is_project(identity)
    }

    fn name(&self, identity: &str) -> String {
        let uuid = uuid_of_identity(identity);
        match self.index.node(identity).and_then(|node| node.title.clone()) {
            Some(title) if !title.is_empty() => format!("{title} ({uuid})"),
            _ => uuid,
        }
    }

    /// The collections of one association set, in association order, with the
    /// §8.3 rule applied.
    fn associations_of(&self, holder: &str) -> Rc<Vec<Association>> {
        let key = lower(holder);
        if let Some(known) = self.associations.borrow().get(&key) {
            return known.clone();
        }
        // The rows were laid with the set's references, §8.3 already decided.
        let list: Vec<Association> = self
            .index
            .associations_of(holder)
            .into_iter()
            .map(|row| Association {
                index: row.index,
                collection: row.collection.to_string(),
                collection_uri: row.collection_uri.to_string(),
                repeated_of: row.repeated_of,
            })
            .collect();
        let list = Rc::new(list);
        self.associations.borrow_mut().insert(key, list.clone());
        list
    }

    /// The collections an object is a member of, one hop.
    fn links_of(&self, identity: &str) -> Rc<Links> {
        let key = lower(identity);
        if let Some(known) = self.links.borrow().get(&key) {
            return known.clone();
        }
        let uuid = uuid_of_identity(identity);
        let mut out = Links::default();
        // (holder, association) → lowest member position: §8.5, once.
        let mut members: HashMap<(String, u64), (u64, u64)> = HashMap::new();
        let mut order: Vec<(String, u64)> = Vec::new();
        for membership in self.index.memberships_of(identity) {
            let (i, j) = (membership.association, membership.position);
            let slot = (membership.holder_uri.to_string(), i);
            match members.get_mut(&slot) {
                Some((lowest, count)) => {
                    *count += 1;
                    *lowest = (*lowest).min(j);
                }
                None => {
                    members.insert(slot.clone(), (j, 1));
                    order.push(slot);
                }
            }
        }
        let mut best: HashMap<String, Hop> = HashMap::new();
        for (holder, i) in order {
            let (j, count) = members[&(holder.clone(), i)];
            let holder_uuid = uuid_of_identity(&holder);
            if count > 1 {
                out.notes.push(format!(
                    "`{uuid}` is listed {count} times in association {i} of `{holder_uuid}`: counted once \
                     (common TRG 2.3 §8.5)"
                ));
            }
            let associations = self.associations_of(&holder);
            let Some(association) = associations.iter().find(|a| a.index == i) else {
                out.notes.push(format!(
                    "association {i} of `{holder_uuid}` lists `{uuid}` and names no collection: not a membership"
                ));
                continue;
            };
            if let Some(earlier) = association.repeated_of {
                out.notes.push(format!(
                    "collection `{}` is named again by association {i} of `{holder_uuid}` after association \
                     {earlier}: the repeat is ignored (common TRG 2.3 §8.3)",
                    association.collection
                ));
                continue;
            }
            let hop = Hop {
                to: association.collection.clone(),
                to_uri: association.collection_uri.clone(),
                via: Via::Collection,
                field: format!("SingleCollectionAssociation[{i}].Dataobject[{j}]"),
                held_by: Some(holder_uuid.clone()),
                held_by_uri: Some(holder.clone()),
                key: (
                    i,
                    j,
                    format!("{}/{}", lower(&holder), lower(&association.collection_uri)),
                ),
            };
            match best.get(&lower(&hop.to_uri)) {
                Some(kept) if kept.key <= hop.key => {}
                _ => {
                    best.insert(lower(&hop.to_uri), hop);
                }
            }
        }
        out.hops = best.into_values().collect();
        out.hops.sort_by(|a, b| a.key.cmp(&b.key));
        let out = Rc::new(out);
        self.links.borrow_mut().insert(key, out.clone());
        out
    }

    /// The parents of an object, one hop, in the order of the three fields.
    fn parents_of(&self, identity: &str) -> Rc<Vec<Hop>> {
        let key = lower(identity);
        if let Some(known) = self.parents.borrow().get(&key) {
            return known.clone();
        }
        let mut hops: Vec<Hop> = self
            .index
            .parent_rows(identity)
            .into_iter()
            .map(|(at, parent)| Hop {
                key: (at as u64, 0, lower(&parent.uri)),
                to: parent.uuid.to_string(),
                to_uri: parent.uri.to_string(),
                via: Via::Parent,
                field: PARENT_FIELDS[at].to_string(),
                held_by: None,
                held_by_uri: None,
            })
            .collect();
        hops.sort_by(|a, b| a.key.cmp(&b.key));
        let hops = Rc::new(hops);
        self.parents.borrow_mut().insert(key, hops.clone());
        hops
    }

    fn climb(&self, climb: &Climb<'_>, start: &str, identity: &str) -> Vec<Hop> {
        match climb {
            Climb::Index => self.parents_of(identity).as_ref().clone(),
            Climb::Kinds(kinds) => {
                let at = if identity.eq_ignore_ascii_case(start) {
                    0
                } else {
                    match kinds.iter().position(|k| k.eq_ignore_ascii_case(identity)) {
                        Some(p) => p + 1,
                        None => return Vec::new(),
                    }
                };
                match kinds.get(at) {
                    Some(kind) => vec![Hop {
                        to: uuid_of_identity(kind),
                        to_uri: kind.clone(),
                        via: Via::Parent,
                        field: if at == 0 { "PropertyKind" } else { "Parent" }.to_string(),
                        held_by: None,
                        held_by_uri: None,
                        key: (0, 0, lower(kind)),
                    }],
                    None => Vec::new(),
                }
            }
        }
    }

    // -- the walk --------------------------------------------------------------

    /// What the object inherits, for one piece of information: the first tier
    /// that holds a carrier answers.
    fn run(&self, start: &str, climb: &Climb<'_>, with_model: bool) -> Walked<I> {
        let mut state = WalkState::new(start, with_model);
        self.run_state(&mut state, climb)
    }

    fn run_state(&self, state: &mut WalkState, climb: &Climb<'_>) -> Walked<I> {
        while let Some(tier) = self.advance_with(state, climb) {
            let hits: Vec<(&Candidate, I)> = tier
                .candidates
                .iter()
                .filter_map(|node| self.carried(&node.uri).map(|value| (node, value)))
                .collect();
            if let Some(found) = Self::decide(hits, tier.rank, &tier.notes) {
                return found;
            }
        }
        Walked {
            resolution: None,
            notes: state.notes.clone(),
        }
    }

    /// The next [`Tier`] of the walk of an object — **the walk, resumable**.
    ///
    /// The order of the module is a sequence of tiers that does not depend on
    /// what is looked for: the object (rank 1), the collections that hold it,
    /// level by level (2, 2 bis), its parents (3), theirs (4, 4 bis), and so on
    /// up (5), the model (6). What depends on the question is where the walk
    /// *stops*. A caller that asks for several things of one object — the five
    /// targets of a colour — keeps the state and the tiers it has read, looks
    /// each question up in them, and calls this again **only** for the
    /// questions still unanswered: the tiers already read are not walked twice
    /// (ADR-0009).
    ///
    /// `None` when the order is exhausted; the notes the walk ends with (a rank 7
    /// information that was not followed) are then in [`WalkState::notes`].
    pub fn advance(&self, state: &mut WalkState) -> Option<Tier> {
        self.advance_with(state, &Climb::Index)
    }

    fn advance_with(&self, st: &mut WalkState, climb: &Climb<'_>) -> Option<Tier> {
        loop {
            match st.phase {
                Phase::Subjects => {
                    if st.depth >= MAX_STAGES || st.stage.is_empty() {
                        st.phase = if st.with_model { Phase::Models } else { Phase::Done };
                        continue;
                    }
                    st.stage.sort_by(|a, b| a.key.cmp(&b.key));
                    for node in &st.stage {
                        st.touched.insert(lower(&node.uri));
                    }
                    let (rank_self, rank_collection) = match st.depth {
                        0 => (1, 2),
                        1 => (3, 4),
                        _ => (5, 5),
                    };
                    st.collections = Some(Collections {
                        rank: rank_collection,
                        visited: st.stage.iter().map(|n| lower(&n.uri)).collect(),
                        level: st.stage.clone(),
                        edges: Vec::new(),
                    });
                    st.phase = Phase::Collections;
                    return Some(Tier {
                        rank: rank_self,
                        candidates: st.stage.clone(),
                        notes: st.notes.clone(),
                    });
                }
                Phase::Collections => {
                    let mut coll = st.collections.take().expect("a collections phase has its state");
                    if let Some(tier) = self.collections_level(&mut coll, st) {
                        st.collections = Some(coll);
                        return Some(tier);
                    }
                    st.phase = Phase::Up;
                }
                Phase::Up => {
                    st.all.extend(st.stage.iter().cloned());
                    let mut up: Vec<Candidate> = Vec::new();
                    for node in &st.stage {
                        for hop in self.climb(climb, &st.start, &node.uri) {
                            let target = lower(&hop.to_uri);
                            let on_the_way = target == lower(&st.start)
                                || node.steps.iter().any(|s| lower(&s.to_uri) == target);
                            if on_the_way {
                                push_note(
                                    &mut st.notes,
                                    format!(
                                        "cycle: `{}` is its own ancestor (`{}` → `{}` by `{}`); the chain stops here",
                                        self.name(&hop.to_uri),
                                        self.name(&node.uri),
                                        self.name(&hop.to_uri),
                                        hop.field
                                    ),
                                );
                                continue;
                            }
                            if st.seen.insert(target) {
                                up.push(node.then(&hop));
                            }
                        }
                    }
                    st.stage = up;
                    st.depth += 1;
                    st.phase = Phase::Subjects;
                }
                Phase::Models => {
                    if !st.models_built {
                        st.models = self.model_candidates(&st.all);
                        st.models_built = true;
                        for model in &st.models {
                            st.touched.insert(lower(&model.uri));
                        }
                    }
                    if st.models_next >= st.models.len() {
                        self.model_collections_not_followed(st);
                        st.phase = Phase::Done;
                        continue;
                    }
                    let distance = st.models[st.models_next].steps.len();
                    let mut group = Vec::new();
                    while st.models_next < st.models.len()
                        && st.models[st.models_next].steps.len() == distance
                    {
                        group.push(st.models[st.models_next].clone());
                        st.models_next += 1;
                    }
                    return Some(Tier {
                        rank: 6,
                        candidates: group,
                        notes: st.notes.clone(),
                    });
                }
                Phase::Done => return None,
            }
        }
    }

    /// The memoised answer of the predicate for one object — what a caller
    /// that drives [`Walker::advance`] reads for each candidate of a tier, by
    /// its **uri** ([`Candidate::uri`]).
    pub fn carried_of(&self, identity: &str) -> Option<I> {
        self.carried(identity)
    }

    /// The best of the hits at one distance: the first in key order wins, the
    /// others are its ties.
    fn decide(mut hits: Vec<(&Candidate, I)>, rank: u8, notes: &[String]) -> Option<Walked<I>> {
        if hits.is_empty() {
            return None;
        }
        hits.sort_by(|a, b| a.0.key.cmp(&b.0.key));
        let mut iter = hits.into_iter();
        let (winner, value) = iter.next()?;
        let ties = iter
            .map(|(node, value)| Tie {
                carrier: node.uuid.clone(),
                carrier_uri: node.uri.clone(),
                value,
                path: node.steps.clone(),
            })
            .collect();
        let resolution = Resolution {
            value,
            rank,
            carrier: winner.uuid.clone(),
            carrier_uri: winner.uri.clone(),
            path: winner.steps.clone(),
            ties,
            notes: notes.to_vec(),
        };
        Some(Walked {
            notes: notes.to_vec(),
            resolution: Some(resolution),
        })
    }

    /// Ranks 2, 4, 4 bis, 5, one level at a time: the collections of the
    /// subjects, then of those collections. `None` when there is no level left.
    ///
    /// A level whose every collection is a `project` is not a tier (§2.1: the
    /// collections of the model are not followed, and said); the next level is
    /// looked for from it all the same.
    fn collections_level(&self, coll: &mut Collections, st: &mut WalkState) -> Option<Tier> {
        loop {
            let mut next: Vec<Candidate> = Vec::new();
            for node in &coll.level {
                st.touched.insert(lower(&node.uri));
                let links = self.links_of(&node.uri);
                for note in &links.notes {
                    push_note(&mut st.notes, note.clone());
                }
                for hop in &links.hops {
                    coll.edges.push((node.uri.clone(), hop.to_uri.clone()));
                    if coll.visited.insert(lower(&hop.to_uri)) {
                        next.push(node.then(hop));
                    }
                }
            }
            self.name_cycles(&coll.edges, &mut st.notes);
            if next.is_empty() {
                return None;
            }

            let mut kept: Vec<Candidate> = Vec::new();
            for candidate in next {
                st.touched.insert(lower(&candidate.uri));
                if self.is_project(&candidate.uri) {
                    // §2.1: a project is the model's collection — not followed, said.
                    if self.carried(&candidate.uri).is_some() {
                        push_note(
                            &mut st.notes,
                            format!(
                                "information on the collection {} (project): the collections of the model \
                                 are not followed",
                                self.name(&candidate.uri)
                            ),
                        );
                    }
                    continue;
                }
                kept.push(candidate);
            }
            coll.level = kept.clone();
            if kept.is_empty() {
                continue;
            }
            kept.sort_by(|a, b| a.key.cmp(&b.key));
            return Some(Tier {
                rank: coll.rank,
                candidates: kept,
                notes: st.notes.clone(),
            });
        }
    }

    /// A cycle in the memberships seen so far, named once.
    fn name_cycles(&self, edges: &[(String, String)], notes: &mut Vec<String>) {
        let mut graph: HashMap<&str, Vec<&str>> = HashMap::new();
        for (from, to) in edges {
            graph.entry(from).or_default().push(to);
        }
        // 0 = on the stack, 1 = done.
        let mut state: HashMap<&str, u8> = HashMap::new();
        let mut stack: Vec<&str> = Vec::new();
        let mut cycles: Vec<Vec<String>> = Vec::new();
        let mut roots: Vec<&str> = graph.keys().copied().collect();
        // By uuid first, as the walks have always named a cycle from its lowest
        // uuid; the uri breaks the tie between two versions.
        roots.sort_by_cached_key(|root| (lower(&uuid_of_identity(root)), root.to_string()));
        for root in roots {
            Self::visit(root, &graph, &mut state, &mut stack, &mut cycles);
        }
        for cycle in cycles {
            let named: Vec<String> = cycle.iter().map(|uuid| self.name(uuid)).collect();
            push_note(
                notes,
                format!(
                    "cycle in the collections: {} → {}; the walk does not follow the edge that closes it",
                    named.join(" → "),
                    named[0]
                ),
            );
        }
    }

    fn visit<'g>(
        node: &'g str,
        graph: &HashMap<&'g str, Vec<&'g str>>,
        state: &mut HashMap<&'g str, u8>,
        stack: &mut Vec<&'g str>,
        cycles: &mut Vec<Vec<String>>,
    ) {
        match state.get(node) {
            Some(_) => return,
            None => {
                state.insert(node, 0);
            }
        }
        stack.push(node);
        for &next in graph.get(node).map(|v| v.as_slice()).unwrap_or_default() {
            match state.get(next) {
                Some(0) => {
                    let at = stack.iter().position(|n| *n == next).unwrap_or(0);
                    cycles.push(stack[at..].iter().map(|s| s.to_string()).collect());
                }
                Some(_) => {}
                None => Self::visit(next, graph, state, stack, cycles),
            }
        }
        stack.pop();
        state.insert(node, 1);
    }

    /// Rank 6: the models that reference the object or one of its parents, by
    /// distance then document order.
    fn model_candidates(&self, subjects: &[Candidate]) -> Vec<Candidate> {
        let mut candidates: HashMap<String, Candidate> = HashMap::new();
        let mut offer = |node: Candidate| {
            let key = lower(&node.uri);
            match candidates.get(&key) {
                Some(kept) if (kept.steps.len(), &kept.key) <= (node.steps.len(), &node.key) => {}
                _ => {
                    candidates.insert(key, node);
                }
            }
        };
        for subject in subjects {
            for model in self.index.models_referencing(&subject.uri) {
                let kind = model.type_key;
                let (a, b) = numbers(model.path);
                let hop = Hop {
                    to: model.uuid.to_string(),
                    to_uri: model.uri.to_string(),
                    via: Via::Model,
                    field: model.path.to_string(),
                    held_by: None,
                    held_by_uri: None,
                    // By uuid first, as the order has always been; the uri tells
                    // two versions apart.
                    key: (a, b, format!("{}|{}", lower(model.uuid), lower(model.uri))),
                };
                let model_side = subject.then(&hop);
                // The `Model` the earth model interpretation interprets.
                if kind == "EarthModelInterpretation" {
                    for parent in self.parents_of(&model_side.uri).iter() {
                        let model = self
                            .index
                            .node(&parent.to_uri)
                            .is_some_and(|node| &*node.type_key == "Model");
                        if model {
                            let mut hop = parent.clone();
                            hop.via = Via::Model;
                            offer(model_side.then(&hop));
                        }
                    }
                }
                offer(model_side);
            }
        }
        let mut ordered: Vec<Candidate> = candidates.into_values().collect();
        ordered.sort_by(|a, b| (a.steps.len(), &a.key).cmp(&(b.steps.len(), &b.key)));
        ordered
    }

    /// Rank 7, left aside on purpose (§2.1): said when it would have answered —
    /// at the end of the walk, so that it is said only of a walk that found
    /// nothing else.
    fn model_collections_not_followed(&self, st: &mut WalkState) {
        let models = st.models.clone();
        for model in &models {
            let mut visited: HashSet<String> = HashSet::from([lower(&model.uri)]);
            let mut frontier = vec![model.uri.clone()];
            while !frontier.is_empty() {
                let mut next = Vec::new();
                for identity in &frontier {
                    st.touched.insert(lower(identity));
                    for hop in &self.links_of(identity).hops {
                        if !visited.insert(lower(&hop.to_uri)) {
                            continue;
                        }
                        st.touched.insert(lower(&hop.to_uri));
                        if self.carried(&hop.to_uri).is_some() {
                            push_note(
                                &mut st.notes,
                                format!(
                                    "information on the collection {} of the model {}: the collections of the \
                                     model are not followed",
                                    self.name(&hop.to_uri),
                                    self.name(&model.uri)
                                ),
                            );
                        }
                        next.push(hop.to_uri.clone());
                    }
                }
                frontier = next;
            }
        }
    }
}

fn push_note(notes: &mut Vec<String>, note: String) {
    if !notes.contains(&note) {
        notes.push(note);
    }
}

/// [`Walker::resolve`] for one object, by its address (the uri; a bare uuid is
/// adapted at the entry).
pub fn resolve<I: Clone>(
    index: &PackageIndex,
    objects: &dyn ObjectSource,
    address: &str,
    has: &dyn Fn(&str) -> Option<I>,
) -> Walked<I> {
    Walker::new(index, objects, has).resolve(address)
}

/// [`Walker::resolve_all`]: one walker, so what is common is read once.
pub fn resolve_all<I: Clone, S: AsRef<str>>(
    index: &PackageIndex,
    objects: &dyn ObjectSource,
    addresses: &[S],
    has: &dyn Fn(&str) -> Option<I>,
) -> Vec<Walked<I>> {
    Walker::new(index, objects, has).resolve_all(addresses)
}
