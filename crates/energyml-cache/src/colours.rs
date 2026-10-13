//! The colour of an object, **one rule for every face** — the walks that
//! a WebAssembly front end used to keep for itself (the inheritance-resolution order), now held by the [`Cache`].
//!
//! All the deciding is `energyml_domain::color_inherit` (the order) over
//! `energyml_domain::inherit` (the walk) and `energyml_domain::color_walks` (the
//! walk of one object, its targets, its cache); this module asks them with the
//! package index. Nothing here knows a rank.
//!
//! Before the cache existed the front end answered by those walks and the command line, the
//! scene's colour ramps and `well_strata` by `Cascade::resolve` — a shorter
//! order that ignores the collections and the model: **two colour rules
//! depending on the face** (ADR-0009). The command line now asks
//! [`Cache::decision`] and [`of_package`], the front end asks the same.
//!
//! # What is kept
//!
//! A [`ColourSlot`], beside the package index: the walk of each object asked,
//! and the merged `GraphicalInformationSet`. Both are exact **for the index and
//! the store** because every question first brings the index up to date and
//! then drops what the update disturbed ([`IndexSlot::take_disturbed`]) — the
//! walks that looked at an object that changed, or that gained or lost a
//! reference, and the walks of the objects an entry of the set decorated
//! differently. With the layer off the slot is a fresh one, dropped after the
//! call.

use std::collections::{HashMap, HashSet};

use energyml_domain::color::{Cascade, Decision, GraphicalSet, Paint, Request, Scope};
use energyml_domain::color_inherit::Ladder;
use energyml_domain::color_walks::{ColourCache, Stats, changed_targets};
use energyml_domain::package_index::PackageIndex;
use energyml_domain::property::{self, Property};
use energyml_domain::property_kind::Resolver;
use energyml_storage::{EpcStore, ObjectSummary};

use crate::index::{Consumer, Disturbed};
use crate::{Cache, Error};

/// One row asked for: the object (its full uri), its kind chain when it is a
/// property, the property when it is.
///
/// **Build it with [`Row::at`]** from the object's name: `at` is the one place
/// that turns a name into the full identity the walk beneath
/// (`energyml_domain::color_walks`) is keyed by, refusing a name that cannot
/// say which of two versions is meant.
pub struct Row<'a> {
    /// The object's full uri, the key the walk beneath works in.
    pub key: String,
    /// For a property, its kind then the kind's parents, nearest first.
    pub kinds: Vec<String>,
    /// The property asked about, if the row is one.
    pub property: Option<&'a str>,
}

impl<'a> Row<'a> {
    /// The row of the object `name` names: a full uri (the form to pass), a
    /// short uri or a bare uuid (adapters, refused by name when the package
    /// holds the object in two versions and the name cannot say which).
    /// A name the package does not hold is walked as given: a cascade may ask
    /// about a target that lives elsewhere.
    pub fn at(store: &EpcStore, name: &str, kinds: Vec<String>, property: Option<&'a str>) -> Result<Self, Error> {
        Ok(Row { key: walk_key(store, name)?, kinds, property })
    }
}

/// The full uri the walks are keyed by, of the object `name` names, through
/// the store's one door ([`EpcStore::resolve`]): the version a uri names is the
/// object, a name that fits two is [`energyml_storage::Error::Ambiguous`].
/// `name` itself (lowercased) when the package holds no such object: a cascade
/// may ask about a target that lives elsewhere.
fn walk_key(store: &EpcStore, name: &str) -> Result<String, Error> {
    match store.resolve(name.trim()) {
        Ok(summary) => Ok(summary.uri.clone()),
        Err(energyml_storage::Error::NotFound(_)) => Ok(name.trim().to_ascii_lowercase()),
        Err(other) => Err(other.into()),
    }
}

/// What a cache keeps of its colours: the walks, and the set they read.
#[derive(Default)]
pub(crate) struct ColourSlot {
    cache: ColourCache,
    /// The package's `GraphicalInformationSet`s read as one — `None` when it has
    /// none, once `read` is true.
    set: Option<GraphicalSet>,
    /// The uuids of the sets that were read.
    sets: HashSet<String>,
    read: bool,
}

impl ColourSlot {
    pub(crate) fn len(&self) -> usize {
        self.cache.len() + usize::from(self.read && self.set.is_some())
    }
}

/// The switches of the cascade a caller may turn: the same two that
/// [`Cascade`] has, and the same defaults.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// Whether to walk up past the object itself. `true` is the decided
    /// default; off, only what the object carries.
    pub inherit: bool,
    /// Only entries for this viewer kind, plus those that name none.
    pub viewer_kind: Option<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self { inherit: true, viewer_kind: Some("3d".to_string()) }
    }
}

/// Every `GraphicalInformationSet` of the package, read, with its (lowercase)
/// uuid — what `energyml_domain::well_strata` asks all of.
pub fn graphical_sets(store: &EpcStore) -> Result<Vec<(String, GraphicalSet)>, Error> {
    let mut sets = Vec::new();
    for summary in store.resources() {
        if !summary.qualified_type.ends_with("GraphicalInformationSet") {
            continue;
        }
        let object = store.get_object(&summary.uri)?;
        sets.push((summary.uuid.to_ascii_lowercase(), GraphicalSet::read(object.as_reflect())));
    }
    Ok(sets)
}

/// The package's sets read as **one** (`GraphicalSet::merged`, in package
/// order — the first alone hid, on Lutetia, the chart's colours behind the
/// strata table's set), or `None` when it has none.
pub fn graphical_set(store: &EpcStore) -> Result<Option<GraphicalSet>, Error> {
    let sets = graphical_sets(store)?;
    Ok((!sets.is_empty()).then(|| GraphicalSet::merged(sets.into_iter().map(|(_, set)| set))))
}

/// A property's kind then the kind's parents, nearest first — because a colour
/// set on `length` applies to every kind derived from it, and that inheritance
/// is most of the point of colouring by kind at all. Empty when the property
/// states no kind or the kind does not resolve.
pub fn kind_chain(store: &EpcStore, read: &Property<'_>) -> Vec<String> {
    let Some(reference) = read.kind.as_ref() else {
        return Vec::new();
    };
    match Resolver::new(store).resolve(reference) {
        Ok(resolution) => resolution.chain.into_iter().map(|kind| kind.uuid).collect(),
        Err(_) => Vec::new(),
    }
}

/// [`kind_chain`] of the property `name` names: a full uri (the version it
/// names is the property read), or a bare uuid as an adapter. Empty when the
/// store holds no such property; an error when the name fits two versions.
pub fn kind_chain_at(store: &EpcStore, name: &str) -> Result<Vec<String>, Error> {
    let summary = match store.resolve(name.trim()) {
        Ok(summary) => summary,
        Err(energyml_storage::Error::NotFound(_)) => return Ok(Vec::new()),
        Err(other) => return Err(other.into()),
    };
    Ok(store
        .get_object(&summary.uri)
        .ok()
        .and_then(|object| property::read(object.as_reflect()).ok().map(|read| kind_chain(store, &read)))
        .unwrap_or_default())
}

/// [`kind_chain_at`] for a caller that cannot refuse: **empty** for a name that
/// fits two versions. Kept for the callers that still hold a bare uuid.
pub fn kind_chain_of(store: &EpcStore, property_uuid: &str) -> Vec<String> {
    kind_chain_at(store, property_uuid).unwrap_or_default()
}

/// Bring the slot up to date with what the index's updates disturbed.
///
/// The walks that looked at a disturbed object go; the set is read again when
/// one of the objects is a `GraphicalInformationSet` (or was), and the walks of
/// the objects whose entries changed go with it.
fn settle(store: &EpcStore, slot: &mut ColourSlot, index: &PackageIndex, dirty: Disturbed) -> Result<(), Error> {
    let ColourSlot { cache, set, sets, read } = slot;
    let touches_a_set = dirty.everything
        || dirty.uuids.iter().any(|uuid| {
            sets.contains(uuid)
                || index.node(uuid).is_some_and(|node| node.type_key.ends_with("GraphicalInformationSet"))
        });
    if dirty.everything {
        cache.clear();
    } else {
        cache.disturb(&dirty.uuids);
    }
    if !*read || touches_a_set {
        let found = graphical_sets(store)?;
        let uuids: HashSet<String> = found.iter().map(|(uuid, _)| uuid.clone()).collect();
        let fresh = if found.is_empty() { None } else { Some(GraphicalSet::merged(found.into_iter().map(|(_, one)| one))) };
        if *read && !dirty.everything {
            let nothing = GraphicalSet::default();
            let changed = changed_targets(set.as_ref().unwrap_or(&nothing), fresh.as_ref().unwrap_or(&nothing));
            cache.disturb(&changed);
        }
        *set = fresh;
        *sets = uuids;
        *read = true;
    }
    Ok(())
}

/// [`Cache::decision`] over an index the caller already holds — what a loop
/// over many objects asks, so that a cache that keeps nothing builds its index
/// once for the loop and not once per object.
fn decide_in(
    index: &PackageIndex,
    store: &EpcStore,
    set: &GraphicalSet,
    representation: &str,
    property: Option<&str>,
    chain: &[String],
    options: &Options,
) -> Result<Option<Decision>, Error> {
    let mut cascade = Cascade::new(set);
    cascade.inherit = options.inherit;
    cascade.viewer_kind = options.viewer_kind.clone();
    let walked = match property {
        Some(property) => cascade.inherited_property(index, store, property, chain),
        None => cascade.inherited(index, store, representation),
    }?;
    Ok(walked.resolution.map(|resolution| {
        let mut decision = resolution.value;
        for note in resolution.notes {
            if !decision.notes.contains(&note) {
                decision.notes.push(note);
            }
        }
        decision
    }))
}

impl Cache {
    /// The ladders of `rows`, in order, **from the walks the cache keeps**: one
    /// walker for all of them, the walk of an object done once, a target the
    /// tiers already read does not answer resuming it. `None` when the package
    /// holds no `GraphicalInformationSet` — the caller answers `null` per row.
    pub fn ladders(&self, store: &EpcStore, rows: &[Row<'_>]) -> Result<Option<Vec<Ladder>>, Error> {
        let mut fresh = ColourSlot::default();
        let mut held = self.colours.try_borrow_mut();
        // A question asked while another is being answered finds the slot held:
        // it answers from a slot of its own rather than panic (as
        // `IndexSlot::with`). Only the real slot reads the index's account of
        // what was disturbed — a throwaway one must not eat it.
        let (slot, kept): (&mut ColourSlot, bool) = match held.as_deref_mut() {
            Ok(slot) if self.layers().colours => (slot, true),
            _ => (&mut fresh, false),
        };
        self.with_index(store, |index| -> Result<Option<Vec<Ladder>>, Error> {
            let dirty = if kept { self.index.take_disturbed(Consumer::Colours) } else { Disturbed::default() };
            settle(store, slot, index, dirty)?;
            let ColourSlot { cache, set, .. } = slot;
            let Some(set) = set.as_ref() else {
                return Ok(None);
            };
            let cascade = Cascade::new(set);
            let requests: Vec<Request<'_>> = rows
                .iter()
                .map(|row| Request {
                    representation: &row.key,
                    property: row.property,
                    kind_chain: &row.kinds,
                    element: Scope::WholeObject,
                })
                .collect();
            Ok(Some(cascade.ladders_cached(cache, index, store, &requests)?))
        })
    }

    /// The colour the **file** gives each object `names` name, as the 3D view's
    /// ladder decides it (a constant colour, `#rrggbbaa`, or `None` when no
    /// rung answers or the answer is a map), **keyed by the object's full uri**
    /// (the name itself for an object the package does not hold). One walker
    /// for the lot, from the walks the cache keeps: what the section view asks
    /// instead of walking again.
    ///
    /// A name that fits two versions is refused by name. What each version
    /// *answers* is bounded by the walk beneath, which compares uuids: the
    /// answers are two, one per uri, and they agree until the domain's walk is
    /// keyed by identity too.
    pub fn file_colours_at(&self, store: &EpcStore, names: &[String]) -> Result<HashMap<String, Option<String>>, Error> {
        let mut keys = Vec::with_capacity(names.len());
        let mut rows: Vec<Row<'_>> = Vec::with_capacity(names.len());
        for name in names {
            let key = match store.resolve(name.trim()) {
                Ok(summary) => summary.uri.clone(),
                Err(energyml_storage::Error::NotFound(_)) => name.trim().to_string(),
                Err(other) => return Err(other.into()),
            };
            rows.push(Row::at(store, name, Vec::new(), None)?);
            keys.push(key);
        }
        let Some(ladders) = self.ladders(store, &rows)? else {
            return Ok(keys.into_iter().map(|key| (key, None)).collect());
        };
        Ok(keys
            .into_iter()
            .zip(ladders)
            .map(|(key, ladder)| {
                let colour = match ladder.walked.resolution.map(|found| found.value) {
                    Some(Decision { paint: Paint::Constant(colour), .. }) => Some(colour.to_hex()),
                    _ => None,
                };
                (key, colour)
            })
            .collect())
    }

    /// [`Cache::file_colours_at`] for the callers that hold a bare uuid: the
    /// answer is keyed by the **lowercase uuid** asked. A uuid the package
    /// holds in two versions is refused by name; ask by uri.
    pub fn file_colours(&self, store: &EpcStore, uuids: &[String]) -> Result<HashMap<String, Option<String>>, Error> {
        let by_uri = self.file_colours_at(store, uuids)?;
        let mut out = HashMap::with_capacity(uuids.len());
        for uuid in uuids {
            let key = match store.resolve(uuid.trim()) {
                Ok(summary) => summary.uri.clone(),
                Err(_) => uuid.trim().to_string(),
            };
            out.insert(uuid.to_ascii_lowercase(), by_uri.get(&key).cloned().flatten());
        }
        Ok(out)
    }

    /// **The colour rule, once**: what one object — or one property of it —
    /// inherits, by the inheritance-resolution order: the object, its
    /// collections, its parents and theirs, then the model; for a property, its
    /// kind and the kind's parents and nothing beyond. The answer carries its
    /// level; the notes of the walk join the decision's. `None` means no level
    /// answered — an answer, not a failure: the standard defines no default
    /// colour.
    ///
    /// `representation` is a **uuid** — an entry's target is one, and the walk
    /// up to the interpretation and the feature yields them; passing the uri
    /// (which the front end once did) meant a colour set on the
    /// representation itself was never found. A caller that holds the uri asks
    /// [`Cache::decision_at`], which turns it into that key. `chain` is the property's kind
    /// chain ([`kind_chain`]). `set` is read by the caller, once, for many
    /// questions ([`graphical_set`]).
    pub fn decision(
        &self,
        store: &EpcStore,
        set: &GraphicalSet,
        representation: &str,
        property: Option<&str>,
        chain: &[String],
        options: &Options,
    ) -> Result<Option<Decision>, Error> {
        self.with_index(store, |index| decide_in(index, store, set, representation, property, chain, options))
    }

    /// [`Cache::decision`] for the object `name` names: its **full uri** (a
    /// short uri or a bare uuid are adapters, refused by name when they fit two
    /// versions). The one place a uri becomes the walk's key.
    pub fn decision_at(
        &self,
        store: &EpcStore,
        set: &GraphicalSet,
        name: &str,
        property: Option<&str>,
        chain: &[String],
        options: &Options,
    ) -> Result<Option<Decision>, Error> {
        let key = walk_key(store, name)?;
        self.decision(store, set, &key, property, chain, options)
    }

    /// What the colour walks did so far — walks begun and dropped, tiers read,
    /// objects read. **Work, not time**: what a test asserts to say a walk was
    /// not done twice, and what an invalidation cost.
    pub fn colour_walk_stats(&self) -> Stats {
        self.colours.try_borrow().map(|slot| slot.cache.stats()).unwrap_or_default()
    }

    /// Whether the cache holds the walk of this object — for the tests of the
    /// invalidation, which say what was thrown away and what was kept.
    #[doc(hidden)]
    pub fn colour_walk_held(&self, uuid: &str) -> bool {
        self.colours.try_borrow().map(|slot| slot.cache.holds(uuid)).unwrap_or(false)
    }
}

/// A cache and the store it is the cache of — what the scene asks its colours
/// of (`energyml_geometry::scene::Request::colours`).
pub struct Colouring<'a> {
    /// The cache.
    pub cache: &'a Cache,
    /// The store it answers for.
    pub store: &'a EpcStore,
}

impl energyml_geometry::scene::Decide for Colouring<'_> {
    fn decide(&self, set: &GraphicalSet, request: &Request<'_>) -> Option<Decision> {
        self.cache
            .decision(self.store, set, request.representation, request.property, request.kind_chain, &Options::default())
            .ok()
            .flatten()
    }
}

// ---------------------------------------------------------------------------
// The package, object by object
// ---------------------------------------------------------------------------

/// What [`of_package`] is asked.
#[derive(Clone, Debug, Default)]
pub struct Wanted<'a> {
    /// Only this object, by **uri** (a bare uuid is an adapter, refused by name
    /// when the package holds it in two versions). `None` for every
    /// representation and property.
    pub object: Option<&'a str>,
    /// The cascade's switches.
    pub options: Options,
}

/// One object and the colour the package gives it.
#[derive(Clone, Debug)]
pub struct Coloured {
    /// The object.
    pub summary: ObjectSummary,
    /// What its walk decided; `None` when no level answered.
    pub decision: Option<Decision>,
}

/// The colours of a package, object by object.
#[derive(Clone, Debug, Default)]
pub struct Report {
    /// How many `GraphicalInformationSet`s the package holds. `0` is the
    /// standard, not a gap, for a RESQML 2.0.1 package: that release has no
    /// notion of colour at all.
    pub sets: usize,
    /// What reading the sets said on the way.
    pub notes: Vec<String>,
    /// Every representation and property asked, in package order.
    pub rows: Vec<Coloured>,
    /// How many of them have a colour — the count a test can assert, so that
    /// an empty answer does not pass for a true one.
    pub coloured: usize,
}

/// What every representation and every property of the package inherits, by
/// [`Cache::decision`] — **what `energyml colors` prints**, and what the front end's
/// `colour_of` answers object by object.
///
/// A property is walked through its own chain (its kind and the kind's
/// parents, [`kind_chain`]) with its supporting representation; a property
/// that does not read is left out, as it always was.
pub fn of_package(cache: &Cache, store: &EpcStore, wanted: &Wanted<'_>) -> Result<Report, Error> {
    let sets: Vec<GraphicalSet> = graphical_sets(store)?.into_iter().map(|(_, set)| set).collect();
    if sets.is_empty() {
        return Ok(Report::default());
    }
    let count = sets.len();
    let set = GraphicalSet::merged(sets);
    let notes = set.notes.clone();
    // One door: the object asked is the one the store resolves the name to,
    // compared by its full uri so that one version of a uuid is not the other.
    let wanted_uri: Option<String> = match wanted.object {
        None => None,
        Some(name) => match store.resolve(name.trim()) {
            Ok(summary) => Some(summary.uri.clone()),
            Err(energyml_storage::Error::NotFound(_)) => Some(name.trim().to_string()),
            Err(other) => return Err(other.into()),
        },
    };

    // One index for the whole walk: a cache that keeps nothing builds it once.
    let rows = cache.with_index(store, |index| -> Result<Vec<Coloured>, Error> {
        let mut rows = Vec::new();
        for summary in store.resources() {
            let is_representation = summary.qualified_type.contains("Representation");
            let is_property = summary.qualified_type.ends_with("Property");
            if !is_representation && !is_property {
                continue;
            }
            if let Some(wanted) = &wanted_uri
                && summary.uri != *wanted
            {
                continue;
            }
            let decision = if is_property {
                let Ok(object) = store.get_object(&summary.uri) else {
                    continue;
                };
                let Ok(read) = property::read(object.as_reflect()) else {
                    continue;
                };
                let chain = kind_chain(store, &read);
                // By the uri the property names, so the version it names is the
                // one whose colour it inherits.
                let representation = read
                    .supporting_representation_uri
                    .clone()
                    .or_else(|| read.supporting_representation.clone())
                    .unwrap_or_default();
                decide_in(index, store, &set, &representation, Some(&summary.uri), &chain, &wanted.options)?
            } else {
                decide_in(index, store, &set, &summary.uri, None, &[], &wanted.options)?
            };
            rows.push(Coloured { summary: summary.clone(), decision });
        }
        Ok(rows)
    })?;
    let coloured = rows.iter().filter(|row| row.decision.is_some()).count();
    Ok(Report { sets: count, notes, rows, coloured })
}
