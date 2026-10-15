//! The level above one dataspace: ETP's Dataspace protocol (24).
//!
//! A [`crate::write::EnergymlWriter`] is one dataspace. A store holding several -
//! an ETP server, a database - lists, creates and deletes them here, and hands
//! out the writer of one. An EPC holds exactly one: it lists it, reaches it, and
//! refuses by name to create or delete any.
//!
//! The writer is reached through `&mut self`: one dataspace at a time through
//! this door. A server that serves several dataspaces at once keeps one writer
//! per dataspace behind its own lock - the dataspace is the unit it serialises
//! writes on anyway - and uses this trait for what it is: the one way to list,
//! create, delete and reach them.

use crate::error::Result;
use crate::resource::Dataspace;
use crate::write::EnergymlWriter;

use async_trait::async_trait;

/// The dataspaces of a store, and the way to each one's writer.
// `async_trait` marks the boxed future of each default method `#[must_use]`,
// which clippy 1.99 (`double_must_use`) reads as ours: see `storage.rs`.
#[allow(clippy::double_must_use)]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
pub trait EnergymlDataspaces {
    /// ETP `GetDataspaces`.
    async fn list_dataspaces(&self) -> Result<Vec<Dataspace>>;

    /// ETP `PutDataspaces`: create each dataspace, or update one that exists -
    /// "add or update", and neither ETP nor this trait says which happened. One
    /// result per dataspace, in order.
    async fn put_dataspaces(&mut self, dataspaces: Vec<Dataspace>) -> Vec<Result<()>>;

    /// ETP `DeleteDataspaces`. One result per URI, in order.
    async fn delete_dataspaces(&mut self, uris: &[String]) -> Vec<Result<()>>;

    /// The writer of one dataspace, by its URI (`eml:///` for the default one).
    /// A URI naming no dataspace of this store is [`crate::Error::NotFound`].
    fn open_dataspace(&mut self, uri: &str) -> Result<&mut dyn EnergymlWriter>;
}
