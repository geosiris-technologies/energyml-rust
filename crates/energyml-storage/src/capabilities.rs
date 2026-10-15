//! What a store can do, said before it is asked.
//!
//! An ETP server announces its protocols and capabilities when a session opens.
//! A store that refuses sub-arrays or transactions must be able to say so then,
//! or the server announces a protocol it then refuses message by message. Each
//! refusal keeps its name ([`crate::Error::Unsupported`]); a capability only
//! says it in advance.

use crate::raw::RawFormat;

/// Of which edges a store states the kind (primary or secondary).
///
/// Meaningful when [`Capabilities::edge_kinds`] is on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum EdgeKindCoverage {
    /// Of every edge: a server states it for each one it answers.
    #[default]
    All,
    /// Of the edges whose source belongs to these MLs, by qualified-type prefix
    /// (`resqml22`, `witsml21`), sorted. An edge of another ML has no kind and a
    /// request by kind that meets it is refused by name
    /// ([`crate::Error::UnstatedKind`]).
    Mls(Vec<String>),
}

impl EdgeKindCoverage {
    /// Whether the kind of the edges held by objects of this ML prefix is stated.
    pub fn covers(&self, prefix: &str) -> bool {
        match self {
            EdgeKindCoverage::All => true,
            EdgeKindCoverage::Mls(prefixes) => prefixes.iter().any(|p| p == prefix),
        }
    }
}

/// What a store offers beyond reading objects and arrays.
///
/// `Default` is "nothing beyond": every flag off, every list empty - the
/// answer of a store that has not said. Built with [`Capabilities::default`] and
/// its fields set; `#[non_exhaustive]`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Capabilities {
    /// The store writes objects and arrays ([`crate::write::EnergymlWriter`]).
    pub writes: bool,
    /// The store opens transactions (ETP protocol 18).
    pub transactions: bool,
    /// The store holds several dataspaces, and creates and deletes them
    /// ([`crate::dataspaces::EnergymlDataspaces`]).
    pub several_dataspaces: bool,
    /// The store knows containment (a WITSML container and its parts) and
    /// honours `prune_contained`. Off: `prune_contained = true` is refused by
    /// name.
    pub contained_objects: bool,
    /// The store writes part of an array (`PutDataSubarrays`).
    pub subarrays: bool,
    /// The store declares an array before writing it (`PutUninitializedDataArrays`).
    pub uninitialized_arrays: bool,
    /// The store remembers deletions ([`crate::EnergymlStorage::deleted_resources`]),
    /// for this many seconds (ETP `ChangeRetentionPeriod`). `None`: it does not.
    pub deletions_kept_for: Option<u64>,
    /// The formats [`crate::EnergymlStorage::get_raw`] answers in. Empty: none.
    pub raw_formats: Vec<RawFormat>,
    /// The store keeps the times ETP reports (`storeCreated`, `storeLastWrite`).
    pub store_times: bool,
    /// The store states the kind (primary, secondary) of some of its edges at
    /// least; [`Capabilities::edge_kind_coverage`] says which.
    pub edge_kinds: bool,
    /// Of which edges `edge_kinds` holds. An ETP server: all. An EPC: the MLs
    /// the rules of [`crate::kinds`] cover.
    pub edge_kind_coverage: EdgeKindCoverage,
    /// The store answers a context natively rather than by walking every edge
    /// ([`crate::EnergymlStorage::resources_in`]).
    pub native_context: bool,
    /// The store reports what changed since a revision
    /// ([`crate::changes::EnergymlChanges`]).
    pub changes: bool,
}
