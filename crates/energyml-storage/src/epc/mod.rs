//! Reading an EPC file.
//!
//! [`EpcStore`] carries **two call surfaces**, and that is deliberate
//! (ADR-0002):
//!
//! - a complete **synchronous** inherent API — what an application that only
//!   ever touches local files should use. It never writes `async`, never adds
//!   an executor, and need not know [`EnergymlStorage`] exists;
//! - an implementation of [`EnergymlStorage`], **asynchronous**, one line per
//!   method over the synchronous one. That is what lets an application swap an
//!   EPC for an ETP dataspace by changing the line that builds the store.
//!
//! Which to use: the synchronous one, unless being indifferent to the transport
//! is the point.
//!
//! # What it reads, and what it costs
//!
//! | Call | Opens | Cost |
//! |---|---|---|
//! | [`EpcStore::resources`] | nothing | already in memory after `open` |
//! | [`EpcStore::headers`] | the first bytes of each part | one partial inflate per object |
//! | [`EpcStore::get_objects`] | the whole part | full deserialization |
//!
//! # Tolerance
//!
//! Real packages break the specification — the reference package published with
//! fesapi breaks four rules at once. This reader accepts
//! them and records every departure in [`EpcStore::diagnostics`]. Nothing is
//! swallowed, nothing is fatal that need not be.

pub mod core_props;
pub mod header;
pub mod media;
pub mod overlay;
pub mod revision;
pub mod package;
pub mod write;

use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use async_trait::async_trait;
use energyml_core::identifiers;
use energyml_core::identity;
use energyml_core::naming;
use energyml_core::reflect::TypeInfo;
use energyml_core::registry::{DynObject, ReadMode, Registry};

use crate::array::Resolution;
use crate::backend::Origin;
use energyml_core::value::Reflect;

use crate::array::{ArrayData, ArrayLocation, ArrayMetadata, ByteOrder, ProxyResolver};
use crate::error::{Diagnostic, Error, ExternalDataReason, Result};
use crate::resource::{Dataspace, ObjectHeader, ObjectSummary, Relationship};
use crate::storage::{EnergymlStorage, LocalPackage};

use overlay::{Change, Overlay, SavePolicy};
use revision::What;
use package::{Package, ReadSeek};

/// A part of the package that holds no readable object, and why.
///
/// What [`EpcStore::unreadable`] returns: the worklist of a repair, and the
/// only way to reach a part whose damage cost it its uri.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unreadable {
    /// Name of the part in the archive. Always usable with
    /// [`EpcStore::raw_part`] and [`EpcStore::set_raw_part`].
    pub part: String,
    /// Its uri, when the inventory could still address it. `None` is the worse
    /// case: the part is in the archive and in no listing.
    pub uri: Option<String>,
    /// Content type as `[Content_Types].xml` declares it, when it does.
    pub content_type: Option<String>,
    /// How many bytes are there — zero being a diagnosis of its own.
    pub bytes: usize,
    /// What went wrong, in the words the reader used.
    pub reason: String,
}

/// How a package is written.
#[derive(Debug, Clone)]
pub struct SaveOptions {
    /// `dc:creator` of the core properties. Mandatory in EPC (§3.5.3).
    ///
    /// `None` keeps whatever the source package said, and falls back to this
    /// crate only when it said nothing: the core properties
    /// are *kept* if they existed and *created* if they were missing. Only
    /// `created` is always rewritten, because it means the moment the package
    /// was made and this is that moment.
    pub creator: Option<String>,
    /// `dcterms:created`, in W3CDTF. `None` means now — which is what the
    /// specification asks for, since the field is the *package* creation time.
    /// Setting it explicitly makes a build reproducible, which is what the
    /// tests do.
    pub created: Option<String>,
    /// Where the XML parts go. The specification imposes nothing (§3.6); flat
    /// is what the widest range of tools reads back.
    pub layout: energyml_core::identity::EpcLayout,
}

impl Default for SaveOptions {
    fn default() -> Self {
        Self {
            creator: None,
            created: None,
            layout: energyml_core::identity::EpcLayout::Flat,
        }
    }
}

/// What one part of a package is, for a listing that shows the archive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartKind {
    /// An EnergyML object — the only kind the rest of this library reads.
    Object,
    /// `[Content_Types].xml`, the index OPC requires. Regenerated on save.
    ContentTypes,
    /// A `.rels`. Regenerated on save, from the objects' own references.
    Relationships,
    /// `docProps/core.xml`. Regenerated on save.
    CoreProperties,
    /// Anything else under `docProps/` — a producer's extension, copied
    /// verbatim because this library neither writes it nor understands it.
    PackageExtension,
    /// A file carried along: a PDF, an image, a note. Copied verbatim.
    Attachment,
}

/// One part of the package, as a listing shows it.
///
/// The three flags are the difference between the archive and what a save would
/// write, and they are separate because they are separate facts: a part can be
/// `added` and not `staged` in no case, `staged` without being `added` (bytes
/// repaired over an archive part), and `dropped` while still being in the
/// archive — which is precisely the state a listing has to be able to show.
#[derive(Clone, Debug)]
pub struct PartListing {
    /// The part name, which is its address inside the package.
    pub part: String,
    /// What it is: an object, the package's own machinery, or a file along for
    /// the ride.
    pub kind: PartKind,
    /// Uncompressed size, as declared, or the length of what is staged.
    pub size: Option<u64>,
    /// What `[Content_Types].xml` declares, or what a caller stated when adding
    /// the part. `None` leaves the writer to guess from the extension.
    pub content_type: Option<String>,
    /// For an object part: the uri it is addressed by.
    pub uri: Option<String>,
    /// For an object part: the type the inventory filed it under.
    pub qualified_type: Option<String>,
    /// Put here in this session; the archive never had it.
    pub added: bool,
    /// Removed in this session; a save will not write it.
    pub dropped: bool,
    /// Bytes are staged for it, so what a save writes is not what was opened.
    pub staged: bool,
}

/// One EnergyML part of the package, as the inventory knows it.
struct PartEntry {
    summary: ObjectSummary,
    /// The registered type, when the registry knows it. `None` means the
    /// package holds a version whose crate was not registered: the object is
    /// still listed — a listing that hides rows is worse than one that shows a
    /// row it cannot open — but reading it fails with [`Error::UnknownType`].
    info: Option<&'static TypeInfo>,
    /// The other versions the root element could also be, best first.
    ///
    /// Empty when the identification was unambiguous. When it is not, a read
    /// walks this list after `info` fails — see [`EpcStore::root_candidates`]
    /// for why one namespace covers every version of a package and
    /// `schemaVersion` cannot always settle it.
    alternatives: Vec<&'static TypeInfo>,
    /// Why no type reads it, when the answer is a version the registry **knows
    /// and does not hold** (`Registry::declare_absent`): the named refusal a
    /// read returns. `None` for a version nobody declared, which keeps the
    /// substitution and the plain `UnknownType`.
    refusal: Option<String>,
    /// The key the overlay and the journal know this entry by — see
    /// [`slot_key`]. Fixed when the entry is born and never recomputed: the
    /// uri of an entry can change (a second version of its uuid arrives), its
    /// slot cannot.
    slot: String,
    /// The `objectVersion` the part declared, `None` when it declares none. One
    /// third of the identity (ADR-0010): it is read by the peek
    /// every part already gets at open, so it costs no read.
    object_version: Option<String>,
}

/// An object this session created that the archive never held.
///
/// Kept beside the archive's entries so that a created object is addressable
/// at once (see the `made` field) — and with the identity it has
/// beside its namesakes: the slot, and the `objectVersion` that tells it from
/// another object of the same uuid and type.
struct Made {
    slot: String,
    summary: ObjectSummary,
    object_version: Option<String>,
}

/// One object of the package as it stands now: the archive's entries and the
/// objects created in this session, minus what was removed.
struct Live<'a> {
    slot: &'a str,
    summary: &'a ObjectSummary,
    version: Option<&'a str>,
    /// The archive's entry, when it has one.
    archive: Option<&'a PartEntry>,
}

/// What an address says, once parsed: a uuid, and — when the address is a URI —
/// the type and the version it names.
struct Address {
    /// Lowercased.
    uuid: String,
    qualified_type: Option<String>,
    version: Option<String>,
}

/// The identity key of an entry, the way the overlay and the journal spell it.
///
/// **An entry is (uuid, qualified type, objectVersion)** — Identifier
/// Specification v5.0 §3.4.2 (p. 14) puts the version in the URI beside the
/// uuid, and EPC v1.1 §3.6 (p. 17) lets a package hold a versioned object and
/// say so in its path. The overlay and the journal were keyed by the uuid; they
/// are now keyed by this, and **for the first entry of a uuid it is the uuid
/// itself** — so every package that never had two entries of one uuid (all of
/// the corpus) keeps exactly the keys it had. A second entry gets a composite:
/// the uuid, the lowercased type and the version in hex (the journal lowercases
/// its keys, and a version is case-sensitive), joined by the unit separator, so
/// the uuid is always what comes before the first one ([`uuid_of_slot`]).
fn slot_key(uuid: &str, qualified_type: &str, version: Option<&str>) -> String {
    let mut out = String::with_capacity(uuid.len() + qualified_type.len() + 8);
    out.push_str(&uuid.to_ascii_lowercase());
    out.push('\u{1f}');
    out.push_str(&qualified_type.to_ascii_lowercase());
    out.push('\u{1f}');
    for byte in version.unwrap_or_default().bytes() {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// The lowercased uuid a slot key belongs to.
fn uuid_of_slot(slot: &str) -> &str {
    slot.split('\u{1f}').next().unwrap_or(slot)
}

/// An `objectVersion` as an identity: surrounding blanks do not count, and an
/// empty one is no version — the element is optional, and an empty one says the
/// same as none.
fn normal_version(version: Option<&str>) -> Option<String> {
    version.map(str::trim).filter(|v| !v.is_empty()).map(str::to_string)
}

/// An EPC file, open for reading.
///
/// Reading only, in this pass. The in-memory layer that makes an open package
/// modifiable comes with the writing pass; the shape here
/// is what it will sit on.
pub struct EpcStore {
    package: Package,
    registry: Arc<Registry>,
    /// Name of the single dataspace this file is: the file name, or whatever
    /// the caller supplied when it opened bytes rather than a path.
    name: String,
    /// Where it came from, when it came from a disk.
    path: Option<PathBuf>,
    entries: Vec<PartEntry>,
    /// Lowercased uuid → the indices into `entries` of **every** part that
    /// carries it, in archive order: the same uuid under two `objectVersion`s,
    /// or under two releases, is two entries. Only an exact duplicate of
    /// (uuid, type, version) is dropped.
    by_uuid: HashMap<String, Vec<usize>>,
    /// Behind a mutex because a lazy reader keeps noticing things after it was
    /// opened: the fallback taken for a missing HDF5 proxy is discovered the
    /// first time an array is located.
    diagnostics: Mutex<Vec<Diagnostic>>,
    /// Which file each `(declared name, dataset)` pair was actually found in.
    ///
    /// The search opens files to ask what they hold; without this, a package
    /// with a thousand arrays would pay for that a thousand times.
    array_files: Mutex<HashMap<(String, String), std::path::PathBuf>>,
    /// The `externalResource` of a part's `.rels`, remembered by part name.
    ///
    /// # Why this is not a micro-optimisation
    ///
    /// An HDF5 proxy's own `.rels` holds one relationship **per object that
    /// references it**, and exactly one `externalResource` — the file name. On
    /// the Lyon package (4484 objects, 2694 array pointers) that part is
    /// **585 KB and 2687 relationships**, and every array pointer inflated and
    /// parsed the whole of it to pick the one entry that names the file. 1.6 GB
    /// of XML, re-parsed, to answer the same question 2694 times.
    ///
    /// Measured 08/09/2026, release build, warm cache: evaluating the package's
    /// 894 `WellboreFrameRepresentation`s took **12.2 s**, of which the frames'
    /// own HDF5 reads were 1.7 s. h5py reads the same 894 datasets in 0.12 s, so
    /// the file was never the problem. With this map: **0.9 s**, same 894
    /// representations and same 5516 points.
    ///
    /// **Nothing invalidates it, and that is a property rather than an
    /// oversight**: [`EpcStore::external_resource_in`] reads through
    /// `self.package` — the archive — and never consults the overlay, so no
    /// edit, removal or staged part can change what it answers. A memo of a
    /// function that already ignores every mutation cannot go stale. The day
    /// that function learns to read staged bytes, this map has to be cleared
    /// with them.
    external_resources: Mutex<HashMap<String, Option<String>>>,
    /// Every distinct file the package declares as an external resource,
    /// computed once, on demand. Reading every `.rels` at open would have made
    /// opening proportional to the number of objects, which is exactly what
    /// level 1 promises not to be.
    external_files: OnceLock<Vec<String>>,
    /// The media relations the **archive** records, computed once:
    /// `(part, lowercased uuid)` for every `destinationMedia` of an object's
    /// `.rels`, and for every `sourceMedia` of a non-object part's `.rels`.
    ///
    /// On a package of 16 250 parts, loading the scene had become endless — every `media_links` re-read and
    /// re-parsed every part's `.rels` (3.4 s a call), and the head images asked
    /// once per object read. Same argument as `external_resources`: it reads
    /// through `self.package` only, so no edit can make it stale; the overlay's
    /// relations are laid over it on every call, and the attachments filter is
    /// applied on every call too, since an edit can add or drop a part.
    archive_media: OnceLock<Vec<(String, String)>>,
    /// What has changed since the package was opened. Empty until
    /// something is edited, so a reader pays nothing for it.
    overlay: Overlay,
    /// Identities of objects the layer holds that the archive never did.
    ///
    /// # Why this exists, and what it was hiding
    ///
    /// [`EpcStore::resources`] already walked the overlay, so a created object
    /// appeared in an inventory. But [`EpcStore::resource`] and
    /// [`EpcStore::find_by_uuid`] both go through `by_uuid`, which is built from
    /// the archive and never touched again — so the same object was **listed and
    /// not addressable**. Anything asking *what is at this uri* got `NotFound`
    /// for something a caller had just created and could already read through
    /// [`EpcStore::get_object`].
    ///
    /// That is not a small inconsistency. Spelling a reference starts by asking
    /// for the holder's resource, so **no reference could be written from a
    /// newly created object at all** — which is what made a workshop create
    /// eighteen objects and link none of them, reporting only that some links
    /// could not be written. And `find_by_uuid` is what a reference resolves
    /// through, so every reference *pointing at* a created object read as
    /// dangling.
    ///
    /// A map rather than a recomputation, because both accessors hand back a
    /// borrow — and it never needs revisiting, since a uuid is immutable by
    /// schema rule: written on `put`, dropped on `remove`. Keyed by the
    /// lowercased uuid, **one list per uuid**: a second version of an object
    /// created here is a second [`Made`], and its uri is the one
    /// `reconcile_uris` gave it.
    made: HashMap<String, Vec<Made>>,
    /// When changes reach the disk.
    policy: SavePolicy,
    /// How to write, when the policy writes on its own.
    save_options: SaveOptions,
    /// Set by `close`, so `Drop` knows it has nothing left to complain about.
    closed: bool,
    /// How the names in array pointers become paths.
    external: crate::backend::ExternalFiles,
    /// What can read an array. Empty is a supported state, not a broken one.
    backends: crate::backend::Backends,
    /// External files declared by the caller rather than found in a `.rels`.
    ///
    /// The `.rels` of a package that was just opened say what its *author*
    /// wrote; a proxy created in this session has no `.rels` yet, and would
    /// otherwise be written out naming no file at all. Lowercased proxy uuid →
    /// file name.
    declared_files: std::collections::BTreeMap<String, String>,
    /// Content type of each attachment added in this session.
    ///
    /// The archive's own `[Content_Types].xml` answers for the parts that were
    /// in it; a part added since has no entry there, and the writer would fall
    /// back to guessing from the extension. Guessing is a reasonable last
    /// resort and a poor first one — a caller that *knows* it is handing over a
    /// PDF should not have that turned into `application/octet-stream` because
    /// the name ended in `.PDF`.
    attachment_types: std::collections::BTreeMap<String, String>,
    /// In-place edits whose `LINKS_OUT` is **provisional**: the journal said the
    /// references may have moved because the path could not say otherwise, and
    /// the references the object held *before* the edit are kept here so the
    /// flag can be taken off when they are found unmoved
    /// ([`EpcStore::settle_edits`]). Behind a mutex because the settling happens
    /// when the journal is read, through `&self`.
    pending_edits: Mutex<Vec<PendingEdit>>,
    /// The slots whose object was lent out for an edit in place since the last
    /// [`EpcStore::settle_versions`] — the only objects whose `objectVersion` can
    /// have drifted from their entry's. Settling walks these and nothing else: it
    /// runs at the start of every `put`, and walking the whole layer there made
    /// writing *n* objects cost *n²* (2 699 BSS wells, 70 000 objects,
    /// from 12 minutes to more than 50).
    unsettled: BTreeSet<String>,
    /// Read parts that break the schema's cardinality instead of refusing
    /// them. Off by default; see [`EpcStore::set_lenient`].
    lenient: bool,
    /// Refuse a part that omits something the schema requires. Off by default;
    /// see [`EpcStore::set_strict`].
    strict: bool,
}

/// The references an object held before an in-place edit, and the journal line
/// that said they may have moved. See [`EpcStore::edit_at`].
struct PendingEdit {
    revision: u64,
    /// The slot of the entry ([`slot_key`]) — its uuid for all but a second
    /// entry of one uuid.
    slot: String,
    references: Vec<(String, String)>,
}

/// How many provisional edits are remembered; older ones keep the wider answer.
const PENDING_EDITS: usize = 256;

/// `(path, uuid)` of every reference an object makes — what the front end's
/// `referencesMoved` compares, read from the reference graph instead of from the
/// text.
fn reference_list(object: &dyn Reflect) -> Vec<(String, String)> {
    crate::graph::references(object)
        .into_iter()
        .map(|pointer| (pointer.path, pointer.uuid.to_ascii_lowercase()))
        .collect()
}

impl EpcStore {
    /// Open an EPC file from disk.
    ///
    /// The registry decides which types can be deserialized; it is supplied,
    /// never assumed, so adding a schema version is an
    /// `register` call in the application and not a change here.
    pub fn open(path: impl AsRef<Path>, registry: Arc<Registry>) -> Result<Self> {
        let path = path.as_ref();
        let _span = tracing::info_span!("epc.open", path = %path.display()).entered();
        let file = std::fs::File::open(path)?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let mut store = Self::from_reader(Box::new(file), name, registry)?;
        // An array pointer names its file with a bare name, and the
        // specification says the file sits beside the package (§3.2.3).
        if let Some(folder) = path.parent() {
            store.external.set_root(folder);
        }
        store.path = Some(path.to_path_buf());
        Ok(store)
    }

    /// Open an EPC from anything readable and seekable — bytes in memory, for
    /// instance.
    ///
    /// A cloud service receives an EPC as the body of a response and has no
    /// disk to put it on, so this is a requirement rather than a convenience.
    pub fn from_reader(
        source: Box<dyn ReadSeek + Send>,
        name: impl Into<String>,
        registry: Arc<Registry>,
    ) -> Result<Self> {
        let _span = tracing::info_span!("epc.read_package").entered();
        let mut diagnostics = Vec::new();
        let package = Package::open(source, &mut diagnostics)?;
        let mut store = Self {
            package,
            registry,
            name: name.into(),
            path: None,
            entries: Vec::new(),
            by_uuid: HashMap::new(),
            diagnostics: Mutex::new(diagnostics),
            array_files: Mutex::new(HashMap::new()),
            external_resources: Mutex::new(HashMap::new()),
            external_files: OnceLock::new(),
            archive_media: OnceLock::new(),
            overlay: Overlay::new(),
            made: HashMap::new(),
            policy: SavePolicy::default(),
            save_options: SaveOptions::default(),
            closed: false,
            external: crate::backend::ExternalFiles::default(),
            backends: crate::backend::Backends::compiled_in(),
            declared_files: std::collections::BTreeMap::new(),
            attachment_types: std::collections::BTreeMap::new(),
            pending_edits: Mutex::new(Vec::new()),
            unsettled: BTreeSet::new(),
            lenient: false,
            strict: false,
        };
        store.build_inventory();
        Ok(store)
    }

    /// The file this store was opened from, when it was opened from one.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// The registry this store deserializes with.
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// The same registry, shareable — what opening a **second** store with the
    /// same schema knowledge needs, and what an extraction does.
    ///
    /// Handing out the `Arc` rather than making the caller build a registry
    /// again is not a convenience: two registries in one process can disagree
    /// about which versions exist, and a package written by one and read by the
    /// other would refuse parts it had just produced.
    pub fn registry_arc(&self) -> Arc<Registry> {
        Arc::clone(&self.registry)
    }

    // -- Level 1: the inventory ------------------------------------------

    /// The single dataspace this file is.
    ///
    /// An EPC **is** a dataspace, so this returns exactly one entry. That is
    /// what lets code walking a remote store walk a file unchanged.
    pub fn dataspace(&self) -> Dataspace {
        let (created, last_changed) = self.core_property_dates();
        Dataspace {
            uri: "eml:///".to_string(),
            name: self.name.clone(),
            created,
            last_changed,
        }
    }

    /// Every EnergyML object in the package, identity only.
    ///
    /// Built once when the package was opened, from `[Content_Types].xml` and —
    /// only for the parts that file gets wrong — the root element. Then merged
    /// with the in-memory layer: what was removed is gone from this list, what
    /// was added is in it.
    ///
    /// **One row per entry**, an entry being (uuid, qualified type,
    /// `objectVersion`): the same uuid under two versions, or under two
    /// releases, is two rows, each with the uri that names it alone — see
    /// [`EpcStore::resolve`].
    pub fn resources(&self) -> Vec<ObjectSummary> {
        let mut out: Vec<ObjectSummary> = self
            .entries
            .iter()
            .filter(|entry| !matches!(self.overlay.get(&entry.slot), Some(Change::Removed)))
            .map(|entry| entry.summary.clone())
            .collect();

        for (slot, change) in self.overlay.changed() {
            let Change::Put(object) = change else { continue };
            if self.is_archive_slot(slot) {
                continue; // already listed, from the archive
            }
            // The summary `put` gave it — its uri is reconciled with its
            // namesakes — or, for a layer that never went through `put`, the
            // identity the object states.
            let summary = self
                .made_by_slot(slot)
                .map(|made| made.summary.clone())
                .or_else(|| self.summary_of(object.as_reflect()));
            out.extend(summary);
        }
        out
    }

    /// The identity an object added to the layer will have once written.
    ///
    /// The **short** uri: whether it must be spelled with a version depends on
    /// the object's namesakes, which only the store knows —
    /// [`EpcStore::reconcile_uris`] settles it.
    fn summary_of(&self, object: &dyn Reflect) -> Option<ObjectSummary> {
        let key = identity::key_of_reflect(object)?;
        Some(ObjectSummary {
            uri: key.etp_uri(None),
            uuid: key.uuid.to_string(),
            qualified_type: key.qualified_type(),
            content_type: key.content_type(),
            locator: key.part_path(self.save_options.layout),
        })
    }

    /// The summary of one object.
    ///
    /// The archive's entries and the objects created in this session are one
    /// namespace: an object created here is addressable the moment it exists —
    /// an accessor that said otherwise made it impossible to write a reference
    /// from one. A uri that names more than one object is refused,
    /// [`Error::Ambiguous`], with the uri of each candidate.
    pub fn resource(&self, uri: &str) -> Result<&ObjectSummary> {
        self.locate(uri).map(|live| live.summary)
    }

    /// Which object an address names — [`EpcStore::resource`] under the name
    /// the rules are written for. **Rule**: the short uri is accepted when there is
    /// no conflict (or when the object has no version), and the version is
    /// required when there is one. So:
    ///
    /// - a **versioned** uri (`(uuid=…,version='…')`, Identifier Specification
    ///   v5.0 §3.4.2, p. 14) names exactly that entry, or nothing —
    ///   [`Error::NotFound`];
    /// - a **short** uri names the one entry of its (uuid, type); when there
    ///   are several, the one that declares no version, if exactly one does not
    ///   (§3.4.2.1, p. 15 leaves *the most recent* to the server and ETP 1.2
    ///   gives no way to tell, so this store does not guess); otherwise
    ///   [`Error::Ambiguous`], naming every candidate;
    /// - a **bare uuid** names the one entry that carries it, or is ambiguous.
    ///
    /// A type that differs only by its release still resolves while there is a
    /// single candidate (*a wrong version is not a wrong type*): it is the
    /// checker's business to report it, not the lookup's to refuse it.
    pub fn resolve(&self, uri: &str) -> Result<&ObjectSummary> {
        self.resource(uri)
    }

    /// The object a **reference** names, resolved by (uuid, qualified type,
    /// `objectVersion` when the reference carries one) with the rules of
    /// [`EpcStore::resolve`]. A reference whose version no entry carries still
    /// resolves when it has a single candidate — the checker reports the
    /// mismatch (`ObjectVersionMismatch`) — and is [`Error::NotFound`] when
    /// several entries leave no way to say which was meant.
    pub fn resolve_reference(
        &self,
        uuid: &str,
        qualified_type: Option<&str>,
        object_version: Option<&str>,
    ) -> Result<&ObjectSummary> {
        self.locate_triple(uuid, qualified_type, object_version, true).map(|live| live.summary)
    }

    /// **The one lookup.** The entry named by (uuid, qualified type, `objectVersion`),
    /// each part optional but the uuid: the uuid first, then the type, then the
    /// version — and the version only matters when more than one entry is left
    /// (Identifier Specification v5.0 §3.4.2.1, p. 15). [`EpcStore::resolve`] (a
    /// URI) and [`EpcStore::lookup_uuid`] (a bare uuid) are this with the triple
    /// read from their argument; every resolution inside the store goes through
    /// it, and no other code filters entries by uuid, type or version. The refusal
    /// names what was asked, spelled from the triple.
    pub fn lookup(
        &self,
        uuid: &str,
        qualified_type: Option<&str>,
        object_version: Option<&str>,
    ) -> Result<&ObjectSummary> {
        self.locate_triple(uuid, qualified_type, object_version, false).map(|live| live.summary)
    }

    /// [`EpcStore::lookup`] with only a uuid: the one entry that carries it, or
    /// [`Error::Ambiguous`] when the uuid alone is not enough.
    pub fn lookup_uuid(&self, uuid: &str) -> Result<&ObjectSummary> {
        self.lookup(uuid, None, None)
    }

    /// The core, over a triple. `reference` is the grace a reference gets for a
    /// version nobody carries (see `choose`).
    fn locate_triple(
        &self,
        uuid: &str,
        qualified_type: Option<&str>,
        object_version: Option<&str>,
        reference: bool,
    ) -> Result<Live<'_>> {
        let version = normal_version(object_version);
        let given = match qualified_type {
            Some(qualified_type) => {
                identifiers::EtpUri::object_versioned(None, qualified_type, uuid, version.as_deref())
            }
            None => uuid.to_string(),
        };
        Self::choose(
            self.live_of(&uuid.to_ascii_lowercase()),
            qualified_type,
            version.as_deref(),
            &given,
            reference,
        )
    }

    /// The object carrying this uuid, whatever its type.
    ///
    /// A uuid identifies an object across every version, so this is what a
    /// reference resolves through — see [`crate::graph::check_references`]. With
    /// several entries under one uuid it answers **one of them** — the one that
    /// declares no `objectVersion` if there is one, otherwise the first in
    /// archive order — which is enough for what it is used for, *is there such
    /// an object*. Whoever needs *which* asks [`EpcStore::resolve_reference`]
    /// or [`EpcStore::resolve`], which refuse to guess.
    pub fn find_by_uuid(&self, uuid: &str) -> Option<&ObjectSummary> {
        // A reference resolves through this. Without the layer, every
        // reference pointing at an object created in this session read as
        // dangling — the package it was written into held it.
        Self::primary(self.live_of(&uuid.to_ascii_lowercase())).map(|live| live.summary)
    }

    /// Every object of the package that carries this uuid — one per entry: the
    /// same uuid under two `objectVersion`s, or under two releases, is two
    /// summaries, each with the uri that names it alone. Empty when there is
    /// none. What a derived view of the package asks when a uuid is journaled and
    /// it must know whether the uuid still names a single object.
    pub fn summaries_of(&self, uuid: &str) -> Vec<ObjectSummary> {
        self.live_of(&uuid.to_ascii_lowercase()).into_iter().map(|one| one.summary.clone()).collect()
    }

    /// Every entry of one uuid — the archive's and this session's — minus what
    /// was removed. `uuid` is lowercased.
    fn live_of(&self, uuid: &str) -> Vec<Live<'_>> {
        let mut out = Vec::new();
        for &at in self.by_uuid.get(uuid).map(Vec::as_slice).unwrap_or_default() {
            let entry = &self.entries[at];
            if matches!(self.overlay.get(&entry.slot), Some(Change::Removed)) {
                continue;
            }
            out.push(Live {
                slot: &entry.slot,
                summary: &entry.summary,
                version: self.current_version(&entry.slot, entry.object_version.as_deref()),
                archive: Some(entry),
            });
        }
        for made in self.made.get(uuid).map(Vec::as_slice).unwrap_or_default() {
            // `made` is not rolled back with a transaction; the layer is, and it
            // is the layer that says whether the object is still there.
            if !matches!(self.overlay.get(&made.slot), Some(Change::Put(_))) {
                continue;
            }
            out.push(Live {
                slot: &made.slot,
                summary: &made.summary,
                version: self.current_version(&made.slot, made.object_version.as_deref()),
                archive: None,
            });
        }
        out
    }

    /// The `objectVersion` an entry has **now**: the object the layer holds when it
    /// holds one — an edit of the version is visible to a lookup at once — else
    /// what the entry was born with.
    fn current_version<'a>(&'a self, slot: &str, stored: Option<&'a str>) -> Option<&'a str> {
        match self.overlay.get(slot) {
            Some(Change::Put(object)) => identity::object_version_of_reflect(object.as_reflect())
                .map(str::trim)
                .filter(|version| !version.is_empty()),
            _ => stored,
        }
    }

    /// The entry that stands for its uuid when only the uuid is known: the one
    /// without a version, else the first.
    fn primary(mut live: Vec<Live<'_>>) -> Option<Live<'_>> {
        match live.iter().position(|one| one.version.is_none()) {
            Some(at) => Some(live.swap_remove(at)),
            None if live.is_empty() => None,
            None => Some(live.remove(0)),
        }
    }

    /// Which entry an address names. See [`EpcStore::resolve`].
    fn locate(&self, uri: &str) -> Result<Live<'_>> {
        let address = address_of(uri)?;
        Self::choose(
            self.live_of(&address.uuid),
            address.qualified_type.as_deref(),
            address.version.as_deref(),
            uri,
            false,
        )
    }

    /// The one entry of `live` that an address (or a reference) names — or a
    /// **named** refusal. `reference` makes a version no entry carries resolve
    /// when there is a single candidate: a reference is a copy of what its
    /// target says, and a copy that is stale is a finding, not a dangling
    /// pointer.
    fn choose<'a>(
        mut live: Vec<Live<'a>>,
        qualified_type: Option<&str>,
        version: Option<&str>,
        given: &str,
        reference: bool,
    ) -> Result<Live<'a>> {
        if live.is_empty() {
            return Err(Error::NotFound(given.to_string()));
        }
        // The type narrows when it matches something; when it matches nothing it
        // is a release apart, and the uuid alone decides.
        if let Some(qualified_type) = qualified_type
            && live.iter().any(|one| one.summary.qualified_type == qualified_type)
        {
            live.retain(|one| one.summary.qualified_type == qualified_type);
        }
        if let Some(wanted) = version {
            let hits = live.iter().filter(|one| one.version == Some(wanted)).count();
            match hits {
                0 if reference && live.len() == 1 => return Ok(live.remove(0)),
                0 => return Err(Error::NotFound(given.to_string())),
                _ => live.retain(|one| one.version == Some(wanted)),
            }
        }
        if live.len() == 1 {
            return Ok(live.remove(0));
        }
        // Several: a short uri still names the one that declares no version —
        // the rule above — if exactly one does. A bare uuid names no type, but
        // when every candidate is of one type it says exactly what the short uri
        // of that type says, and gets the same answer: the readers of the domain
        // resolve a reference through `ObjectSource::object(uuid)`, and a
        // reference to the original of a versioned copy must not stop resolving
        // because the copy arrived. Two types under one uuid stay ambiguous.
        let one_type = live.iter().all(|one| one.summary.qualified_type == live[0].summary.qualified_type);
        if (qualified_type.is_some() || one_type) && version.is_none() {
            let bare: Vec<usize> =
                live.iter().enumerate().filter(|(_, one)| one.version.is_none()).map(|(at, _)| at).collect();
            if let [only] = bare[..] {
                return Ok(live.remove(only));
            }
        }
        Err(Error::Ambiguous {
            uri: given.to_string(),
            candidates: live.iter().map(|one| one.summary.uri.clone()).collect(),
        })
    }

    /// The slot of the entry an address names.
    fn slot_of(&self, uri: &str) -> Result<String> {
        self.locate(uri).map(|live| live.slot.to_string())
    }

    /// Whether an archive entry holds this slot.
    fn is_archive_slot(&self, slot: &str) -> bool {
        self.by_uuid
            .get(uuid_of_slot(slot))
            .is_some_and(|list| list.iter().any(|&at| self.entries[at].slot == slot))
    }

    /// The object this session created, by slot.
    fn made_by_slot(&self, slot: &str) -> Option<&Made> {
        self.made.get(uuid_of_slot(slot))?.iter().find(|made| made.slot == slot)
    }

    /// Spell the uri of every live entry of a uuid, again: **with its version
    /// whenever it declares one**, the short form only for an entry that
    /// declares none — internally the uri always carries the version number when
    /// there is one (ADR-0003). The short form stays
    /// accepted **as an address** ([`EpcStore::resolve`]), never emitted for a
    /// versioned entry. Called whenever the set of entries of a uuid
    /// changes: at open, after a put, after a removal. Returns, for each entry
    /// whose uri moved, its slot and the uri it had — the caller says so in the
    /// journal (an identity event, ADR-0010: *a second version
    /// changes the uri of the first*).
    fn reconcile_uris(&mut self, uuid: &str) -> Vec<(String, String)> {
        let key = uuid.to_ascii_lowercase();
        // (in the archive?, position, slot, type, version) of everything live.
        let mut rows: Vec<(bool, usize, String, String, Option<String>)> = Vec::new();
        for &at in self.by_uuid.get(&key).map(Vec::as_slice).unwrap_or_default() {
            let entry = &self.entries[at];
            if matches!(self.overlay.get(&entry.slot), Some(Change::Removed)) {
                continue;
            }
            rows.push((
                true,
                at,
                entry.slot.clone(),
                entry.summary.qualified_type.clone(),
                self.current_version(&entry.slot, entry.object_version.as_deref()).map(str::to_string),
            ));
        }
        for (at, made) in self.made.get(&key).map(Vec::as_slice).unwrap_or_default().iter().enumerate() {
            if !matches!(self.overlay.get(&made.slot), Some(Change::Put(_))) {
                continue;
            }
            rows.push((
                false,
                at,
                made.slot.clone(),
                made.summary.qualified_type.clone(),
                self.current_version(&made.slot, made.object_version.as_deref()).map(str::to_string),
            ));
        }
        let mut moved = Vec::new();
        for (archive, at, slot, qualified_type, version) in &rows {
            let summary = if *archive {
                &mut self.entries[*at].summary
            } else {
                match self.made.get_mut(&key) {
                    Some(list) => &mut list[*at].summary,
                    None => continue,
                }
            };
            let uri = identifiers::EtpUri::object_versioned(
                None,
                qualified_type,
                &summary.uuid,
                version.as_deref(),
            );
            if summary.uri != uri {
                moved.push((slot.clone(), std::mem::replace(&mut summary.uri, uri)));
            }
        }
        moved
    }

    // -- Level 2: the headers --------------------------------------------

    /// Citations for the given objects, without deserializing them.
    ///
    /// One result per requested URI, in the same order; an unreadable object
    /// does not fail the others. Each part is inflated only as far as the end
    /// of its citation.
    pub fn headers(&self, uris: &[String]) -> Vec<Result<ObjectHeader>> {
        uris.iter().map(|uri| self.header(uri)).collect()
    }

    /// The citation of one object.
    ///
    /// From the object itself when the in-memory layer holds it — reading the
    /// archive would return the citation as it was before the edit.
    pub fn header(&self, uri: &str) -> Result<ObjectHeader> {
        let slot = self.slot_of(uri)?;
        if let Some(Change::Put(object)) = self.overlay.get(&slot) {
            return self.header_of(object.as_reflect());
        }
        let entry = self.entry(uri)?;
        let peeked = self
            .package
            .with_part(&entry.summary.locator, |r| header::peek_reader(BufReader::new(r)))?;
        Ok(ObjectHeader {
            summary: entry.summary.clone(),
            title: peeked.title,
            originator: peeked.originator,
            creation: peeked.creation,
            last_update: peeked.last_update,
            format: peeked.format,
            object_version: peeked.object_version,
            schema_version: peeked.schema_version,
        })
    }

    /// Every object's citation, in inventory order.
    ///
    /// What an interface listing the file shows. Costs one partial inflate per
    /// object and no full deserialization at all.
    pub fn all_headers(&self) -> Vec<Result<ObjectHeader>> {
        self.entries.iter().map(|e| self.header(&e.summary.uri)).collect()
    }

    // -- Level 3: the objects --------------------------------------------

    /// The objects themselves.
    ///
    /// One result per requested URI, in the same order.
    pub fn get_objects(&self, uris: &[String]) -> Vec<Result<Box<dyn DynObject>>> {
        uris.iter().map(|uri| self.get_object(uri)).collect()
    }

    /// One object, fully deserialized.
    ///
    /// The in-memory layer first: an object that was edited must not come back
    /// as it was on disk. Each caller gets its own copy.
    pub fn get_object(&self, uri: &str) -> Result<Box<dyn DynObject>> {
        match self.overlay.get(&self.slot_of(uri)?) {
            Some(Change::Put(object)) => return Ok(object.clone_box()),
            Some(Change::Removed) => return Err(Error::NotFound(uri.to_string())),
            None => {}
        }
        let entry = self.entry(uri)?;
        let Some(info) = entry.info else {
            // A version the project ships and this build does not carry is
            // refused by the sentence that names what would bring it; any
            // other unregistered type stays the plain unknown one.
            return Err(match &entry.refusal {
                Some(why) => energyml_core::error::Error::VersionAbsent(why.clone()).into(),
                None => Error::UnknownType(entry.summary.content_type.clone()),
            });
        };
        let part = entry.summary.locator.clone();
        let bytes = self.raw_part(&part)?;

        // The version the identification settled on, then the ones it could not
        // rule out. One namespace covers every version of a package and
        // `schemaVersion` is optional in practice, so "which release is this"
        // is sometimes a question only the parser can answer — and it answers
        // it by failing on the wrong one. See `root_candidates`.
        let mut attempts = Vec::with_capacity(1 + entry.alternatives.len());
        attempts.push(info);
        attempts.extend(entry.alternatives.iter().copied());

        // **Two passes, and the first one is strict on purpose.**
        //
        // Telling two releases apart works by letting the wrong one fail, and
        // an element required on one side and absent on the other is very often
        // the only difference the parser can see. Reading tolerantly on the
        // first pass would blunt exactly that: the wrong version would no
        // longer refuse, and a part would come back as a plausible object of
        // the wrong release with only the fields the two share.
        //
        // So: strict first, over the candidates best-first — that is the pass
        // that decides *which release this is*. Only when no candidate reads
        // strictly does the caller's own mode get a turn, and by then the
        // question is no longer which version but how much to put right.
        let passes: &[ReadMode] = match self.mode() {
            ReadMode::Strict => &[ReadMode::Strict],
            ReadMode::Tolerant => &[ReadMode::Strict, ReadMode::Tolerant],
            ReadMode::Lenient => &[ReadMode::Strict, ReadMode::Lenient],
        };

        let mut first: Option<Error> = None;
        for mode in passes {
        for (rank, candidate) in attempts.iter().enumerate() {
            // Dispatched on the **candidate**, not on its content type. A
            // development drop shares that string with the release it precedes
            // — `resqml 2.2_dev3` declares `version="2.2"` — so a lookup by
            // content type hands every candidate of such a pair the same type,
            // and this walk would read the same thing each time while looking
            // like it had tried both.
            let outcome = self
                .registry
                .read_xml_as_named(candidate, &bytes, *mode)
                .or_else(|first| {
                    // A type the registry cannot address by name: fall back to
                    // the string, which is what every release pair used before
                    // drops existed.
                    match candidate.content_type {
                        Some(ct) => self.registry.read_xml_as(ct, &bytes, *mode),
                        None => self
                            .registry
                            .read_xml_as(&entry.summary.content_type, &bytes, *mode)
                            .map_err(|_| first),
                    }
                });
            match outcome {
                Ok((object, departures)) => {
                    if rank > 0 {
                        self.record(Diagnostic::warning(
                            &part,
                            format!(
                                "would not read as `{}`; read as `{}` instead — the document's \
                                 namespace names the package and not the version",
                                attempts[0].qualified_type.unwrap_or(attempts[0].name),
                                candidate.qualified_type.unwrap_or(candidate.name)
                            ),
                        ));
                    }
                    // Vocal, as everywhere else here: the object now holds
                    // values nobody wrote, and a caller that saves it would put
                    // them on disk. The two kinds are kept apart — an absent
                    // element is a producer forgetting, a value that could not
                    // be read is a producer writing something wrong, and the
                    // second is the worse news.
                    for item in departures.defaulted {
                        self.record(Diagnostic::warning(&part, item.to_string()));
                    }
                    for item in departures.repaired {
                        self.record(Diagnostic::warning(&part, item.to_string()));
                    }
                    return Ok(object);
                }
                Err(e) => {
                    // The error worth reporting is the one from the strict
                    // pass over the document's own version: it names what is
                    // actually wrong, where a later pass would report the
                    // second-best guess failing for a second-best reason.
                    if first.is_none() {
                        first = Some(e.into());
                    }
                }
            }
        }
        }
        // Every candidate refused it in every pass. The first error is the one
        // to report: it is the version the document itself pointed at, read
        // strictly.
        Err(first.unwrap_or_else(|| Error::UnknownType(entry.summary.content_type.clone())))
    }

    /// Read parts that break the schema's cardinality instead of refusing
    /// them.
    ///
    /// Off by default, and it should stay off for anything that will be
    /// written back: an absent required element comes back as its Rust default
    /// — an empty string, a zero — which is not what the document said. Each
    /// one becomes a `Diagnostic`, so [`diagnostics`][Self::diagnostics] tells
    /// exactly what was invented and in which part.
    ///
    /// It earns its place on packages a producer wrote without some mandatory
    /// element: refusing those loses the geometry, the references and the
    /// citations the file *did* carry, over a field nothing downstream reads.
    pub fn set_lenient(&mut self, lenient: bool) {
        self.lenient = lenient;
    }

    /// Refuse a part that omits something the schema requires.
    ///
    /// Off by default. An absent required element is filled with its Rust
    /// default and reported as a diagnostic, because refusing there costs a
    /// caller every field the part *did* carry — see
    /// [`ReadMode`].
    ///
    /// Turn it on for the one job where the difference bites: **writing the
    /// package back**. An object read tolerantly holds values nobody wrote, and
    /// saving it puts them on disk. That is why the round-trip check sets this.
    pub fn set_strict(&mut self, strict: bool) {
        self.strict = strict;
    }

    /// Whether parts that omit a required item are refused.
    pub fn is_strict(&self) -> bool {
        self.strict
    }

    /// The reading mode the two switches add up to.
    fn mode(&self) -> ReadMode {
        match (self.strict, self.lenient) {
            (true, _) => ReadMode::Strict,
            (false, true) => ReadMode::Lenient,
            (false, false) => ReadMode::Tolerant,
        }
    }

    /// Whether parts breaking the schema's cardinality are read.
    pub fn is_lenient(&self) -> bool {
        self.lenient
    }

    /// The raw XML of one part, as stored — or as staged, once it has been.
    ///
    /// For a round-trip check, or for a caller that has its own parser.
    pub fn part_bytes(&self, uri: &str) -> Result<Vec<u8>> {
        let part = self.entry(uri)?.summary.locator.clone();
        self.raw_part(&part)
    }

    /// The bytes of a part **by part name**, whether or not it is an object.
    ///
    /// The addressing of last resort, and the reason it exists: a part whose
    /// root element cannot be read has no uuid, so no uri, so
    /// [`EpcStore::part_bytes`] cannot reach it. This can. What comes back is
    /// what a save would write — the staged bytes when there are any, the
    /// archive's otherwise.
    pub fn raw_part(&self, part: &str) -> Result<Vec<u8>> {
        if let Some(bytes) = self.overlay.raw(part) {
            return Ok(bytes.to_vec());
        }
        self.package.read(part)
    }

    /// Stage replacement bytes for a part, to be written **verbatim**.
    ///
    /// The repair path, and the last one there is. Every other way of changing
    /// a package goes through an object: read it, edit it by reflection, let
    /// the writer serialise it back. That requires the part to parse, and the
    /// whole point here is that it does not.
    ///
    /// So these bytes are not parsed, not validated and not rewritten — they go
    /// into the archive as they are. Two consequences worth stating, because
    /// neither is recoverable afterwards:
    ///
    /// - **The `.rels` are not regenerated for this part.** They are built from
    ///   an object's `DataObjectReference`s, and there is no object. Whatever
    ///   relationships the part had are the ones it keeps.
    /// - **Nothing checks that the bytes are the object the package thinks the
    ///   part holds.** Staging a point set under a triangulated set's part name
    ///   produces a package that lies, and says nothing.
    ///
    /// Both are the price of being able to fix a file no parser will touch.
    /// [`EpcStore::staged_part_parses`] answers the first question a caller
    /// should ask before saving.
    pub fn set_raw_part(&mut self, part: &str, bytes: Vec<u8>) {
        self.overlay.set_raw(part, bytes);
        // A part's bytes are what `get_object` reads, and a part name carries
        // no uuid to name: every derived view rebuilds.
        self.overlay.touch_all();
    }

    /// Drop what was staged for a part. `true` when there was something.
    pub fn clear_raw_part(&mut self, part: &str) -> bool {
        let dropped = self.overlay.clear_raw(part);
        if dropped {
            self.overlay.touch_all();
        }
        dropped
    }

    /// The revision of the last object write — what a derived view of the
    /// package (`energyml_domain::package_index`) compares with its own to know
    /// whether it is still exact (ADR-0010).
    pub fn revision(&self) -> u64 {
        self.overlay.revision()
    }

    /// How many times the package's **files** changed — a part staged or
    /// dropped, a media relation made or undone, a transaction opened, merged or
    /// thrown away. Not an object write, so not in [`EpcStore::revision`]: a
    /// reader of what is attached to an object compares this stamp with its own
    /// and re-reads when they differ.
    pub fn files_revision(&self) -> u64 {
        self.overlay.files_revision()
    }

    /// Which objects were written after `since`: nothing, these uuids
    /// (lower-cased), or everything — a reader re-reads accordingly.
    pub fn changed_since(&self, since: u64) -> revision::Changed {
        match self.overlay.changed_since(since) {
            // The journal names slots; a reader of this answer knows uuids, and
            // two versions of one uuid are one name to it.
            revision::Changed::Some(slots) => {
                revision::Changed::Some(slots.iter().map(|slot| uuid_of_slot(slot).to_string()).collect())
            }
            other => other,
        }
    }

    /// What was written after `since`, and **what in each object** — the
    /// journal of ADR-0010: per object
    /// the citation, the body, the links, a creation, a removal, an identity
    /// change; the datasets written; whether parts or media moved; the data file
    /// names declared. [`revision::Since::Everything`] when the log cannot say:
    /// the reader rebuilds, it never concludes "nothing".
    ///
    /// Uris are in **the store's spelling** (what `resources()` lists); an object
    /// the store no longer knows keeps the uri its gate gave.
    pub fn changes_since(&self, since: u64) -> revision::Since {
        self.settle_edits(None);
        match self.overlay.changes_since(since) {
            revision::Since::Exact(mut change) => {
                // The journal keys its lines by slot ([`slot_key`]): the uuid for
                // all but a second entry of one uuid. What a reader gets is the
                // uuid, and the uri that tells the entries of one uuid apart.
                for object in &mut change.objects {
                    if let Some(known) = self.known_uri(&object.uuid) {
                        object.uri = Some(known);
                    }
                    object.uuid = uuid_of_slot(&object.uuid).to_string();
                }
                change.objects.sort_by(|a, b| (&a.uuid, &a.uri).cmp(&(&b.uuid, &b.uri)));
                revision::Since::Exact(change)
            }
            everything => everything,
        }
    }

    /// Datasets of a data file were written **by someone other than this store**
    /// — the front end through h5wasm, a tool that wrote the `.h5` itself — and the
    /// journal must say so, at the same place it says what happened to objects.
    /// The writes made through [`EpcStore::write_array_to`] and
    /// [`EpcStore::write_arrays_to`] are journaled by those gates; calling this
    /// for them too is harmless, only wide.
    ///
    /// `file` is the name the package gives the file. Returns the new revision.
    pub fn arrays_written(&self, file: &str, paths: &[&str]) -> u64 {
        if paths.is_empty() {
            return self.data_file_written(file);
        }
        self.overlay.journal().note_datasets(file, paths.iter().copied())
    }

    /// A data file was brought, replaced or created **as a whole** — an import
    /// that copies it, a creation that writes it. Its datasets are not listed:
    /// the journal says the file (an empty path).
    pub fn data_file_written(&self, file: &str) -> u64 {
        self.overlay.journal().note_datasets(file, std::iter::empty())
    }

    /// The uri this store gives an object it holds or held, if it can tell.
    fn known_uri(&self, slot: &str) -> Option<String> {
        // The entry the slot belongs to, as the store spells it now — which, for
        // an entry held beside a namesake, is the form with its version.
        let held = self
            .by_uuid
            .get(uuid_of_slot(slot))
            .and_then(|list| list.iter().map(|&at| &self.entries[at]).find(|entry| entry.slot == slot))
            .map(|entry| (&entry.summary, entry.object_version.as_deref()))
            .or_else(|| self.made_by_slot(slot).map(|made| (&made.summary, made.object_version.as_deref())));
        if let Some(Change::Put(object)) = self.overlay.get(slot) {
            let stated = self.summary_of(object.as_reflect())?;
            let stated_version = normal_version(identity::object_version_of_reflect(object.as_reflect()));
            // An object edited in place may state another identity than the one
            // the entry was born with (an edited uuid, **or an edited
            // `objectVersion`** — an object is (uuid, type, version), and two
            // versions of one uuid are two objects): its journal line then names
            // the identity it states.
            return match held {
                Some((entry, version))
                    if entry.uuid.eq_ignore_ascii_case(&stated.uuid)
                        && entry.qualified_type == stated.qualified_type
                        && normal_version(version) == stated_version =>
                {
                    Some(entry.uri.clone())
                }
                _ => Some(stated.uri),
            };
        }
        held.map(|(summary, _)| summary.uri.clone())
    }

    /// Take `LINKS_OUT` off the journal lines of in-place edits whose object
    /// turns out to make the same references as before — the refinement the
    /// design asks for (ADR-0010), done at the gate instead of at the index
    /// because the gate is where the *before* is at hand. With `only`, just that
    /// object's pending edit (it is about to be edited again).
    fn settle_edits(&self, only: Option<&str>) {
        let mut pending = self.pending_edits.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if pending.is_empty() {
            return;
        }
        let mut kept = Vec::new();
        for edit in pending.drain(..) {
            if only.is_some_and(|slot| slot != edit.slot) {
                kept.push(edit);
                continue;
            }
            if let Some(Change::Put(object)) = self.overlay.get(&edit.slot)
                && reference_list(object.as_reflect()) == edit.references
            {
                self.overlay.journal().narrow(edit.revision, &edit.slot, What::LINKS_OUT);
            }
        }
        *pending = kept;
    }

    /// The parts with bytes staged for them.
    pub fn staged_parts(&self) -> Vec<String> {
        self.overlay.raw_parts()
    }

    // -- The package as a container of files ---------------------------------

    /// Every part of the package, whatever it is — **the archive as a listing**.
    ///
    /// # Why the library and not the caller
    ///
    /// A front end can open the same zip itself, and the answer would be wrong in
    /// exactly the way that matters: the bytes on disk are not the package any
    /// more once anything has been edited. A part added in this session is in no
    /// archive, a part dropped is still in one, and an object edited is neither
    /// its old bytes nor a file yet. This walks what a **save would write**, and
    /// marks the three states rather than flattening them.
    pub fn parts(&self) -> Vec<PartListing> {
        let mut out: Vec<PartListing> = Vec::new();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        // A map, not `entries.iter().find` per part: on a package of 16 250
        // parts that was a quarter of a billion comparisons, 2.3 s a call
        // (the scene's loading had become endless).
        let objects: HashMap<&str, &PartEntry> = self.entries.iter().map(|e| (e.summary.locator.as_str(), e)).collect();

        for name in self.package.entries() {
            if name.ends_with('/') {
                continue;
            }
            seen.insert(name.clone());
            let entry = objects.get(name.as_str()).copied();
            out.push(PartListing {
                kind: match () {
                    _ if entry.is_some() => PartKind::Object,
                    _ if name == "[Content_Types].xml" => PartKind::ContentTypes,
                    _ if name.ends_with(".rels") => PartKind::Relationships,
                    _ if name.eq_ignore_ascii_case("docProps/core.xml") => PartKind::CoreProperties,
                    _ if name.starts_with("docProps/") => PartKind::PackageExtension,
                    _ => PartKind::Attachment,
                },
                uri: entry.map(|e| e.summary.uri.clone()),
                qualified_type: entry.map(|e| e.summary.qualified_type.clone()),
                size: self.package.size(name),
                content_type: self.package.content_type(name).map(str::to_string),
                part: name.clone(),
                added: false,
                // Both of the ways a part leaves: dropped by a caller, or
                // superseded by an object that now stands where it stood.
                dropped: self.overlay.is_dropped(name) || self.overlay.is_superseded(name),
                staged: self.overlay.raw(name).is_some(),
            });
        }

        // What the archive never held: attachments added since it was opened.
        for part in self.overlay.raw_parts() {
            if seen.contains(&part) {
                continue;
            }
            let bytes = self.overlay.raw(&part).map(<[u8]>::len).unwrap_or(0) as u64;
            out.push(PartListing {
                kind: PartKind::Attachment,
                uri: None,
                qualified_type: None,
                size: Some(bytes),
                content_type: self.attachment_types.get(&part).cloned(),
                part,
                added: true,
                dropped: false,
                staged: true,
            });
        }

        out.sort_by(|a, b| a.part.cmp(&b.part));
        out
    }

    /// Put a file in the package — a PDF, an image, a note.
    ///
    /// # What it refuses, and why each refusal is not a nuisance
    ///
    /// - **A part name an object already occupies.** Those bytes are written
    ///   from the object on save, so an attachment there would either be
    ///   overwritten without a word or would silently replace an object with
    ///   something that is not one. Repairing an object's bytes is
    ///   [`EpcStore::set_raw_part`], which says what it is doing.
    /// - **The package's own infrastructure** — `[Content_Types].xml`, a
    ///   `.rels`, `docProps/core.xml`. All three are *regenerated* by the writer
    ///   from the objects in memory, so bytes put there would be discarded
    ///   silently at the exact moment they were supposed to take effect.
    /// - **A name OPC or EPC would not accept, or reserves** — see
    ///   [`media::check_part_name`], each rule with its section.
    /// - **A name that differs from another part's only by case.** OPC compares
    ///   part names without case, so the package would hold one part twice.
    /// - **A content type that is not a media type** ([`media::check_media_type`]).
    ///
    /// The content type is the caller's to state. Absent, the writer guesses
    /// from the extension, which is what OPC's `Default` entries are for.
    ///
    /// Which object the file is about is a relationship, not a content type:
    /// [`EpcStore::relate_media`].
    pub fn set_attachment(
        &mut self,
        part: &str,
        bytes: Vec<u8>,
        content_type: Option<String>,
    ) -> Result<()> {
        let part = part.trim_start_matches('/').to_string();
        if part.is_empty() || part.ends_with('/') {
            return Err(Error::Package("an attachment needs a part name".to_string()));
        }
        if self.entries.iter().any(|e| e.summary.locator == part) {
            return Err(Error::Package(format!(
                "`{part}` is the part of an object; a save writes it from the object, so bytes \
                 placed there would be discarded. Use a different name, or `set_raw_part` to \
                 repair the object itself."
            )));
        }
        if part == "[Content_Types].xml"
            || part.ends_with(".rels")
            || part.eq_ignore_ascii_case("docProps/core.xml")
        {
            return Err(Error::Package(format!(
                "`{part}` is regenerated when the package is written; bytes placed there would \
                 be discarded"
            )));
        }
        // Then what OPC and EPC say of any part name and any content type —
        // each rule with its section in `media`.
        let part = media::check_part_name(&part)?;
        let content_type = content_type.map(|declared| media::check_media_type(&declared)).transpose()?;
        if let Some(other) = self
            .package
            .entries()
            .iter()
            .filter(|name| !self.overlay.is_dropped(name))
            .cloned()
            .chain(self.overlay.raw_parts())
            .find(|name| *name != part && name.eq_ignore_ascii_case(&part))
        {
            return Err(Error::Package(format!(
                "`{part}` and `{other}` differ only by case, and OPC compares part names without \
                 case: the package would hold one part twice"
            )));
        }

        // A part put back after being dropped is a part that is there again.
        self.overlay.restore_part(&part);
        match content_type {
            Some(declared) => {
                self.attachment_types.insert(part.clone(), declared);
            }
            None => {
                self.attachment_types.remove(&part);
            }
        }
        self.overlay.set_raw(&part, bytes);
        Ok(())
    }

    /// Take a file back out of the package.
    ///
    /// Only an attachment: an object is removed through [`EpcStore::remove`],
    /// which knows to take its `.rels` and its index entry with it. Refusing
    /// here rather than deleting a part behind the inventory's back is the
    /// difference between removing an object and making one unreachable.
    pub fn remove_attachment(&mut self, part: &str) -> Result<()> {
        let part = part.trim_start_matches('/');
        if self.entries.iter().any(|e| e.summary.locator == part) {
            return Err(Error::Package(format!(
                "`{part}` holds an object; remove the object by its uri, so the package's index \
                 and relationships go with it"
            )));
        }
        if !self.package.has(part) && self.overlay.raw(part).is_none() {
            return Err(Error::Package(format!("no part `{part}` in the package")));
        }
        // Both, and in this order: bytes added in this session simply cease to
        // exist, while a part that came from the archive has to be *remembered*
        // as removed or the save would copy it through again.
        self.overlay.clear_raw(part);
        self.attachment_types.remove(part);
        if self.package.has(part) {
            self.overlay.drop_part(part);
        }
        Ok(())
    }

    /// Say that a part of the package is a media representation of an object —
    /// EPC §3.4.2 (pp. 15–16): `destinationMedia` in the object's `.rels`,
    /// `sourceMedia` in the part's, both written on save (see [`media`]).
    ///
    /// `object` is an ETP uri or a bare uuid. Refused by name: an object the
    /// package does not hold, and a part that is not one of its attachments —
    /// put the bytes in first ([`EpcStore::set_attachment`]); a relationship to
    /// an object's own part, or to nothing, is not a media representation.
    pub fn relate_media(&mut self, object: &str, part: &str) -> Result<()> {
        let (uuid, part) = self.media_ends(object, part)?;
        self.overlay.set_media_link(&part, &uuid, true);
        Ok(())
    }

    /// Take a media relation back out. `true` when there was one.
    ///
    /// The part stays in the package; removing it is
    /// [`EpcStore::remove_attachment`], which takes its relations with it.
    pub fn unrelate_media(&mut self, object: &str, part: &str) -> Result<bool> {
        let (uuid, part) = self.media_ends(object, part)?;
        let was = self.media_links().iter().any(|link| link.part == part && link.uuid == uuid);
        self.overlay.set_media_link(&part, &uuid, false);
        Ok(was)
    }

    /// Every media relation a save would write: what the archive's `.rels`
    /// said — `destinationMedia` from an object, `sourceMedia` from a part,
    /// either one is the link — with this session's changes over it.
    ///
    /// **Read back from the archive, so a rewrite keeps them.** The `.rels`
    /// are regenerated on save from what the store knows; before this, a media
    /// relation read in a package was lost at the next save (a gap found
    /// while building the synthetic reference model, ADR-0018). A part no longer in the package takes its
    /// relations with it; an object removed does not — the writer names that
    /// relation rather than dropping it in silence.
    ///
    /// This reads the `.rels` of every object and every attachment: a cost
    /// proportional to the package, paid by a save anyway.
    pub fn media_links(&self) -> Vec<media::MediaLink> {
        let mut links: BTreeSet<(String, String)> = self.archive_media().iter().cloned().collect();
        let attachments: HashSet<String> = self.attachments().into_iter().collect();
        for ((part, uuid), related) in self.overlay.media_links() {
            if related {
                links.insert((part, uuid));
            } else {
                links.remove(&(part, uuid));
            }
        }
        links
            .into_iter()
            .filter(|(part, _)| attachments.contains(part))
            .map(|(part, uuid)| media::MediaLink { part, uuid })
            .collect()
    }

    /// What the archive's `.rels` say about media, read once (`archive_media`).
    fn archive_media(&self) -> &Vec<(String, String)> {
        self.archive_media.get_or_init(|| {
            let mut links: BTreeSet<(String, String)> = BTreeSet::new();
            let objects: HashMap<&str, &str> =
                self.entries.iter().map(|e| (e.summary.locator.as_str(), e.summary.uuid.as_str())).collect();
            for entry in &self.entries {
                let Ok(relationships) = self.relationships_recorded(&entry.summary.locator) else {
                    continue;
                };
                for relationship in relationships {
                    if relationship.external || relationship.short_kind() != media::DESTINATION_MEDIA {
                        continue;
                    }
                    if let Some(part) = media::resolve_target(&entry.summary.locator, &relationship.target) {
                        links.insert((part, entry.summary.uuid.to_ascii_lowercase()));
                    }
                }
            }
            for part in self.package.entries().iter() {
                if is_package_infrastructure(part) || objects.contains_key(part.as_str()) {
                    continue;
                }
                let Ok(relationships) = self.relationships_recorded(part) else {
                    continue;
                };
                for relationship in relationships {
                    if relationship.external || relationship.short_kind() != media::SOURCE_MEDIA {
                        continue;
                    }
                    let Some(object_part) = media::resolve_target(part, &relationship.target) else {
                        continue;
                    };
                    if let Some(uuid) = objects.get(object_part.as_str()) {
                        links.insert((part.clone(), uuid.to_ascii_lowercase()));
                    }
                }
            }
            links.into_iter().collect()
        })
    }

    /// The media parts related to one object, by part name.
    pub fn media_of(&self, object: &str) -> Result<Vec<String>> {
        let uuid = uuid_in_uri(object)?.to_ascii_lowercase();
        Ok(self.media_links().into_iter().filter(|link| link.uuid == uuid).map(|link| link.part).collect())
    }

    /// The two ends of a media relation, checked: the object's lowercased uuid
    /// and the part's name.
    fn media_ends(&self, object: &str, part: &str) -> Result<(String, String)> {
        let uuid = uuid_in_uri(object)?.to_ascii_lowercase();
        if self.find_by_uuid(&uuid).is_none() {
            return Err(Error::NotFound(object.to_string()));
        }
        let part = part.trim_start_matches('/').to_string();
        if !self.attachments().contains(&part) {
            return Err(Error::Package(format!(
                "`{part}` is not a file of this package, so it cannot be a media representation of \
                 {uuid}; put it in first (`set_attachment`)"
            )));
        }
        Ok((uuid, part))
    }

    /// Whether the bytes staged for a part would now read as an object, and as
    /// what.
    ///
    /// The check to run between staging and saving: `Ok(qualified type)` means
    /// the repair worked, `Err` means it did not and says what is still wrong.
    ///
    /// Deliberately **not** called by [`EpcStore::set_raw_part`]. Staging bytes
    /// that still do not parse is a legitimate step — a repair can take more
    /// than one pass, and refusing to hold an intermediate state would make the
    /// tool useless for the files that need it most.
    pub fn staged_part_parses(&self, part: &str) -> Result<String> {
        let (info, _) = self.staged_object(part)?;
        Ok(info.qualified_type.unwrap_or(info.name).to_string())
    }

    /// Read what is staged for a part, as the type it says it is.
    ///
    /// The one place that decides *which* type a repaired part is, so the check
    /// and the staging cannot disagree about it. The content type comes from
    /// the type found rather than from `[Content_Types].xml`: a part the index
    /// never declared — the invisible case — has none to offer, and the root
    /// element is what identified it in the first place.
    fn staged_object(&self, part: &str) -> Result<(&'static TypeInfo, Box<dyn DynObject>)> {
        let bytes = self.raw_part(part)?;
        let peeked = header::peek_reader(BufReader::new(std::io::Cursor::new(&bytes)));
        if peeked.root_name.is_empty() {
            return Err(Error::Package(format!("{part}: no root element")));
        }
        let declared = self.package.content_type(part).map(str::to_string);
        let info = declared
            .as_deref()
            .and_then(|ct| self.registry.by_content_type(ct).map(|e| e.info))
            .or_else(|| self.type_of_root(&peeked.root_name, peeked.schema_version.as_deref()))
            .ok_or_else(|| {
                Error::Package(format!(
                    "{part}: root element `{}` names no type this build knows",
                    peeked.root_name
                ))
            })?;
        let content_type = info
            .content_type
            .map(str::to_string)
            .or(declared)
            .ok_or_else(|| Error::Package(format!("{part}: no content type to read it as")))?;
        let object = self
            .registry
            .read_xml_as(&content_type, &bytes, self.mode())
            .map(|(object, _)| object)
            .map_err(|e| Error::Package(format!("{part}: {e}")))?;
        Ok((info, object))
    }

    /// Stage repaired bytes, and put the part back among the objects.
    ///
    /// The difference from [`EpcStore::set_raw_part`] is what happens on
    /// success, and it is the whole reason this exists: bytes that **parse**
    /// are not staged as bytes at all. They become an object in the ordinary
    /// layer, so the writer serialises it like any other and — the part that
    /// matters — **rebuilds its `.rels` from its own `DataObjectReference`s**.
    /// A part repaired through `set_raw_part` alone comes out of the save with
    /// no relationships, because verbatim bytes have no references a writer can
    /// see.
    ///
    /// So: repair through this, and fall back to [`EpcStore::set_raw_part`]
    /// only for bytes that still do not parse and have to be written anyway.
    ///
    /// Returns the qualified type it now reads as. On failure **nothing is
    /// staged**: the store is left exactly as it was.
    pub fn stage_repaired_part(&mut self, part: &str, bytes: Vec<u8>) -> Result<String> {
        let previous = self.overlay.raw(part).map(<[u8]>::to_vec);
        self.overlay.set_raw(part, bytes);
        let (info, object) = match self.staged_object(part) {
            Ok(found) => found,
            Err(e) => {
                match previous {
                    Some(bytes) => self.overlay.set_raw(part, bytes),
                    None => {
                        self.overlay.clear_raw(part);
                    }
                }
                return Err(e);
            }
        };
        // It parses, so it rejoins the normal world: the object layer, where
        // the writer can see its references and rebuild its `.rels` — and where
        // it is placed under the part name its type and uuid dictate, which for
        // a part the inventory had lost is not the name it arrived under.
        self.overlay.clear_raw(part);
        self.overlay.supersede(part);
        self.put(object)?;
        Ok(info.qualified_type.unwrap_or(info.name).to_string())
    }

    /// Every part this build cannot turn into an object.
    ///
    /// The worklist of a repair. Two kinds are in it, and the second is the one
    /// that used to be invisible:
    ///
    /// - a part the inventory knows, whose XML fails to parse — it has a uri;
    /// - a part the inventory **dropped**, because the damage reached the root
    ///   element or the uuid and nothing could address it. It has no uri, only
    ///   a name, which is why [`EpcStore::raw_part`] takes one.
    ///
    /// Attachments that were never meant to be objects — a PDF, an image — are
    /// not in it. The test is the extension and the declared content type: an
    /// `.xml` part of an EPC that is not package infrastructure was meant to be
    /// an object, and if it is not one, something is wrong with it.
    pub fn unreadable(&self) -> Vec<Unreadable> {
        let mut out = Vec::new();
        for entry in &self.entries {
            if matches!(self.overlay.get(&entry.slot), Some(Change::Removed)) {
                continue;
            }
            let part = entry.summary.locator.clone();
            if let Err(e) = self.get_object(&entry.summary.uri) {
                out.push(Unreadable {
                    bytes: self.raw_part(&part).map(|b| b.len()).unwrap_or(0),
                    part,
                    uri: Some(entry.summary.uri.clone()),
                    content_type: Some(entry.summary.content_type.clone()),
                    reason: e.to_string(),
                });
            }
        }
        for part in self.attachments() {
            if !part.to_ascii_lowercase().ends_with(".xml") {
                continue;
            }
            let declared = self.package.content_type(&part).map(str::to_string);
            // An attachment declared as something other than EnergyML is an
            // attachment, not a casualty.
            if let Some(content_type) = &declared
                && identity::parse_content_type(content_type).is_err()
            {
                continue;
            }
            let reason = self
                .diagnostics()
                .into_iter()
                .find(|d| d.part == part)
                .map(|d| d.message)
                .unwrap_or_else(|| "the inventory could not address it".to_string());
            out.push(Unreadable {
                bytes: self.raw_part(&part).map(|b| b.len()).unwrap_or(0),
                part,
                uri: None,
                content_type: declared,
                reason,
            });
        }
        out.sort_by(|a, b| a.part.cmp(&b.part));
        out
    }

    /// The types this store can produce: whatever the registry it was given
    /// knows about.
    pub fn supported_types(&self) -> Vec<&'static TypeInfo> {
        self.registry.top_level_types().collect()
    }

    // -- External data ----------------------------------------------------

    /// Every array the object points at, with proxies resolved through this
    /// package's relationships.
    ///
    /// Both spellings are handled: the eml 2.3 `ExternalDataArrayPart`, which
    /// carries the file name itself, and the older `Hdf5Dataset`, whose file
    /// name is supposed to live in the `.rels` of its
    /// `EpcExternalPartReference`. "Supposed to" is doing work in that sentence
    /// — see [`EpcStore::external_file_for`].
    pub fn arrays_of(&self, object: &dyn Reflect) -> Vec<ArrayLocation> {
        // The part this object sits in, whose `.rels` may name its file. Of two
        // entries sharing a uuid, the one that is this object — its type and
        // version — rather than the first.
        let stated = identity::key_of_reflect(object).map(|key| {
            (key.uuid.to_ascii_lowercase(), key.qualified_type(), normal_version(key.object_version))
        });
        let source = stated.and_then(|(uuid, qualified_type, version)| {
            let list = self.by_uuid.get(&uuid)?;
            let own = list.iter().map(|&at| &self.entries[at]).find(|entry| {
                entry.summary.qualified_type == qualified_type && entry.object_version == version
            });
            own.or_else(|| list.first().map(|&at| &self.entries[at])).map(|entry| entry.summary.locator.clone())
        });
        let resolver = |proxy_uuid: &str| self.external_target_for(proxy_uuid, source.as_deref());
        crate::array::locate_with(object, &resolver)
    }

    /// Shape and element type of an array, without reading it — ETP protocol 9,
    /// `GetDataArrayMetadata`.
    ///
    /// What a viewer asks before deciding whether it can afford the data.
    pub fn array_metadata(&self, location: &ArrayLocation) -> Result<ArrayMetadata> {
        self.attempt(location, |backend, file| {
            backend.metadata(file, &location.path_in_file)
        })
    }

    /// Whether **the file the location names** holds its dataset — that file
    /// alone.
    ///
    /// Not [`EpcStore::array_metadata`], whose search goes on to a companion
    /// or a neighbour when the named file says no, records the departure and
    /// remembers the file it settled on. That is right for reading and wrong
    /// for asking *is this path free in this file* before writing there
    /// (`import`): the answer would come from another file, and the question
    /// would leave a diagnostic and a resolution behind in the package's own
    /// health report. Here nothing is searched, recorded or remembered, and
    /// every failure — no such file, no backend — is `false`.
    pub fn holds_array(&self, location: &ArrayLocation) -> bool {
        self.array_backend(location)
            .is_ok_and(|(backend, file)| backend.metadata(&file, &location.path_in_file).is_ok())
    }

    /// Which end of a multi-byte number the file writes first.
    ///
    /// Asked apart from [`EpcStore::array_metadata`] because it answers a
    /// different need: a viewer wants a shape and a type, and a **writer** —
    /// one declaring an EML 2.3 external array, whose storage type carries the
    /// order — wants this. `None` when the backend cannot say or the width
    /// makes the question meaningless; never a guess, because writing the usual
    /// order where the file holds the other makes every value read wrong while
    /// the document stays valid.
    pub fn array_byte_order(&self, location: &ArrayLocation) -> Result<Option<ByteOrder>> {
        self.attempt(location, |backend, file| backend.byte_order(file, &location.path_in_file))
    }

    /// An array, or the slice the location names — `GetDataArrays` and
    /// `GetDataSubarrays`.
    ///
    /// Fails with [`ExternalDataReason::FeatureNotEnabled`] when this build has
    /// no backend for the format: a **missing capability**, which the caller
    /// must be able to tell apart from missing data.
    pub fn read_array(&self, location: &ArrayLocation) -> Result<ArrayData> {
        self.attempt(location, |backend, file| {
            backend.read(file, &location.path_in_file, &location.start, &location.count)
        })
    }

    /// Read an array **into memory the caller already owns**.
    ///
    /// The path a 3D application takes: allocate the vertex buffer once, ask
    /// for the shape with [`EpcStore::array_metadata`], and have the file
    /// decoded straight into it. Nothing is allocated here and nothing is
    /// copied afterwards.
    pub fn read_array_into(
        &self,
        location: &ArrayLocation,
        out: &mut crate::array::ArrayBuffer<'_>,
    ) -> Result<ArrayMetadata> {
        // Not through [`EpcStore::attempt`], and the difference matters: the
        // buffer belongs to the caller, so a first attempt that failed halfway
        // may already have written into it, and retrying would lay a second
        // read over a partial one. The file is settled **before** anything is
        // written — one metadata call, remembered, next to a read of a million
        // points.
        let (backend, file) = match self.array_backend(location) {
            Ok((backend, file)) if backend.metadata(&file, &location.path_in_file).is_ok() => {
                (backend, file)
            }
            _ => self.search_array_file(location)?,
        };
        backend.read_into(
            &file,
            &location.path_in_file,
            &location.start,
            &location.count,
            out,
        )
    }

    /// Check that an external file is the one the package means.
    ///
    /// EPC §3.2.3 requires an HDF5 file to carry a `uuid` attribute at its root
    /// equal to that of its `EpcExternalPartReference`. Nothing else catches
    /// the mistake this catches: reading somebody else's file because it
    /// happened to have the right name.
    ///
    /// `Ok(())` when they agree **or when the file claims no uuid** — the
    /// newer EnergyML form has no proxy, so there is nothing to compare with.
    pub fn verify_external_file(&self, location: &ArrayLocation) -> Result<()> {
        let Some(expected) = &location.proxy_uuid else {
            return Ok(());
        };
        if !energyml_core::identifiers::is_uuid(expected) {
            return Ok(()); // not a uuid at all; already reported when resolving
        }
        // Through the same two speeds as a read, so that what is verified is
        // the file that would actually be read — checking the guess while
        // reading the search's answer would be a cross-check of nothing.
        let (found, file) = self.attempt(location, |backend, file| {
            Ok((backend.file_uuid(file)?, file.to_path_buf()))
        })?;
        let Some(found) = found else {
            return Ok(());
        };
        if found.eq_ignore_ascii_case(expected) {
            return Ok(());
        }
        Err(Error::ExternalData {
            location: file.display().to_string(),
            reason: ExternalDataReason::UuidMismatch {
                expected: expected.clone(),
                found,
            },
        })
    }

    /// Write an array to the file an existing pointer names.
    ///
    /// The counterpart of [`EpcStore::read_array`], and it goes through the
    /// same resolution: the pointer says which file and which path inside it,
    /// so replacing the values of an array an object already declares needs
    /// nothing else.
    ///
    /// Writing under a *new* pointer is [`EpcStore::write_array_to`], which
    /// takes the file name directly — a location that resolves to nothing
    /// cannot say where to put anything.
    pub fn write_array(&self, location: &ArrayLocation, data: &crate::array::ArrayData) -> Result<()> {
        let name = self.resolved_file(location).ok_or_else(|| Error::ExternalData {
            location: location.field_path.clone(),
            reason: ExternalDataReason::FileMissing,
        })?;
        self.write_array_to(&name, &location.path_in_file, data)
    }

    /// Several arrays into one external file, the file opened **once** by a
    /// backend that can ([`crate::ArrayBackend::write_many`]). One result per
    /// item, in order; the shape of each is checked as
    /// [`EpcStore::write_array_to`] checks it.
    pub fn write_arrays_to(&self, file_name: &str, items: &[(&str, &crate::array::ArrayData)]) -> Vec<Result<()>> {
        // One span per file, with the number of datasets — never one per array.
        let _span = tracing::info_span!("epc.write_arrays", file = file_name, datasets = items.len()).entered();
        let checked: Vec<Result<()>> = items
            .iter()
            .map(|(path_in_file, data)| {
                let promised: i64 = data.metadata.dimensions.iter().product::<i64>().max(0);
                if promised as usize != data.values.len() {
                    return Err(Error::ExternalData {
                        location: format!("{file_name}:{path_in_file}"),
                        reason: ExternalDataReason::ShapeDisagrees { promised, held: data.values.len() },
                    });
                }
                Ok(())
            })
            .collect();
        let refuse_all = |why: String| -> Vec<Result<()>> {
            items
                .iter()
                .map(|(path_in_file, _)| {
                    Err(Error::ExternalData {
                        location: format!("{file_name}:{path_in_file}"),
                        reason: ExternalDataReason::Backend(why.clone()),
                    })
                })
                .collect()
        };
        let probe = ArrayLocation { file: file_name.to_string(), mime: None, ..Default::default() };
        let backend = match self.backends.find(&probe, std::path::Path::new(file_name)) {
            Ok(backend) => backend,
            Err(why) => return refuse_all(why.to_string()),
        };
        if !backend.writes() {
            return refuse_all(format!("the `{}` backend reads but does not write", backend.name()));
        }
        let path = match self.external.destination_for(file_name) {
            Ok(path) => path,
            Err(why) => return refuse_all(why.to_string()),
        };
        // Only the items whose shape agrees go to the backend; the others keep
        // their refusal, in place.
        let good: Vec<(&str, &crate::array::ArrayData)> =
            items.iter().zip(&checked).filter(|(_, ok)| ok.is_ok()).map(|(item, _)| *item).collect();
        // Journaled whatever the backend answers: a write that failed half way
        // may have changed some of the datasets, and a journal that says less
        // than the file holds leaves a cache serving numbers that are gone.
        self.overlay.journal().note_datasets(file_name, good.iter().map(|(path_in_file, _)| *path_in_file));
        let mut written = backend.write_many(&path, &good).into_iter();
        checked.into_iter().map(|ok| if ok.is_ok() { written.next().unwrap_or(Ok(())) } else { ok }).collect()
    }

    /// Write an array to a named external file, creating it if needed.
    ///
    /// The file is placed by [`ExternalFiles::destination_for`]: an override if
    /// one is pinned, else beside the package. Nothing is created until the
    /// backend is found, so a build without HDF5 leaves no empty file behind.
    ///
    /// This writes the *numbers*. The EnergyML object that points at them, and
    /// the `.rels` entry naming the file, are
    /// [`EpcStore::register_external_file`] and a `put` of the object — see
    /// that method for why the two halves are separate.
    ///
    /// [`ExternalFiles::destination_for`]: crate::backend::ExternalFiles::destination_for
    pub fn write_array_to(
        &self,
        file_name: &str,
        path_in_file: &str,
        data: &crate::array::ArrayData,
    ) -> Result<()> {
        // **The shape and the values must agree**, and it is checked here
        // rather than left to each backend: a shape promising more numbers than
        // there are writes without error in some formats and reads back with
        // zeros at the end. Every writer gets this — the command line, the
        // build layer, the Python bridge — because it is the file that would be
        // wrong, not one caller.
        let promised: i64 = data.metadata.dimensions.iter().product::<i64>().max(0);
        if promised as usize != data.values.len() {
            return Err(Error::ExternalData {
                location: format!("{file_name}:{path_in_file}"),
                reason: ExternalDataReason::ShapeDisagrees {
                    promised,
                    held: data.values.len(),
                },
            });
        }
        let probe = ArrayLocation {
            file: file_name.to_string(),
            path_in_file: path_in_file.to_string(),
            mime: None,
            ..Default::default()
        };
        let backend = self.backends.find(&probe, std::path::Path::new(file_name))?;
        if !backend.writes() {
            return Err(Error::ExternalData {
                location: format!("{file_name}:{path_in_file}"),
                reason: ExternalDataReason::Backend(format!(
                    "the `{}` backend reads but does not write",
                    backend.name()
                )),
            });
        }
        let path = self.external.destination_for(file_name)?;
        // Journaled whatever the backend answers, for the reason given in
        // `write_arrays_to`.
        self.overlay.journal().note_datasets(file_name, std::iter::once(path_in_file));
        backend.write(&path, path_in_file, data)
    }

    /// Stamp an external file with the uuid of the proxy that owns it.
    ///
    /// EPC §3.2.3 requires it, and [`EpcStore::verify_external_file`] enforces
    /// it on the way back in — so a file written without this is one this very
    /// crate refuses to trust.
    pub fn stamp_external_file(&self, file_name: &str, proxy_uuid: &str) -> Result<()> {
        let probe = ArrayLocation {
            file: file_name.to_string(),
            ..Default::default()
        };
        let backend = self.backends.find(&probe, std::path::Path::new(file_name))?;
        let path = self.external.destination_for(file_name)?;
        backend.set_file_uuid(&path, proxy_uuid)
    }

    /// What will read the array, and from where.
    ///
    /// The backend is chosen **before** the file is looked for, and the order
    /// matters: a build with no HDF5 backend cannot read the array whether or
    /// not the file is on the disk, and "no support for this format" is the
    /// more useful of the two answers. Once a backend exists, a file that is
    /// not there is a file that is not there.
    /// Which file to try **first** for this array.
    ///
    /// Optimistic on purpose: whatever the search settled on last time, or else
    /// the first candidate, handed back without opening anything. In the normal
    /// case — the document names a file that exists and holds the dataset —
    /// this is the whole resolution and it costs nothing.
    ///
    /// When the guess is wrong the read fails, and [`EpcStore::attempt`] runs
    /// [`EpcStore::search_array_file`], which opens candidates and asks each
    /// whether it holds the dataset. Probing first would be simpler to read and
    /// would double the file opens of every correct package to pay for the
    /// broken ones.
    fn array_backend(
        &self,
        location: &ArrayLocation,
    ) -> Result<(&dyn crate::backend::ArrayBackend, std::path::PathBuf)> {
        let declared = self.resolved_file(location).unwrap_or_default();
        if let Some(found) = self.remembered(&declared, &location.path_in_file) {
            let backend = self.backends.find(location, &found)?;
            return Ok((backend, found));
        }
        // Only among the places someone **stated**: what the caller pinned,
        // what the caller offered, what the document wrote. A companion or a
        // neighbour is a *recovery*, and handing one back unopened would be the
        // silent-wrong-file mistake this whole design is arranged against (ADR-0005) —
        // those go through the search, which asks before it answers.
        let candidates = self.external.candidates(&declared, self.path.as_deref());
        // Kept so that "nothing can read this format" does not come out as
        // "the file is not there". A build with no HDF5 cannot read the array
        // whether or not the file is on the disk, and sending someone to look
        // for a file that is sitting right beside the package costs an hour.
        let mut no_backend: Option<Error> = None;
        for candidate in &candidates {
            if candidate.origin > Origin::Declared {
                break;
            }
            match self.backends.find(location, &candidate.path) {
                Ok(backend) => return Ok((backend, candidate.path.clone())),
                Err(why) => no_backend.get_or_insert(why),
            };
        }
        Err(missing_or_unsupported(location, &self.backends, no_backend, None))
    }

    /// Which file actually holds this array — by opening candidates and asking.
    ///
    /// **The content decides, not the name.** The document's name is tried
    /// first because it is usually right, but a candidate that exists and does
    /// not hold the dataset steps aside for the next one rather than failing
    /// the read — see [`ExternalFiles::candidates`] for the order and
    /// [`Origin`] for why each place is in it.
    ///
    /// That is what makes a renamed pair readable. Someone who renames
    /// `model.epc` and `model.h5` to `model_final.*` has not lost a number;
    /// they have broken a string, and every array they own is still sitting
    /// next to the package. Refusing them over the string is refusing to read
    /// data that is present.
    ///
    /// Every departure from what the document named is recorded. Being
    /// permissive is a trade — the name is also what stops a package reading
    /// somebody else's file — and it is paid for by saying so, once per array
    /// pointer, with what was asked for and what was used instead.
    ///
    /// [`ExternalFiles::candidates`]: crate::backend::ExternalFiles::candidates
    /// [`Origin`]: crate::backend::Origin
    fn search_array_file(
        &self,
        location: &ArrayLocation,
    ) -> Result<(&dyn crate::backend::ArrayBackend, std::path::PathBuf)> {
        let (backend, resolution) = self.search_array(location)?;
        Ok((backend, resolution.path))
    }

    /// The same search, keeping **why** the answer was chosen.
    ///
    /// What [`EpcStore::resolve_array`] hands a caller that is about to tell a
    /// person where their numbers came from, or to write the answer back into
    /// the package.
    fn search_array(
        &self,
        location: &ArrayLocation,
    ) -> Result<(&dyn crate::backend::ArrayBackend, Resolution)> {
        let declared = self.resolved_file(location).unwrap_or_default();
        let dataset = location.path_in_file.clone();
        let candidates = self.external.candidates(&declared, self.path.as_deref());

        let mut opened = 0usize;
        let mut no_backend: Option<Error> = None;
        for candidate in &candidates {
            // A file no backend reads is not a candidate at all: no format, no
            // question to ask it. The reason is kept: if *no* candidate ever
            // produced a backend, the answer is "nothing here reads this
            // format", not "the file is missing".
            let backend = match self.backends.find(location, &candidate.path) {
                Ok(backend) => backend,
                Err(why) => {
                    no_backend.get_or_insert(why);
                    continue;
                }
            };
            // The question. An empty dataset path asks nothing, and existence
            // is a test the candidate list has already applied.
            if !dataset.is_empty() {
                opened += 1;
                if backend.metadata(&candidate.path, &dataset).is_err() {
                    continue;
                }
            }

            if candidate.origin.is_a_departure() {
                self.record(Diagnostic::warning(
                    &location.field_path,
                    match declared.is_empty() {
                        true => format!(
                            "no part, relationship or declaration names the file holding this \
                             array; found `{}` — {} — which holds `{dataset}`",
                            candidate.path.display(),
                            candidate.origin.describe()
                        ),
                        false => format!(
                            "the array names `{declared}`, which does not hold `{dataset}`; \
                             read from `{}` instead — {}",
                            candidate.path.display(),
                            candidate.origin.describe()
                        ),
                    },
                ));
            }
            self.remember(&declared, &dataset, &candidate.path);
            return Ok((
                backend,
                Resolution {
                    declared: declared.clone(),
                    path: candidate.path.clone(),
                    origin: candidate.origin,
                },
            ));
        }

        // Nothing held it. The message says how hard it looked, because "file
        // missing" over a folder full of files is the least useful thing this
        // could say.
        //
        // **Kept, not just logged.** This used to go only into the diagnostic
        // log, a separate channel a caller may never read; the error itself
        // carried nothing past "file not found", which is what a viewer showed
        // next to an object whose file was open on disk under a name the
        // package's own relationships did not use. The detail now rides on the
        // error's `location` too, the way the wasm backend already keeps what
        // the browser said. The
        // *reason* stays [`ExternalDataReason::FileMissing`], because that is
        // what tells the caller to try another candidate or ask for the file;
        // only the display string carries the how-hard-it-looked detail.
        let detail = match (opened, &no_backend) {
            // Zero opens *and* no backend anywhere: the search never asked a
            // question, it had nothing to ask it with.
            (0, Some(_)) => format!(
                "`{dataset}` was not looked for: no backend in this build reads {}",
                location.mime.as_deref().unwrap_or("this format")
            ),
            (0, None) => format!(
                "`{dataset}` is in none of the {} candidate file(s) for `{declared}`",
                candidates.len()
            ),
            (n, _) => format!(
                "`{dataset}` is in none of the {} candidate file(s) for `{declared}`; \
                 {n} of them were opened and did not hold it",
                candidates.len()
            ),
        };
        self.record(Diagnostic::warning(&location.field_path, detail.clone()));
        Err(missing_or_unsupported(location, &self.backends, no_backend, Some(&detail)))
    }

    /// Where an array's numbers actually are, and why there.
    ///
    /// The answer a person needs when the package says one thing and the disk
    /// says another: not "file missing", but *this file, for this reason*. What
    /// `energyml h5` prints, and what it writes back with `--fix`.
    pub fn resolve_array(&self, location: &ArrayLocation) -> Result<Resolution> {
        self.search_array(location).map(|(_, resolution)| resolution)
    }

    /// Do something with an array's file, and if that fails, **look harder**.
    ///
    /// The two-speed shape the whole resolution rests on: the guess costs
    /// nothing and is right almost always; the search costs file opens and only
    /// runs when the guess was wrong. A package whose names are correct never
    /// pays for the permissiveness that rescues one whose names are not.
    ///
    /// The retry runs only when the search lands somewhere **different**, so a
    /// genuine read error — a corrupt dataset, a shape that does not fit — is
    /// reported once and not attempted twice.
    fn attempt<T>(
        &self,
        location: &ArrayLocation,
        act: impl Fn(&dyn crate::backend::ArrayBackend, &std::path::Path) -> Result<T>,
    ) -> Result<T> {
        let first = self.array_backend(location);
        if let Ok((backend, path)) = &first
            && let Ok(value) = act(*backend, path)
        {
            return Ok(value);
        }
        let (backend, path) = self.search_array_file(location)?;
        if let Ok((_, guessed)) = &first
            && *guessed == path
        {
            // The search agreed with the guess, so the failure was the read's
            // and not the file's. Report that one rather than a second attempt
            // at the same bytes.
            return act(backend, &path);
        }
        act(backend, &path)
    }

    /// What the search settled on for this pair last time.
    fn remembered(&self, declared: &str, dataset: &str) -> Option<std::path::PathBuf> {
        self.array_files
            .lock()
            .expect("the resolved-file lock is never held across a panic")
            .get(&(declared.to_string(), dataset.to_string()))
            .cloned()
    }

    fn remember(&self, declared: &str, dataset: &str, path: &std::path::Path) {
        self.array_files
            .lock()
            .expect("the resolved-file lock is never held across a panic")
            .insert((declared.to_string(), dataset.to_string()), path.to_path_buf());
    }

    /// The external file the package's own name suggests: `model.epc` →
    /// `model.h5`, when that file exists.
    ///
    /// The fallback of last resort for packages whose pointers are broken —
    /// proxies with no file name (SKUA-GOCAD exports), URIs pointing at objects
    /// instead of files. It is never preferred to anything the package says,
    /// and using it is always recorded.
    fn companion_name(&self) -> Option<String> {
        let path = self.path.as_deref()?;
        let companion = path.with_extension("h5");
        if !companion.exists() {
            return None;
        }
        Some(companion.file_name()?.to_string_lossy().into_owned())
    }

    /// How names in array pointers become paths: the folder to look in, and
    /// the files the caller pins by hand.
    ///
    /// A cloud service has neither the right names nor a disk, and a package
    /// moved after export names files that are no longer there.
    pub fn external_files_mut(&mut self) -> &mut crate::backend::ExternalFiles {
        &mut self.external
    }

    /// The same settings, read-only — where a named file would be found, and
    /// whether this store has a disk at all.
    ///
    /// Not `external_files`, which answers a different question (the names the
    /// package declares) and was here first.
    pub fn external_files_ref(&self) -> &crate::backend::ExternalFiles {
        &self.external
    }

    /// Add a backend, tried before the ones compiled in.
    ///
    /// The extension point: a WebAssembly application plugs h5wasm in here, and
    /// CSV or Parquet will arrive the same way.
    pub fn add_array_backend(&mut self, backend: Box<dyn crate::backend::ArrayBackend>) {
        self.backends.push(backend);
    }

    /// The file an [`EpcExternalPartReference`] stands for, from its `.rels`.
    ///
    /// The specification is explicit (EPC §3.2.3): the file name and its MIME
    /// type are read **in the relationship**, never in the object — an
    /// `EpcExternalPartReference` inside an EPC must carry null `Filename` and
    /// `MimeType` attributes.
    ///
    /// [`EpcExternalPartReference`]: https://schemas.energistics.org
    pub fn external_file_of(&self, proxy_uuid: &str) -> Option<String> {
        // **What this session declared wins**, the same rule
        // [`EpcStore::external_file_map`] applies on the way out. Without it a
        // rename would be honoured by the writer and ignored by the reader:
        // the saved package would name the new file while every array read in
        // this session went on resolving to the old one. Two readers of one
        // fact, disagreeing about what it means.
        if let Some(declared) = self.declared_files.get(&proxy_uuid.to_ascii_lowercase()) {
            return Some(declared.clone());
        }
        let index = *self.by_uuid.get(&proxy_uuid.to_ascii_lowercase())?.first()?;
        self.external_resource_in(&self.entries[index].summary.locator)
    }

    /// The file an array pointer means, trying every place it is written.
    ///
    /// The specification gives one answer; the corpus gives four. Observed in
    /// `testingPackageCpp22.epc`: two RESQML 2.0.1 objects survive inside an
    /// otherwise 2.2 package, and the DOR that should name their
    /// `EpcExternalPartReference` carries **the HDF5 file name in its `UUID`
    /// element** — `<eml:UUID>testingPackageCpp22.h5</eml:UUID>`. No part has
    /// that uuid, because it is not a uuid. A strict reader loses those two
    /// arrays; this one finds them and says how.
    ///
    /// In order:
    ///
    /// 1. the `externalResource` of the proxy's own `.rels` — what EPC §3.2.3
    ///    prescribes, and what works for every conformant object;
    /// 2. the "uuid" taken **literally as a file name**, when it is not a uuid
    ///    at all and either the package declares an external file by that exact
    ///    name or one sits beside the package on disk. Narrow on purpose: it
    ///    fires only when the writer's mistake is unambiguous;
    /// 3. the `externalResource` of the **referencing object's** `.rels`;
    /// 4. the only external file the package declares, if there is exactly one.
    ///    A guess, and stated as one: an EPC that names two HDF5 files gets no
    ///    answer here rather than a coin toss;
    /// 5. when the package declares nothing at all, the **companion file** —
    ///    `model.h5` beside `model.epc` — if it exists. The SKUA-GOCAD exports
    ///    of the corpus need this: their proxies carry no file name anywhere.
    ///
    /// Steps 2 to 5 each record a diagnostic. None is silent.
    pub fn external_file_for(&self, proxy_uuid: &str, source_part: Option<&str>) -> Option<String> {
        if let Some(file) = self.external_file_of(proxy_uuid) {
            return Some(file);
        }

        if !identifiers::is_uuid(proxy_uuid) {
            // The fesapi trick: the file name written where a uuid belongs.
            // Two ways to be sure enough to act on it — the package declares an
            // external file by that exact name, or (a package with no `.rels`
            // at all, which happens) a file by that name sits beside the
            // package on disk.
            let declared = self.external_files().iter().any(|file| file == proxy_uuid);
            if declared || self.external.resolve(proxy_uuid).is_ok() {
                let evidence = if declared {
                    "it is the name of an external file this package declares"
                } else {
                    "a file by that name sits beside the package"
                };
                self.record(Diagnostic::warning(
                    source_part.unwrap_or_default(),
                    format!(
                        "the HDF5 proxy reference carries `{proxy_uuid}` where a uuid belongs; \
                         {evidence}, and it was used as such"
                    ),
                ));
                return Some(proxy_uuid.to_string());
            }
        }

        if let Some(part) = source_part
            && let Some(file) = self.external_resource_in(part)
        {
            self.record(Diagnostic::warning(
                part,
                format!(
                    "the proxy {proxy_uuid} is not a part of this package; \
                     the file name was taken from the object's own relationships (`{file}`)"
                ),
            ));
            return Some(file);
        }

        match self.external_files() {
            [only] => {
                self.record(Diagnostic::warning(
                    source_part.unwrap_or_default(),
                    format!(
                        "the proxy {proxy_uuid} is not a part of this package and no relationship \
                         names its file; fell back to `{only}`, the only external file the package declares"
                    ),
                ));
                Some(only.clone())
            }
            [] => {
                // Nothing declared anywhere — the SKUA-GOCAD export shape:
                // proxies whose `.rels` are absent or empty. The package's own
                // name is the last thing left to try.
                let companion = self.companion_name()?;
                self.record(Diagnostic::warning(
                    source_part.unwrap_or_default(),
                    format!(
                        "the proxy {proxy_uuid} names no file and the package declares none; \
                         fell back to `{companion}`, the file sitting beside the package with its name"
                    ),
                ));
                Some(companion)
            }
            _ => None,
        }
    }

    /// The `externalResource` target recorded for one part.
    ///
    /// Answered from `external_resources` after the first time, which is where
    /// the argument for that map lives — a proxy's `.rels` is read once per
    /// package rather than once per array pointer.
    fn external_resource_in(&self, part: &str) -> Option<String> {
        if let Ok(memo) = self.external_resources.lock()
            && let Some(found) = memo.get(part)
        {
            return found.clone();
        }
        let found = self
            .relationships_recorded(part)
            .ok()
            .and_then(|relationships| {
                relationships
                    .into_iter()
                    .find(|r| r.is_external_resource())
                    .map(|r| r.target)
            })
            .filter(|target| !target.is_empty());
        if let Ok(mut memo) = self.external_resources.lock() {
            memo.insert(part.to_string(), found.clone());
        }
        found
    }

    /// Every distinct external file the package declares, across all its
    /// `.rels`. Computed once, the first time something needs it.
    pub fn external_files(&self) -> &[String] {
        self.external_files.get_or_init(|| {
            let mut files: Vec<String> = Vec::new();
            for part in self.package.entries() {
                if !part.ends_with(".rels") {
                    continue;
                }
                let Ok(bytes) = self.package.read(part) else {
                    continue;
                };
                let (relationships, problems) = package::parse_relationships_reporting(&bytes, part);
                for problem in problems {
                    self.record(problem);
                }
                for relationship in relationships {
                    if relationship.is_external_resource()
                        && !relationship.target.is_empty()
                        && !files.contains(&relationship.target)
                    {
                        files.push(relationship.target);
                    }
                }
            }
            files.sort();
            files
        })
    }

    /// Uuid → file, for the **ordinary objects** whose own `.rels` named one.
    ///
    /// The counterpart of [`EpcStore::external_file_map`] for the relationship
    /// §3.2.3 does not describe. Read back from the archive rather than derived
    /// from the arrays: the point is to return what the package said, not to
    /// decide what it ought to have said. An object this session created has no
    /// recorded `.rels` and so gets nothing, which is right — it never claimed
    /// one.
    pub fn object_external_file_map(&self) -> std::collections::BTreeMap<String, String> {
        self.entries
            .iter()
            .filter(|entry| {
                !(entry.summary.qualified_type.contains("EpcExternalPartReference")
                    || entry.info.is_some_and(|i| i.type_key == "EpcExternalPartReference"))
            })
            .filter_map(|entry| {
                let uuid = entry.summary.uuid.to_ascii_lowercase();
                // Declared **only where the package already said something**:
                // this map is what the archive stated, and adding a uuid it
                // never mentioned would invent a relationship. Replacing the
                // value of one it did mention is what a rename is.
                let file = match self.declared_files.get(&uuid) {
                    Some(declared) => {
                        self.external_resource_in(&entry.summary.locator)?;
                        declared.clone()
                    }
                    None => self.external_resource_in(&entry.summary.locator)?,
                };
                Some((uuid, file))
            })
            .collect()
    }

    /// Every `EpcExternalPartReference` in the package, mapped to the file it
    /// stands for.
    ///
    /// What the writer needs: the proxy object must not carry the name itself
    /// (EPC §3.2.3), so it travels beside the objects and lands in the proxy's
    /// `.rels`.
    pub fn external_file_map(&self) -> std::collections::BTreeMap<String, String> {
        let mut map: std::collections::BTreeMap<String, String> = self
            .entries
            .iter()
            .filter(|entry| {
                entry.summary.qualified_type.contains("EpcExternalPartReference")
                    || entry
                        .info
                        .is_some_and(|i| i.type_key == "EpcExternalPartReference")
            })
            .filter_map(|entry| {
                let uuid = entry.summary.uuid.to_ascii_lowercase();
                self.external_file_for(&uuid, Some(&entry.summary.locator))
                    .map(|file| (uuid, file))
            })
            .collect();
        // What the caller declared wins: it is the more recent statement, and
        // it is the only source for a proxy this session created.
        map.extend(self.declared_files.iter().map(|(k, v)| (k.clone(), v.clone())));
        map
    }

    /// Name the external file a proxy stands for.
    ///
    /// Needed for a proxy created in this session: `save` regenerates every
    /// `.rels` from what the store knows, and without this the new
    /// `EpcExternalPartReference` is written out naming no file — a warning at
    /// best, a package whose arrays cannot be found at worst.
    ///
    /// Deliberately separate from writing the numbers. The two halves fail
    /// independently and for different reasons — a disk full against a package
    /// that will not save — and pretending otherwise would leave a file on disk
    /// that nothing points at, or a pointer to a file that was never written.
    pub fn register_external_file(&mut self, proxy_uuid: &str, file_name: impl Into<String>) {
        let file_name = file_name.into();
        // A proxy that now names another file resolves its arrays elsewhere:
        // whoever read them through the old name has to be told.
        self.overlay.journal().note_renamed(proxy_uuid, &file_name);
        self.declared_files.insert(proxy_uuid.to_ascii_lowercase(), file_name);
    }

    /// The external files this session declared, as opposed to found.
    pub fn declared_external_files(&self) -> &std::collections::BTreeMap<String, String> {
        &self.declared_files
    }

    /// The citation of an object held in memory, without going near the archive.
    fn header_of(&self, object: &dyn Reflect) -> Result<ObjectHeader> {
        let summary = self
            .summary_of(object)
            .ok_or_else(|| Error::Package("this object carries no identity".to_string()))?;
        let citation = identity::title_of_reflect(object).map(str::to_string);
        Ok(ObjectHeader {
            schema_version: Some(object.type_info().schema_version)
                .filter(|version| !version.is_empty())
                .map(str::to_string),
            object_version: identity::object_version_of_reflect(object).map(str::to_string),
            title: citation,
            // The rest lives in the citation and is not worth a second walk:
            // an application that edited the object has it in hand already.
            originator: None,
            creation: None,
            last_update: None,
            format: None,
            summary,
        })
    }

    // -- Modifying ---------------------------------------------------------

    /// Borrow an object for editing, marking it modified.
    ///
    /// The normal way to change something. Rust does not report when a field is
    /// written, so the mark is taken **when the borrow is taken** — the moment
    /// an application asks to edit is the moment it intends to. Simple, exact,
    /// and it is the call an editing interface makes anyway.
    ///
    /// The archive is untouched until a save.
    pub fn edit(&mut self, uri: &str) -> Result<&mut dyn Reflect> {
        // Nothing is known of what the caller will change: the broad answer, and
        // no refinement (the references are compared only when a path is given).
        self.edit_what(uri, What::BROAD, false)
    }

    /// [`EpcStore::edit`] for an edit of **one value, addressed by its dotted
    /// path** — what a form or a front end's JSON edit does. The path is what lets
    /// the journal say *what* changed ([`What::of_edit`]): a citation path is
    /// `CITATION`; any other is `BODY | LINKS_OUT`, and `LINKS_OUT` is taken off
    /// again when the object's references are found unmoved, the next time the
    /// journal is read ([`EpcStore::changes_since`]) — the rule of the front end's
    /// `referencesMoved`, over the reference graph instead of the text.
    pub fn edit_at(&mut self, uri: &str, path: &str) -> Result<&mut dyn Reflect> {
        let mut what = What::of_edit(path);
        // **An edit of the `objectVersion` is an identity change**: the uri an
        // object is known by carries its version whenever it declares one
        // (internally the uri always carries the version number when there is
        // one, ADR-0003), so the uri moves with it. The
        // entry keeps its slot; the uri, and every key built on it, do not.
        let last = path.rsplit('.').next().unwrap_or(path);
        if last.eq_ignore_ascii_case("objectVersion") || last.eq_ignore_ascii_case("VersionString") {
            what = What::CITATION | What::BROAD | What::IDENTITY;
        }
        let refine = what.contains(What::LINKS_OUT) && !what.contains(What::IDENTITY);
        self.edit_what(uri, what, refine)
    }

    /// Give an entry another `objectVersion`, through the layer. The entry keeps
    /// its slot; its uri, which carries the version, changes, and the journal
    /// says `IDENTITY`. Refused, by name, when a
    /// namesake already carries that version — two entries of one identity are one
    /// object. Returns the uri the entry is now known by.
    pub fn set_object_version(&mut self, uri: &str, version: &str) -> Result<String> {
        let version = normal_version(Some(version))
            .ok_or_else(|| Error::Package("an empty objectVersion names no version".to_string()))?;
        let (slot, uuid, qualified_type) = {
            let live = self.locate(uri)?;
            (live.slot.to_string(), live.summary.uuid.to_ascii_lowercase(), live.summary.qualified_type.clone())
        };
        let namesakes: Vec<(Option<String>, String)> = self
            .live_of(&uuid)
            .into_iter()
            .filter(|one| one.summary.qualified_type == qualified_type && one.slot != slot)
            .map(|one| (one.version.map(str::to_string), one.summary.uri.clone()))
            .collect();
        if let Some((_, held)) = namesakes.iter().find(|(held, _)| held.as_deref() == Some(version.as_str())) {
            return Err(Error::Package(format!(
                "`{held}` already carries the objectVersion `{version}`: two entries of one identity are one object"
            )));
        }
        let what = What::CITATION | What::BROAD | What::IDENTITY;
        let object = self.edit_what(uri, what, false)?;
        let value = || energyml_core::value::OwnedValue::Str(version.clone());
        energyml_core::path::set(object, "objectVersion", value())
            .or_else(|_| energyml_core::path::set(object, "Citation.VersionString", value()))
            .map_err(|why| Error::Package(format!("`{uri}` has no place for an objectVersion: {why}")))?;
        self.settle_versions();
        Ok(self.live_of(&uuid).into_iter().find(|one| one.slot == slot).map(|one| one.summary.uri.clone()).unwrap_or_default())
    }

    /// The version an entry was recorded with, `None` when the slot is unknown.
    fn recorded_version(&self, slot: &str) -> Option<Option<String>> {
        let uuid = uuid_of_slot(slot);
        self.by_uuid
            .get(uuid)
            .into_iter()
            .flatten()
            .map(|&at| &self.entries[at])
            .find(|entry| entry.slot == slot)
            .map(|entry| entry.object_version.clone())
            .or_else(|| self.made_by_slot(slot).map(|made| made.object_version.clone()))
    }

    /// Bring the entries' recorded versions and uris up to what the layer holds:
    /// an object edited in place may state another `objectVersion` than its entry
    /// was born with. Run at the start of every gate that writes; a lookup does not
    /// wait for it (`current_version`), only the spelling of a uri does.
    /// Bring every entry's recorded version, and so its uri, up to what the
    /// layer holds — after an edit through the borrow [`EpcStore::edit_at`] hands
    /// out, which the store cannot watch while the caller holds it. Every writing
    /// gate does this first; a face that edits and then lists (the front end's
    /// `set` and `set_json`) calls it once its edit is done, so the inventory
    /// names the object by the uri its version now gives it.
    pub fn settle_identities(&mut self) {
        self.settle_versions();
    }

    fn settle_versions(&mut self) {
        if self.unsettled.is_empty() {
            return;
        }
        let mut drifted: Vec<(String, String, Option<String>)> = Vec::new();
        for slot in std::mem::take(&mut self.unsettled) {
            let Some(Change::Put(object)) = self.overlay.get(&slot) else { continue };
            let now = normal_version(identity::object_version_of_reflect(object.as_reflect()));
            if self.recorded_version(&slot).is_some_and(|recorded| recorded != now) {
                drifted.push((uuid_of_slot(&slot).to_string(), slot.clone(), now));
            }
        }
        for (uuid, slot, now) in drifted {
            let before: Vec<(String, String)> =
                self.live_of(&uuid).iter().map(|one| (one.slot.to_string(), one.summary.uri.clone())).collect();
            self.set_entry_version(&uuid, &slot, now);
            self.moved_uris(&uuid, &slot, &before);
        }
    }

    fn edit_what(&mut self, uri: &str, what: What, refine: bool) -> Result<&mut dyn Reflect> {
        self.settle_versions();
        let slot = self.slot_of(uri)?;
        if !matches!(self.overlay.get(&slot), Some(Change::Put(_))) {
            // Not in the layer yet: materialise it there first. Materialising is
            // not a write; the borrow below is, and journals itself.
            let object = self.get_object(uri)?;
            self.overlay.place_unjournaled(&slot, Change::Put(object));
        }
        // Lent out: the caller may change its `objectVersion`, which the next
        // writing gate settles (`unsettled`).
        self.unsettled.insert(slot.clone());
        let before = match (refine, self.overlay.get(&slot)) {
            (true, Some(Change::Put(object))) => Some(reference_list(object.as_reflect())),
            _ => None,
        };
        if before.is_some() {
            // An earlier provisional edit of this object is judged against the
            // state it left, not against the one this edit is about to leave.
            self.settle_edits(Some(&slot));
        }
        self.overlay
            .get_mut_what(&slot, Some(uri), what)
            .ok_or_else(|| Error::NotFound(uri.to_string()))?;
        if let Some(references) = before {
            let revision = self.overlay.revision();
            let mut pending = self.pending_edits.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            if pending.len() >= PENDING_EDITS {
                // The oldest keeps the wider answer, which is the safe one.
                pending.remove(0);
            }
            pending.push(PendingEdit { revision, slot: slot.clone(), references });
        }
        let object = self
            .overlay
            .get_mut_unjournaled(&slot)
            .ok_or_else(|| Error::NotFound(uri.to_string()))?;
        Ok(object.as_reflect_mut())
    }

    /// Add an object, or replace one wholesale — ETP protocol 4, `PutDataObjects`.
    ///
    /// Returns the URI it will be known by.
    ///
    /// **An ordinary put never creates a second version of an object.** An entry
    /// is (uuid, qualified type, `objectVersion`), and this replaces the entry of
    /// the object's (uuid, type):
    ///
    /// - there is none: the object is **new**;
    /// - there is one: it is replaced — even when the object states another
    ///   `objectVersion` than the entry had (the entry takes it: that is an
    ///   edited version, an identity event in the journal, not a second object);
    /// - there are several (the package holds versions of it): the one of the
    ///   same version is replaced; an object naming a version none of them
    ///   carries is refused, [`Error::Ambiguous`] — it could be any of them, and
    ///   this store does not guess. Adding a version is
    ///   [`EpcStore::put_as_new_version`], and nothing else.
    pub fn put(&mut self, object: Box<dyn DynObject>) -> Result<String> {
        self.put_entry(object, false)
    }

    /// Add an object **as a new version** of an object this package holds: the
    /// same uuid and type under another `objectVersion` — a second entry, with
    /// its own part when the package is written (EPC v1.1 §3.6, p. 17), its own
    /// uri, and a `CREATED` line in the journal.
    ///
    /// The gate exists so that a version is never made by accident: an ordinary
    /// [`EpcStore::put`] replaces. It is refused, by name, when the exact
    /// identity — uuid, type *and* version, an absent version included — is
    /// already there; replacing is what `put` is for.
    ///
    /// The uri of the entries moves with the new one: the short uri is accepted
    /// when there is no conflict (or when the object has no version), the version
    /// is required when there is one — the entry that
    /// declares no version keeps the short uri, every versioned entry of the
    /// (uuid, type) takes the form with its version, and the journal says so for
    /// each entry whose uri changed.
    pub fn put_as_new_version(&mut self, object: Box<dyn DynObject>) -> Result<String> {
        self.put_entry(object, true)
    }

    fn put_entry(&mut self, object: Box<dyn DynObject>, new_version: bool) -> Result<String> {
        self.settle_versions();
        let summary = self
            .summary_of(object.as_reflect())
            .ok_or_else(|| Error::Package("this object carries no uuid, so it cannot be a part".into()))?;
        // **Present is not the same as usable.** The reader leaves an absent
        // attribute at its default, so an object with no identity arrives
        // carrying an empty string rather than nothing at all — and an empty
        // string sails past the check above, then becomes the part
        // `eml:///resqml22.ColorMapDictionary()`, which is not an address.
        // Every later call that takes a uri then refuses it, far from here,
        // with a message about the uri rather than about the object.
        if !identifiers::is_uuid(&summary.uuid) {
            return Err(Error::Package(format!(
                "`{}` is not a usable uuid, so this object cannot be a part",
                summary.uuid
            )));
        }
        let key = summary.uuid.to_ascii_lowercase();
        let version = normal_version(identity::object_version_of_reflect(object.as_reflect()));
        let versioned = |version: Option<&str>| {
            identifiers::EtpUri::object_versioned(None, &summary.qualified_type, &summary.uuid, version)
        };

        // The entries of this (uuid, type) as they stand: slot and version.
        let same_type: Vec<(String, Option<String>, String)> = self
            .live_of(&key)
            .into_iter()
            .filter(|one| one.summary.qualified_type == summary.qualified_type)
            .map(|one| (one.slot.to_string(), one.version.map(str::to_string), one.summary.uri.clone()))
            .collect();

        // Which slot this object takes, and whether it replaces something there.
        let (slot, replaces) = if new_version {
            if same_type.iter().any(|(_, held, _)| *held == version) {
                return Err(Error::Package(format!(
                    "`{}` is already held{}: a put replaces it, a new version is another \
                     `objectVersion`",
                    versioned(version.as_deref()),
                    if version.is_none() { " (it declares no objectVersion)" } else { "" }
                )));
            }
            (self.slot_for_new(&key, &summary.qualified_type, version.as_deref()), false)
        } else if let Some((slot, _, _)) = same_type.iter().find(|(_, held, _)| *held == version) {
            (slot.clone(), true)
        } else if let [(slot, _, _)] = &same_type[..] {
            (slot.clone(), true)
        } else if same_type.is_empty() {
            (self.slot_for_new(&key, &summary.qualified_type, version.as_deref()), false)
        } else {
            return Err(Error::Ambiguous {
                uri: versioned(version.as_deref()),
                candidates: same_type.iter().map(|(_, _, uri)| uri.clone()).collect(),
            });
        };

        let before: Vec<(String, String)> =
            self.live_of(&key).iter().map(|one| (one.slot.to_string(), one.summary.uri.clone())).collect();

        // Whether this replaces an object or adds one decides what the journal
        // says: the broad answer for a replacement (a reader that held the old
        // one has a different one), `CREATED` for a new entry. An entry removed
        // earlier in the layer is new again.
        let mut what = if replaces { What::BROAD } else { What::CREATED };
        if replaces {
            // An object that states another version than its entry had.
            let entry_version = self
                .live_of(&key)
                .into_iter()
                .find(|one| one.slot == slot)
                .map(|one| one.version.map(str::to_string));
            if entry_version.is_some_and(|held| held != version) {
                // The uri carries the version: another version is another uri.
                what |= What::IDENTITY;
                self.set_entry_version(&key, &slot, version.clone());
            }
        }
        // Remembered only when the archive does not hold the entry: an edit to
        // an existing part goes through here too, and `entries` answers for it.
        if !self.is_archive_slot(&slot) {
            let list = self.made.entry(key.clone()).or_default();
            let made = Made { slot: slot.clone(), summary: summary.clone(), object_version: version.clone() };
            match list.iter_mut().find(|held| held.slot == slot) {
                Some(held) => *held = made,
                None => list.push(made),
            }
        }
        self.overlay.set_what(&slot, Some(&summary.uri), Change::Put(object), what);
        self.moved_uris(&key, &slot, &before);
        let uri = self
            .live_of(&key)
            .into_iter()
            .find(|one| one.slot == slot)
            .map(|one| one.summary.uri.clone())
            .unwrap_or(summary.uri);
        self.after_change()?;
        Ok(uri)
    }

    /// The slot for an entry that is new to the package — or comes back to it.
    ///
    /// An entry the archive held, removed here and put again, takes its own slot
    /// back (ADR-0010: re-creating the same uuid after a removal
    /// is the same slot); otherwise the uuid itself, if no entry holds it — the
    /// rule that leaves every package without namesakes with the keys it always
    /// had — and a composite slot if one does.
    fn slot_for_new(&self, uuid: &str, qualified_type: &str, version: Option<&str>) -> String {
        if let Some(entry) = self.by_uuid.get(uuid).into_iter().flatten().map(|&at| &self.entries[at]).find(|entry| {
            entry.summary.qualified_type == qualified_type && entry.object_version.as_deref() == version
        }) {
            return entry.slot.clone();
        }
        let taken = self
            .by_uuid
            .get(uuid)
            .into_iter()
            .flatten()
            .any(|&at| self.entries[at].slot == uuid)
            || self.made.get(uuid).into_iter().flatten().any(|made| made.slot == uuid);
        if taken { slot_key(uuid, qualified_type, version) } else { uuid.to_string() }
    }

    /// An entry that states another `objectVersion` than it was born with.
    fn set_entry_version(&mut self, uuid: &str, slot: &str, version: Option<String>) {
        if let Some(entry) = self
            .by_uuid
            .get(uuid)
            .into_iter()
            .flatten()
            .copied()
            .find(|&at| self.entries[at].slot == slot)
        {
            self.entries[entry].object_version = version;
        } else if let Some(made) = self.made.get_mut(uuid).and_then(|list| list.iter_mut().find(|made| made.slot == slot)) {
            made.object_version = version;
        }
    }

    /// After the set of entries of a uuid changed: spell their uris again, and
    /// journal each entry **other than `own`** whose uri moved. `before` is the
    /// slot and uri of every live entry before the change.
    ///
    /// That the second version of an object changes the uri of the first is the
    /// price of the short form (ADR-0010): the front end handles it
    /// the way it handles an identity change, and the journal has to say it.
    fn moved_uris(&mut self, uuid: &str, own: &str, before: &[(String, String)]) {
        self.reconcile_uris(uuid);
        let after: Vec<(String, String)> =
            self.live_of(uuid).iter().map(|one| (one.slot.to_string(), one.summary.uri.clone())).collect();
        for (slot, uri) in &after {
            if slot == own {
                continue;
            }
            if before.iter().any(|(held, was)| held == slot && was != uri) {
                self.overlay.journal().touch_what(slot, Some(uri), What::BROAD | What::IDENTITY);
            }
        }
        // The entry itself, when its own uri moved too (a version edited in
        // place, or an entry coming back beside a namesake): an identity event.
        if let (Some((_, was)), Some((_, now))) =
            (before.iter().find(|(slot, _)| slot == own), after.iter().find(|(slot, _)| slot == own))
            && was != now
        {
            self.overlay.journal().touch_what(own, Some(now), What::IDENTITY);
        }
    }

    /// Remove an object — ETP protocol 4, `DeleteDataObjects`.
    ///
    /// The parts that referenced it keep their references: this does **not**
    /// walk the package fixing them up, because deciding what a dangling
    /// reference should become is the application's business, not a store's.
    /// `energyml check` finds them afterwards.
    ///
    /// Removing one version of an object leaves the others; the one that is left
    /// alone takes the short uri back, and the journal says so.
    pub fn remove(&mut self, uri: &str) -> Result<()> {
        self.settle_versions();
        let slot = match self.slot_of(uri) {
            Ok(slot) => slot,
            // Removing what was already removed in this layer is not a failure:
            // the answer is the same as the first time.
            Err(Error::NotFound(absent)) => {
                let uuid = address_of(uri)?.uuid;
                if self.overlay.get(&uuid).is_none() {
                    return Err(Error::NotFound(absent));
                }
                uuid
            }
            Err(other) => return Err(other),
        };
        let uuid = uuid_of_slot(&slot).to_string();
        let before: Vec<(String, String)> =
            self.live_of(&uuid).iter().map(|one| (one.slot.to_string(), one.summary.uri.clone())).collect();
        if let Some(list) = self.made.get_mut(&uuid) {
            list.retain(|made| made.slot != slot);
        }
        self.overlay.set_what(&slot, Some(uri), Change::Removed, What::REMOVED);
        self.moved_uris(&uuid, &slot, &before);
        self.after_change()
    }

    /// Whether anything has changed since the package was opened.
    pub fn is_dirty(&self) -> bool {
        self.overlay.is_dirty()
    }

    /// Choose when changes reach the disk. See [`SavePolicy`].
    pub fn set_save_policy(&mut self, policy: SavePolicy) {
        self.policy = policy;
    }

    /// Choose how the package is written when a policy writes on its own.
    pub fn set_save_options(&mut self, options: SaveOptions) {
        self.save_options = options;
    }

    /// Mark the edited object and honour [`SavePolicy::OnChange`].
    ///
    /// Called after a mutation rather than before, so a failed write leaves the
    /// change in memory instead of losing it.
    fn after_change(&mut self) -> Result<()> {
        if self.policy == SavePolicy::OnChange && self.path.is_some() {
            let options = self.save_options.clone();
            self.save(&options)?;
        }
        Ok(())
    }

    // -- Transactions ------------------------------------------------------

    /// Start a transaction — ETP protocol 18.
    ///
    /// One layer more on the in-memory stack, and nothing on disk. ETP v1.2
    /// forbids nesting, so a second call is an error.
    pub fn start_transaction(&mut self) -> Result<()> {
        self.overlay.start_transaction()
    }

    /// Merge the transaction into the changes below it.
    pub fn commit_transaction(&mut self) -> Result<()> {
        self.overlay.commit_transaction()?;
        self.after_change()
    }

    /// Throw the transaction away. It never reached the disk, so nothing there
    /// has to be undone — which is the whole reason transactions are a layer.
    pub fn rollback_transaction(&mut self) -> Result<()> {
        // What the layer held decides which entries are live, so the uris of the
        // uuids it touched are spelled again once it is gone: a version added in
        // the transaction no longer makes its namesake's uri a long one.
        let touched: BTreeSet<String> =
            self.overlay.changed().iter().map(|(slot, _)| uuid_of_slot(slot).to_string()).collect();
        // What the layer below holds again may state another version than an
        // entry recorded during the transaction: settle every slot it held.
        let held: Vec<String> = self.overlay.changed().iter().map(|(slot, _)| (*slot).clone()).collect();
        self.unsettled.extend(held);
        self.overlay.rollback_transaction()?;
        for uuid in touched {
            self.reconcile_uris(&uuid);
        }
        Ok(())
    }

    /// Whether a transaction is open.
    pub fn in_transaction(&self) -> bool {
        self.overlay.in_transaction()
    }

    // -- Writing -----------------------------------------------------------

    /// Write the package to any sink, and say what could not be represented.
    ///
    /// **Everything is materialised first.** Regenerating the relationships
    /// means walking every object's DORs, so saving reads the whole package —
    /// the lazy levels are a convenience for looking at a file, never a way to
    /// write half of one (`write.rs`).
    ///
    /// A part the registry cannot deserialize is **copied verbatim** rather
    /// than dropped, with a warning: its bytes are right there, and losing an
    /// object because this build lacks its schema version would be the worst
    /// possible answer. Its relationships cannot be regenerated, and the
    /// warning says so.
    pub fn write_to<W: std::io::Write + std::io::Seek>(
        &self,
        out: W,
        options: &SaveOptions,
    ) -> Result<Vec<Diagnostic>> {
        let mut diagnostics = Vec::new();
        let mut objects: Vec<Box<dyn DynObject>> = Vec::new();
        let mut verbatim = Vec::new();

        for entry in &self.entries {
            // Removed in the layer: not written, and its bytes are not kept
            // either — that is what removing means.
            if matches!(self.overlay.get(&entry.slot), Some(Change::Removed)) {
                continue;
            }
            // Bytes staged for this part win over everything: they are the
            // repair path, and the only reason to take it is that the parser
            // was wrong about this part. Written exactly as given.
            if let Some(bytes) = self.overlay.raw(&entry.summary.locator) {
                verbatim.push(write::Attachment {
                    part: entry.summary.locator.clone(),
                    content_type: Some(entry.summary.content_type.clone()),
                    bytes: bytes.to_vec(),
                });
                continue;
            }
            match self.get_object(&entry.summary.uri) {
                Ok(object) => objects.push(object),
                Err(e) => {
                    diagnostics.push(Diagnostic::warning(
                        entry.summary.locator.clone(),
                        format!(
                            "{e}; the part was copied unchanged, so nothing is lost — but its \
                             relationships could not be regenerated"
                        ),
                    ));
                    verbatim.push(write::Attachment {
                        part: entry.summary.locator.clone(),
                        content_type: Some(entry.summary.content_type.clone()),
                        bytes: self.package.read(&entry.summary.locator)?,
                    });
                }
            }
        }

        // Objects the layer added, which have no part in the archive: **lent**,
        // not copied. The writer only reads them, and `&self` keeps the layer
        // still while it does; a deep copy of each was the price of nothing
        // (07/10/2026: seventy thousand objects copied for one save).
        let added: Vec<&dyn DynObject> = self
            .overlay
            .changed()
            .into_iter()
            .filter_map(|(slot, change)| match change {
                Change::Put(object) if !self.is_archive_slot(slot) => Some(object.as_ref()),
                _ => None,
            })
            .collect();

        let mut package = write::Package::new(
            objects.iter().map(|object| object.as_ref()).chain(added).collect(),
            self.core_properties_for(options),
        );
        package.layout = options.layout;
        package.external_files = self.external_file_map();
        package.object_external_files = self.object_external_file_map();
        package.verbatim = verbatim;
        package.media_links = self.media_links();

        // Attachments and the parts the specification does not describe travel
        // as bytes: a PDF is a PDF, and fesapi's `extendedCore.xml` is nobody's
        // business but fesapi's.
        for part in self.attachments() {
            // A part that has been repaired into an object is written by the
            // object, under the name its type gives it. Copying the wreck
            // through as well would leave the package holding both.
            if self.overlay.is_superseded(&part) {
                continue;
            }
            package.verbatim.push(write::Attachment {
                // The archive's declaration when it had one, the caller's when
                // the part was added in this session. Neither invented: with no
                // answer the writer falls back to the extension, which is what
                // OPC's `Default` entries are for.
                content_type: self
                    .package
                    .content_type(&part)
                    .map(str::to_string)
                    .or_else(|| self.attachment_types.get(&part).cloned()),
                // `raw_part` rather than `package.read`: a part the inventory
                // dropped is reachable only here, and repairing one is the
                // whole point of the raw layer.
                bytes: self.raw_part(&part)?,
                part,
            });
        }

        // `docProps/` is the package's own metadata, so it is not an
        // attachment — but everything there beyond `core.xml` is a producer's
        // extension that this library neither writes nor understands, and
        // dropping it on rewrite is a silent loss. fesapi's
        // `extendedCore.xml` is the real case, in both reference packages.
        //
        // `core.xml` itself is excluded because it is regenerated, and the
        // `.rels` because they are rebuilt from the DORs.
        let extra: Vec<String> = self
            .package
            .entries()
            .iter()
            .filter(|name| name.starts_with("docProps/"))
            .filter(|name| !name.eq_ignore_ascii_case("docProps/core.xml"))
            .filter(|name| !name.ends_with(".rels") && !name.ends_with('/'))
            .cloned()
            .collect();
        for part in extra {
            package.verbatim.push(write::Attachment {
                content_type: self.package.content_type(&part).map(str::to_string),
                bytes: self.package.read(&part)?,
                part,
            });
        }

        write::write_package(&package, out, &mut diagnostics)?;
        Ok(diagnostics)
    }

    /// The core properties to write: the source's, with `created` refreshed.
    ///
    /// §3.5.3 makes `creator`, `created` and `version` mandatory and closes the
    /// list of the rest. `created` is the **package** creation time, so it is
    /// this save; everything else is the caller's or the source's.
    fn core_properties_for(&self, options: &SaveOptions) -> core_props::CoreProperties {
        let mut core = self
            .core_properties()
            .map(|bytes| core_props::CoreProperties::parse(&bytes))
            .unwrap_or_default();

        core.created = options.created.clone().unwrap_or_else(core_props::now_w3cdtf);
        core.version = core_props::EPC_VERSION.to_string();
        if let Some(creator) = &options.creator {
            core.creator = creator.clone();
        }
        if core.creator.is_empty() {
            core.creator = format!("Geosiris:energyml-rust:{}", env!("CARGO_PKG_VERSION"));
        }
        core
    }

    /// Write the package to a path, atomically.
    ///
    /// Through a temporary file beside the target, then a rename: a crash
    /// leaves the previous package intact, never a half-written archive.
    ///
    /// The store keeps reading from the archive it was opened on, even when
    /// that is the file just replaced. Re-open to see what was written.
    pub fn save_as(&self, path: impl AsRef<Path>, options: &SaveOptions) -> Result<Vec<Diagnostic>> {
        let path = path.as_ref();
        let _span = tracing::info_span!("epc.save", path = %path.display()).entered();
        let temporary = path.with_extension(format!("epc-tmp-{}", std::process::id()));
        let diagnostics = {
            let file = std::fs::File::create(&temporary)?;
            let mut writer = std::io::BufWriter::new(file);
            let diagnostics = self.write_to(&mut writer, options)?;
            // Flush before the rename, or the rename publishes a truncated file.
            std::io::Write::flush(&mut writer)?;
            diagnostics
        };
        match std::fs::rename(&temporary, path) {
            Ok(()) => Ok(diagnostics),
            Err(e) => {
                let _ = std::fs::remove_file(&temporary);
                Err(e.into())
            }
        }
    }

    /// Write the package back over the file it was opened from.
    ///
    /// Refused while a transaction is open: writing a state a rollback is
    /// supposed to be able to undo would put the disk and memory in
    /// disagreement, and one of the two would be lying.
    pub fn save(&self, options: &SaveOptions) -> Result<Vec<Diagnostic>> {
        if self.in_transaction() {
            return Err(Error::Package(
                "a transaction is open; commit or roll it back before saving".to_string(),
            ));
        }
        let path = self.path.clone().ok_or_else(|| {
            Error::Package("this store was opened from bytes, not a file; use `save_as`".into())
        })?;
        self.save_as(path, options)
    }

    /// Write the package and close it, honouring [`SavePolicy::OnClose`].
    ///
    /// Explicit rather than left to `Drop`, because `Drop` returns no `Result`:
    /// a save that fails at destruction has nowhere to report it, and a
    /// silently failed save is worse than none.
    pub fn close(mut self, options: &SaveOptions) -> Result<Vec<Diagnostic>> {
        self.closed = true;
        if self.policy == SavePolicy::OnClose && self.is_dirty() {
            return self.save(options);
        }
        Ok(Vec::new())
    }

    // -- The package itself ----------------------------------------------

    /// Everything noticed while reading that departs from the specification.
    ///
    /// Grows as the store reads: a departure found only when an array is
    /// located cannot have been known when the archive was indexed.
    pub fn diagnostics(&self) -> Vec<Diagnostic> {
        self.diagnostics
            .lock()
            .expect("the diagnostics lock is never held across a panic")
            .clone()
    }

    /// Record a departure, once. The same fallback taken for three hundred
    /// arrays is one problem, not three hundred lines.
    fn record(&self, diagnostic: Diagnostic) {
        let mut diagnostics = self
            .diagnostics
            .lock()
            .expect("the diagnostics lock is never held across a panic");
        if diagnostics
            .iter()
            .any(|d| d.part == diagnostic.part && d.message == diagnostic.message)
        {
            return;
        }
        diagnostics.push(diagnostic);
    }

    /// Every entry **in the archive**, EnergyML or not, exactly as it was
    /// opened.
    ///
    /// Named for what it is since [`EpcStore::parts`] arrived beside it: this
    /// one answers about the file, that one about the package — which stop being
    /// the same thing the moment anything is edited, added or removed.
    pub fn archive_entries(&self) -> &[String] {
        self.package.entries()
    }

    /// Every object the package holds, read, with the ones that did not.
    ///
    /// The loop four callers had written: read every resource, keep what
    /// deserializes, keep the name of what did not. **Unreadable is a list and
    /// not a silence** — a question asked about *every* object that quietly
    /// answered about some of them is the worst kind of wrong — and it is not
    /// an error either, because one bad part must not cost the answer.
    ///
    /// `of_type` filters on the qualified type before anything is parsed
    /// ([`crate::resource::type_matches`]), which is what makes asking for the
    /// properties of a package cheap on a package that is mostly geometry.
    #[allow(clippy::type_complexity)]
    pub fn read_objects(
        &self,
        of_type: Option<&str>,
    ) -> (Vec<(ObjectSummary, Box<dyn DynObject>)>, Vec<String>) {
        let mut read = Vec::new();
        let mut unreadable = Vec::new();
        for summary in self.resources() {
            if of_type.is_some_and(|want| !crate::resource::type_matches(&summary.qualified_type, want))
            {
                continue;
            }
            match self.get_object(&summary.uri) {
                Ok(object) => read.push((summary, object)),
                Err(why) => unreadable.push(format!("{}: {why}", summary.uri)),
            }
        }
        (read, unreadable)
    }

    /// Relationships recorded for one part.
    ///
    /// An index, not the truth: what an object actually references comes from
    /// its DORs (`identity::collect_refs`). Useful to check one against the
    /// other — a wrong `.rels` is worse than a missing one.
    pub fn relationships(&self, part: &str) -> Result<Vec<Relationship>> {
        self.relationships_recorded(part)
    }

    /// The relationships of a part, with whatever is wrong with the `.rels`
    /// recorded on the store rather than dropped on the floor.
    fn relationships_recorded(&self, part: &str) -> Result<Vec<Relationship>> {
        let (relationships, problems) = self.package.relationships_reporting(part)?;
        for problem in problems {
            self.record(problem);
        }
        Ok(relationships)
    }

    /// The OPC core properties part, as bytes, if the package has one.
    ///
    /// Mandatory in an EPC (§3.5 of the packaging conventions), which is why
    /// its absence is reported at `open` rather than here.
    pub fn core_properties(&self) -> Option<Vec<u8>> {
        self.package.read("docProps/core.xml").ok()
    }

    // -- Internals ---------------------------------------------------------

    /// Resolve a URI, or a bare uuid, to an inventory entry.
    fn entry(&self, uri: &str) -> Result<&PartEntry> {
        self.locate(uri)?.archive.ok_or_else(|| Error::NotFound(uri.to_string()))
    }

    /// The file a location names, resolving the proxy if it has not been.
    fn resolved_file(&self, location: &ArrayLocation) -> Option<String> {
        if location.is_resolved() {
            return Some(location.file.clone());
        }
        Some(
            self.external_target_for(location.proxy_uuid.as_deref()?, None)?
                .file,
        )
    }

    /// Everything an `EpcExternalPartReference` says: the file, and the format.
    ///
    /// The two come from two different places, which is easy to miss. The file
    /// name is in the proxy's `.rels` (EPC §3.2.3 is explicit that a reader
    /// takes it from the relationship). The **MIME type is on the proxy
    /// object** — a mandatory element in eml 2.0, whose own documentation says
    /// `application/x-hdf5` is a *convention*. Dropping it, as this crate did
    /// at first, silently declares every external file in a 2.0.1 package to be
    /// HDF5, and a producer writing CSV has no way to say otherwise.
    pub fn external_target_for(
        &self,
        proxy_uuid: &str,
        source_part: Option<&str>,
    ) -> Option<crate::array::ProxyTarget> {
        let file = self.external_file_for(proxy_uuid, source_part)?;
        Some(crate::array::ProxyTarget {
            file,
            mime: self.proxy_mime(proxy_uuid),
        })
    }

    /// The MIME type an `EpcExternalPartReference` declares, if it declares a
    /// usable one.
    fn proxy_mime(&self, proxy_uuid: &str) -> Option<String> {
        let summary = self.find_by_uuid(proxy_uuid)?;
        let proxy = self.get_object(&summary.uri).ok()?;
        let mime = energyml_core::path::get_string(proxy.as_reflect(), "MimeType").ok()??;
        let mime = mime.trim();
        // §3.2.3 says these "should be null" inside an EPC, and the schema makes
        // the element mandatory: an empty one is the normal case, not an error.
        (!mime.is_empty()).then(|| mime.to_string())
    }

    /// `created` and `modified` from `docProps/core.xml`.
    fn core_property_dates(&self) -> (Option<String>, Option<String>) {
        let Some(bytes) = self.core_properties() else {
            return (None, None);
        };
        let mut reader = quick_xml::Reader::from_reader(bytes.as_slice());
        let mut buf = Vec::new();
        let (mut created, mut modified) = (None, None);
        let mut current = String::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Err(_) | Ok(quick_xml::events::Event::Eof) => break,
                Ok(quick_xml::events::Event::Start(e)) => {
                    current = local_name(e.name().as_ref());
                }
                Ok(quick_xml::events::Event::Text(e)) => {
                    let text = e.xml10_content().unwrap_or_default().trim().to_string();
                    if !text.is_empty() {
                        match current.as_str() {
                            "created" => created = Some(text),
                            "modified" => modified = Some(text),
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
            buf.clear();
        }
        (created, modified)
    }

    /// Walk the archive and decide, part by part, what is an EnergyML object.
    ///
    /// Two passes over each candidate, in this order:
    ///
    /// 1. `[Content_Types].xml` is believed first — it is the index the
    ///    specification defines, and it is right for the overwhelming majority
    ///    of parts.
    /// 2. When it is absent or names something the registry does not know, the
    ///    **root element** decides instead, and the discrepancy is recorded.
    ///    This is what rescues the object that the reference package types as
    ///    `core-properties`.
    fn build_inventory(&mut self) {
        let parts: Vec<String> = self
            .package
            .entries()
            .iter()
            .filter(|name| is_candidate_part(name))
            .cloned()
            .collect();

        // A part shaped like an object but filed where the conventions put the
        // package's own metadata. It is skipped — reading it would put an
        // object in a place no writer will ever look for it again — but the
        // skip is said, because "it is in the file and the tool never mentioned
        // it" is the worst answer a repair tool can give.
        let misfiled: Vec<String> = self
            .package
            .entries()
            .iter()
            .filter(|name| {
                is_package_infrastructure(name)
                    && !name.ends_with(".rels")
                    && uuid_in_part_name(name).is_some()
            })
            .cloned()
            .collect();
        for part in misfiled {
            self.record(Diagnostic::note(
                part.clone(),
                "named like an object but filed under the package's own metadata; not listed",
            ));
        }

        for part in parts {
            let Some(mut entry) = self.classify(&part) else {
                continue;
            };
            let key = entry.summary.uuid.to_ascii_lowercase();
            // **An entry is (uuid, qualified type, objectVersion)**, so the same
            // uuid under another version — or under another release — is
            // another entry, and every part is kept. Only a part that repeats the
            // whole identity is dropped, and said: two parts of one identity
            // cannot both be the object.
            if let Some(previous) = self.by_uuid.get(&key).into_iter().flatten().map(|&at| &self.entries[at]).find(|held| {
                held.summary.qualified_type == entry.summary.qualified_type && held.object_version == entry.object_version
            }) {
                self.record(Diagnostic::warning(
                    part.clone(),
                    format!(
                        "{} already carried by `{}` — same uuid, same type, same objectVersion; \
                         the first part wins",
                        entry.summary.uri, previous.summary.locator
                    ),
                ));
                continue;
            }
            // The first entry of a uuid is keyed by the uuid itself, as every
            // key was before versions existed; its namesakes by a composite.
            entry.slot = if self.by_uuid.contains_key(&key) {
                slot_key(&key, &entry.summary.qualified_type, entry.object_version.as_deref())
            } else {
                key.clone()
            };
            self.by_uuid.entry(key).or_default().push(self.entries.len());
            self.entries.push(entry);
        }
        // Spell the uri of every uuid with a versioned entry: the form with its
        // version (the archive's summaries are built from the part's name and
        // content type, which know no version).
        let versioned: Vec<String> = self
            .by_uuid
            .iter()
            .filter(|(_, list)| list.len() > 1 || list.iter().any(|&at| self.entries[at].object_version.is_some()))
            .map(|(uuid, _)| uuid.clone())
            .collect();
        for uuid in versioned {
            self.reconcile_uris(&uuid);
        }

        if !self.package.has("docProps/core.xml") {
            self.record(Diagnostic::warning(
                "docProps/core.xml",
                "absent, though the EPC conventions make the core properties mandatory (§3.5)",
            ));
        }
    }

    /// Decide what one part is. `None` means "not an EnergyML object".
    fn classify(&self, part: &str) -> Option<PartEntry> {
        let declared = self.package.content_type(part).map(str::to_string);

        // The root element, read once for **every** part.
        //
        // Peeking only when the index was silent is the hole this closes: a
        // `[Content_Types].xml` that types a `TriangulatedSetRepresentation` as
        // a `PointSetRepresentation` was believed without a word, and the
        // object came out of the store as a plausible object of the wrong type
        // — the one failure mode of this reader that produced a wrong answer
        // instead of a named refusal. The peek stops at the end of the
        // citation, so it costs a few hundred inflated bytes per part.
        let peeked = self
            .package
            .with_part(part, |r| header::peek_reader(BufReader::new(r)))
            .ok()
            .filter(|p: &header::PeekedHeader| !p.root_name.is_empty());

        // 1 — the index, when the document itself does not contradict it.
        if let Some(content_type) = &declared
            && let Ok(parts) = identity::parse_content_type(content_type)
            && !self.root_contradicts(peeked.as_ref(), parts.object_type)
        {
            let qualified_type = format!(
                "{}{}.{}",
                parts.domain,
                naming::compact_version(parts.version),
                parts.object_type
            );
            // The root does not *contradict* the index — it names nothing this
            // build knows, so there is no rival type to prefer and the index
            // rightly stands. But it does not agree either, and the read that
            // follows will therefore recognise none of the document: what comes
            // back is an object of the declared type with **every field at its
            // default**, uuid included, and nothing in the values says so.
            //
            // The decision is left alone; only the silence is not. Saying it is
            // the difference between a front end showing a blank object and a front end
            // showing a blank object with a reason.
            if let Some(peeked) = peeked.as_ref()
                && !naming::strip_obj_prefix(&peeked.root_name)
                    .eq_ignore_ascii_case(naming::strip_obj_prefix(parts.object_type))
            {
                self.record(Diagnostic::warning(
                    part,
                    format!(
                        "declared as `{qualified_type}`, but its root element is `{}`, which names \
                         no type this build knows; read as declared, so anything the document \
                         holds under that root is lost",
                        peeked.root_name
                    ),
                ));
            }
            let mut alternatives = Vec::new();
            let mut info = self.registry.by_content_type(content_type).map(|e| e.info);
            // What the substitution below writes into the summary, so the
            // listing and the object cannot disagree: a summary promising a
            // type no read can ever produce helps nobody, and the diagnostic
            // keeps what the file actually said.
            let mut qualified_type = qualified_type;
            let mut content_type = content_type.clone();
            // **A version the registry knows and does not hold is refused, not
            // substituted.** The fallback below is for versions the project
            // does not ship at all; for one it ships and this build left out
            // (`Registry::declare_absent`), reading the part as another release
            // of the package would yield a plausible object holding the fields
            // the two share. The part stays listed, unreadable, and says why.
            let mut refusal = if info.is_none() {
                self.registry
                    .absent_content_type(&content_type)
                    .or_else(|| self.registry.absent_qualified_type(&qualified_type))
                    .map(str::to_string)
            } else {
                None
            };
            // **A registered content type does not outvote a document that
            // names an absent version.** `version=2.2` in the index and
            // `schemaVersion="2.0"` on the root contradict each other toward a
            // release this build left out; following the index would read a
            // 2.0.1 document as 2.2 — a plausible object of the wrong release.
            // The document is the part, so the part is refused by the absent
            // message. A document with no declaration, one naming a compiled
            // release, a development drop or a version nobody declared absent
            // leaves the index in charge, as before.
            if refusal.is_none()
                && info.is_some()
                && let Some(peeked_header) = peeked.as_ref()
                && let Some((package, why)) = self.absent_declaration(peeked_header)
                && package.eq_ignore_ascii_case(parts.domain)
            {
                let release = peeked_header.schema_version.as_deref().map(naming::release_of).unwrap_or_default();
                qualified_type = format!("{package}{}.{}", naming::compact_version(release), parts.object_type);
                content_type = format!(
                    "application/x-{package}+xml;version={release};type={}",
                    parts.object_type
                );
                info = None;
                refusal = Some(why);
            }
            if let Some(why) = &refusal {
                self.record(Diagnostic::warning(
                    part,
                    format!("`{qualified_type}` cannot be read: {why}; the part is listed but not read"),
                ));
            }
            if info.is_none() && refusal.is_none() {
                // The declared version is one this build does not carry. Rather
                // than list a row nothing can open, fall back on the document:
                // the root element names the concept, its namespace names the
                // package, and another release of the same package will read
                // most of it. Said out loud, because it is a substitution.
                let mut candidates = peeked
                    .as_ref()
                    .map(|p| self.root_candidates(p).0)
                    .unwrap_or_default();
                if !candidates.is_empty() {
                    let chosen = candidates.remove(0);
                    self.record(Diagnostic::warning(
                        part,
                        format!(
                            "`{qualified_type}` is not registered in this build; reading it as \
                             `{}` instead, from the root element and its namespace",
                            chosen.qualified_type.unwrap_or(chosen.name)
                        ),
                    ));
                    qualified_type = chosen.qualified_type.unwrap_or(chosen.name).to_string();
                    content_type = chosen.content_type.unwrap_or_default().to_string();
                    info = Some(chosen);
                    alternatives = candidates;
                } else {
                    self.record(Diagnostic::note(
                        part,
                        format!("`{qualified_type}` is not registered; listed but not readable"),
                    ));
                }
            }
            // **A content type cannot name a development drop, so it cannot
            // settle the version when the document names one.**
            //
            // `application/x-resqml+xml;version=2.2;type=X` is what the
            // released 2.2 *and* `resqml 2.2_dev3` both write — the drop's
            // schemas declare the release's own `version="2.2"`. The index
            // therefore resolves to the release whatever the part holds, and
            // the branch above never runs because the lookup *succeeded*.
            //
            // The document knows better: its `schemaVersion` is the only place
            // a drop can be named at all. So when it names one and the index
            // answered with a release, the candidates decide instead — the same
            // "the document beats the index" rule this reader already applies
            // to the *type*, extended to the version for the one case where the
            // index has no way to express what the document said.
            //
            // Only the release moves: a candidate naming another **concept** is
            // refused, because that is the index's job and it is not in doubt.
            if let Some(peeked_header) = peeked.as_ref()
                && let Some(declared) = peeked_header.schema_version.as_deref()
                && naming::is_prerelease(declared)
                && let Some(held) = info
                && !naming::is_prerelease(held.version)
            {
                let mut candidates = self.root_candidates(peeked_header).0;
                candidates.retain(|c| c.type_key == held.type_key);
                if !candidates.is_empty() {
                    let chosen = candidates.remove(0);
                    if !std::ptr::eq(chosen, held) {
                        self.record(Diagnostic::note(
                            part,
                            format!(
                                "declared `{}` in [Content_Types].xml, which names no development drop; its own schemaVersion says `{declared}`, so it is read as `{}` — the document is the only place a drop can be named",
                                content_type,
                                chosen.qualified_type.unwrap_or(chosen.name)
                            ),
                        ));
                        qualified_type = chosen.qualified_type.unwrap_or(chosen.name).to_string();
                        content_type = chosen.content_type.unwrap_or_default().to_string();
                        info = Some(chosen);
                    }
                    alternatives = candidates;
                }
            }
            let Some(uuid) = self.uuid_of(part, peeked.as_ref()) else {
                self.record(Diagnostic::warning(
                    part,
                    format!(
                        "declared as `{qualified_type}` but carries no uuid — none in the part name, \
                         none on the root element; it cannot be addressed, so it is not listed"
                    ),
                ));
                return None;
            };
            return Some(PartEntry {
                summary: ObjectSummary {
                    uri: identifiers::EtpUri::object(None, &qualified_type, &uuid),
                    uuid,
                    qualified_type,
                    content_type,
                    locator: part.to_string(),
                },
                info,
                slot: String::new(),
                object_version: normal_version(peeked.as_ref().and_then(|p| p.object_version.as_deref())),
                alternatives,
                refusal,
            });
        }

        // 2 — the root element, when the index is absent, unusable, or wrong.
        //
        // Every way out of this branch is a part that will not appear in the
        // inventory, so every way out says why. An invisible part is worse than
        // an unreadable one: the second can be repaired, the first is not even
        // known to be there.
        let Some(peeked) = peeked else {
            self.record(Diagnostic::note(
                part,
                match &declared {
                    Some(ct) => format!("declared `{ct}`, but nothing readable was found in it"),
                    None => "no content type and no readable root element; not listed".to_string(),
                },
            ));
            return None;
        };
        let Some(uuid) = self.uuid_of(part, Some(&peeked)) else {
            self.record(Diagnostic::note(
                part,
                format!(
                    "root element `{}` carries no uuid, and the part name holds none either; not listed",
                    peeked.root_name
                ),
            ));
            return None;
        };
        // **The document declares a version the registry knows and does not
        // hold: refuse it by name, before any candidate is offered.** Of the
        // candidates `root_candidates` returns, none is this version — the
        // best of them would be another release of the package, read with
        // whatever fields the two share.
        if let Some((package, why)) = self.absent_declaration(&peeked) {
            let release = peeked.schema_version.as_deref().map(naming::release_of).unwrap_or_default();
            let qualified_type = format!("{package}{}.{}", naming::compact_version(release), peeked.root_name);
            self.record(Diagnostic::warning(
                part,
                format!("`{qualified_type}` cannot be read: {why}; the part is listed but not read"),
            ));
            let content_type = declared.clone().unwrap_or_else(|| {
                format!("application/x-{package}+xml;version={release};type={}", peeked.root_name)
            });
            return Some(PartEntry {
                summary: ObjectSummary {
                    uri: identifiers::EtpUri::object(None, &qualified_type, &uuid),
                    uuid,
                    qualified_type,
                    content_type,
                    locator: part.to_string(),
                },
                info: None,
                slot: String::new(),
                object_version: normal_version(peeked.object_version.as_deref()),
                alternatives: Vec::new(),
                refusal: Some(why),
            });
        }
        let (mut candidates, confident) = self.root_candidates(&peeked);
        let Some(info) = (!candidates.is_empty()).then(|| candidates.remove(0)) else {
            self.record(Diagnostic::warning(
                part,
                match &declared {
                    Some(ct) => format!(
                        "declared `{ct}`, which this build cannot parse, and its root element \
                         `{}` names no type it knows; not listed",
                        peeked.root_name
                    ),
                    None => format!(
                        "not declared in [Content_Types].xml, and its root element `{}` names no \
                         type this build knows; not listed",
                        peeked.root_name
                    ),
                },
            ));
            return None;
        };
        let Some(qualified_type) = info.qualified_type.map(str::to_string) else {
            self.record(Diagnostic::warning(
                part,
                format!(
                    "root element `{}` resolves to a type with no qualified name; not listed",
                    peeked.root_name
                ),
            ));
            return None;
        };
        self.record(Diagnostic::warning(
            part,
            match &declared {
                Some(ct) => format!(
                    "declared as `{ct}`, but its root element is `{}`; the root element wins",
                    peeked.root_name
                ),
                None => format!(
                    "not declared in [Content_Types].xml; typed `{qualified_type}` from its root element"
                ),
            },
        ));
        // The namespace named the package and `schemaVersion` did not settle
        // the release, so more than one is still possible. Said now, because a
        // reader that silently picked one and got a plausible object out of it
        // is the failure this whole path exists to avoid.
        if !candidates.is_empty() && !confident {
            self.record(Diagnostic::note(
                part,
                format!(
                    "{} does not identify a single version{}; read as `{qualified_type}`, with {} \
                     tried in turn if it will not parse",
                    match &peeked.schema_version {
                        Some(version) => format!("`schemaVersion=\"{version}\"`"),
                        None => "no `schemaVersion`".to_string(),
                    },
                    match &peeked.namespace {
                        Some(_) => " — one namespace covers every version of a package",
                        None => "",
                    },
                    candidates
                        .iter()
                        .map(|info| info.qualified_type.unwrap_or(info.name))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            ));
        }
        Some(PartEntry {
            summary: ObjectSummary {
                uri: identifiers::EtpUri::object(None, &qualified_type, &uuid),
                uuid,
                qualified_type,
                content_type: info.content_type.unwrap_or_default().to_string(),
                locator: part.to_string(),
            },
            info: Some(info),
            slot: String::new(),
            object_version: normal_version(peeked.object_version.as_deref()),
            alternatives: candidates,
            refusal: None,
        })
    }

    /// The registered type whose element is `root_name`, in the version the
    /// document declares. Both spellings of the root are accepted, because both
    /// occur: the reference package writes `obj_DiscreteProperty` where the
    /// schema says `DiscreteProperty`.
    /// Which registered type a root element is, and what else it could be.
    ///
    /// # Why the namespace is not enough, and neither is the name
    ///
    /// Energistics gave **every version of a package the same namespace**:
    /// `resqmlv2` is RESQML 2.0.1 *and* RESQML 2.2, `witsmlv2` is WITSML 2.0
    /// *and* 2.1. So the namespace answers "which package" — which is worth
    /// something, since `Activity` exists in both `resqml20` and `eml23` with
    /// different namespaces — and says nothing at all about the version.
    ///
    /// The version comes from `schemaVersion`, and that is where it gets thin:
    /// the attribute is optional in practice, producers write `2.0` where the
    /// release is 2.0.1, and a file may name a version this build does not
    /// have. Until 02/08 an unusable `schemaVersion` fell through to
    /// `candidates.first()` — registry insertion order, silently. A part read
    /// as the wrong version does not fail: it produces a plausible object with
    /// the fields the two versions share and nothing else, which is the one
    /// outcome this reader is built to avoid.
    ///
    /// So this returns **every** candidate, best first, and the caller reads
    /// down the list until one parses. The order is:
    ///
    /// 1. same namespace as the document, when it declared one;
    /// 2. `schemaVersion` matching exactly — on the release or on the schema's
    ///    own two-digit form;
    /// 3. `schemaVersion` compatible by prefix, so `2.0` finds 2.0.1;
    /// 4. the `obj_` prefix, which only the older releases write — a **positive**
    ///    signal when present and no signal at all when absent, because
    ///    `ALWYN-RESQML.epc` is a 2.0.1 package whose roots do not carry it;
    /// 5. newest first, as a last resort that is at least deterministic.
    ///
    /// Returns the candidates and whether the first one was **identified**
    /// rather than merely preferred — an exact `schemaVersion` match, or a
    /// single candidate. The rest are insurance, and silent when the choice
    /// was confident: reporting an ambiguity that the document settled would
    /// bury the ones it did not.
    fn root_candidates(&self, peeked: &header::PeekedHeader) -> (Vec<&'static TypeInfo>, bool) {
        let mut candidates: Vec<&'static TypeInfo> = self
            .registry
            .types_by_key(&peeked.root_name)
            .into_iter()
            .filter(|info| info.is_top_level)
            .collect();

        // The package, when the document said which. Applied as a filter and
        // not as a preference: a `resqml20.Activity` is not a worse
        // `eml23.Activity`, it is a different object.
        if let Some(namespace) = &peeked.namespace {
            let same: Vec<&'static TypeInfo> = candidates
                .iter()
                .filter(|info| info.xml_ns.uri == namespace)
                .copied()
                .collect();
            if !same.is_empty() {
                candidates = same;
            }
        }

        // The declared version, trimmed to its digits: the 2016 Geosiris
        // editor wrote `Resqml V2.0`, and a spelling the comparison cannot
        // read must not throw the *whole* declaration away — `V2.0` still
        // says 2.0. Found by the BRT acceptance packages: with the raw
        // string unusable, every candidate ranked equal, "newest first"
        // picked 2.2, and a 2.0.1 part read as a plausible 2.2 object whose
        // reference was empty — the exact outcome this ladder exists to
        // avoid.
        let declared = peeked.schema_version.as_deref();
        // Split into the release and the development drop, because those are
        // two different questions. `naming::release_of` also drops a leading
        // label — the 2016 Geosiris editor wrote `Resqml V2.0`, and a spelling
        // the comparison cannot read must not throw the *whole* declaration
        // away. Found by the BRT acceptance packages: with the raw string
        // unusable every candidate ranked equal, "newest first" picked 2.2,
        // and a 2.0.1 part read as a plausible 2.2 object whose reference was
        // empty — the exact outcome this ladder exists to avoid.
        let declared_release = declared.map(naming::release_of);
        let declared_drop = declared.and_then(|v| naming::split_prerelease(v).1);
        let obj_prefixed = naming::has_obj_prefix(&peeked.root_name);
        let rank = |info: &&'static TypeInfo| -> u8 {
            match declared_release {
                Some(version) => {
                    // Compared on releases: `2.2_dev3` *is* a 2.2, and which
                    // of the two crates it is comes next, not here.
                    let by_dir = naming::release_of(info.version);
                    let by_wire = naming::release_of(info.schema_version);
                    if naming::version_eq(by_wire, version) || naming::version_eq(by_dir, version) {
                        0
                    } else if version_compatible(by_dir, version)
                        || version_compatible(by_wire, version)
                    {
                        1
                    } else if obj_prefixed && naming::has_obj_prefix(info.object_type) {
                        // The declared version helped nobody — the `obj_`
                        // prefix is still a positive signal, same as when
                        // no version was declared at all.
                        2
                    } else {
                        3
                    }
                }
                // No version to go on: the `obj_` prefix is the only other
                // thing the document said about its age.
                None if obj_prefixed && naming::has_obj_prefix(info.object_type) => 0,
                None => 2,
            }
        };
        // **The release outranks its development drops, and a named drop
        // outranks the release.**
        //
        // Energistics ships `resqml 2.2_dev3` beside `resqml 2.2`, and the drop
        // declares the release's own `version="2.2"` — so on the wire the two
        // are indistinguishable and *both* rank 0 above. A document saying
        // `2.2` is overwhelmingly a released 2.2, so that is tried first and
        // the drop is the fallback the strict pass falls to; a document that
        // names the drop gets the drop first. Neither is a guess: whichever is
        // tried second only ever gets a turn because the first *refused*.
        //
        // With no drop registered every candidate scores 0 here, which is why
        // adding one changes the order of nothing else.
        let drop_rank = |info: &&'static TypeInfo| -> u8 {
            match (naming::split_prerelease(info.version).1, declared_drop) {
                (None, None) => 0,
                (Some(held), Some(said)) if naming::prerelease_eq(held, said) => 0,
                // Asked for a drop and offered the release it became, or asked
                // for the release and offered a drop of it: the near miss.
                (None, Some(_)) | (Some(_), None) => 1,
                // A different drop of the same release: further still.
                (Some(_), Some(_)) => 2,
            }
        };
        candidates.sort_by(|a, b| {
            rank(a)
                .cmp(&rank(b))
                .then_with(|| drop_rank(a).cmp(&drop_rank(b)))
                // Newest first among equals: deterministic, and the better bet
                // for a file that did not say.
                .then_with(|| b.version.cmp(a.version))
        });
        let confident = candidates.len() <= 1
            || (declared.is_some()
                && candidates.first().map(|i| (rank(i), drop_rank(i))) == Some((0, 0)));
        (candidates, confident)
    }

    /// The package and the refusal, when the document's own namespace and
    /// `schemaVersion` name a version the registry was told is absent.
    ///
    /// The namespace answers "which package" (`resqmlv2` is RESQML, whatever
    /// the release) and `schemaVersion` answers the release; both have to be
    /// there, since a document that says one only does not name a version.
    fn absent_declaration(&self, peeked: &header::PeekedHeader) -> Option<(String, String)> {
        let uri = peeked.namespace.as_deref()?;
        let declared = peeked.schema_version.as_deref()?;
        let package = energyml_core::ns::well_known_namespaces().iter().find(|known| known.uri == uri)?.prefix;
        self.registry.absent(package, declared).map(|why| (package.to_string(), why.to_string()))
    }

    /// The best candidate alone, for the callers that only need a name.
    fn type_of_root(&self, root_name: &str, schema_version: Option<&str>) -> Option<&'static TypeInfo> {
        self.root_candidates(&header::PeekedHeader {
            root_name: root_name.to_string(),
            schema_version: schema_version.map(str::to_string),
            ..Default::default()
        })
        .0
        .first()
        .copied()
    }

    /// Whether the root element names a **different** type from the one the
    /// index declares, in a way this build can adjudicate.
    ///
    /// Both spellings are one name — the reference package writes
    /// `obj_DiscreteProperty` in the document and `DiscreteProperty` in the
    /// content type — and the comparison ignores case, because a difference of
    /// case is a producer's habit rather than a claim about the type.
    ///
    /// A root element this build does not know is **not** a contradiction: the
    /// declared type would be thrown away for nothing and the part would drop
    /// out of the inventory entirely. Only a disagreement where the document's
    /// own word can be honoured counts, so the worst case remains "listed under
    /// the type it declares", never "lost".
    fn root_contradicts(&self, peeked: Option<&header::PeekedHeader>, declared_type: &str) -> bool {
        let Some(peeked) = peeked else {
            return false;
        };
        let root = naming::strip_obj_prefix(&peeked.root_name);
        if root.eq_ignore_ascii_case(naming::strip_obj_prefix(declared_type)) {
            return false;
        }
        self.type_of_root(&peeked.root_name, peeked.schema_version.as_deref())
            .is_some_and(|info| info.qualified_type.is_some())
    }

    /// The uuid of a part: from its name if it is spelled the way the
    /// specification asks (`<objectType>_<uuid>.xml`, §3.2.1), from its root
    /// element otherwise.
    fn uuid_of(&self, part: &str, peeked: Option<&header::PeekedHeader>) -> Option<String> {
        if let Some(uuid) = uuid_in_part_name(part) {
            return Some(uuid);
        }
        let peeked = peeked?;
        if peeked.uuid.is_some() {
            self.record(Diagnostic::note(
                part,
                "part name does not end in `_<uuid>.xml`; the uuid was read from the object",
            ));
        }
        peeked.uuid.clone()
    }
}

/// A last resort, not a save.
///
/// `Drop` cannot return a `Result`, so it cannot write: a failure would have
/// nowhere to go. What it can do is refuse to let unsaved work disappear in
/// silence.
impl Drop for EpcStore {
    fn drop(&mut self) {
        if !self.closed && self.policy == SavePolicy::OnClose && self.overlay.is_dirty() {
            eprintln!(
                "energyml-storage: `{}` was dropped with unsaved changes and the OnClose policy. \
                 Drop cannot report a failed write, so nothing was written — call `close()`.",
                self.name
            );
        }
    }
}

/// Resolving proxies is something only the package can do, so the store is the
/// resolver.
impl ProxyResolver for EpcStore {
    fn resolve(&self, proxy_uuid: &str) -> Option<crate::array::ProxyTarget> {
        self.external_target_for(proxy_uuid, None)
    }
}

/// The asynchronous face of the same store: one line per method.
///
/// Nothing happens here that does not happen synchronously above. That is the
/// point of option D — the wrappers are mechanical, so the two surfaces cannot
/// drift apart.
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
impl EnergymlStorage for EpcStore {
    fn store_kind(&self) -> &'static str {
        "EPC"
    }

    async fn dataspaces(&self) -> Result<Vec<Dataspace>> {
        Ok(vec![self.dataspace()])
    }

    async fn resources(&self) -> Result<Vec<ObjectSummary>> {
        Ok(EpcStore::resources(self))
    }

    async fn headers(&self, uris: &[String]) -> Vec<Result<ObjectHeader>> {
        EpcStore::headers(self, uris)
    }

    async fn get_objects(&self, uris: &[String]) -> Vec<Result<Box<dyn DynObject>>> {
        EpcStore::get_objects(self, uris)
    }

    async fn supported_types(&self) -> Vec<&'static TypeInfo> {
        EpcStore::supported_types(self)
    }

    fn arrays_of(&self, object: &dyn Reflect) -> Vec<ArrayLocation> {
        EpcStore::arrays_of(self, object)
    }

    async fn array_metadata(&self, location: &ArrayLocation) -> Result<ArrayMetadata> {
        EpcStore::array_metadata(self, location)
    }

    async fn read_array(&self, location: &ArrayLocation) -> Result<ArrayData> {
        EpcStore::read_array(self, location)
    }

    fn diagnostics(&self) -> Vec<Diagnostic> {
        EpcStore::diagnostics(self)
    }
}

impl LocalPackage for EpcStore {
    /// What a save would carry, not what the archive held.
    ///
    /// Three differences, and each one was a way for this list to lie: a part
    /// dropped in this session is gone, a file added in it is here, and neither
    /// is visible in `package.entries()`.
    fn attachments(&self) -> Vec<String> {
        // A set, not `entries.iter().any` per part: 16 250 parts against as many
        // objects was a quarter of a billion comparisons (02/10/2026).
        let objects: HashSet<&str> = self.entries.iter().map(|e| e.summary.locator.as_str()).collect();
        let mut out: Vec<String> = self
            .package
            .entries()
            .iter()
            .filter(|name| !is_package_infrastructure(name))
            .filter(|name| !objects.contains(name.as_str()))
            .filter(|name| !self.overlay.is_dropped(name))
            .cloned()
            .collect();
        for part in self.overlay.raw_parts() {
            let known = self.package.has(&part)
                || objects.contains(part.as_str())
                || is_package_infrastructure(&part);
            if !known {
                out.push(part);
            }
        }
        out.sort();
        out.dedup();
        out
    }

    fn read_attachment(&self, part: &str) -> Result<Vec<u8>> {
        // `raw_part`, not `package.read`: a file added in this session is not in
        // the archive at all, and one that was replaced is not what the archive
        // holds. Reading the archive here would answer about a package that no
        // longer exists. And a part this session removed is gone for the
        // reader too — the archive still holds its bytes until the save.
        if self.overlay.raw(part).is_none() && self.overlay.is_dropped(part) {
            return Err(Error::Package(format!("no part `{part}` in the package: it was removed")));
        }
        self.raw_part(part)
    }

    fn relationships(&self, part: &str) -> Result<Vec<Relationship>> {
        self.relationships_recorded(part)
    }
}

/// Whether a declared version is a prefix of a candidate's, or the reverse.
///
/// `schemaVersion="2.0"` on a RESQML 2.0.1 object is the schema's own rule —
/// *"the schema version is the first 2 digits of an ML version"* — so the two
/// have to meet, and they meet on the dotted components rather than on the
/// string: `2.1` must not match `2.10`.
fn version_compatible(candidate: &str, declared: &str) -> bool {
    let parts = |value: &str| -> Vec<String> {
        value
            .trim()
            .split('.')
            .map(|piece| piece.trim().to_string())
            .filter(|piece| !piece.is_empty())
            .collect()
    };
    let (a, b) = (parts(candidate), parts(declared));
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a.iter().zip(b.iter()).all(|(x, y)| x == y)
}

/// Absent data, or an absent capability — and never the first when it is the
/// second.
///
/// A build with no HDF5 cannot read the array whether or not the file is on the
/// disk, and *"no support for this format"* is the more useful of the two
/// answers: the other one sends someone hunting for a file that is sitting
/// beside the package. `no_backend` carries the reason the registry gave, when
/// it was ever asked; `backends.is_empty()` covers the case where the candidate
/// list was empty so nothing was asked at all.
///
/// `detail`, when given, is how hard [`EpcStore::search_array`] looked before
/// giving up — folded into the `FileMissing` error's own display string so a
/// caller that only prints the error, not the diagnostic log beside it, still
/// gets more than *file not found*. The reason itself stays `FileMissing`:
/// that is what a caller matches on to try another candidate or ask for the
/// file, and it must not depend on whether a search ever ran.
fn missing_or_unsupported(
    location: &ArrayLocation,
    backends: &crate::backend::Backends,
    no_backend: Option<Error>,
    detail: Option<&str>,
) -> Error {
    if let Some(reason) = no_backend {
        return reason;
    }
    if backends.is_empty() {
        return Error::ExternalData {
            location: location.field_path.clone(),
            reason: ExternalDataReason::FeatureNotEnabled {
                feature: "hdf5",
                mime: location.mime.clone(),
            },
        };
    }
    let location = match detail {
        Some(detail) => format!("{} — {detail}", location.field_path),
        None => location.field_path.clone(),
    };
    Error::ExternalData { location, reason: ExternalDataReason::FileMissing }
}

/// Parts that describe the package rather than belong to it.
fn is_package_infrastructure(name: &str) -> bool {
    name == "[Content_Types].xml"
        || name.ends_with(".rels")
        || name.contains("/_rels/")
        || name.starts_with("_rels/")
        || name.starts_with("docProps/")
        || name.ends_with('/')
}

/// Whether a part is worth asking "is this an EnergyML object?".
fn is_candidate_part(name: &str) -> bool {
    !is_package_infrastructure(name) && name.to_ascii_lowercase().ends_with(".xml")
}

/// `resqml20/obj_Activity_<uuid>.xml` → the uuid.
///
/// The name of an XML part is `<objectType>_<uuid>.xml` (EPC §3.2.1). A uuid
/// contains no underscore, so the last one separates the two.
fn uuid_in_part_name(part: &str) -> Option<String> {
    let file = part.rsplit('/').next()?;
    let stem = file.get(..file.len().checked_sub(4)?)?;
    let candidate = stem.rsplit('_').next()?;
    identifiers::is_uuid(candidate).then(|| candidate.to_string())
}

/// What an address names: the uuid, and — for a URI — the qualified type and the
/// `objectVersion` it spells (Identifier Specification v5.0 §3.4.2, p. 14). A
/// bare uuid names neither.
fn address_of(uri: &str) -> Result<Address> {
    if let Ok(parsed) = identifiers::EtpUri::parse(uri)
        && let Some(uuid) = parsed.uuid.clone()
    {
        return Ok(Address {
            uuid: uuid.to_ascii_lowercase(),
            qualified_type: parsed.qualified_type().map(|qualified| qualified.to_string()),
            version: normal_version(parsed.object_version.as_deref()),
        });
    }
    let trimmed = uri.trim();
    if identifiers::is_uuid(trimmed) {
        return Ok(Address { uuid: trimmed.to_ascii_lowercase(), qualified_type: None, version: None });
    }
    Err(Error::BadUri {
        uri: uri.to_string(),
        reason: "expected an ETP data-object URI or a bare uuid".to_string(),
    })
}

/// The uuid an address names — an ETP URI, or a bare uuid for the caller who
/// has one in hand.
fn uuid_in_uri(uri: &str) -> Result<String> {
    if let Ok(parsed) = identifiers::EtpUri::parse(uri)
        && let Some(uuid) = parsed.uuid
    {
        return Ok(uuid);
    }
    let trimmed = uri.trim();
    if identifiers::is_uuid(trimmed) {
        return Ok(trimmed.to_string());
    }
    Err(Error::BadUri {
        uri: uri.to_string(),
        reason: "expected an ETP data-object URI or a bare uuid".to_string(),
    })
}

fn local_name(raw: &[u8]) -> String {
    String::from_utf8_lossy(raw.rsplit(|b| *b == b':').next().unwrap_or_default()).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_capability_never_comes_out_as_a_missing_file() {
        // The distinction this exists for, and the hour it costs when it is
        // lost: a build with no HDF5 reported `file not found` for arrays whose
        // `.h5` was sitting beside the package, which sends the reader after
        // the file, the paths and the `.rels` — everything except the build.
        let location = ArrayLocation {
            field_path: "NodeMd.Values.ExternalDataArrayPart[0]".into(),
            path_in_file: "/resqmlv2/x/node_md".into(),
            mime: Some("application/x-hdf5".into()),
            ..Default::default()
        };

        // Nothing compiled in, and no candidate was ever asked.
        let empty = crate::backend::Backends::none();
        assert!(empty.is_empty());
        assert!(matches!(
            missing_or_unsupported(&location, &empty, None, None),
            Error::ExternalData {
                reason: ExternalDataReason::FeatureNotEnabled { .. },
                ..
            }
        ));

        // A candidate was asked and the registry said it handles nothing of
        // this kind: that reason is better than anything invented here.
        let refused = Error::ExternalData {
            location: "somewhere".into(),
            reason: ExternalDataReason::FeatureNotEnabled {
                feature: "hdf5",
                mime: Some("application/x-hdf5".into()),
            },
        };
        assert!(matches!(
            missing_or_unsupported(&location, &empty, Some(refused), None),
            Error::ExternalData {
                reason: ExternalDataReason::FeatureNotEnabled { .. },
                ..
            }
        ));

        // A backend exists and every candidate was opened and did not hold the
        // dataset: that one really is a missing file, and must stay one. Only
        // checkable in a build that has a backend — which is the point.
        #[cfg(feature = "hdf5")]
        {
            let stocked = crate::backend::Backends::compiled_in();
            assert!(!stocked.is_empty());
            assert!(matches!(
                missing_or_unsupported(&location, &stocked, None, None),
                Error::ExternalData {
                    reason: ExternalDataReason::FileMissing,
                    ..
                }
            ));
        }
    }

    #[test]
    fn a_part_name_yields_its_uuid() {
        assert_eq!(
            uuid_in_part_name("resqml20/obj_Activity_f0d2a4ec-1234-4f22-9d1a-000000000001.xml").as_deref(),
            Some("f0d2a4ec-1234-4f22-9d1a-000000000001")
        );
        assert_eq!(
            uuid_in_part_name("f0d2a4ec-1234-4f22-9d1a-000000000001.xml").as_deref(),
            Some("f0d2a4ec-1234-4f22-9d1a-000000000001")
        );
        assert_eq!(uuid_in_part_name("docProps/core.xml"), None);
    }

    #[test]
    fn an_address_may_be_a_uri_or_a_uuid() {
        let uuid = "f0d2a4ec-1234-4f22-9d1a-000000000001";
        assert_eq!(
            uuid_in_uri(&format!("eml:///resqml20.obj_Activity({uuid})")).unwrap(),
            uuid
        );
        assert_eq!(uuid_in_uri(uuid).unwrap(), uuid);
        assert!(uuid_in_uri("not an address").is_err());
    }

    #[test]
    fn infrastructure_is_not_an_object() {
        assert!(!is_candidate_part("[Content_Types].xml"));
        assert!(!is_candidate_part("_rels/.rels"));
        assert!(!is_candidate_part("resqml20/_rels/obj_Activity_x.xml.rels"));
        assert!(!is_candidate_part("docProps/core.xml"));
        assert!(!is_candidate_part("media/report.pdf"));
        assert!(is_candidate_part("resqml20/obj_Activity_x.xml"));
    }
}
