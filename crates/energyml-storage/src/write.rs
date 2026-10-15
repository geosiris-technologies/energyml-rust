//! The writing half of the storage interface, shaped after ETP like the reading
//! half.
//!
//! [`EnergymlWriter`] is to `PutDataObjects`, `DeleteDataObjects`, the DataArray
//! writes and the Transaction protocol what [`crate::EnergymlStorage`] is to the
//! reads: plural, partial by nature, addressed by URI. An EPC implements it, so
//! does an ETP dataspace; a server's handlers are written against it.
//!
//! # A writer is one dataspace
//!
//! Nothing here names a dataspace: a writer writes into the one it is. A store
//! holding several hands out one writer per dataspace through
//! [`crate::dataspaces::EnergymlDataspaces`]. That is also the unit a server
//! serialises writes on - one transaction at a time per dataspace.
//!
//! # A transaction is an object
//!
//! [`EnergymlWriter::begin`] returns a [`Transaction`], and committing or
//! rolling back names it. A transaction is not a mode of the store that any
//! caller could end: ending one with a [`Transaction`] that is not the open one
//! is refused by name. ETP allows one transaction per session (ETP 1.2 §18,
//! `StartTransaction`); a writer allows one at a time.
//!
//! **Outside a transaction, each write stands alone**: nothing groups two calls,
//! and a caller that needs them applied together begins a transaction first.
//!
//! **After a refused end**, the two cases differ, after ETP: a refused commit
//! applied nothing and the transaction is **over** (ETP 1.2 §18.1.1.1, step
//! 8.b.ii); a refused rollback leaves it **open** (the specification does not say
//! otherwise; `TODO(verify)` §18.1.1.1, step 6.b). [`EnergymlWriter::open_transaction`]
//! tells which.

use energyml_core::registry::DynObject;

use crate::array::{ArrayData, ArrayLocation, ArrayMetadata};
use crate::error::{Error, Result};
use crate::raw::RawObject;
use crate::storage::EnergymlStorage;

use async_trait::async_trait;

/// What ETP's `StartTransaction` asks for.
///
/// Built with [`TransactionRequest::new`]; `#[non_exhaustive]`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct TransactionRequest {
    /// The dataspaces the transaction covers. Empty: the writer's own.
    pub dataspaces: Vec<String>,
    /// A transaction that only reads (a consistent view), writing nothing.
    pub read_only: bool,
    /// What the transaction is for, as the caller words it.
    pub message: String,
}

impl TransactionRequest {
    /// A writing transaction over the writer's own dataspace.
    pub fn new() -> Self {
        Self::default()
    }

    /// Only reading.
    pub fn read_only(mut self) -> Self {
        self.read_only = true;
        self
    }

    /// With a message.
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = message.into();
        self
    }

    /// Over these dataspaces.
    pub fn over(mut self, dataspaces: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.dataspaces = dataspaces.into_iter().map(Into::into).collect();
        self
    }
}

/// An open transaction, as [`EnergymlWriter::begin`] returned it.
///
/// `#[non_exhaustive]`: only a writer makes one.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Transaction {
    /// The writer's name for it: the server's `transactionUuid` for an ETP
    /// dataspace, a number the store gives for an EPC.
    pub id: String,
    /// What was asked.
    pub request: TransactionRequest,
}

impl Transaction {
    /// A transaction a writer has opened.
    pub fn new(id: impl Into<String>, request: TransactionRequest) -> Self {
        Self {
            id: id.into(),
            request,
        }
    }
}

/// What writing one object did: ETP's `PutResponse`.
///
/// Built with [`Put::new`]; `#[non_exhaustive]`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Put {
    /// The full URI the object is now known under.
    pub uri: String,
    /// Contained objects (a WITSML container's parts) the write created.
    pub created_contained: Vec<String>,
    /// Contained objects it deleted (`pruneContainedObjects`).
    pub deleted_contained: Vec<String>,
    /// Contained objects it joined to this container.
    pub joined_contained: Vec<String>,
    /// Contained objects it unjoined from it.
    pub unjoined_contained: Vec<String>,
}

impl Put {
    /// An object written, nothing contained touched.
    pub fn new(uri: impl Into<String>) -> Self {
        Self {
            uri: uri.into(),
            ..Self::default()
        }
    }
}

/// The writing half of a store: objects, arrays, transactions - ETP protocols
/// 4, 9 and 18.
///
/// What a store cannot do it refuses **by name** ([`Error::Unsupported`]), and
/// says so in advance in [`EnergymlStorage::capabilities`]; nothing here answers
/// an empty success for a capability it lacks.
// `async_trait` marks the boxed future of each default method `#[must_use]`,
// which clippy 1.99 (`double_must_use`) reads as ours: see `storage.rs`.
#[allow(clippy::double_must_use)]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
pub trait EnergymlWriter: EnergymlStorage {
    /// ETP `PutDataObjects`: upsert each object (ETP 1.2 §9, `PutDataObjects`:
    /// a complete replacement, never a merge). One result per object, in order.
    ///
    /// `prune_contained`: a container's contained objects that the new version
    /// no longer contains are deleted. A store that does not know containment
    /// refuses `true` by name rather than ignoring it.
    async fn put_objects(
        &mut self,
        objects: Vec<Box<dyn DynObject>>,
        prune_contained: bool,
    ) -> Vec<Result<Put>>;

    /// The same with the objects as bytes, kept as received where the store can
    /// ([`crate::raw`]). Refused by default.
    async fn put_raw(&mut self, objects: Vec<RawObject>, prune_contained: bool) -> Vec<Result<Put>> {
        let _ = prune_contained;
        objects
            .iter()
            .map(|_| {
                Err(Error::Unsupported {
                    operation: "writing objects as bytes",
                    store: self.store_kind(),
                })
            })
            .collect()
    }

    /// ETP `DeleteDataObjects`: per URI, the URIs actually deleted (the object,
    /// and its contained objects when `prune_contained`).
    async fn delete_objects(&mut self, uris: &[String], prune_contained: bool) -> Vec<Result<Vec<String>>>;

    /// ETP `StartTransaction`. One transaction at a time: a second is refused.
    async fn begin(&mut self, request: TransactionRequest) -> Result<Transaction>;

    /// ETP `CommitTransaction`, of the transaction [`EnergymlWriter::begin`] returned.
    async fn commit(&mut self, transaction: &Transaction) -> Result<()>;

    /// ETP `RollbackTransaction`, of the transaction [`EnergymlWriter::begin`] returned.
    async fn rollback(&mut self, transaction: &Transaction) -> Result<()>;

    /// The transaction open now, if any.
    fn open_transaction(&self) -> Option<Transaction>;

    /// ETP `PutUninitializedDataArrays`: declare each array's shape and type
    /// before any value is written, so that it can be written part by part.
    /// Refused by default.
    async fn declare_arrays(&mut self, arrays: &[(ArrayLocation, ArrayMetadata)]) -> Vec<Result<()>> {
        arrays
            .iter()
            .map(|_| {
                Err(Error::Unsupported {
                    operation: "declaring an array before writing it",
                    store: self.store_kind(),
                })
            })
            .collect()
    }

    /// ETP `PutDataArrays` and `PutDataSubarrays`: the whole array when the
    /// location's `start` and `count` are empty, the block they name otherwise -
    /// the same addressing [`EnergymlStorage::read_array`] reads.
    async fn put_arrays(&mut self, arrays: &[(ArrayLocation, ArrayData)]) -> Vec<Result<()>>;
}

/// Refuse ending a transaction that is not the open one: the shared check of
/// every writer, so that the wording is one.
pub fn check_open(open: Option<&Transaction>, given: &Transaction, store: &'static str) -> Result<()> {
    match open {
        Some(open) if open.id == given.id => Ok(()),
        Some(open) => Err(Error::Transaction {
            store,
            reason: format!(
                "transaction `{}` is not the open one (`{}` is)",
                given.id, open.id
            ),
        }),
        None => Err(Error::Transaction {
            store,
            reason: format!("transaction `{}` is not open (no transaction is)", given.id),
        }),
    }
}
