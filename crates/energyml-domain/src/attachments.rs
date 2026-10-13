//! The files attached to an object — read, in every form the package can hold
//! one in (ADR-0014).
//!
//! # The four forms
//!
//! | | where the bytes are | how it is found |
//! |---|---|---|
//! | **A** [`Form::Part`] | a part of the zip | `destinationMedia` in the object's `.rels`, `sourceMedia` back (EPC §3.4.2 pp. 15–16) |
//! | **B** [`Form::Inline`] | the XML of an `eml23.Attachment`, base 64 in `Content` | `ObjectReference` |
//! | **C** [`Form::PartAddress`] | a part of the zip, `Content` empty | `ContentUri` read as a **part name** — this tool's convention, said |
//! | **D** [`Form::External`] | a server | `ContentUri` is an `http(s)` address |
//!
//! Two rules without which a form lies (§2 of the design):
//!
//! - **an empty `Content` is not an empty file.** `xsd:base64Binary` allows the
//!   empty string, so C and D write it; a reader that finds `Content` empty
//!   *and* a `ContentUri` follows the address. Empty with no address is an
//!   empty file, and it is said (`problems`);
//! - **B and C together** have two sources. The content is the authority — the
//!   schema requires it — the address is shown, and a measured disagreement
//!   (the part exists and its bytes differ) is said.
//!
//! # What is read, and what is not
//!
//! [`attachments_of`] lists, in a batch, **without a byte of a file**: the
//! `.rels`, the index's `ObjectReference` edges, and the `Attachment` objects
//! those edges lead to (their `Content` is parsed with the object, as any
//! object is, but never returned). [`attachment_bytes`] reads **one** piece, at
//! the moment somebody asks. An external address is **never followed here**:
//! this module has no network, and the command line must not have one it was
//! not asked for (§6) — [`Fetched::External`] hands the address back.
//!
//! # Who attaches to whom
//!
//! A piece names an object by its own relation (A) or `ObjectReference` (B–D).
//! One that nobody names is **inherited** through the walk of
//! [`crate::inherit`] — the object's collections, its parents — and says so in
//! its [`Provenance`]: *a symbol is never listed without where it came from*.
//! The listing keeps to what the walk resolves: the nearest carrier, with the
//! tie-break the walk states. An object that has pieces of its own lists those.
//!
//! The base 64 codec is here too ([`encode_base64`], [`decode_base64`]): the
//! crate needs it for `Content` and has no dependency for it, and the writer
//! (`energyml-build`'s `attach`) calls the same two functions as the reader.

use std::collections::{HashMap, HashSet};

use energyml_core::value::{FieldValue, Reflect};
use energyml_storage::epc::{PartKind, media};
use energyml_storage::{EpcStore, LocalPackage};

use crate::inherit::{Via, Walker};
use crate::package_index::PackageIndex;
use crate::source::ObjectSource;

/// The files of a package, beside its objects — what [`ObjectSource`] is to the
/// objects. An EPC implements it below; a store with no files answers empty,
/// and the pieces that need one are not found, which is the truth.
pub trait FileSource: ObjectSource {
    /// Every media relation: `(object uuid lowercased, part name)`.
    fn media_links(&self) -> Vec<(String, String)>;
    /// The attachments of the package — `(part, size, content type)`.
    fn files(&self) -> Vec<(String, Option<u64>, Option<String>)>;
    /// The bytes of one part.
    fn read_part(&self, part: &str) -> Result<Vec<u8>, String>;
    /// The same source as the objects — spelled out, since a trait object
    /// does not upcast on the toolchain this crate supports (1.85).
    fn objects(&self) -> &dyn ObjectSource;
}

impl FileSource for EpcStore {
    fn media_links(&self) -> Vec<(String, String)> {
        EpcStore::media_links(self).into_iter().map(|link| (link.uuid, link.part)).collect()
    }

    fn files(&self) -> Vec<(String, Option<u64>, Option<String>)> {
        self.parts()
            .into_iter()
            .filter(|listing| listing.kind == PartKind::Attachment && !listing.dropped)
            .map(|listing| (listing.part, listing.size, listing.content_type))
            .collect()
    }

    fn read_part(&self, part: &str) -> Result<Vec<u8>, String> {
        self.read_attachment(part).map_err(|why| why.to_string())
    }

    fn objects(&self) -> &dyn ObjectSource {
        self
    }
}

/// Which of the four forms a piece is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
    /// A — a part of the zip, tied by `destinationMedia`.
    Part,
    /// B — an `Attachment` carrying its bytes in `Content`.
    Inline,
    /// C — an `Attachment` whose `ContentUri` names a part of the package.
    PartAddress,
    /// D — an `Attachment` whose `ContentUri` is an external address.
    External,
}

impl Form {
    /// `A`, `B`, `C`, `D` — the letters of the design.
    pub fn letter(self) -> char {
        match self {
            Form::Part => 'A',
            Form::Inline => 'B',
            Form::PartAddress => 'C',
            Form::External => 'D',
        }
    }

    /// `part`, `inline`, `part-address`, `external`.
    pub fn as_str(self) -> &'static str {
        match self {
            Form::Part => "part",
            Form::Inline => "inline",
            Form::PartAddress => "part-address",
            Form::External => "external",
        }
    }
}

/// Where the bytes of a piece are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Location {
    /// A part of the package.
    Part(String),
    /// In the XML of the `Attachment`.
    Inline,
    /// At an address the module never follows.
    Address(String),
    /// Nowhere: no content and no address.
    Nowhere,
}

/// How a piece reached the object that asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Provenance {
    /// `self`, `parent`, `collection` or `model`.
    pub kind: &'static str,
    /// The uuid of the object the piece is attached to.
    pub carrier: String,
    /// Its title, when the index knows it.
    pub carrier_title: Option<String>,
    /// The steps from the asking object to the carrier, as words:
    /// `collection`, `parent (InterpretedFeature)`.
    pub path: Vec<String>,
}

impl Provenance {
    /// The sentence the front end shows: *inherited from the collection X*.
    pub fn says(&self) -> String {
        let name = self.carrier_title.clone().unwrap_or_else(|| self.carrier.clone());
        match self.kind {
            "self" => "attached to this object".to_string(),
            kind => format!("inherited from the {kind} {name}"),
        }
    }
}

/// A measured depth, as the `Attachment` states it.
#[derive(Clone, Debug, PartialEq)]
pub struct Depth {
    /// The number.
    pub value: f64,
    /// Its unit, as written.
    pub uom: String,
}

/// One file attached to an object.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    /// What [`attachment_bytes`] takes: `part:<name>` or `object:<uri>`.
    pub key: String,
    /// A, B, C or D.
    pub form: Form,
    /// How it reached the object that asked.
    pub provenance: Provenance,
    /// The file's name — the `FileName` of an `Attachment`, the part's last
    /// segment otherwise.
    pub file_name: Option<String>,
    /// Its media type: `FileType`, or the part's content type.
    pub media_type: Option<String>,
    /// Its size in bytes, when this reading knows it without fetching.
    pub size: Option<u64>,
    /// The depth along the hole it is indexed at; `None` is the whole object.
    pub md: Option<Depth>,
    /// `Category` — `CuttingsPicture`, `Symbol`.
    pub category: Option<String>,
    /// Where the bytes are.
    pub location: Location,
    /// The uuid of the `Attachment` object, for B, C and D.
    pub attachment: Option<String>,
    /// What does not go: a part absent, content empty with no address, an
    /// address that is not followed, two sources that differ.
    pub problems: Vec<String>,
}

/// The pieces of one object.
#[derive(Clone, Debug, PartialEq)]
pub struct Listing {
    /// The object asked about, lowercased.
    pub uuid: String,
    /// Its pieces: its own first, the inherited ones after.
    pub pieces: Vec<Piece>,
    /// What the walk skipped or tied.
    pub notes: Vec<String>,
}

impl Piece {
    /// The JSON every face writes the same way — camelCase, `null` for what is
    /// not known, and the provenance with its sentence.
    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::json;
        json!({
            "key": self.key,
            "form": self.form.as_str(),
            "letter": self.form.letter().to_string(),
            "provenance": {
                "kind": self.provenance.kind,
                "carrier": self.provenance.carrier,
                "carrierTitle": self.provenance.carrier_title,
                "path": self.provenance.path,
                "says": self.provenance.says(),
            },
            "fileName": self.file_name,
            "mediaType": self.media_type,
            "size": self.size,
            "md": self.md.as_ref().map(|md| json!({ "value": md.value, "uom": md.uom })),
            "category": self.category,
            "location": match &self.location {
                Location::Part(part) => json!({ "kind": "part", "part": part }),
                Location::Inline => json!({ "kind": "inline" }),
                Location::Address(url) => json!({ "kind": "address", "url": url }),
                Location::Nowhere => json!({ "kind": "nowhere" }),
            },
            "attachment": self.attachment,
            "problems": self.problems,
        })
    }
}

impl Listing {
    /// `{ uuid, pieces: [...], notes: [...] }`.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "uuid": self.uuid,
            "pieces": self.pieces.iter().map(Piece::to_json).collect::<Vec<_>>(),
            "notes": self.notes,
        })
    }
}

/// What [`attachment_bytes`] returns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fetched {
    /// The file.
    Bytes {
        /// Its bytes.
        bytes: Vec<u8>,
        /// Its media type, when stated.
        media_type: Option<String>,
        /// Its name, when stated.
        file_name: Option<String>,
    },
    /// An address the module does not follow: the caller decides, and says so.
    External {
        /// The address.
        url: String,
        /// The media type the `Attachment` declares, if any.
        media_type: Option<String>,
    },
}

/// The pieces attached to each of `uuids`, in order — a batch, so that what
/// belongs to the package is read once.
pub fn attachments_of<S: AsRef<str>>(store: &dyn FileSource, index: &PackageIndex, uuids: &[S]) -> Vec<Listing> {
    attachments_of_with(&Files::read(store), store, index, uuids)
}

/// [`attachments_of`] over [`Files`] the caller already holds: the call then
/// reads **nothing of the package** — the index's tables for who names whom, the
/// files' for the media, and the `Attachment` objects the listing is about.
pub fn attachments_of_with<S: AsRef<str>>(
    files: &Files,
    store: &dyn FileSource,
    index: &PackageIndex,
    uuids: &[S],
) -> Vec<Listing> {
    let reader = Reader::new(store, files, index);
    let has = |uuid: &str| reader.carries(uuid).then_some(());
    let walker = Walker::new(index, store.objects(), &has);
    uuids.iter().map(|asked| list_one(&reader, &walker, index, asked.as_ref()).0).collect()
}

/// One object's listing, and **every object it was read from** (lowercase
/// uuids): the object itself, the `Attachment` objects whose content it shows,
/// and — for an inherited piece — everything the walk looked at and the
/// carrier. A change to one of them can change the listing; a change to any
/// other cannot ([`Cache`] is dropped by it).
fn list_one(
    reader: &Reader<'_>,
    walker: &Walker<'_, ()>,
    index: &PackageIndex,
    asked: &str,
) -> (Listing, HashSet<String>) {
    let uuid = asked.to_ascii_lowercase();
    let own = reader.pieces_of(&uuid, &self_provenance(index, &uuid));
    if !own.is_empty() {
        let mut read: HashSet<String> = index.attachments_naming(&uuid).into_iter().collect();
        read.insert(uuid.clone());
        return (Listing { uuid, pieces: own, notes: Vec::new() }, read);
    }
    let (walked, mut read) = walker.resolve_traced(&uuid);
    read.insert(uuid.clone());
    let mut pieces = Vec::new();
    if let Some(found) = &walked.resolution {
        let provenance = Provenance {
            kind: kind_of(&found.path),
            carrier: found.carrier.clone(),
            carrier_title: title_of(index, &found.carrier),
            path: found
                .path
                .iter()
                .map(|step| match step.via {
                    Via::Parent => format!("parent ({})", step.field),
                    Via::Collection => "collection".to_string(),
                    Via::Model => "model".to_string(),
                })
                .collect(),
        };
        let carrier = found.carrier.to_ascii_lowercase();
        read.extend(index.attachments_naming(&carrier));
        read.insert(carrier.clone());
        pieces = reader.pieces_of(&carrier, &provenance);
    }
    (Listing { uuid, pieces, notes: walked.notes }, read)
}

// ---------------------------------------------------------------------------
// The cache
// ---------------------------------------------------------------------------

/// What a [`Cache`] did: work, not time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheStats {
    /// Listings built: an object asked that the cache did not hold.
    pub listings_built: u64,
    /// Listings thrown away by [`Cache::disturb`] or by a change of the files.
    pub listings_dropped: u64,
    /// Times the files (media relations, attachment parts) were listed.
    pub files_read: u64,
}

/// The listings of a package, **kept between questions** — the attachments' own
/// `ColourCache` (ADR-0009).
///
/// **What is kept** is a [`Listing`] per object asked, with every object it was
/// read from. **What drops it**: a change to one of those objects, and
/// nothing else — [`PackageIndex::update_with`] says which objects a change
/// disturbed (the ones that changed, and the ones that gained or lost a
/// reference from them: an `Attachment` that comes to name an object disturbs
/// **that object**), and [`Cache::disturb`] drops the listings that read one.
///
/// The files are the exception: a media relation or a part is not an object, so
/// no journal of objects names it. They carry a stamp
/// ([`EpcStore::files_revision`]) that the caller hands in; when it moves, the
/// files are listed again and **every** listing goes. Coarse, and rare — an
/// attachment added or a relation made is a person's act.
///
/// **One walk or two?** The walk of an object's collections and parents does not
/// depend on what is looked for, so the colour cache and this one could share
/// it. They do not: once the links are tables the walk is a few hash lookups per
/// tier, and sharing would tie two invalidations (a colour set that changes, a
/// part that is added) to one structure. They read the same tables, which is
/// where the cost was.
#[derive(Default)]
pub struct Cache {
    files: Option<(u64, Files)>,
    held: HashMap<String, (Listing, HashSet<String>)>,
    /// Lowercase uuid → the listings (by object) that were read from it.
    touching: HashMap<String, HashSet<String>>,
    stats: CacheStats,
}

impl Cache {
    /// An empty cache.
    pub fn new() -> Cache {
        Cache::default()
    }

    /// What was done so far.
    pub fn stats(&self) -> CacheStats {
        self.stats
    }

    /// How many listings are held.
    pub fn len(&self) -> usize {
        self.held.len()
    }

    /// Whether no listing is held.
    pub fn is_empty(&self) -> bool {
        self.held.is_empty()
    }

    /// Whether the listing of this object is held.
    pub fn holds(&self, uuid: &str) -> bool {
        self.held.contains_key(&uuid.to_ascii_lowercase())
    }

    /// Forget everything: the package was replaced.
    pub fn clear(&mut self) {
        self.stats.listings_dropped += self.held.len() as u64;
        self.held.clear();
        self.touching.clear();
    }

    /// Drop the listings read from one of these objects — the uuids a change
    /// disturbed — and nothing else. Returns how many.
    pub fn disturb(&mut self, uuids: &[String]) -> usize {
        let mut doomed: HashSet<String> = HashSet::new();
        for uuid in uuids {
            if let Some(starts) = self.touching.get(&uuid.to_ascii_lowercase()) {
                doomed.extend(starts.iter().cloned());
            }
        }
        for start in &doomed {
            let Some((_, read)) = self.held.remove(start) else { continue };
            self.stats.listings_dropped += 1;
            for uuid in read {
                if let Some(starts) = self.touching.get_mut(&uuid) {
                    starts.remove(start);
                    if starts.is_empty() {
                        self.touching.remove(&uuid);
                    }
                }
            }
        }
        doomed.len()
    }

    /// The listing of each of `uuids`, in order — from what is held, and a
    /// walk only for the objects that are not. `files_stamp` is
    /// [`EpcStore::files_revision`] (or whatever counts the changes of the
    /// files of this source): when it differs from the one the held listings
    /// were made under, they go and the files are read again.
    pub fn listings<S: AsRef<str>>(
        &mut self,
        store: &dyn FileSource,
        files_stamp: u64,
        index: &PackageIndex,
        uuids: &[S],
    ) -> Vec<Listing> {
        if self.files.as_ref().is_none_or(|(stamp, _)| *stamp != files_stamp) {
            self.clear();
            self.files = Some((files_stamp, Files::read(store)));
            self.stats.files_read += 1;
        }
        let Cache { files, held, touching, stats } = self;
        let (_, files) = files.as_ref().expect("the files were just read");
        let missing: Vec<String> = {
            let mut seen: HashSet<String> = HashSet::new();
            uuids
                .iter()
                .map(|uuid| uuid.as_ref().to_ascii_lowercase())
                .filter(|uuid| !held.contains_key(uuid) && seen.insert(uuid.clone()))
                .collect()
        };
        if !missing.is_empty() {
            let reader = Reader::new(store, files, index);
            let has = |uuid: &str| reader.carries(uuid).then_some(());
            let walker = Walker::new(index, store.objects(), &has);
            for uuid in missing {
                let (listing, read) = list_one(&reader, &walker, index, &uuid);
                stats.listings_built += 1;
                for object in &read {
                    touching.entry(object.clone()).or_default().insert(uuid.clone());
                }
                held.insert(uuid, (listing, read));
            }
        }
        uuids
            .iter()
            .map(|uuid| held[&uuid.as_ref().to_ascii_lowercase()].0.clone())
            .collect()
    }
}

/// The bytes of one piece, by its [`Piece::key`] — read now, not before.
pub fn attachment_bytes(store: &dyn FileSource, key: &str) -> Result<Fetched, String> {
    if let Some(part) = key.strip_prefix("part:") {
        let bytes = store.read_part(part).map_err(|why| format!("`{part}`: {why}"))?;
        let media_type = store.files().into_iter().find(|(name, _, _)| name == part).and_then(|(_, _, kind)| kind);
        let file_name = part.rsplit('/').next().map(str::to_string);
        return Ok(Fetched::Bytes { bytes, media_type, file_name });
    }
    let Some(uuid) = key.strip_prefix("object:") else {
        return Err(format!("`{key}` is not a piece key: expected `part:<name>` or `object:<uri>`"));
    };
    // The uri names the version; a bare uuid (an older key) reads as before.
    let object = store.object_at(uuid).ok_or_else(|| format!("the package holds no attachment `{uuid}`"))?;
    let raw = RawAttachment::of(object.as_reflect());
    if !raw.content.trim().is_empty() {
        let bytes = decode_base64(&raw.content).map_err(|why| format!("`Content` of `{uuid}` is not base 64: {why}"))?;
        return Ok(Fetched::Bytes { bytes, media_type: raw.file_type, file_name: raw.file_name });
    }
    match raw.content_uri.as_deref().map(classify) {
        Some(Address::External(url)) => Ok(Fetched::External { url, media_type: raw.file_type }),
        Some(Address::Part(part)) => {
            let bytes = store
                .read_part(&part)
                .map_err(|_| format!("`ContentUri` of `{uuid}` names the part `{part}`, and the package holds no such part"))?;
            Ok(Fetched::Bytes { bytes, media_type: raw.file_type, file_name: raw.file_name })
        }
        Some(Address::Unsupported(scheme)) => Err(format!(
            "`ContentUri` of `{uuid}` has the scheme `{scheme}`: only http and https addresses are read, and by a front end, on request"
        )),
        None => Err(format!("`{uuid}` has no content and no `ContentUri`: an attachment with nothing in it")),
    }
}

// ---------------------------------------------------------------------------
// The reading
// ---------------------------------------------------------------------------

fn title_of(index: &PackageIndex, uuid: &str) -> Option<String> {
    index.node(uuid).and_then(|node| node.title.as_ref().map(|title| title.to_string()))
}

fn self_provenance(index: &PackageIndex, uuid: &str) -> Provenance {
    Provenance { kind: "self", carrier: uuid.to_string(), carrier_title: title_of(index, uuid), path: Vec::new() }
}

/// `collection` when any step is a membership — the carrier is then the member
/// of a group, the more telling of the two words — else `parent` or `model`.
fn kind_of(path: &[crate::inherit::Step]) -> &'static str {
    if path.iter().any(|step| step.via == Via::Collection) {
        "collection"
    } else if path.iter().any(|step| step.via == Via::Model) {
        "model"
    } else if path.is_empty() {
        "self"
    } else {
        "parent"
    }
}

/// What the package's **files** say, read once: the media relations by object
/// (`destinationMedia` / `sourceMedia`) and the size and content type of each
/// attachment part.
///
/// Not in the package index, and on purpose: the index follows the **objects**,
/// and a media relation or a part is not an object — the store's journal of
/// object writes never names one. It has its own stamp instead
/// ([`EpcStore::files_revision`]), which [`Cache`] compares.
///
/// This is the half of the old `Reader::new` that is a property of the package
/// and not of the question: it listed every part and every media relation again
/// for every call (`parts()` over 16 250 parts on `forages-unites-surfaces`).
pub struct Files {
    /// Object uuid (lowercase) → its media parts.
    parts_of: HashMap<String, Vec<String>>,
    /// Part → (size, content type), for the attachments of the package.
    parts: HashMap<String, (Option<u64>, Option<String>)>,
}

impl Files {
    /// Read the files of a source: one listing of the parts, one of the media
    /// relations.
    pub fn read(store: &dyn FileSource) -> Files {
        let mut parts_of: HashMap<String, Vec<String>> = HashMap::new();
        for (uuid, part) in store.media_links() {
            parts_of.entry(uuid).or_default().push(part);
        }
        let parts = store.files().into_iter().map(|(part, size, kind)| (part, (size, kind))).collect();
        Files { parts_of, parts }
    }
}

struct Reader<'a> {
    store: &'a dyn FileSource,
    files: &'a Files,
    /// Which `Attachment` objects name an object: **the index's table**, laid
    /// while it read them (`PackageIndex::attachments_naming`) — it used to be a
    /// scan of every node of the package, for every call.
    index: &'a PackageIndex,
}

impl<'a> Reader<'a> {
    fn new(store: &'a dyn FileSource, files: &'a Files, index: &'a PackageIndex) -> Self {
        Reader { store, files, index }
    }

    fn carries(&self, uuid: &str) -> bool {
        self.files.parts_of.contains_key(&uuid.to_ascii_lowercase()) || self.index.has_attachment_naming(uuid)
    }

    fn pieces_of(&self, carrier: &str, provenance: &Provenance) -> Vec<Piece> {
        let mut out = Vec::new();
        let mut parts = self.files.parts_of.get(carrier).cloned().unwrap_or_default();
        parts.sort();
        for part in parts {
            let (size, media_type) = self.files.parts.get(&part).cloned().unwrap_or((None, None));
            out.push(Piece {
                key: format!("part:{part}"),
                form: Form::Part,
                provenance: provenance.clone(),
                file_name: part.rsplit('/').next().map(str::to_string),
                media_type,
                size,
                md: None,
                category: None,
                location: Location::Part(part),
                attachment: None,
                problems: Vec::new(),
            });
        }
        for uri in self.index.attachment_uris_naming(carrier) {
            if let Some(piece) = self.attachment(&uri, provenance) {
                out.push(piece);
            }
        }
        out
    }

    /// The piece an `Attachment` object makes, read by its **uri** — the
    /// version the index laid is the version shown.
    fn attachment(&self, uri: &str, provenance: &Provenance) -> Option<Piece> {
        let object = self.store.object_at(uri)?;
        let raw = RawAttachment::of(object.as_reflect());
        let mut problems = Vec::new();
        let address = raw.content_uri.as_deref().map(classify);
        let has_content = !raw.content.trim().is_empty();

        let inline = if has_content {
            match decode_base64(&raw.content) {
                Ok(bytes) => Some(bytes),
                Err(why) => {
                    problems.push(format!("`Content` is not base 64: {why}"));
                    None
                }
            }
        } else {
            None
        };

        let (form, location, size) = match (&address, has_content) {
            // B — and B with an address beside it: the content is the authority.
            (_, true) => {
                if let Some(Address::Part(part)) = &address {
                    match (self.files.parts.get(part), &inline) {
                        (None, _) => problems.push(format!(
                            "`ContentUri` names the part `{part}` and the package holds none; the content is used"
                        )),
                        (Some(_), Some(bytes)) => {
                            if let Ok(held) = self.store.read_part(part)
                                && &held != bytes
                            {
                                problems.push(format!(
                                    "the content and the part `{part}` that `ContentUri` names are two different files; the content is used"
                                ));
                            }
                        }
                        _ => {}
                    }
                }
                (Form::Inline, Location::Inline, inline.as_ref().map(|bytes| bytes.len() as u64))
            }
            (Some(Address::Part(part)), false) => {
                let held = self.files.parts.get(part);
                if held.is_none() {
                    problems.push(format!("`ContentUri` names the part `{part}` and the package holds no such part"));
                }
                (Form::PartAddress, Location::Part(part.clone()), held.and_then(|(size, _)| *size))
            }
            (Some(Address::External(url)), false) => (Form::External, Location::Address(url.clone()), None),
            (Some(Address::Unsupported(scheme)), false) => {
                problems.push(format!(
                    "`ContentUri` has the scheme `{scheme}`: only http and https addresses are read, and on request"
                ));
                (Form::External, Location::Address(raw.content_uri.clone().unwrap_or_default()), None)
            }
            (None, false) => {
                problems.push("attachment with no content and no `ContentUri`: an empty file".to_string());
                (Form::Inline, Location::Nowhere, Some(0))
            }
        };
        let media_type = raw.file_type.clone().or_else(|| match &location {
            Location::Part(part) => self.files.parts.get(part).and_then(|(_, kind)| kind.clone()),
            _ => None,
        });
        Some(Piece {
            key: format!("object:{uri}"),
            form,
            provenance: provenance.clone(),
            file_name: raw.file_name.or_else(|| match &location {
                Location::Part(part) => part.rsplit('/').next().map(str::to_string),
                _ => None,
            }),
            media_type,
            size,
            md: raw.md,
            category: raw.category,
            location,
            attachment: energyml_core::identity::uuid_of_reflect(object.as_reflect()).map(str::to_string),
            problems,
        })
    }
}

/// An `eml23.Attachment` read by name — no generated type is needed.
struct RawAttachment {
    content: String,
    content_uri: Option<String>,
    file_name: Option<String>,
    file_type: Option<String>,
    category: Option<String>,
    md: Option<Depth>,
}

impl RawAttachment {
    fn of(object: &dyn Reflect) -> Self {
        let text = |name: &str| match object.field(name) {
            Some(FieldValue::Scalar(scalar)) => Some(scalar.to_string()),
            _ => None,
        };
        let md = match object.field("Md") {
            Some(FieldValue::Object(md)) => match md.field("MeasuredDepth") {
                Some(FieldValue::Object(measure)) => {
                    let value = ["Value", "value"].iter().find_map(|name| match measure.field(name) {
                        Some(FieldValue::Scalar(scalar)) => scalar.as_f64(),
                        _ => None,
                    });
                    let uom = ["uom", "Uom"].iter().find_map(|name| match measure.field(name) {
                        Some(FieldValue::Scalar(scalar)) => Some(scalar.to_string()),
                        _ => None,
                    });
                    value.zip(uom).map(|(value, uom)| Depth { value, uom })
                }
                _ => None,
            },
            _ => None,
        };
        RawAttachment {
            content: text("Content").unwrap_or_default(),
            content_uri: text("ContentUri").filter(|uri| !uri.trim().is_empty()),
            file_name: text("FileName").filter(|name| !name.is_empty()),
            file_type: text("FileType").filter(|kind| !kind.is_empty()),
            category: text("Category").filter(|category| !category.is_empty()),
            md,
        }
    }
}

/// What a `ContentUri` is.
enum Address {
    /// A part name — this tool's reading of a relative address.
    Part(String),
    /// `http` or `https`.
    External(String),
    /// Any other scheme.
    Unsupported(String),
}

fn classify(uri: &str) -> Address {
    let uri = uri.trim();
    let lower = uri.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Address::External(uri.to_string());
    }
    // A scheme is two letters at least, so `C:` is not one.
    if let Some(colon) = uri.find(':') {
        let scheme = &uri[..colon];
        if scheme.len() >= 2
            && scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
            && scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
        {
            return Address::Unsupported(scheme.to_string());
        }
    }
    match media::resolve_target("", uri) {
        Some(part) => Address::Part(part),
        None => Address::Part(uri.trim_start_matches('/').to_string()),
    }
}

// ---------------------------------------------------------------------------
// Base 64 — RFC 4648 §4
// ---------------------------------------------------------------------------

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base 64 with padding, no line breaks (RFC 4648 §4).
pub fn encode_base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { ALPHABET[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { ALPHABET[n as usize & 63] as char } else { '=' });
    }
    out
}

/// Decode base 64; white space is ignored, as `xsd:base64Binary` allows, and
/// anything else outside the alphabet — or a bad padding — is refused.
pub fn decode_base64(text: &str) -> Result<Vec<u8>, String> {
    let mut sextets: Vec<u8> = Vec::with_capacity(text.len());
    let mut padding = 0usize;
    for c in text.chars().filter(|c| !c.is_ascii_whitespace()) {
        if c == '=' {
            padding += 1;
            continue;
        }
        if padding > 0 {
            return Err("a character follows the `=` padding".to_string());
        }
        let value = match c {
            'A'..='Z' => c as u8 - b'A',
            'a'..='z' => c as u8 - b'a' + 26,
            '0'..='9' => c as u8 - b'0' + 52,
            '+' => 62,
            '/' => 63,
            other => return Err(format!("`{other}` is not in the base 64 alphabet")),
        };
        sextets.push(value);
    }
    if padding > 2 || (sextets.len() + padding) % 4 != 0 || sextets.len() % 4 == 1 {
        return Err("the length is not that of padded base 64".to_string());
    }
    let mut out = Vec::with_capacity(sextets.len() * 3 / 4);
    for group in sextets.chunks(4) {
        let n = group.iter().enumerate().fold(0u32, |n, (at, v)| n | (u32::from(*v) << (18 - 6 * at)));
        out.push((n >> 16) as u8);
        if group.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if group.len() > 3 {
            out.push(n as u8);
        }
    }
    Ok(out)
}
