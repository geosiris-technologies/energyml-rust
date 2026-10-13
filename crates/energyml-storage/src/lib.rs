//! Reading EnergyML data stores behind one abstraction: an EPC file today, an
//! ETP v1.2 dataspace later.
//!
//! # The shape, in one paragraph
//!
//! [`EnergymlStorage`] is the mother trait, and it is drawn after the **ETP
//! protocol**, not after a zip file — addressing by URI, plural operations,
//! partial responses. An EPC fits inside that shape (a file *is* a dataspace);
//! the reverse would not have been true. [`EpcStore`] implements it, and also
//! carries a complete **synchronous** API so that an application which only
//! reads local files never has to write `async`. The reasoning is in
//! ADR-0002.
//!
//! ```no_run
//! use std::sync::Arc;
//! use energyml_core::registry::Registry;
//! use energyml_storage::EpcStore;
//!
//! let mut registry = Registry::new();
//! // energyml_resqml_2_0_1::register(&mut registry); — the application chooses
//! // which versions it knows: nothing is hard-wired here.
//!
//! let store = EpcStore::open("model.epc", Arc::new(registry))?;
//! for summary in store.resources() {
//!     let header = store.header(&summary.uri)?;
//!     println!("{} — {}", header.summary.qualified_type, header.display_name());
//! }
//! # Ok::<(), energyml_storage::Error>(())
//! ```
//!
//! # Three levels, and a listing needs all three
//!
//! | Level | Call | What it opens |
//! |---|---|---|
//! | identity | [`EpcStore::resources`] | the package index only |
//! | citation | [`EpcStore::header`] | the first bytes of the part |
//! | object | [`EpcStore::get_object`] | the whole part |
//!
//! An interface listing a two-gigabyte EPC lives at level 2, and getting there
//! cheaply is the point of the split.
//!
//! # Tolerant, and never silent
//!
//! Real packages break the specification. This reader accepts what it can and
//! records every departure as a [`Diagnostic`] — the same rule the code
//! generator follows for the schema constructs it cannot map: *nothing is lost
//! in silence*.
//!
//! # Writing
//!
//! An open package is **modifiable**: [`EpcStore::edit`] hands out a borrow and
//! marks the object, [`EpcStore::put`] and [`EpcStore::remove`] add and delete,
//! and nothing touches the archive until [`EpcStore::save`]. Saving does not
//! patch the old package, it **rebuilds** one — `[Content_Types].xml`, every
//! `.rels` in both directions, `docProps/core.xml` — because a description that
//! no longer matches is worse than none. Transactions are one more layer of the
//! same in-memory stack, so a rollback really undoes.
//!
//! # External arrays
//!
//! An object points at its numbers; it does not hold them. [`EpcStore`] finds
//! every pointer, in both the spellings EnergyML uses, and reads them through
//! an [`ArrayBackend`]. HDF5 is one, behind the **optional `hdf5` feature** —
//! without it an EPC is still read, edited and written, and only reading an
//! array fails, saying which capability is missing rather than pretending the
//! data is absent. A WebAssembly application supplies its own backend over
//! h5wasm and nothing here changes.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod array;
pub mod backend;
pub mod epc;
pub mod error;
pub mod extract;
pub mod graph;
/// Bringing part of another package into the open one: a quote, then the writing.
pub mod import;
/// Changing what a part declares itself to be, without parsing what it holds.
pub mod relabel;
pub mod rename;
#[cfg(feature = "hdf5")]
pub mod hdf5;
pub mod resource;
pub mod storage;

pub use array::{
    ArrayBuffer, ArrayData, ArrayLocation, ArrayMetadata, ArrayValues, NoProxies, ProxyResolver, ProxyTarget,
    Resolution,
};
pub use backend::{ArrayBackend, Backends, Candidate, ExternalFiles, Origin};
pub use epc::overlay::SavePolicy;
pub use epc::{EpcStore, SaveOptions, Unreadable};
pub use error::{Diagnostic, Error, ExternalDataReason, Result, Severity};
pub use graph::{RefIssue, RefIssueKind};
pub use rename::{Applied as Renamed, FileMove, Rename};
pub use resource::{Dataspace, ObjectHeader, ObjectSummary, Relationship};
pub use storage::{EnergymlStorage, LocalPackage};
