//! The package index of a store, built when first asked for and kept exact by
//! the store's own revision (ADR-0010),
//! moved here from a WebAssembly front end.
//!
//! # What is kept
//!
//! Nothing but the [`IndexSlot`]: the index itself, and a builder while a pass
//! is in progress. No object is held here.
//!
//! # The invalidation rule (ADR-0010)
//!
//! **The store says what changed; a face's own journal plays no part.**
//! [`IndexSlot::with`] asks `store.changed_since(index.revision())` on every
//! question: `Nothing` keeps the index, `Some(uuids)` re-reads those objects
//! (`PackageIndex::update`), `Everything` rebuilds. Ctrl+Z, `create_apply`,
//! `repair_references` and an ETP `put_xml` all write through the overlay, so
//! all of them are covered without this file knowing they exist.
//!
//! The slot is a set of `RefCell`s: a face's methods take `&self` and wasm is
//! single-threaded. [`Cache`](crate::Cache) is what decides whether a slot is
//! used at all.

use std::cell::{Cell, RefCell};

use energyml_domain::package_index::{IndexBuilder, PackageIndex, Progress};
use energyml_storage::EpcStore;
use energyml_storage::epc::revision::Changed;

/// The package index of a store, built when first asked for and kept exact by
/// the store's own revision.
///
/// No borrow is held across a call that could come back in: a
/// question asked **while** another is being answered (a reader that calls
/// another reader) finds the index it is already holding and uses it.
#[derive(Default)]
pub struct IndexSlot {
    index: RefCell<Option<PackageIndex>>,
    /// A pass begun by [`IndexSlot::step`] and not finished: the slices of an incremental pass.
    builder: RefCell<Option<IndexBuilder>>,
    /// See [`IndexSlot::set_partial`].
    partial: Cell<bool>,
    /// What the index's updates have disturbed since each consumer last asked
    /// ([`IndexSlot::take_disturbed`]) — what a cache of derived answers must
    /// drop, see `PackageIndex::update_with`. **One account per consumer**: the
    /// colour walks and the attachment listings each keep answers derived from
    /// the same links, and the one that asked first must not eat what the other
    /// still has to drop.
    disturbed: [RefCell<Disturbed>; 2],
}

/// A cache of answers derived from the index's links: it has its own account of
/// what the updates disturbed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Consumer {
    /// [`crate::colours`]: the colour walks.
    Colours,
    /// [`crate::Cache::attachments_of`]: the listings of what is attached to an object.
    Attachments,
}

/// An account that nobody reads grows with every edit; past this many uuids it
/// is told to throw everything away instead — too wide costs a re-walk, never
/// a stale answer.
const DISTURBED_CAP: usize = 50_000;

/// What the index changed under a cache of what is derived from it.
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct Disturbed {
    /// The index was built (or rebuilt) from scratch: nothing derived from the
    /// previous one is known to hold.
    pub everything: bool,
    /// Lowercase uuids whose derived answers may have changed — the objects that
    /// changed and the ones that gained or lost a reference from them.
    pub uuids: Vec<String>,
}

/// What [`IndexSlot::settle`] decided.
enum Plan {
    Keep,
    Build,
    Update(Vec<String>),
}

impl IndexSlot {
    /// Say that the store is **not the whole package**: an ETP connection holds
    /// what was fetched and nothing else.
    ///
    /// The index is then marked incomplete (`PackageIndex::is_complete` is
    /// `false`: the index of a connection is never `complete`)
    /// — it answers for **what was taken**, and a caller that needs more asks
    /// the server. It does not change what is indexed: the store's inventory is
    /// what it is. Sticky: it describes the store, so a rebuild keeps it, until
    /// `set_partial(false)` is said.
    pub fn set_partial(&self, partial: bool) {
        self.partial.set(partial);
        if partial
            && let Ok(mut slot) = self.index.try_borrow_mut()
            && let Some(index) = slot.as_mut()
        {
            index.mark_incomplete();
        }
    }

    /// Drop the index and any pass in progress; the next question rebuilds.
    pub fn forget(&self) {
        if let Ok(mut slot) = self.index.try_borrow_mut() {
            *slot = None;
        }
        self.note(|dirty| dirty.everything = true);
        if let Ok(mut building) = self.builder.try_borrow_mut() {
            *building = None;
        }
    }

    /// The revision the index is exact for, or `None` when there is none yet.
    pub fn revision(&self) -> Option<u64> {
        self.index.try_borrow().ok()?.as_ref().map(PackageIndex::revision)
    }

    /// How many nodes the index holds; `None` when there is no index.
    pub fn node_count(&self) -> Option<usize> {
        self.index.try_borrow().ok()?.as_ref().map(PackageIndex::len)
    }

    /// Whether the index covers the store's whole inventory; `None` when there
    /// is no index.
    pub fn is_complete(&self) -> Option<bool> {
        self.index
            .try_borrow()
            .ok()?
            .as_ref()
            .map(PackageIndex::is_complete)
    }

    /// What the updates of the index disturbed since this consumer last asked —
    /// and forget it. A cache of answers derived from the links asks this
    /// **inside** [`IndexSlot::with`], once the index is exact for the store, and
    /// drops what it names (`color_walks::ColourCache::disturb`,
    /// `attachments::Cache::disturb`).
    pub fn take_disturbed(&self, consumer: Consumer) -> Disturbed {
        std::mem::take(&mut *self.disturbed[consumer as usize].borrow_mut())
    }

    /// Say something to **every** consumer's account.
    fn note(&self, say: impl Fn(&mut Disturbed)) {
        for account in &self.disturbed {
            let mut account = account.borrow_mut();
            say(&mut account);
            if account.uuids.len() > DISTURBED_CAP {
                account.uuids.sort_unstable();
                account.uuids.dedup();
                if account.uuids.len() > DISTURBED_CAP {
                    account.uuids.clear();
                    account.everything = true;
                }
            }
        }
    }

    /// Run `f` on an index that is exact for `store` — **the invalidation rule
    /// of ADR-0010**.
    ///
    /// No index: build one, in one call. A pass started by
    /// [`IndexSlot::step`] and unfinished: finish it, then check (the store may
    /// have moved since the pass began; the check below re-reads exactly those
    /// objects). Otherwise ask the store what changed since the index's
    /// revision: nothing, these uuids, or everything.
    ///
    /// An ETP store is an `EpcStore` like another here; see
    /// [`IndexSlot::set_partial`] for what it must say about itself.
    pub fn with<R>(&self, store: &EpcStore, f: impl FnOnce(&PackageIndex) -> R) -> R {
        self.settle(store);
        match self.index.try_borrow() {
            Ok(slot) => match slot.as_ref() {
                Some(index) => f(index),
                None => f(&PackageIndex::build(store, store.revision())),
            },
            // Mutably borrowed: only `tamper` does that, and it does not call
            // back. Answer from a fresh index rather than panic.
            Err(_) => f(&PackageIndex::build(store, store.revision())),
        }
    }

    /// Advance the pass by `n` objects, and say how far it is — the
    /// worker loops, yielding between two slices, so a `mesh` or a click is
    /// answered in the middle of a long pass.
    ///
    /// Begins a pass when there is no index, or when the store answers
    /// *everything* changed. When the index exists and only some objects
    /// changed, they are re-read at once (that is object-by-object and cheap)
    /// and the answer is *done*. The index is installed the moment the last
    /// slice is read; a question asked before then finishes the pass itself
    /// ([`IndexSlot::with`]), it never answers from half an index.
    pub fn step(&self, store: &EpcStore, n: usize) -> Progress {
        let Ok(mut building) = self.builder.try_borrow_mut() else {
            return Progress { done: 0, total: 0 };
        };
        if building.is_none() {
            let begin = match self.index.try_borrow() {
                Ok(slot) => match slot.as_ref() {
                    None => true,
                    Some(index) => matches!(store.changed_since(index.revision()), Changed::Everything),
                },
                Err(_) => false,
            };
            if begin {
                *building = Some(IndexBuilder::new(store.resources(), store.revision()));
            } else {
                drop(building);
                self.settle(store);
                let held = self
                    .index
                    .try_borrow()
                    .ok()
                    .and_then(|slot| slot.as_ref().map(PackageIndex::len))
                    .unwrap_or(0);
                return Progress {
                    done: held,
                    total: held,
                };
            }
        }
        let Some(builder) = building.as_mut() else {
            return Progress { done: 0, total: 0 };
        };
        let progress = builder.step(store, n);
        if progress.is_done()
            && let Some(done) = building.take()
        {
            self.install(done.finish());
        }
        progress
    }

    /// **Test sabotage**: change the index in place, so that a test can show its
    /// confrontation with the direct readers *can* fail. Brings the index up to
    /// date first, so the store's revision does not undo the damage on the next
    /// question. `None` when there is no index to damage.
    #[doc(hidden)]
    pub fn tamper<R>(&self, store: &EpcStore, f: impl FnOnce(&mut PackageIndex) -> R) -> Option<R> {
        self.settle(store);
        let mut slot = self.index.try_borrow_mut().ok()?;
        slot.as_mut().map(f)
    }

    fn install(&self, mut index: PackageIndex) {
        // A whole new index: nothing derived from the one before it is known to hold.
        self.note(|dirty| dirty.everything = true);
        if self.partial.get() {
            index.mark_incomplete();
        }
        if let Ok(mut slot) = self.index.try_borrow_mut() {
            *slot = Some(index);
        }
    }

    /// Bring the index to the store's revision, building or updating it.
    fn settle(&self, store: &EpcStore) {
        let unfinished = match self.builder.try_borrow_mut() {
            Ok(mut building) => building.take(),
            Err(_) => None,
        };
        if let Some(mut builder) = unfinished {
            let total = builder.total();
            builder.step(store, total);
            self.install(builder.finish());
        }

        let plan = match self.index.try_borrow() {
            Ok(slot) => match slot.as_ref() {
                None => Plan::Build,
                Some(index) => match store.changed_since(index.revision()) {
                    Changed::Nothing => Plan::Keep,
                    Changed::Some(uuids) => Plan::Update(uuids.into_iter().collect()),
                    Changed::Everything => Plan::Build,
                },
            },
            // Held by a question being answered: it is exact for this call.
            Err(_) => Plan::Keep,
        };
        match plan {
            Plan::Keep => {}
            Plan::Build => self.install(PackageIndex::build(store, store.revision())),
            Plan::Update(uuids) => {
                if let Ok(mut slot) = self.index.try_borrow_mut()
                    && let Some(index) = slot.as_mut()
                {
                    let disturbed = index.update(store, &uuids, store.revision());
                    self.note(|dirty| dirty.uuids.extend(disturbed.iter().cloned()));
                }
            }
        }
    }
}
