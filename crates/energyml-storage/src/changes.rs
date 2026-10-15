//! What changed since a reader last looked: the shape of a change feed.
//!
//! ETP's StoreNotification protocol (5) tells a subscriber what was written and
//! deleted; a server builds it on a store that can say what changed since a
//! revision. [`EnergymlChanges`] is that question. An EPC answers it from its
//! journal (ADR-0010); a store whose log cannot say answers
//! [`Since::Everything`], and the reader rebuilds - it never concludes
//! "nothing".
//!
//! The subscription itself (which context, which objects to send) belongs to the
//! server, around this trait: the store only knows what it wrote.

pub use crate::epc::revision::{Change, DatasetId, ObjectChange, Since};

/// A store that reports what it wrote.
pub trait EnergymlChanges {
    /// The revision of the last write. A reader keeps it and asks
    /// [`EnergymlChanges::changes_since`] with it next time.
    fn revision(&self) -> u64;

    /// What was written after `since`, object by object, or
    /// [`Since::Everything`] when the store cannot say.
    fn changes_since(&self, since: u64) -> Since;
}
