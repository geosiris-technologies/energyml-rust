//! What a package keeps of itself, layer by layer.
//!
//! A face — the front end's session, the command line, Python — holds one
//! [`Cache`] per open store. Every layer is optional and lazy: [`Cache::off`]
//! keeps nothing, and a function that can use a cache takes one rather than
//! existing twice, so the fast answer and the slow one are the same code
//! (ADR-0010).
//!
//! This crate holds what a WebAssembly front end used to keep for
//! itself alone:
//!
//! | layer | kept | module |
//! |---|---|---|
//! | `index` | the package's links and property sheets, exact for the store's revision | [`index`] |
//! | `colours` | the colour walks of the objects asked, and the set they read | [`colours`] |
//! | `attachments` | the listings of what is attached to an object | [`Cache::attachments_of`] |
//!
//! # One implementation, a cache that keeps nothing
//!
//! A layer that is off is not a second code path: the same function runs, over
//! a slot that is thrown away when the call returns. A question to
//! [`Cache::off`] therefore builds the index it needs and drops it — slower,
//! never different. `tests/` holds the proof: every answer, cache on and cache
//! off, side by side.
//!
//! The colours and the attachments are **derived from the index** (they read
//! its links, and what its updates disturb is what they drop), so turning
//! either on turns the index on ([`Layers::normalised`]).
//!
//! # What this crate does not hold yet
//!
//! Identity is still the uuid and the uri the store gives (`ObjKey` is not
//! there yet), and there is no dependency registry (`Ledger`). The
//! creation pieces and the section view's work are still held by
//! that front end.
//!
//! The tests of `energyml-cache` are one binary (one crate, one test
//! binary, ADR-0016): a new file is a `mod` line in `tests/main.rs`, or it does
//! not run.

use std::cell::RefCell;

use energyml_domain::attachments::{self, CacheStats, Listing};
use energyml_domain::package_index::{PackageIndex, Progress};
use energyml_storage::EpcStore;

pub mod colours;
pub mod index;

pub use index::{Consumer, Disturbed, IndexSlot};

/// What can go wrong asking a [`Cache`] a question.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The store refused an object.
    #[error("{0}")]
    Store(#[from] energyml_storage::Error),
    /// A domain reader refused.
    #[error("{0}")]
    Domain(#[from] energyml_domain::Error),
}

/// Which layers are on. Every one is off by default and turned on by name.
///
/// ```
/// # use energyml_cache::Layers;
/// let layers = Layers::none().with_index().with_colours();
/// assert!(layers.index && layers.colours && !layers.attachments);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Layers {
    /// The package index.
    pub index: bool,
    /// The colour walks (derived from the index).
    pub colours: bool,
    /// The attachment listings (derived from the index).
    pub attachments: bool,
}

impl Layers {
    /// Nothing on.
    pub const fn none() -> Self {
        Self {
            index: false,
            colours: false,
            attachments: false,
        }
    }

    /// Every layer there is — what a long-lived session wants.
    pub const fn all() -> Self {
        Self {
            index: true,
            colours: true,
            attachments: true,
        }
    }

    /// Turn the index on.
    pub const fn with_index(mut self) -> Self {
        self.index = true;
        self
    }

    /// Turn the colour walks on.
    pub const fn with_colours(mut self) -> Self {
        self.colours = true;
        self
    }

    /// Turn the attachment listings on.
    pub const fn with_attachments(mut self) -> Self {
        self.attachments = true;
        self
    }

    /// The layers with what they stand on turned on too: a layer derived from
    /// the index has no index to be exact for when the index is off.
    pub const fn normalised(mut self) -> Self {
        self.index = self.index || self.colours || self.attachments;
        self
    }
}

/// What a [`Cache`] holds right now — a count, so that a test can say "nothing"
/// without a clock.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Held {
    /// Objects the index holds a node for.
    pub index_nodes: usize,
    /// Colour walks held.
    pub colour_walks: usize,
    /// Attachment listings held.
    pub listings: usize,
}

impl Held {
    /// Whether nothing at all is held.
    pub fn is_empty(&self) -> bool {
        *self == Held::default()
    }
}

/// What a package keeps of itself: one per open store, with the layers it was
/// made with.
///
/// Methods take `&self` — a face's methods do, and wasm is single-threaded —
/// and no borrow is held across a call that could come back in: a question
/// asked while another is being answered finds the slot held and answers from
/// one of its own, as [`IndexSlot::with`] does.
pub struct Cache {
    layers: Layers,
    pub(crate) index: IndexSlot,
    pub(crate) colours: RefCell<colours::ColourSlot>,
    listings: RefCell<attachments::Cache>,
}

impl Cache {
    /// A cache that keeps nothing: every lookup is a branch, every answer is
    /// computed from the store.
    pub fn off() -> Self {
        Self::new(Layers::none())
    }

    /// A cache with these layers on (see [`Layers::normalised`]).
    pub fn new(layers: Layers) -> Self {
        Self {
            layers: layers.normalised(),
            index: IndexSlot::default(),
            colours: RefCell::default(),
            listings: RefCell::default(),
        }
    }

    /// Which layers are on.
    pub fn layers(&self) -> Layers {
        self.layers
    }

    /// What is held now.
    pub fn held(&self) -> Held {
        Held {
            index_nodes: self.index.node_count().unwrap_or(0),
            colour_walks: self.colours.try_borrow().map(|slot| slot.len()).unwrap_or(0),
            listings: self.listings.try_borrow().map(|cache| cache.len()).unwrap_or(0),
        }
    }

    // -- the index ----------------------------------------------------------

    /// Run `f` on an index that is exact for `store` — **the invalidation rule
    /// of ADR-0010** (see [`index`]). With the index layer off, the index is
    /// built for this call and dropped after it.
    pub fn with_index<R>(&self, store: &EpcStore, f: impl FnOnce(&PackageIndex) -> R) -> R {
        if self.layers.index {
            self.index.with(store, f)
        } else {
            f(&PackageIndex::build(store, store.revision()))
        }
    }

    /// Advance the index's build by `n` objects, and say how far it is. A cache
    /// whose index layer is off builds nothing between questions and answers
    /// *nothing to do*.
    pub fn index_step(&self, store: &EpcStore, n: usize) -> Progress {
        if self.layers.index {
            self.index.step(store, n)
        } else {
            Progress { done: 0, total: 0 }
        }
    }

    /// The slot holding the index — for a face's negative controls (`tamper`,
    /// `revision`, `take_disturbed`). It exists whatever the layers; only
    /// [`Cache::with_index`] decides whether it is used.
    #[doc(hidden)]
    pub fn index_slot(&self) -> &IndexSlot {
        &self.index
    }

    /// Say that the store is **not the whole package** — see
    /// [`IndexSlot::set_partial`].
    pub fn set_partial(&self, partial: bool) {
        self.index.set_partial(partial);
    }

    // -- the attachments ----------------------------------------------------

    /// The listing of what is attached to each of `uuids`, in order.
    ///
    /// **Why the listing cache is held from `energyml-domain`, not moved**:
    /// what a listing reads, and what drops it, is the domain's algorithm
    /// (`attachments::Cache` and its own tests); this crate decides only
    /// whether a cache is kept between questions, and which account of the
    /// index's disturbances it reads. With the layer off the cache is a fresh
    /// one, dropped after the call.
    pub fn attachments_of(&self, store: &EpcStore, uuids: &[&str]) -> Vec<Listing> {
        let mut fresh = attachments::Cache::new();
        let mut held = self.listings.try_borrow_mut();
        // A question asked while another is being answered finds the slot held:
        // it answers from a cache of its own rather than panic.
        let (cache, kept): (&mut attachments::Cache, bool) = match held.as_deref_mut() {
            Ok(cache) if self.layers.attachments => (cache, true),
            _ => (&mut fresh, false),
        };
        self.with_index(store, |index| {
            if kept {
                // The index is exact for the store here: drop the listings that
                // read what its update disturbed, and nothing else.
                let dirty = self.index.take_disturbed(Consumer::Attachments);
                if dirty.everything {
                    cache.clear();
                } else {
                    cache.disturb(&dirty.uuids);
                }
            }
            cache.listings(store, store.files_revision(), index, uuids)
        })
    }

    /// What the listing cache did so far — listings built and dropped, files
    /// read. **Work, not time.**
    pub fn attachment_stats(&self) -> CacheStats {
        self.listings
            .try_borrow()
            .map(|cache| cache.stats())
            .unwrap_or_default()
    }

    /// Whether the cache holds the listing of this object.
    #[doc(hidden)]
    pub fn attachment_listing_held(&self, uuid: &str) -> bool {
        self.listings
            .try_borrow()
            .map(|cache| cache.holds(uuid))
            .unwrap_or(false)
    }
}
