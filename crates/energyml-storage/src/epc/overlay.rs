//! The in-memory layer that makes an open package modifiable.
//!
//! Decided in ADR-0002: an EPC is not opened
//! read-only to be rewritten elsewhere. It is opened, its objects are edited,
//! and it is saved. That is what the reflection built in step 2 was for -
//! editing an object without knowing its type at compile time, and later
//! generating editing interfaces from the same metadata.
//!
//! **The archive on disk is never touched until a save.** Above it sits this
//! layer, holding the objects that were materialised and knowing which are
//! *modified*, *added* or *removed*. A read consults the layer, then the
//! archive. A save merges.
//!
//! It is cheap: an EnergyML object is a few kilobytes of XML. What is big in an
//! EPC - the arrays - is not in the EPC at all, it is beside it.
//!
//! # Transactions are one more layer
//!
//! ETP's `StartTransaction` / `Commit` / `Rollback` (protocol 18) map onto this
//! without touching a disk: start pushes a layer, commit merges it down,
//! rollback throws it away. ETP v1.2 forbids nested transactions, so the
//! stack is never more than one deep - which is why this holds an `Option` and
//! not a `Vec`.
//!
//! A rollback that undoes nothing would be a lie, and an abstraction that lies
//! about a rollback is worse than one that refuses it. This one undoes.
//!
//! # The raw layer, beside the objects
//!
//! One thing here is not an object: a part whose XML this build **cannot
//! parse**. Broken markup, a byte that is not UTF-8, a part of zero length -
//! there is no `DynObject` to hold, and no uuid to key it by when the damage
//! reaches the root element. Repairing such a part is exactly what a design
//! tool is for, so it is staged as bytes, keyed by **part name**, in a map
//! beside the objects.
//!
//! Two maps rather than one enum because they are addressed differently and
//! both have to survive a transaction. Where they collide - bytes staged for
//! the part of an object that was also edited - the **bytes win**, because
//! staging bytes is the more deliberate act: it says "I know better than the
//! parser", and the only reason to say it is that the parser was wrong.

use std::collections::{BTreeMap, BTreeSet};

use energyml_core::registry::DynObject;

use crate::error::{Error, Result};

use super::revision::{Changed, Revisions, Since, What};

/// What happened to one object since the package was opened.
pub enum Change {
    /// Materialised and edited, or written over.
    Put(Box<dyn DynObject>),
    /// Deleted. Kept as an entry rather than removed from the map, because
    /// "deleted" and "never mentioned" are different answers to a read.
    Removed,
}

impl Change {
    fn clone_change(&self) -> Change {
        match self {
            Change::Put(object) => Change::Put(object.clone_box()),
            Change::Removed => Change::Removed,
        }
    }
}

/// The layer, plus the transaction layer when one is open.
#[derive(Default)]
pub struct Overlay {
    /// Lowercased uuid → what happened to it.
    committed: BTreeMap<String, Change>,
    /// The open transaction, if any. `None` is "no transaction", which is not
    /// the same as an empty one.
    transaction: Option<BTreeMap<String, Change>>,
    /// Part name → replacement bytes, committed.
    raw: BTreeMap<String, Vec<u8>>,
    /// Part name → replacement bytes, inside the open transaction.
    raw_transaction: Option<BTreeMap<String, Vec<u8>>>,
    /// Archive parts that an object now supersedes, and that a save must not
    /// copy through.
    ///
    /// One case makes this necessary: a broken part the inventory could not
    /// address is copied verbatim on save, because nothing knows what it is.
    /// Repair it and it becomes an object - which the writer then places under
    /// the name its type and uuid dictate. Without this set the package would
    /// come out holding **both**, the repaired object and the wreck it was
    /// made from.
    superseded: BTreeSet<String>,
    /// Parts a caller has removed - an attachment taken back out of the package.
    dropped: BTreeSet<String>,
    /// `(media part, lowercased object uuid)` → related (`true`) or unrelated
    /// (`false`) in this session, committed. What the archive's `.rels` said
    /// is read from the archive; this is only what changed since.
    media: BTreeMap<(String, String), bool>,
    /// The same, inside the open transaction - a rollback undoes a relation
    /// like it undoes an object.
    media_transaction: Option<BTreeMap<(String, String), bool>>,
    /// Every object write, counted and named - what a derived view of the
    /// package (the package index) asks to know what to re-read.
    /// (ADR-0010): this layer is the one
    /// door every object write goes through, so the signal lives here and
    /// nowhere else.
    revisions: Revisions,
    /// Every change to the **files** of the package - a part staged, dropped or
    /// restored, a media relation made or undone, a transaction opened, merged
    /// or thrown away - counted, and **not** named. Beside `revisions` because
    /// none of these is an object write, so the object journal never saw them;
    /// a derived view of the files (the listing of what is attached to an
    /// object, `energyml_domain::attachments::Cache`) compares its own stamp
    /// with this one and drops what it holds when they differ.
    files: u64,
}

impl Overlay {
    /// The revision of the last object write.
    pub fn revision(&self) -> u64 {
        self.revisions.current()
    }

    /// The count of changes to the package's files - see the field.
    pub fn files_revision(&self) -> u64 {
        self.files
    }

    /// Which objects were written after `since` - see [`Revisions::changed_since`].
    pub fn changed_since(&self, since: u64) -> Changed {
        self.revisions.changed_since(since)
    }

    /// What was written after `since`, and what in each object - see
    /// [`Revisions::changes_since`].
    pub fn changes_since(&self, since: u64) -> Since {
        self.revisions.changes_since(since)
    }

    /// The journal itself, for the gates that write something other than an
    /// object (a dataset, a data file name): they note it through `&self`.
    pub fn journal(&self) -> &Revisions {
        &self.revisions
    }

    /// Something changed that has no per-object name (a part's bytes
    /// replaced): every reader older than now rebuilds.
    pub fn touch_all(&mut self) {
        self.revisions.touch_all();
    }

    /// An empty layer over an untouched package.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether anything at all has changed.
    pub fn is_dirty(&self) -> bool {
        !self.committed.is_empty()
            || self.transaction.as_ref().is_some_and(|t| !t.is_empty())
            || !self.raw.is_empty()
            || self.raw_transaction.as_ref().is_some_and(|t| !t.is_empty())
            || !self.superseded.is_empty()
            || !self.dropped.is_empty()
            || !self.media.is_empty()
            || self.media_transaction.as_ref().is_some_and(|t| !t.is_empty())
    }

    /// Relate (`true`) or unrelate (`false`) a media part and an object, in
    /// whichever layer is on top.
    pub fn set_media_link(&mut self, part: &str, uuid: &str, related: bool) {
        self.files += 1;
        self.revisions.note_files();
        let key = (part.to_string(), uuid.to_ascii_lowercase());
        match &mut self.media_transaction {
            Some(transaction) => transaction.insert(key, related),
            None => self.media.insert(key, related),
        };
    }

    /// Every media relation changed in this session, the transaction on top.
    pub fn media_links(&self) -> Vec<((String, String), bool)> {
        let mut out: BTreeMap<&(String, String), bool> = self.media.iter().map(|(k, v)| (k, *v)).collect();
        if let Some(transaction) = &self.media_transaction {
            out.extend(transaction.iter().map(|(k, v)| (k, *v)));
        }
        out.into_iter().map(|(k, v)| (k.clone(), v)).collect()
    }

    /// The bytes staged for a part, if any - the transaction first.
    pub fn raw(&self, part: &str) -> Option<&[u8]> {
        self.raw_transaction
            .as_ref()
            .and_then(|t| t.get(part))
            .or_else(|| self.raw.get(part))
            .map(Vec::as_slice)
    }

    /// Stage bytes for a part, in whichever layer is on top.
    pub fn set_raw(&mut self, part: &str, bytes: Vec<u8>) {
        self.files += 1;
        self.revisions.note_files();
        match &mut self.raw_transaction {
            Some(transaction) => transaction.insert(part.to_string(), bytes),
            None => self.raw.insert(part.to_string(), bytes),
        };
    }

    /// Drop what was staged for a part. `true` when there was something.
    ///
    /// Only from the layer on top: a rollback is what undoes a transaction,
    /// and letting a clear reach through it would make the two disagree.
    pub fn clear_raw(&mut self, part: &str) -> bool {
        self.files += 1;
        self.revisions.note_files();
        match &mut self.raw_transaction {
            Some(transaction) => transaction.remove(part).is_some(),
            None => self.raw.remove(part).is_some(),
        }
    }

    /// Mark an archive part as replaced by an object.
    pub fn supersede(&mut self, part: &str) {
        self.superseded.insert(part.to_string());
    }

    /// Whether a save should skip this archive part.
    pub fn is_superseded(&self, part: &str) -> bool {
        self.superseded.contains(part)
    }

    /// Drop an archive part: a save must not copy it through.
    ///
    /// Beside `superseded` rather than inside it, because the two say different
    /// things and only one of them is reversible. *Superseded* is a fact the
    /// library worked out - an object now stands where a wreck was - and nothing
    /// asks for it to be undone. *Dropped* is a person removing an attachment,
    /// which they may want back before the save; hence [`Overlay::restore_part`]
    /// and no equivalent for the other.
    pub fn drop_part(&mut self, part: &str) {
        self.files += 1;
        self.revisions.note_files();
        self.dropped.insert(part.to_string());
    }

    /// Whether a part has been dropped.
    pub fn is_dropped(&self, part: &str) -> bool {
        self.dropped.contains(part)
    }

    /// Take a part off the dropped list. `true` when it was on it.
    pub fn restore_part(&mut self, part: &str) -> bool {
        self.files += 1;
        self.revisions.note_files();
        self.dropped.remove(part)
    }

    /// The parts dropped since the package was opened.
    pub fn dropped_parts(&self) -> Vec<String> {
        self.dropped.iter().cloned().collect()
    }

    /// Every part with bytes staged, transaction included.
    pub fn raw_parts(&self) -> Vec<String> {
        let mut out: BTreeMap<&String, ()> = self.raw.keys().map(|k| (k, ())).collect();
        if let Some(transaction) = &self.raw_transaction {
            out.extend(transaction.keys().map(|k| (k, ())));
        }
        out.into_keys().cloned().collect()
    }

    /// Whether a transaction is open.
    pub fn in_transaction(&self) -> bool {
        self.transaction.is_some()
    }

    /// What the layer says about this uuid - the transaction first, since it
    /// sits above.
    pub fn get(&self, uuid: &str) -> Option<&Change> {
        let key = uuid.to_ascii_lowercase();
        self.transaction
            .as_ref()
            .and_then(|t| t.get(&key))
            .or_else(|| self.committed.get(&key))
    }

    /// A mutable borrow of an object already in the layer, for editing in
    /// place. `None` when the layer has never heard of it.
    ///
    /// Nothing is known of what the caller will change: [`What::BROAD`].
    pub fn get_mut(&mut self, uuid: &str) -> Option<&mut Box<dyn DynObject>> {
        self.get_mut_what(uuid, None, What::BROAD)
    }

    /// The same, for a gate that knows what it is about to change (`uri` is its
    /// spelling of the object, kept in the journal for a reader that wants it).
    pub fn get_mut_what(
        &mut self,
        uuid: &str,
        uri: Option<&str>,
        what: What,
    ) -> Option<&mut Box<dyn DynObject>> {
        let key = uuid.to_ascii_lowercase();
        let map = match &mut self.transaction {
            Some(transaction) => transaction,
            None => &mut self.committed,
        };
        match map.get_mut(&key) {
            Some(Change::Put(object)) => {
                // A mutable borrow is an edit in place: counted as a write
                // whether or not the caller ends up changing anything - too
                // wide costs a re-read, too narrow is a stale index.
                self.revisions.touch_what(&key, uri, what);
                Some(object)
            }
            _ => None,
        }
    }

    /// The same borrow, with no journal line - for a gate that has journaled
    /// the edit itself, and takes the borrow again to hand it out.
    pub fn get_mut_unjournaled(&mut self, uuid: &str) -> Option<&mut Box<dyn DynObject>> {
        let key = uuid.to_ascii_lowercase();
        let map = match &mut self.transaction {
            Some(transaction) => transaction,
            None => &mut self.committed,
        };
        match map.get_mut(&key) {
            Some(Change::Put(object)) => Some(object),
            _ => None,
        }
    }

    /// Record a change in whichever layer is on top. The journal is told the
    /// widest thing that can be true of such a write: [`What::REMOVED`] for a
    /// removal, [`What::BROAD`] for an object put.
    pub fn set(&mut self, uuid: &str, change: Change) {
        let what = match change {
            Change::Removed => What::REMOVED,
            Change::Put(_) => What::BROAD,
        };
        self.set_what(uuid, None, change, what);
    }

    /// Record a change, and say what it is: the gate that writes knows whether
    /// the object is new, replaced or gone.
    pub fn set_what(&mut self, uuid: &str, uri: Option<&str>, change: Change, what: What) {
        let key = uuid.to_ascii_lowercase();
        self.revisions.touch_what(&key, uri, what);
        self.place(key, change);
    }

    /// Put an object in the layer **without** a journal line: the caller
    /// journals what it is about to do with it. Materialising an object for an
    /// edit is not a write; the borrow that follows is.
    pub fn place_unjournaled(&mut self, uuid: &str, change: Change) {
        self.place(uuid.to_ascii_lowercase(), change);
    }

    fn place(&mut self, key: String, change: Change) {
        match &mut self.transaction {
            Some(transaction) => transaction.insert(key, change),
            None => self.committed.insert(key, change),
        };
    }

    /// Every uuid the layer has an opinion about.
    pub fn changed(&self) -> Vec<(&String, &Change)> {
        let mut out: BTreeMap<&String, &Change> = self.committed.iter().collect();
        if let Some(transaction) = &self.transaction {
            out.extend(transaction.iter());
        }
        out.into_iter().collect()
    }

    /// Push a transaction layer.
    ///
    /// ETP v1.2 allows one transaction at a time; a second `StartTransaction`
    /// is an error rather than a nesting.
    pub fn start_transaction(&mut self) -> Result<()> {
        if self.transaction.is_some() {
            return Err(Error::Package(
                "a transaction is already open; ETP v1.2 does not nest them".to_string(),
            ));
        }
        self.files += 1;
        self.transaction = Some(BTreeMap::new());
        self.raw_transaction = Some(BTreeMap::new());
        self.media_transaction = Some(BTreeMap::new());
        Ok(())
    }

    /// Merge the transaction layer into the one below.
    pub fn commit_transaction(&mut self) -> Result<()> {
        let transaction = self.take_transaction()?;
        self.files += 1;
        self.committed.extend(transaction);
        if let Some(raw) = self.raw_transaction.take() {
            self.raw.extend(raw);
        }
        if let Some(media) = self.media_transaction.take() {
            self.media.extend(media);
        }
        Ok(())
    }

    /// Throw the transaction layer away. Nothing reached the disk, so nothing
    /// has to be undone there.
    pub fn rollback_transaction(&mut self) -> Result<()> {
        // What the transaction touched was counted when it was written; a
        // reader that looked in between saw a state that now vanishes, so the
        // same objects are named again. Raw bytes staged in it have no uuid:
        // their undoing is a change nobody can name per object.
        self.files += 1;
        let outcome = self.take_transaction().map(|undone| {
            // Whether the object is still there, and what it holds, is no longer
            // what the reader saw: every flag.
            for uuid in undone.keys() {
                self.revisions.touch_what(uuid, None, What::EVERYTHING);
            }
        });
        if self.media_transaction.as_ref().is_some_and(|t| !t.is_empty())
            || self.raw_transaction.as_ref().is_some_and(|t| !t.is_empty())
        {
            self.revisions.note_files();
        }
        if self.raw_transaction.as_ref().is_some_and(|t| !t.is_empty()) {
            self.revisions.touch_all();
        }
        self.raw_transaction = None;
        self.media_transaction = None;
        outcome
    }

    fn take_transaction(&mut self) -> Result<BTreeMap<String, Change>> {
        self.transaction
            .take()
            .ok_or_else(|| Error::Package("no transaction is open".to_string()))
    }

    /// Forget every change, after they have been written.
    pub fn clear(&mut self) {
        self.files += 1;
        self.committed.clear();
        self.transaction = None;
        self.raw.clear();
        self.raw_transaction = None;
        self.superseded.clear();
        self.dropped.clear();
        self.media.clear();
        self.media_transaction = None;
    }
}

impl Clone for Overlay {
    fn clone(&self) -> Self {
        let copy = |map: &BTreeMap<String, Change>| -> BTreeMap<String, Change> {
            map.iter().map(|(k, v)| (k.clone(), v.clone_change())).collect()
        };
        Self {
            committed: copy(&self.committed),
            transaction: self.transaction.as_ref().map(copy),
            raw: self.raw.clone(),
            raw_transaction: self.raw_transaction.clone(),
            superseded: self.superseded.clone(),
            dropped: self.dropped.clone(),
            media: self.media.clone(),
            media_transaction: self.media_transaction.clone(),
            revisions: self.revisions.clone(),
            files: self.files,
        }
    }
}

/// When the package is written back.
///
/// Chosen at open, because it changes what every later call costs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SavePolicy {
    /// Nothing is written until [`crate::EpcStore::save`] is called.
    #[default]
    Manual,
    /// Every change rewrites the archive.
    ///
    /// **A zip cannot be modified in place**, so this rewrites the *whole*
    /// package on every edit. On a two-gigabyte EPC it is unusable; it exists
    /// for small packages and for tests, and this sentence is the warning.
    OnChange,
    /// Written when [`crate::EpcStore::close`] is called.
    ///
    /// Note `close`, not `Drop`: destruction returns no `Result`, so a write
    /// that fails there has nowhere to say so, and a silently failed save is
    /// worse than no save at all. `Drop` only complains on standard error.
    OnClose,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_transaction_does_not_nest() {
        let mut overlay = Overlay::new();
        overlay.start_transaction().expect("the first one opens");
        assert!(overlay.start_transaction().is_err(), "the second is refused");
        overlay.commit_transaction().expect("it closes");
        assert!(overlay.commit_transaction().is_err(), "and only once");
    }

    #[test]
    fn a_rollback_actually_undoes() {
        let mut overlay = Overlay::new();
        overlay.set("A", Change::Removed);
        overlay.start_transaction().unwrap();
        overlay.set("B", Change::Removed);
        assert_eq!(overlay.changed().len(), 2);

        overlay.rollback_transaction().unwrap();
        assert_eq!(overlay.changed().len(), 1, "B is gone, A stayed");
        assert!(overlay.get("B").is_none());
        assert!(overlay.get("a").is_some(), "uuids compare without case");
    }

    #[test]
    fn a_commit_merges_downwards() {
        let mut overlay = Overlay::new();
        overlay.start_transaction().unwrap();
        overlay.set("B", Change::Removed);
        overlay.commit_transaction().unwrap();
        assert!(!overlay.in_transaction());
        assert!(overlay.get("B").is_some(), "it survived the commit");
    }
}
