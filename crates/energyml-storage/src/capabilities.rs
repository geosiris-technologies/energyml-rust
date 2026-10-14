//! What a store can do, said before it is asked.
//!
//! An ETP server announces its protocols and capabilities when a session opens.
//! A store that refuses sub-arrays or transactions must be able to say so then,
//! or the server announces a protocol it then refuses message by message. Each
//! refusal keeps its name ([`crate::Error::Unsupported`]); a capability only
//! says it in advance.

use crate::raw::RawFormat;

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
    /// The store states the kind of each edge (primary, secondary).
    pub edge_kinds: bool,
    /// The store answers a context natively rather than by walking every edge
    /// ([`crate::EnergymlStorage::resources_in`]).
    pub native_context: bool,
    /// The store reports what changed since a revision
    /// ([`crate::changes::EnergymlChanges`]).
    pub changes: bool,
}
