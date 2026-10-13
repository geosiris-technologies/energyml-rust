# energyml-storage: architecture

## Purpose and position

Reads and writes EnergyML data stores behind one abstraction. `EnergymlStorage` is the mother trait, drawn from the ETP v1.2 requests (addressing by URI, plural operations, partial responses) rather than from a zip file. `EpcStore` implements it for EPC packages and also has a complete synchronous API. Arrays in external files are located, read and written through pluggable backends (HDF5 behind the `hdf5` feature).

It sits above `energyml-core` and below `energyml-domain`, `energyml-geometry` and every face. The ETP implementation of the same trait lives in the sibling repository `retp`.

Related decisions: [ADR-0002](../../docs/adr/0002-storage-trait-follows-etp.md), [ADR-0003](../../docs/adr/0003-identity-full-uri-one-door.md), [ADR-0004](../../docs/adr/0004-reading-policy.md), [ADR-0005](../../docs/adr/0005-refuse-rather-than-draw.md), [ADR-0006](../../docs/adr/0006-registries-rather-than-match.md), [ADR-0010](../../docs/adr/0010-package-index-and-caches.md), [ADR-0011](../../docs/adr/0011-tests-follow-the-specification.md), [ADR-0013](../../docs/adr/0013-witsml-channels-in-hdf5.md).

## Shape of the layer

**Designed from ETP, not from a zip** - an EPC fits inside the ETP shape (a file is a dataspace); the reverse would have forced a rewrite the day a network transport arrived.

**The mother trait is asynchronous; `EpcStore` also has a synchronous API** - ETP is a WebSocket and wasm cannot block, while an application reading local files must not pay for `async`. The asynchronous implementation is one line per method and a test compares the two faces.

**The trait only does I/O** - graph building, `.rels` regeneration and array location are free functions over objects already in memory, so nothing has a synchronous and an asynchronous copy. Anything that is a check belongs in the library, not in a binary.

**Three levels of materialisation** - identity from the package index, citation from a parser that stops, then the whole object. The second level reads a zip entry as a stream, so a package of two gigabytes lists without inflating a part in full.

**What is local is a separate trait** - `LocalPackage` carries attachments and raw `.rels`; putting a PDF in a dataspace makes no sense, and an application using it knows at compile time that it is local. `check_relationships` stays there too: a `.rels` is an OPC artefact, and a dataspace has nothing to confront.

**Every access function is reader-independent** - no public function of the domain, geometry or export crates takes an `EpcStore`; they take `&dyn ObjectSource` / `&dyn ArraySource`, and exporters are handed a scene object. A store with no file behind it proves it. Two capabilities were added to the trait because a network client needs them: `arrays_of` (locating arrays, synchronous: reflection plus the store's own index) and `edges` / `graph` as provided methods (the default derives edges by reading the objects; ETP answers natively from the server's index, and `graph` gets cheaper with no caller changed). The synchronous/asynchronous seam is crossed by prefetching (one asynchronous pass that follows references, then synchronous serving), not by blocking; arrays that were not asked for are refused by name.

**Tolerant and vocal** - real packages depart from the specification, so departures are accepted and recorded as `Diagnostic`s, never swallowed, never fatal when they need not be. `diagnostics()` returns by value because a lazy reader keeps finding them after open.

**Refuse by name, never answer empty** - without a backend, reading an array returns `ExternalData { reason: FeatureNotEnabled }`, never an empty array: a missing capability is not missing data. The backend is chosen before the file is looked for, because "no support for this format" is the more useful answer.

## Identity and lookup

**An object is (uuid, qualified type, `objectVersion`)** - a uuid used to hold one entry ("first part wins"); now all entries are kept and only a repeat of the full identity is dropped, with a notice. The emitted URI is the short form while a (uuid, type) has one entry or the entry declares no version, and carries `version='...'` for every versioned entry of a (uuid, type) held several times (Identifier Specification v5.0, section 3.4.2, p. 14).

**Resolution never guesses** - a versioned URI resolves to that entry or `NotFound`; a short URI to the single entry, or the single unversioned one, otherwise `Error::Ambiguous { candidates }`; a bare uuid likewise. The specification leaves "the most recent" to the server (section 3.4.2.1, p. 15) and ETP 1.2 does not say how to recognise it. A type differing only by release still resolves while there is one candidate.

**One door per lookup** - `EpcStore::lookup` is the door; `resolve` and the uuid lookups are adapters that turn their input into a full identity at once. The internal key (`slot_key`) is the uuid itself for the first entry of a uuid and a composite for its namesakes. `put` never makes a second version; `put_as_new_version` is the only door that adds one, and refuses an identity that exists.

**Versioned objects are written under `version_<id>/`** (EPC v1.1, section 3.6, p. 17), and a still-taken part name under a `namespace_` folder, never overwritten. Reading does not read the folder: the version comes from the attribute, so the same identities come back.

## The package

**Saving rebuilds, it never patches** - `[Content_Types].xml`, every `.rels` and `docProps/core.xml` are recomputed from the objects in memory; only attachment bytes are copied. Patching is how a package ends up describing a state it no longer has. A save therefore reads everything; laziness is for looking at a file, never for writing half of one.

**A minimal EPC has three parts** - `[Content_Types].xml`, `_rels/.rels` and `docProps/core.xml` (core properties are mandatory, EPC section 3.5; the package relationship is required by section 3.5.2). The writer that creates a package and the reader that judges one must agree on this, otherwise a freshly created in-memory package opens on an alarm nobody can clear.

**Relationships are written both ways** (EPC section 3.4.1), and an external-file proxy gets the proxy-specific relationship pair rather than the generic one. The `.rels` are an index and the objects' references are the truth, so they are confronted: a link the objects make and the index omits, a link the index makes and the objects do not, a relationship to no part, and a *reversed* link (reported once, not as two complaints). The checker accepts both forms of the proxy pair. Rendering is linear in the number of relationships.

**What is not modelled is copied, not dropped** - unreadable parts, attachments and everything under `docProps/` other than the regenerated `core.xml` travel through a rewrite byte for byte.

**A file is tied to an object by a relationship, never by its content type** - the content type says what a part is (EPC section 2.2.1, p. 9); which object it shows is `destinationMedia` in the object's `.rels` and `sourceMedia` in the part's (section 3.4.2, pp. 15-16). `set_attachment` refuses by name, with the section, an invalid part name, a `_rels` folder, a `namespace_` / `version_` folder, a name differing from another only by case and a content type that is not `type/subtype`; it also refuses bytes for parts that are regenerated, since they would be discarded exactly when meant to take effect. `parts()` lists what a save would write, with `added`, `dropped` and `staged` flags the archive cannot show.

**A transaction is a layer, not a temporary folder** - nothing reaches the disk, so a rollback genuinely undoes; a `save` during an open transaction is refused. `OnClose` needs `close()` rather than `Drop`, which cannot return a failed write.

## The inventory never loses anything

- A part that cannot be deserialized is copied verbatim with a warning that its relationships could not be regenerated. An object of an unregistered version is still listed; only deserializing it fails.
- **The document beats the index** - `[Content_Types].xml` is metadata about a part, while the root element is the part, so the root is read for every part and compared with the declared type; on disagreement the root wins and the discrepancy is recorded. Exception: a root this build cannot type is not evidence, so the part stays listed under the type it declares.
- **Nothing leaves the inventory in silence** - every path that does not list a part says why; a malformed `.rels` keeps what it read and names the byte it broke at; duplicate relationship ids are reported. Reading every `.rels` stays off the open path.
- **When there is nothing to fall back to, fall back to the bytes** - `raw_part` / `set_raw_part` read and stage a part's bytes, keyed by part name, written verbatim. `stage_repaired_part` parses first and, when the bytes read, puts the object in the ordinary layer so the writer rebuilds that part's relationships. `unreadable()` is the worklist, including parts that no listing could otherwise reach.
- **A failed read is reported, never skipped** - every walk over the package counts what it could not read, names the first few and points at the lenient switch. The type filter runs before the read.
- **A wrong version is not a wrong type** - a reference to `resqml20.X` where the package holds `resqml22.X` is a warning (an EPC may mix versions, section 3.2); naming a different concept is an error.
- **A reference is four copies, each checked** - type, title, uuid and revision. A reference naming a revision the object is not at is an error; naming none where the object has one is a warning (the specification has a reader assume the latest); a stale or empty title is a warning (readers resolve by uuid). `repair_references` rewrites the copies the target owns, and leaves the reference alone when the target states nothing.

## Development drops

A pre-release drop and its release are indistinguishable on the wire (same namespace, `schemaVersion`, content type, qualified type), so they are told apart like two releases: candidates are ordered, and the parser settles it by refusing the wrong one. The release is tried first, a document that names a drop (any spelling) gets it first, and a drop the build lacks falls back to the release, never to another drop. The document also beats the content type for the *version*, since the index cannot express the one distinction at stake. Correcting a declaration is a byte edit (`relabel.rs`): the root tag is located, one attribute rewritten and every other byte copied, because parsing a drop declared as the release would drop what only the drop has. It refuses a version containing quote, angle or ampersand characters, requires the caller to name the package (the eml parts of a RESQML package are another release), and refuses a package holding two releases of the named package until the source version is given.

## External arrays

**The backend is a trait and HDF5 is a feature** - without it the crate still reads, edits and writes EPC files, and compiles for wasm; a browser plugs a JavaScript HDF5 implementation into the same seam.

**The MIME type decides the format** - eml 2.3 carries it on the external array part; eml 2.0-2.2 carry it on the external part reference, whose schema calls `application/x-hdf5` a convention. A resolved proxy is a `ProxyTarget { file, mime }`, not just a name.

**The path inside an external file is opaque** - never parse, split, validate or infer a uuid from it. A leading `/` is the one exception, and only as the marker of the WITSML channel-data form (an extension beyond WITSML 2.1 that stores channel rows as a dataset); that form is recognised by syntax in one function, and `array::locate` finds it so every walk (cut, copy, import, rename, prefetch) carries it.

**External files are found by content, not by name** - candidates are tried in order (pinned, offered, declared, companion, neighbour) and each is asked whether it holds the dataset; a candidate that exists and does not hold it steps aside. Every departure from the declared name is a diagnostic.

**An `.h5` is opened once, and trusted only while unchanged** - a cached handle is valid while the file's modification time and length are what they were at open: serving data the file *used to* hold is a correctness bug. The cache is bounded (file-descriptor limits), evicted before any write, and `close_cached_files()` exists because an open handle makes a rename fail on Windows.

**One copy is the floor** - `read_array` fills a typed vector (one copy), accessors borrow (zero), `read_array_into` fills the caller's buffer (none). Going lower needs memory-mapping and `unsafe`, worth doing when a profile says so.

**The byte order is read from the file and asked for separately** - a default of "unknown", never the likely order, and not part of the array metadata, because ETP's metadata does not carry it. It matters to exactly one caller: one that must write down how the bytes are stored; writing the usual order over a file holding the other makes every value wrong while the document stays schema-valid.

**Writing** - `write_array_to` (numbers into the file) and `register_external_file` (the package names the file) are separate calls because they fail for unrelated reasons; `open_rw`, never `File::create`, because one package keeps all arrays in one file; many arrays are written in one opening (`write_many`, default-implemented as a loop so a backend that does not override it is correct, only slower); a written `.h5` holds no clock (modification-time tracking is off) so the same arrays make the same bytes. A writer is tested by reading its output back bit for bit: `==` on floats hides differences between identical bytes (`NaN != NaN`) and would pass a canonicalising writer.

## Operations

**Cutting and copying are asymmetric on purpose** - `extract` cuts to a set the caller chose and refuses to break what was whole; `copy_into` takes a set and its closure, since a copy cannot leave a dependency behind. `extract::plan` is pure and does not close the set (closing is `graph::closure`; two walks would be two answers); it checks, reports reference by reference, and returns three numbers: kept, left behind, and how many left-behind objects nothing kept references. Arrays that could not be copied are counted and listed; external files are declared before saving. Graph functions answer different questions and have different costs: `references` (one object), `dependents` (reads every object, since no package indexes that direction), `build` (whole graph), `closure` (walk).

**Importing asks, it does not overwrite** - `import.rs` is `copy_into` that asks. The imported part is closed by `graph::closure`; "different" is judged on read objects through reflection (never bytes; `NaN` equals `NaN`); there is no default, so an unanswered conflict blocks `apply_plan`; a taken dataset path is relocated rather than overwritten, using a probe that asks only the named file; `apply_plan` touches memory only.

**Renaming a data file touches three places and shows a quote first** - the `Uri` in each external array part, the proxy's `.rels` (and the one real producers put on the object), and the file on disk. Forgetting one gives a package that opens with unfindable arrays. There is no mode that renames without touching references. A name is compared whole or by last segment with its prefix kept; an unreadable object is a refusal; renaming to a name already used, or renaming what nothing names, is refused; the package changes first and the disk second; the file move has three outcomes (moved, nothing to move, this store has no disk); handles the process holds open are closed first. The declared name wins on read as well as on save.

**Every object write is counted at the overlay** - the overlay keeps a revision counter and a bounded journal of touched uuids, saying what kind of change it was (citation, body, outgoing links, created, removed, identity); files have a separate counter. A derived index asks `changes_since(rev)`. The journal may be too broad, never too narrow: a write that bypassed the overlay would be a stale index that looks right, so every new door goes through it.

## Deliberately not here

- No schema version is named: the registry is passed in (`energyml-schemas` builds it).
- No ETP transport (it lives in `retp`), no geodesy, no interpretation of what an object means.
- No `unsafe`, no memory-mapped arrays.

## How it is tested

One test binary (`tests/main.rs`; a new file is one `mod` line). Fixtures are built by hand (zip, part names, attributes); a store made of a `Vec` and two XML strings, plus a variant that answers `edges` natively and refuses to hand over any object, proves reader independence. Tests that read the written package back use the `zip` crate independently, and byte-level checks read HDF5 bytes through a second reader. Many checks were validated by sabotage (remove the behaviour, watch named tests fail). Tests against local real packages are gated behind a feature that is off by default; the public suite uses fabricated packages.
