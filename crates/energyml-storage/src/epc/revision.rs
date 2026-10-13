//! A revision counter and a bounded log of what was written, beside the
//! overlay.
//!
//! Decided in ADR-0010: whoever keeps a
//! derived view of a package (the package index) must know **which
//! objects changed since it was built**, so it re-reads those and nothing else.
//! Extended (ADR-0010) so that each modification says what it applies to, which
//! lets the optimised cache structures know whom to send modification/refresh
//! signals to, and down to what link depth:
//! the log says **what** changed in each object ([`What`]), which datasets of the
//! data files were written, and whether the parts or the data file names moved.
//!
//! The signal lives in the store, not in the front end's journal, because several
//! writes never pass through that journal: `create_apply` (thousands of
//! objects), `repair_references`, `relabel`, `rename`, a package import, the
//! `put_xml` of an ETP fetch — and the CLI and the Python bridge write through
//! the same library with no journal at all. An index kept up to date by the front end
//! would be a second truth about what the store holds. Every object write of
//! `EpcStore` goes through its `Overlay`, so that is where this is fed — **by the
//! gate that writes**, which is the only place that knows what it is writing.
//!
//! The log is **bounded**. Past its capacity the oldest entries are dropped and
//! a floor is raised; a reader older than the floor is told `Everything`, which
//! means "rebuild", never "nothing". An answer that is too wide costs a rebuild;
//! an answer that is too narrow is a stale index that looks exactly like a
//! correct one. The same bargain holds for [`What`]: a gate that cannot tell says
//! more than it knows, **in one direction only** — never less.
//!
//! Pure data: nothing here knows what an object, a package or a transaction is.
//! The log sits behind a mutex because a dataset is written through `&self`
//! (an array write does not need the package to be borrowed mutably), and the
//! journal has to see it.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Mutex;

/// What changed after a given revision, **per object name only** — the shape the
/// package index asks for. [`Revisions::changes_since`] says more.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Changed {
    /// Nothing was written since: the reader's view is still exact.
    Nothing,
    /// Exactly these objects (lower-cased uuids) were written, and no others.
    /// **The set may be empty**: a dataset or a file was written since, which no
    /// object name can carry — the revision moved, and a reader of objects alone
    /// has nothing to re-read.
    Some(BTreeSet<String>),
    /// The log cannot say which: it overflowed, or something changed that has
    /// no per-object name. The reader rebuilds.
    Everything,
}

/// What was written in one object — a set of flags, **per ADR-0010**.
///
/// A plain `u8` newtype rather than `bitflags`: the crate has no such dependency
/// and one set of six bits does not justify adding it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub struct What(u8);

impl What {
    /// Nothing at all.
    pub const NONE: What = What(0);
    /// The citation: title, originator, dates. The front end says `metadata`.
    pub const CITATION: What = What(1);
    /// Another field, with no reference moved. The front end says `content`.
    pub const BODY: What = What(2);
    /// Its outgoing references. The front end says `references`.
    pub const LINKS_OUT: What = What(4);
    /// The object is new to the package.
    pub const CREATED: What = What(8);
    /// The object is gone from the package.
    pub const REMOVED: What = What(16);
    /// The uuid or the `objectVersion` — what a uri is made of — may have moved.
    pub const IDENTITY: What = What(32);

    /// A gate that cannot tell what it changed in an object that **stays**:
    /// the citation, the body and the references. An in-place borrow, a
    /// replacement of an object that was there.
    pub const BROAD: What = What(1 | 2 | 4);
    /// A gate that cannot even tell whether the object is still there: a
    /// rolled back transaction. Every flag, so the reader checks everything.
    pub const EVERYTHING: What = What(1 | 2 | 4 | 8 | 16 | 32);

    /// Every flag of `other` is set here.
    pub const fn contains(self, other: What) -> bool {
        self.0 & other.0 == other.0
    }

    /// Some flag of `other` is set here.
    pub const fn intersects(self, other: What) -> bool {
        self.0 & other.0 != 0
    }

    /// No flag is set.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The flags of both.
    pub const fn union(self, other: What) -> What {
        What(self.0 | other.0)
    }

    /// The flags of `self` that `other` does not have.
    pub const fn without(self, other: What) -> What {
        What(self.0 & !other.0)
    }

    /// What editing **one value at this dotted path** changes, as far as the
    /// path alone can tell — the rule of a front ender's `kindOfEdit` and
    /// `movesIdentity`, which descends
    /// here so that there is one reading of it.
    ///
    /// - a path in the citation (`Citation`, `Citation.Title`): `CITATION`;
    /// - any other path: `BODY | LINKS_OUT` — the path cannot say whether it
    ///   reaches a reference, and a gate that misses a moved reference leaves a
    ///   graph that is wrong without a sign. `LINKS_OUT` is dropped afterwards,
    ///   when the object's references are found unmoved ([`EpcStore::edit_at`]);
    /// - a path ending in `uuid`/`uid`, `objectVersion` or `schemaVersion` may
    ///   move the object's identity: `IDENTITY`, and every other flag with it,
    ///   because the row is keyed by the identity that changed. (`Interpretation.UUID`
    ///   ends in `uuid` too and is no identity: the rule is deliberately as wide
    ///   as the front end's.)
    ///
    /// [`EpcStore::edit_at`]: crate::EpcStore::edit_at
    pub fn of_edit(path: &str) -> What {
        let in_citation = path == "Citation" || path.starts_with("Citation.");
        let mut what = if in_citation { What::CITATION } else { What::BODY.union(What::LINKS_OUT) };
        let last = path.rsplit('.').next().unwrap_or(path);
        if last.eq_ignore_ascii_case("uuid")
            || last.eq_ignore_ascii_case("uid")
            || last.eq_ignore_ascii_case("objectVersion")
            || last.eq_ignore_ascii_case("schemaVersion")
        {
            what = what.union(What::BROAD).union(What::IDENTITY);
        }
        what
    }
}

impl std::ops::BitOr for What {
    type Output = What;
    fn bitor(self, other: What) -> What {
        self.union(other)
    }
}

impl std::ops::BitOrAssign for What {
    fn bitor_assign(&mut self, other: What) {
        *self = self.union(other);
    }
}

impl std::fmt::Debug for What {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names = [
            (What::CITATION, "CITATION"),
            (What::BODY, "BODY"),
            (What::LINKS_OUT, "LINKS_OUT"),
            (What::CREATED, "CREATED"),
            (What::REMOVED, "REMOVED"),
            (What::IDENTITY, "IDENTITY"),
        ];
        let set: Vec<&str> = names.iter().filter(|(flag, _)| self.contains(*flag)).map(|(_, name)| *name).collect();
        if set.is_empty() { write!(f, "NONE") } else { write!(f, "{}", set.join(" | ")) }
    }
}

/// One object, and what was written in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectChange {
    /// The lower-cased uuid.
    pub uuid: String,
    /// The uri, when the gate that wrote it had one — filled in by
    /// `EpcStore::changes_since` from the store when the journal does not hold it.
    pub uri: Option<String>,
    /// What was written, merged over every write of the window.
    pub what: What,
}

/// A dataset of a data file: the file as the package names it, and the path
/// inside it. **An empty path is the whole file** — a data file brought or
/// replaced in one piece.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DatasetId {
    /// The file, as the package names it.
    pub file: String,
    /// The dataset inside it; empty for the whole file.
    pub path: String,
}

/// What one window of writes changed — the merge of every entry between two
/// revisions, **one line per object**.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Change {
    /// The revision the reader had.
    pub from: u64,
    /// The revision of the store now.
    pub to: u64,
    /// The objects written, sorted by uuid.
    pub objects: Vec<ObjectChange>,
    /// The datasets written, sorted.
    pub datasets: Vec<DatasetId>,
    /// A part (attachment, staged bytes) or a media relation moved.
    pub files: bool,
    /// A proxy was given a data file name — a rename, or a file declared for
    /// the first time: `(lower-cased proxy uuid, file name)`, in order.
    pub renamed: Vec<(String, String)>,
}

impl Change {
    /// Nothing changed.
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty() && self.datasets.is_empty() && !self.files && self.renamed.is_empty()
    }
}

/// The answer to *what changed since I last looked*.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Since {
    /// Exactly this, and nothing else — possibly nothing at all.
    Exact(Change),
    /// The log cannot say: it overflowed, or something was written that has no
    /// name. The reader rebuilds, never concludes "nothing".
    Everything,
}

/// One line of the log.
#[derive(Debug, Clone)]
enum Entry {
    Object { uuid: String, uri: Option<String>, what: What },
    Datasets { file: String, paths: Vec<String> },
    Files,
    Renamed { proxy: String, file: String },
}

#[derive(Debug, Clone)]
struct Inner {
    /// The revision of the last write. A fresh store is at 0.
    current: u64,
    /// `(revision, entry)`, oldest first, revisions increasing.
    log: VecDeque<(u64, Entry)>,
    /// The log is complete for every reader at or after this revision. A reader
    /// older than it may have missed a write that was forgotten.
    floor: u64,
}

/// The counter and its log.
#[derive(Debug)]
pub struct Revisions {
    inner: Mutex<Inner>,
    capacity: usize,
}

impl Clone for Revisions {
    fn clone(&self) -> Self {
        Self { inner: Mutex::new(self.lock().clone()), capacity: self.capacity }
    }
}

impl Revisions {
    /// An empty log at revision 0, keeping at most `capacity` entries.
    pub fn new(capacity: usize) -> Self {
        Self { inner: Mutex::new(Inner { current: 0, log: VecDeque::new(), floor: 0 }), capacity }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        // A panic while the log was held leaves plain data behind: usable.
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The revision of the last write.
    pub fn current(&self) -> u64 {
        self.lock().current
    }

    fn push(&self, entry: Entry) -> u64 {
        let mut inner = self.lock();
        inner.current += 1;
        let revision = inner.current;
        // Consecutive dataset writes into one file share a line: a SIGEOL import
        // writes tens of thousands of them, and one line each would push every
        // object entry out of the log. The line takes the newest revision, so a
        // reader who saw the first of them is told about all of them — too wide,
        // never too narrow.
        if let Entry::Datasets { file, paths } = &entry
            && let Some((last_revision, Entry::Datasets { file: last_file, paths: last_paths })) = inner.log.back_mut()
            && last_file == file
        {
            last_paths.extend(paths.iter().cloned());
            *last_revision = revision;
            return revision;
        }
        inner.log.push_back((revision, entry));
        while inner.log.len() > self.capacity {
            if let Some((dropped, _)) = inner.log.pop_front() {
                // Everything up to and including `dropped` is forgotten, so only
                // a reader at `dropped` or later has seen it all.
                inner.floor = dropped;
            }
        }
        revision
    }

    /// One object written, **no more known than "somehow"**: lower-cases the
    /// uuid, advances, logs it with [`What::BROAD`]. Returns the new revision.
    ///
    /// Uuids compare without case everywhere else in the crate, so the log does
    /// too; otherwise `A...` and `a...` would be two objects to a reader.
    pub fn touch(&mut self, uuid: &str) -> u64 {
        self.touch_what(uuid, None, What::BROAD)
    }

    /// One object written, and what the gate knows of it. `uri` is the gate's
    /// spelling of the object, when it has one.
    pub fn touch_what(&self, uuid: &str, uri: Option<&str>, what: What) -> u64 {
        self.push(Entry::Object { uuid: uuid.to_ascii_lowercase(), uri: uri.map(str::to_string), what })
    }

    /// Datasets of `file` were written. An empty `paths` names the file as a
    /// whole (carried or replaced in one piece).
    pub fn note_datasets<'a>(&self, file: &str, paths: impl IntoIterator<Item = &'a str>) -> u64 {
        let mut paths: Vec<String> = paths.into_iter().map(str::to_string).collect();
        if paths.is_empty() {
            paths.push(String::new());
        }
        self.push(Entry::Datasets { file: file.to_string(), paths })
    }

    /// A part or a media relation moved.
    pub fn note_files(&self) -> u64 {
        self.push(Entry::Files)
    }

    /// A proxy was given a data file name.
    pub fn note_renamed(&self, proxy: &str, file: &str) -> u64 {
        self.push(Entry::Renamed { proxy: proxy.to_ascii_lowercase(), file: file.to_string() })
    }

    /// Something changed that cannot be named per object — a raw part rewritten.
    /// Advances, and every reader whose revision is older must rebuild.
    pub fn touch_all(&mut self) -> u64 {
        let mut inner = self.lock();
        inner.current += 1;
        // Nothing before this revision can be told per object any more.
        inner.log.clear();
        inner.floor = inner.current;
        inner.current
    }

    /// Take `flags` off the object entry written at `revision`: the gate that
    /// wrote it said more than it later found true. Narrows **only** that entry,
    /// and only when it is still in the log.
    pub fn narrow(&self, revision: u64, uuid: &str, flags: What) {
        let uuid = uuid.to_ascii_lowercase();
        let mut inner = self.lock();
        for (at, entry) in inner.log.iter_mut() {
            if *at == revision
                && let Entry::Object { uuid: held, what, .. } = entry
                && *held == uuid
            {
                *what = what.without(flags);
            }
        }
    }

    /// What changed after `since`, per object name only.
    ///
    /// `Nothing` when `since` is the current revision; the uuids when the log
    /// still reaches back that far; `Everything` otherwise — the log
    /// overflowed, a `touch_all` came after `since`, or `since` is from the
    /// future (a revision this store never issued, so the reader belongs to
    /// another store).
    pub fn changed_since(&self, since: u64) -> Changed {
        let inner = self.lock();
        if since == inner.current {
            return Changed::Nothing;
        }
        if since > inner.current || since < inner.floor {
            return Changed::Everything;
        }
        Changed::Some(
            inner
                .log
                .iter()
                .filter(|(revision, _)| *revision > since)
                .filter_map(|(_, entry)| match entry {
                    Entry::Object { uuid, .. } => Some(uuid.clone()),
                    _ => None,
                })
                .collect(),
        )
    }

    /// What changed after `since`, **and what in each object**.
    ///
    /// The entries of the window are merged per object, in order: the flags
    /// accumulate, except that a removal followed by a write is a replacement
    /// (the object is back: `REMOVED` goes, `CREATED` and everything broad
    /// stay) and a creation followed by a removal is a removal (the reader never
    /// saw it, and is told it is gone, which is true).
    pub fn changes_since(&self, since: u64) -> Since {
        let inner = self.lock();
        if since > inner.current || since < inner.floor {
            return Since::Everything;
        }
        let mut change = Change { from: since, to: inner.current, ..Default::default() };
        let mut objects: BTreeMap<String, (Option<String>, What)> = BTreeMap::new();
        let mut datasets: BTreeSet<DatasetId> = BTreeSet::new();
        for entry in inner.log.iter().filter(|(revision, _)| *revision > since).map(|(_, entry)| entry) {
            match entry {
                Entry::Object { uuid, uri, what } => {
                    let held = objects.entry(uuid.clone()).or_insert((None, What::NONE));
                    if uri.is_some() {
                        held.0 = uri.clone();
                    }
                    held.1 = merged(held.1, *what);
                }
                Entry::Datasets { file, paths } => {
                    for path in paths {
                        datasets.insert(DatasetId { file: file.clone(), path: path.clone() });
                    }
                }
                Entry::Files => change.files = true,
                Entry::Renamed { proxy, file } => change.renamed.push((proxy.clone(), file.clone())),
            }
        }
        change.objects = objects.into_iter().map(|(uuid, (uri, what))| ObjectChange { uuid, uri, what }).collect();
        change.datasets = datasets.into_iter().collect();
        Since::Exact(change)
    }
}

/// The flags of an object after one more write.
fn merged(held: What, next: What) -> What {
    let both = next.contains(What::CREATED) && next.contains(What::REMOVED);
    if both {
        // A gate that does not know whether the object is there: nothing of what
        // was said before is withdrawn.
        return held.union(next);
    }
    if next.contains(What::REMOVED) {
        return What::REMOVED;
    }
    if held.contains(What::REMOVED) {
        // Gone, then written: it is back, and a reader that held the old one has
        // a different object now.
        return next.union(What::BROAD).union(What::CREATED);
    }
    held.union(next)
}

impl Default for Revisions {
    /// Room for a few thousand writes: a long editing session, not an import.
    fn default() -> Self {
        Self::new(4096)
    }
}
