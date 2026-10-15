//! The writing traits on an EPC: one line of logic per method over the
//! synchronous API, and a named refusal for what a file cannot do.
//!
//! Three refusals, each with its reason:
//!
//! - **Contained objects** (`prune_contained`): an EPC has no notion of a
//!   container pruning its parts; `true` is refused, never ignored.
//! - **Part of an array, or an array declared before it is written**: the
//!   backends write whole datasets. The need is ETP's (a message too large), not
//!   a file's, where a whole array is in memory (design notes,
//!   `ecriture-objets-et-tableaux.md` Q4).
//! - **An array inside a transaction**: a transaction is a layer in memory and
//!   nothing on disk, but an array goes straight into its file - a rollback would
//!   not undo it (same notes, Q8). Refused while a transaction is open, until
//!   arrays are held by the layer.

use energyml_core::identifiers::EtpUri;
use energyml_core::registry::DynObject;

use super::EpcStore;
use crate::array::{ArrayData, ArrayLocation};
use crate::changes::{EnergymlChanges, Since};
use crate::dataspaces::EnergymlDataspaces;
use crate::error::{Error, Result};
use crate::raw::{RawFormat, RawObject};
use crate::resource::Dataspace;
use crate::write::{EnergymlWriter, Put, Transaction, TransactionRequest, check_open};

use async_trait::async_trait;

const STORE: &str = "EPC";

impl EpcStore {
    /// What the writer says of the transaction open now: the one
    /// [`EnergymlWriter::begin`] opened, or - opened through
    /// [`EpcStore::start_transaction`] - one named after the package's state.
    fn writer_open(&self) -> Option<Transaction> {
        if !self.in_transaction() {
            return None;
        }
        Some(self.writer_transaction.clone().unwrap_or_else(|| {
            Transaction::new(
                format!("epc-transaction-{}", self.files_revision()),
                TransactionRequest::new(),
            )
        }))
    }

    /// Refuse a write inside a transaction that only reads.
    fn writable(&self) -> Result<()> {
        match self.writer_open() {
            Some(open) if open.request.read_only => Err(Error::Transaction {
                store: STORE,
                reason: format!("transaction `{}` only reads", open.id),
            }),
            _ => Ok(()),
        }
    }

    /// The objects as bytes, for [`crate::EnergymlStorage::get_raw`]: the XML of
    /// each part, as stored or as staged.
    pub(super) fn raw_objects(&self, uris: &[String], format: RawFormat) -> Vec<Result<RawObject>> {
        uris.iter()
            .map(|uri| match format {
                RawFormat::Xml => {
                    let full = self.entry(uri)?.summary.uri.clone();
                    Ok(RawObject::new(full, RawFormat::Xml, self.part_bytes(uri)?))
                }
                RawFormat::Json => Err(Error::Unsupported {
                    operation: "reading objects as JSON",
                    store: STORE,
                }),
            })
            .collect()
    }
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
impl EnergymlWriter for EpcStore {
    async fn put_objects(
        &mut self,
        objects: Vec<Box<dyn DynObject>>,
        prune_contained: bool,
    ) -> Vec<Result<Put>> {
        objects
            .into_iter()
            .map(|object| {
                self.writable()?;
                if prune_contained {
                    return Err(Error::Unsupported {
                        operation: "pruning contained objects",
                        store: STORE,
                    });
                }
                self.put(object).map(Put::new)
            })
            .collect()
    }

    async fn delete_objects(&mut self, uris: &[String], prune_contained: bool) -> Vec<Result<Vec<String>>> {
        uris.iter()
            .map(|uri| {
                self.writable()?;
                if prune_contained {
                    return Err(Error::Unsupported {
                        operation: "pruning contained objects",
                        store: STORE,
                    });
                }
                // The full URI the object had, before it is gone; one already
                // removed in this layer answers as the first removal did.
                let full = self.entry(uri).map(|entry| entry.summary.uri.clone()).ok();
                self.remove(uri)?;
                Ok(vec![full.unwrap_or_else(|| uri.clone())])
            })
            .collect()
    }

    async fn begin(&mut self, request: TransactionRequest) -> Result<Transaction> {
        if let Some(open) = self.writer_open() {
            return Err(Error::Transaction {
                store: STORE,
                reason: format!("transaction `{}` is already open", open.id),
            });
        }
        let own = EpcStore::dataspace(self).uri;
        if let Some(other) = request.dataspaces.iter().find(|uri| **uri != own) {
            return Err(Error::NotFound(other.clone()));
        }
        self.start_transaction()?;
        let transaction = Transaction::new(format!("epc-transaction-{}", self.files_revision()), request);
        self.writer_transaction = Some(transaction.clone());
        Ok(transaction)
    }

    async fn commit(&mut self, transaction: &Transaction) -> Result<()> {
        check_open(self.writer_open().as_ref(), transaction, STORE)?;
        self.commit_transaction()?;
        self.writer_transaction = None;
        Ok(())
    }

    async fn rollback(&mut self, transaction: &Transaction) -> Result<()> {
        check_open(self.writer_open().as_ref(), transaction, STORE)?;
        self.rollback_transaction()?;
        self.writer_transaction = None;
        Ok(())
    }

    fn open_transaction(&self) -> Option<Transaction> {
        self.writer_open()
    }

    async fn put_arrays(&mut self, arrays: &[(ArrayLocation, ArrayData)]) -> Vec<Result<()>> {
        arrays
            .iter()
            .map(|(location, data)| {
                self.writable()?;
                if self.in_transaction() {
                    return Err(Error::Unsupported {
                        operation: "writing an array inside a transaction (a rollback would not undo it)",
                        store: STORE,
                    });
                }
                if !location.start.is_empty() || !location.count.is_empty() {
                    return Err(Error::Unsupported {
                        operation: "writing part of an array",
                        store: STORE,
                    });
                }
                self.write_array(location, data)
            })
            .collect()
    }
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
impl EnergymlDataspaces for EpcStore {
    async fn list_dataspaces(&self) -> Result<Vec<Dataspace>> {
        Ok(vec![self.dataspace()])
    }

    async fn put_dataspaces(&mut self, dataspaces: Vec<Dataspace>) -> Vec<Result<()>> {
        dataspaces
            .iter()
            .map(|_| {
                Err(Error::Unsupported {
                    operation: "creating a dataspace (an EPC is exactly one)",
                    store: STORE,
                })
            })
            .collect()
    }

    async fn delete_dataspaces(&mut self, uris: &[String]) -> Vec<Result<()>> {
        uris.iter()
            .map(|_| {
                Err(Error::Unsupported {
                    operation: "deleting a dataspace (an EPC is exactly one)",
                    store: STORE,
                })
            })
            .collect()
    }

    fn open_dataspace(&mut self, uri: &str) -> Result<&mut dyn EnergymlWriter> {
        let parsed = EtpUri::parse(uri).map_err(|error| Error::BadUri {
            uri: uri.to_string(),
            reason: error.to_string(),
        })?;
        let own = EtpUri::parse(&EpcStore::dataspace(self).uri).map_err(|error| Error::BadUri {
            uri: uri.to_string(),
            reason: error.to_string(),
        })?;
        let names_a_dataspace = parsed.uuid.is_none() && parsed.object_type.is_none();
        if !names_a_dataspace || parsed.dataspace != own.dataspace {
            return Err(Error::NotFound(uri.to_string()));
        }
        Ok(self)
    }
}

impl EnergymlChanges for EpcStore {
    fn revision(&self) -> u64 {
        EpcStore::revision(self)
    }

    fn changes_since(&self, since: u64) -> Since {
        EpcStore::changes_since(self, since)
    }
}
