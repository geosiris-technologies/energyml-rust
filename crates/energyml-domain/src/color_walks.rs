//! The colour of an object **per target**, walked once and resumed on demand
//! (ADR-0009).
//!
//! The rule: the colour inheritance is computed only once; each sub-part is then
//! looked up in the complete result, and the walk goes on deeper through the
//! fallbacks, on demand, only when it lacks the information for a given cell
//! type. A polyline needs no face or volume colours (except for surfaces of one
//! colour, whose edges are drawn with the face colour to optimise the three.js
//! scene). The question must be settled without recomputing everything each
//! time: going deeper after a hit only when the next type asked for was not
//! found in the one used for a previous cell type. And when an object is
//! modified (its `graphicalInformation`, say), its graph must be updated when
//! linked objects are added (collections included), and the related caches (here
//! the colour resolutions) updated with it, searching only the objects linked to
//! the modified object so as to do only what is necessary.
//!
//! # The pieces
//!
//! - [`drawn_by`] — **the one table** from the kind of representation to the
//!   targets its geometry draws. A polyline never asks for a face or a volume.
//! - [`ColourCache`] — one [`ColourWalk`] per object: the tiers of the
//!   inheritance order already read ([`crate::inherit::Walker::advance`]), what
//!   each carries **for all five targets**, the answer found for each target,
//!   and the walk's state, so that it can go on. Asking a target an earlier
//!   target's tier already answers reads nothing; asking one it does not goes on
//!   from where the walk stopped, **only** for the targets still unanswered.
//! - [`ColourCache::disturb`] — the invalidation: which walks to throw away
//!   when objects change. Only those that went through one of them.
//!
//! # The fallback rule, per target
//!
//! *The closest tier wins; within a tier, the exact target before the whole
//! object* (decision to be validated, ADR-0009). The
//! order of ranks and distances is the one the design fixes for **any**
//! information (§2: on a conflict, the shortest path wins); a colour for the faces is one more piece of information, so a
//! whole-object colour on the object itself (rank 1) beats a faces colour on its
//! collection (rank 2) — and the faces colour of a tier beats the whole-object
//! colour of the **same** tier, because there the object has said which one is
//! meant. Note that the fallback never crosses targets: nodes do not fall back
//! on faces.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};

use crate::color::{Carried, Cascade, Decision, GraphicalSet, Level, Scope};
use crate::color_inherit::level_of;
use crate::inherit::{Resolution, Step, Tie, WalkState, Walked, Walker, identity_of};
use energyml_core::identity;
use crate::package_index::PackageIndex;
use crate::source::ObjectSource;

// ---------------------------------------------------------------------------
// What a geometry draws
// ---------------------------------------------------------------------------

/// The targets the geometry of one kind of representation draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Drawn {
    /// The targets worth asking: a polyline has nodes and edges, never faces.
    /// Edges are **not** here for a surface — see [`Drawn::edges_follow_faces`].
    pub scopes: &'static [Scope],
    /// The edges are drawn **with the colour of the faces** (the wireframe of a
    /// surface or of a grid's skin): the front end's three.js scene merges them for
    /// speed. This merging is accepted as an optimisation; it is flagged here and
    /// the optimised behaviour is kept.
    pub edges_follow_faces: bool,
}

const NO_TARGET: Drawn = Drawn { scopes: &[], edges_follow_faces: false };

/// **The one table**: representation kind → the targets its geometry draws.
///
/// By the concept's name (`PolylineSetRepresentation`, with or without `obj_`),
/// like `energyml_geometry`'s registry of readers, whose list this follows —
/// the geometry crate sits above this one, so the table is written here and a
/// test in that crate holds the two lists together. A kind the table does not
/// know draws no target of its own: it is asked for the whole object only, which
/// is what every other kind of object (a feature, a collection) is.
///
/// | kind | asked | why |
/// |---|---|---|
/// | point set | nodes | a cloud |
/// | polyline, polyline set, trajectory, frame, WITSML trajectory | nodes, edges | a line and its vertices |
/// | triangulated set, 2D grid, plane set | nodes, faces — edges follow the faces | a surface and its mesh |
/// | IJK grid, unstructured grid | nodes, faces, volumes — edges follow the faces | a grid is drawn by its skin; the cells are its volumes |
pub fn drawn_by(type_key: &str) -> Drawn {
    let name = type_key.strip_prefix("obj_").unwrap_or(type_key);
    match name {
        "PointSetRepresentation" => Drawn { scopes: &[Scope::Nodes], edges_follow_faces: false },
        "PolylineRepresentation"
        | "PolylineSetRepresentation"
        | "WellboreTrajectoryRepresentation"
        | "WellboreFrameRepresentation"
        | "Trajectory" => Drawn { scopes: &[Scope::Nodes, Scope::Edges], edges_follow_faces: false },
        "TriangulatedSetRepresentation" | "Grid2dRepresentation" | "PlaneSetRepresentation" => {
            Drawn { scopes: &[Scope::Nodes, Scope::Faces], edges_follow_faces: true }
        }
        "IjkGridRepresentation" | "UnstructuredGridRepresentation" => {
            Drawn { scopes: &[Scope::Nodes, Scope::Faces, Scope::Volumes], edges_follow_faces: true }
        }
        _ => NO_TARGET,
    }
}

// ---------------------------------------------------------------------------
// One answer
// ---------------------------------------------------------------------------

/// The answer for one target of one object.
#[derive(Clone, Debug, PartialEq)]
pub struct Scoped {
    /// The target asked.
    pub scope: Scope,
    /// What the walk found for it, with its path, its ties and its notes — the
    /// same shape [`Cascade::inherited`] answers the whole object with. `None`
    /// for the resolution: neither the target nor the whole object is carried
    /// anywhere the order reaches.
    pub walked: Walked<Decision>,
    /// `true`: the carrier gives **this** target. `false`: it gives the whole
    /// object, which is what stands for a target nobody distinguished.
    pub exact: bool,
    /// Set when this target is not looked up on its own: its colour is the one
    /// of the target named here ([`Drawn::edges_follow_faces`]).
    pub merged_into: Option<Scope>,
    /// When the file gave this target **another** colour than the one it is
    /// drawn with, which one and where — said, never silently overridden.
    pub merged_note: Option<String>,
}

impl Scoped {
    /// The decision, when there is one.
    pub fn decision(&self) -> Option<&Decision> {
        self.walked.resolution.as_ref().map(|r| &r.value)
    }
}

/// One object to resolve, and the targets to resolve it for.
#[derive(Clone, Copy, Debug)]
pub struct Ask<'a> {
    /// The **uri** of the object — its full identity, the version it declares
    /// included: two versions of one object are two walks. A bare uuid is the
    /// library's entry adapter, turned into the uri of the index's node for it
    /// ([`crate::inherit::identity_of`]) before anything is keyed.
    pub uri: &'a str,
    /// The targets, in the order the answers are wanted.
    pub scopes: &'a [Scope],
}

/// What the cache did, for a test or a gauge to read: **work done**, not time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Walks begun: an object asked that the cache did not hold.
    pub walks_begun: u64,
    /// Walks thrown away by [`ColourCache::disturb`] (or by a change of rules).
    pub walks_dropped: u64,
    /// Tiers read from the order — one more step of a walk.
    pub tiers_read: u64,
    /// Objects whose graphical information was read (all five targets at once).
    pub objects_read: u64,
}

// ---------------------------------------------------------------------------
// One walk
// ---------------------------------------------------------------------------

const SWITCHED_OFF: &str = "inheritance is switched off, so the collections, the parents and the model were not \
                            looked at";

struct Member {
    uuid: String,
    uri: String,
    path: Vec<Step>,
    carried: Carried,
}

struct ReadTier {
    rank: u8,
    notes: Vec<String>,
    /// The candidates of the tier that carry something, in document order.
    members: Vec<Member>,
}

/// What a walk needs from the call that drives it.
struct Env<'e> {
    walker: &'e Walker<'e, Carried>,
    skips: &'e RefCell<HashMap<String, Vec<String>>>,
    index: &'e PackageIndex,
    inherit: bool,
}

/// The walk of one object: tiers read so far, answers found, state to go on.
struct ColourWalk {
    state: WalkState,
    tiers: Vec<ReadTier>,
    /// Why some objects on the way carried nothing (a viewer that is not ours).
    skipped: HashMap<String, Vec<String>>,
    /// Per target: the first tier not yet looked at for it.
    cursor: [usize; 5],
    answers: [Option<Scoped>; 5],
    /// The order is exhausted (or inheritance is off): nothing more to read.
    done: bool,
    final_notes: Vec<String>,
}

impl ColourWalk {
    fn new(identity: &str) -> ColourWalk {
        ColourWalk {
            state: WalkState::new(identity, true),
            tiers: Vec::new(),
            skipped: HashMap::new(),
            cursor: [0; 5],
            answers: Default::default(),
            done: false,
            final_notes: Vec::new(),
        }
    }

    /// Read one more tier of the order, or find that there is none.
    fn read_next(&mut self, env: &Env<'_>, stats: &mut Stats) {
        if !env.inherit && !self.tiers.is_empty() {
            self.done = true;
            self.final_notes = vec![SWITCHED_OFF.to_string()];
            return;
        }
        match env.walker.advance(&mut self.state) {
            Some(tier) => {
                stats.tiers_read += 1;
                let mut members = Vec::new();
                for candidate in tier.candidates {
                    let key = candidate.uri.to_ascii_lowercase();
                    if let Some(carried) = env.walker.carried_of(&candidate.uri) {
                        members.push(Member {
                            uuid: candidate.uuid.clone(),
                            uri: candidate.uri.clone(),
                            path: candidate.steps.clone(),
                            carried,
                        });
                    }
                    if let Some(why) = env.skips.borrow().get(&key) {
                        self.skipped.insert(key, why.clone());
                    }
                }
                self.tiers.push(ReadTier { rank: tier.rank, notes: tier.notes, members });
            }
            None => {
                self.done = true;
                self.final_notes = self.state.notes().to_vec();
            }
        }
    }

    /// The members of one tier that answer for `scope`: those that give the
    /// target itself, else — for any target but the whole object — those that
    /// give the whole object. The flag says which.
    fn pick(tier: &ReadTier, scope: Scope) -> Option<(bool, Vec<&Member>)> {
        let exact: Vec<&Member> = tier.members.iter().filter(|m| m.carried.exact(scope).is_some()).collect();
        if !exact.is_empty() {
            return Some((true, exact));
        }
        if scope == Scope::WholeObject {
            return None;
        }
        let whole: Vec<&Member> =
            tier.members.iter().filter(|m| m.carried.exact(Scope::WholeObject).is_some()).collect();
        (!whole.is_empty()).then_some((false, whole))
    }

    fn decision_of(index: &PackageIndex, member: &Member, scope: Scope, exact: bool) -> Decision {
        let source = if exact { scope } else { Scope::WholeObject };
        let mut decision = member.carried.exact(source).cloned().expect("a picked member carries what it was picked for");
        decision.level = level_of(index, false, &member.uri, &member.path);
        if !exact {
            decision
                .notes
                .push(format!("nothing for {scope} on {}; used what it says for the whole object", member.uuid));
        }
        decision
    }

    fn found(&self, index: &PackageIndex, scope: Scope, tier: usize, exact: bool, hits: &[&Member]) -> Scoped {
        let read = &self.tiers[tier];
        let winner = hits[0];
        let value = Self::decision_of(index, winner, scope, exact);
        let ties = hits[1..]
            .iter()
            .map(|member| Tie {
                carrier: member.uuid.clone(),
                carrier_uri: member.uri.clone(),
                value: Self::decision_of(index, member, scope, exact),
                path: member.path.clone(),
            })
            .collect();
        // What was skipped on the way: the objects the answer went through
        // that carried nothing because their entries were for another viewer.
        let mut notes = read.notes.clone();
        let on_the_way = std::iter::once(winner.uri.clone())
            .chain(winner.path.iter().flat_map(|step| [step.from_uri.clone(), step.to_uri.clone()]));
        for identity in on_the_way {
            if let Some(why) = self.skipped.get(&identity.to_ascii_lowercase()) {
                for note in why {
                    if !notes.contains(note) {
                        notes.push(note.clone());
                    }
                }
            }
        }
        let resolution = Resolution {
            value,
            rank: read.rank,
            carrier: winner.uuid.clone(),
            carrier_uri: winner.uri.clone(),
            path: winner.path.clone(),
            ties,
            notes: notes.clone(),
        };
        Scoped { scope, walked: Walked { notes, resolution: Some(resolution) }, exact, merged_into: None, merged_note: None }
    }

    /// What the tiers **already read** answer for `scope`, going no further.
    fn peek(&self, index: &PackageIndex, scope: Scope) -> Option<Scoped> {
        if let Some(known) = &self.answers[scope.index()] {
            return known.resolution_found().then(|| known.clone());
        }
        for (at, tier) in self.tiers.iter().enumerate() {
            if let Some((exact, hits)) = Self::pick(tier, scope) {
                return Some(self.found(index, scope, at, exact, &hits));
            }
        }
        None
    }

    /// The answer for `scope`: from the tiers already read when they hold it,
    /// otherwise by reading further — and only as far as this target needs.
    fn answer(&mut self, env: &Env<'_>, stats: &mut Stats, scope: Scope) -> Scoped {
        let slot = scope.index();
        if let Some(known) = &self.answers[slot] {
            return known.clone();
        }
        loop {
            while self.cursor[slot] < self.tiers.len() {
                let at = self.cursor[slot];
                if let Some((exact, hits)) = Self::pick(&self.tiers[at], scope) {
                    let scoped = self.found(env.index, scope, at, exact, &hits);
                    self.answers[slot] = Some(scoped.clone());
                    return scoped;
                }
                self.cursor[slot] += 1;
            }
            if self.done {
                let scoped = Scoped {
                    scope,
                    walked: Walked { resolution: None, notes: self.final_notes.clone() },
                    exact: true,
                    merged_into: None,
                    merged_note: None,
                };
                self.answers[slot] = Some(scoped.clone());
                return scoped;
            }
            self.read_next(env, stats);
        }
    }
}

impl Scoped {
    fn resolution_found(&self) -> bool {
        self.walked.resolution.is_some()
    }
}

// ---------------------------------------------------------------------------
// The cache
// ---------------------------------------------------------------------------

/// The colour walks of a package, kept between questions.
///
/// **What is cached** is a walk: the tiers of the inheritance order read for an
/// object, with what each carries for the five targets, and where the walk
/// stopped. **What it depends on** is every object the walk looked at
/// ([`WalkState::touched`]) — and nothing else. A change to the package says
/// which objects it touched ([`PackageIndex::update_with`] returns them;
/// [`changed_targets`] names the ones whose graphical information moved), and
/// [`ColourCache::disturb`] throws away the walks that looked at one of them:
/// the others are exact still.
#[derive(Default)]
pub struct ColourCache {
    /// Lowercase identity (the uri) of the start → its walk.
    walks: HashMap<String, ColourWalk>,
    /// Lowercase identity (the uri) → the walks (by start) that looked at it.
    touching: HashMap<String, HashSet<String>>,
    /// Lowercase uuid → the identities of `touching` that carry it: what a
    /// disturbance named by a bare uuid or a short uri reaches (it names every
    /// version that does not contradict it).
    named: HashMap<String, HashSet<String>>,
    viewer_kind: Option<String>,
    inherit: bool,
    configured: bool,
    stats: Stats,
}

impl ColourCache {
    /// An empty cache.
    pub fn new() -> ColourCache {
        ColourCache::default()
    }

    /// What was done so far.
    pub fn stats(&self) -> Stats {
        self.stats
    }

    /// How many walks are held.
    pub fn len(&self) -> usize {
        self.walks.len()
    }

    /// Whether no walk is held.
    pub fn is_empty(&self) -> bool {
        self.walks.is_empty()
    }

    /// Whether the walk of this object is held — `address` is its uri; a bare
    /// uuid (the adapter) asks for **any** version of it.
    pub fn holds(&self, address: &str) -> bool {
        let (uuid, version) = identity::parse_address(address);
        let version = version.map(|version| version.to_ascii_lowercase());
        self.walks.keys().any(|start| {
            let (held, held_version) = identity::parse_address(start);
            held == uuid && identity::versions_agree(version.as_deref(), held_version.as_deref())
        })
    }

    /// Forget everything: the rules changed, or the package was replaced.
    pub fn clear(&mut self) {
        self.stats.walks_dropped += self.walks.len() as u64;
        self.walks.clear();
        self.touching.clear();
        self.named.clear();
    }

    /// Throw away **the walks that looked at one of these objects** and nothing
    /// else. Returns how many.
    ///
    /// Each name is an address — a uri, which names that version of the object
    /// and none other; a bare uuid or a short uri (what the index's updates
    /// still report) names **every version that does not contradict it**, which
    /// is wider than needed, never narrower.
    ///
    /// *How the set is found*: the cache keeps, for each object a walk looked at,
    /// the walks that looked at it (`touching`), so this is a lookup per
    /// disturbed object, not a scan of the walks. What a walk looked at is
    /// everything it reached, read the links of, or considered as a model — see
    /// [`WalkState::touched`] — which includes the objects *above* the answer
    /// (where it stopped is part of what it read) and not the ones beyond.
    pub fn disturb(&mut self, addresses: &[String]) -> usize {
        let mut doomed: HashSet<String> = HashSet::new();
        for address in addresses {
            let (uuid, version) = identity::parse_address(address);
            let Some(identities) = self.named.get(&uuid) else { continue };
            for identity_key in identities {
                let (_, touched_version) = identity::parse_address(identity_key);
                let agrees = identity::versions_agree(
                    version.as_deref().map(str::to_ascii_lowercase).as_deref(),
                    touched_version.as_deref(),
                );
                if agrees && let Some(starts) = self.touching.get(identity_key) {
                    doomed.extend(starts.iter().cloned());
                }
            }
        }
        for start in &doomed {
            self.drop_walk(start);
        }
        doomed.len()
    }

    fn drop_walk(&mut self, start: &str) {
        let Some(walk) = self.walks.remove(start) else {
            return;
        };
        self.stats.walks_dropped += 1;
        for identity_key in walk.state.touched() {
            if let Some(starts) = self.touching.get_mut(identity_key) {
                starts.remove(start);
                if starts.is_empty() {
                    self.touching.remove(identity_key);
                    let (uuid, _) = identity::parse_address(identity_key);
                    if let Some(held) = self.named.get_mut(&uuid) {
                        held.remove(identity_key);
                        if held.is_empty() {
                            self.named.remove(&uuid);
                        }
                    }
                }
            }
        }
    }

    /// The walks are valid for **these rules**: a cascade that answers
    /// differently (another viewer, inheritance off) empties the cache.
    fn configure(&mut self, cascade: &Cascade<'_>) {
        if self.configured && self.viewer_kind == cascade.viewer_kind && self.inherit == cascade.inherit {
            return;
        }
        self.clear();
        self.viewer_kind = cascade.viewer_kind.clone();
        self.inherit = cascade.inherit;
        self.configured = true;
    }

    /// Resolve each object for each target asked, **one walker for the batch**.
    ///
    /// An object the cache holds answers from what it read; a target it cannot
    /// answer resumes the walk from where it stopped. A collection that two
    /// thousand wells share is read once (`Walker` memoises it for the batch,
    /// and the cache keeps what each walk read of it).
    pub fn resolve(
        &mut self,
        cascade: &Cascade<'_>,
        index: &PackageIndex,
        objects: &dyn ObjectSource,
        asks: &[Ask<'_>],
    ) -> Vec<Vec<Scoped>> {
        self.configure(cascade);
        let reads = Cell::new(0u64);
        let skips: RefCell<HashMap<String, Vec<String>>> = RefCell::default();
        let has = |identity: &str| -> Option<Carried> {
            reads.set(reads.get() + 1);
            let carried = cascade.default_information_all(identity, Level::Representation);
            if !carried.skipped.is_empty() {
                skips.borrow_mut().insert(identity.to_ascii_lowercase(), carried.skipped.clone());
            }
            carried.any().then_some(carried)
        };
        let walker = Walker::new(index, objects, &has);
        let env = Env { walker: &walker, skips: &skips, index, inherit: cascade.inherit };

        let mut out = Vec::with_capacity(asks.len());
        for ask in asks {
            // The identity of the walk: the uri of the node the index holds for
            // the address — the one place a bare uuid becomes one.
            let identity = walker.identity(ask.uri);
            let key = identity.to_ascii_lowercase();
            let fresh = !self.walks.contains_key(&key);
            if fresh {
                self.stats.walks_begun += 1;
                self.walks.insert(key.clone(), ColourWalk::new(&identity));
            }
            let walk = self.walks.get_mut(&key).expect("just inserted");
            let before = walk.tiers.len();
            let answers: Vec<Scoped> = ask.scopes.iter().map(|scope| walk.answer(&env, &mut self.stats, *scope)).collect();
            if fresh || walk.tiers.len() != before {
                let touched: Vec<String> = walk.state.touched().iter().cloned().collect();
                for touched_identity in touched {
                    let (uuid, _) = identity::parse_address(&touched_identity);
                    self.named.entry(uuid).or_default().insert(touched_identity.clone());
                    self.touching.entry(touched_identity).or_default().insert(key.clone());
                }
            }
            out.push(answers);
        }
        self.stats.objects_read += reads.get();
        out
    }

    /// What the tiers **already read** for an object answer for a target — no
    /// walk is resumed. `None` when they do not hold an answer (or the object
    /// is not held): *"not known yet"*, which is not *"nothing"*.
    pub fn peek(&self, index: &PackageIndex, address: &str, scope: Scope) -> Option<Scoped> {
        let key = identity_of(index, address).to_ascii_lowercase();
        self.walks.get(&key)?.peek(index, scope)
    }
}

/// The objects whose graphical information **differs** between two readings of
/// the package's sets — the targets of every entry that is not the same at the
/// same position, before or after, **as addresses** (the uris the entries name,
/// the uuid for one that names no type), sorted — what
/// [`ColourCache::disturb`] takes.
///
/// Positions count, not only content: the sets are laid end to end
/// ([`GraphicalSet::merged`]) and an entry's position is what
/// `ActiveColorInformationIndex` and the front end's editor address, so a set that
/// grows **shifts** every later entry, and a decision that carries the old
/// position is stale — rightly disturbed.
pub fn changed_targets(old: &GraphicalSet, new: &GraphicalSet) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for position in 0..old.entries.len().max(new.entries.len()) {
        let (before, after) = (old.entries.get(position), new.entries.get(position));
        if before == after {
            continue;
        }
        for entry in before.into_iter().chain(after) {
            out.extend(entry.addresses().iter().cloned());
        }
    }
    out.sort();
    out.dedup();
    out
}
