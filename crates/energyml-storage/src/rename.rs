//! Renaming the external data file a package points at.
//!
//! Asked for as: rename the EPC and the h5 from the interface. For the h5, check
//! that the rels will be updated, and the objects that reference the file name
//! too.
//!
//! # Why this is an operation and not a string replace
//!
//! The name of an external file is written in **three** places that have to go
//! on agreeing, and they are not the same kind of thing:
//!
//! 1. in every `ExternalDataArrayPart.Uri` (eml 2.3) — one field per array, and
//!    an object may carry several;
//! 2. in the `.rels` of the part, for the older arrangement where the object
//!    names an `EpcExternalPartReference` and the relationship carries the file
//!    name (EPC §3.2.3) — and for the entries real producers write on ordinary
//!    objects, which this crate honours;
//! 3. on the disk.
//!
//! Forgetting one gives a package that opens and whose arrays cannot be found —
//! exactly the fault `energyml check` reports, made by the tool that is supposed
//! to prevent it.
//!
//! What is **not** here: the `uuid` attribute EPC §3.2.3 makes the HDF5 file
//! carry. It is the uuid of the proxy, not a name, so a rename leaves it alone;
//! [`EpcStore::verify_external_file`] goes on agreeing with it afterwards.
//!
//! # A quote first, then the writing
//!
//! The shape the workspace uses everywhere it changes more than one thing —
//! `extract::plan` then `extract`, `Operation::plan` then `apply`. It is also
//! the answer to the question the request itself asks — update automatically,
//! or ask? — because a caller that can show **what will
//! change** does not have to choose between the two: it shows the quote and
//! someone says yes.
//!
//! There is deliberately no third mode that renames the file without touching
//! the references. It produces a broken package, and this workspace refuses
//! rather than draws.

use std::path::PathBuf;

use energyml_core::path;
use energyml_core::value::OwnedValue;

use crate::error::{Error, Result};

use crate::EpcStore;
use crate::array::locate;

/// What renaming a data file would touch, and what it refuses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rename {
    /// The file as the package names it today.
    pub from: String,
    /// The bare file name it would take.
    pub to: String,
    /// The uris of the objects that name the file in a field of their own.
    pub objects: Vec<String>,
    /// How many fields would be rewritten — an object may carry several.
    pub fields: usize,
    /// How many `.rels` entries would be rewritten.
    pub relationships: usize,
    /// Why this rename cannot be carried out, in words a person can act on.
    ///
    /// **Not an error and not a silence.** A caller draws these next to the
    /// button; [`apply`] refuses to do anything while there is one.
    pub refusals: Vec<String>,
}

impl Rename {
    /// Whether the rename can be carried out at all.
    pub fn is_allowed(&self) -> bool {
        self.refusals.is_empty()
    }

    /// Everything that would be rewritten inside the package.
    pub fn touches(&self) -> usize {
        self.fields + self.relationships
    }
}

/// What [`apply`] did to the file itself.
///
/// Three states and not two: *there was nothing to move* and *this store has no
/// disk* are different answers, and a caller that conflated them would tell
/// somebody their file is missing when it is sitting in a browser tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileMove {
    /// The store holds its files somewhere this crate cannot reach — a browser,
    /// a test with an in-memory backend. Whoever owns the bytes renames them.
    NotOnDisk,
    /// Nothing was at the old name. Not an error: a package whose `.h5` never
    /// shipped is already in that state, and the rename does not make it worse.
    Absent,
    /// Moved, and where to.
    Moved(PathBuf),
}

/// What [`apply`] changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    /// Fields rewritten inside the objects.
    pub fields: usize,
    /// `.rels` entries rewritten.
    pub relationships: usize,
    /// What happened to the file itself.
    pub file: FileMove,
}

/// What renaming `from` to `to` would touch.
///
/// Reads every object once, as `energyml check` and the reference graph do.
/// An object that cannot be read is a **refusal** rather than a skip: nothing
/// here can tell whether it names the file, and renaming around it would leave
/// a pointer at a file that no longer exists.
pub fn plan(store: &EpcStore, from: &str, to: &str) -> Rename {
    let mut out = Rename {
        from: from.to_string(),
        to: to.to_string(),
        ..Default::default()
    };

    // The new name, judged on its own first: these do not depend on the package
    // and saying them early keeps the reason next to the thing that is wrong.
    if to.trim().is_empty() {
        out.refusals.push("the new name is empty".to_string());
    }
    if to.contains('/') || to.contains('\\') {
        out.refusals.push(format!(
            "`{to}` is a path; EPC §3.2.3 names an external file by a bare name"
        ));
    }
    if from == to {
        out.refusals
            .push(format!("`{to}` is the name the file already has"));
    }
    if from.trim().is_empty() {
        out.refusals.push("no file was named to rename".to_string());
    }

    let (objects, unreadable) = store.read_objects(None);
    for name in unreadable {
        out.refusals
            .push(format!("{name} — so this cannot tell whether it names `{from}`"));
    }

    for (summary, object) in &objects {
        let mut fields = 0;
        for location in locate(object.as_reflect()) {
            // The older forms carry no file name of their own: they point at a
            // proxy, and the name lives in that proxy's `.rels`. Counted below.
            if location.proxy_uuid.is_some() {
                continue;
            }
            if names_file(&location.file, from) {
                fields += 1;
            } else if names_file(&location.file, to) {
                // Renaming onto a name the package already uses would merge two
                // files into one pointer, silently, and there is no way back.
                out.refusals.push(format!(
                    "`{to}` is already named by {} ({}), so renaming onto it would \
                     make two files one",
                    summary.uri, location.field_path
                ));
            }
        }
        if fields > 0 {
            out.objects.push(summary.uri.clone());
            out.fields += fields;
        }
    }

    for (_, file) in store.external_file_map() {
        if names_file(&file, from) {
            out.relationships += 1;
        } else if names_file(&file, to) {
            out.refusals.push(format!(
                "`{to}` is already the file of an external part reference"
            ));
        }
    }
    for (_, file) in store.object_external_file_map() {
        if names_file(&file, from) {
            out.relationships += 1;
        }
    }

    if out.touches() == 0 && out.is_allowed() {
        // Renaming nothing is not a rename, and a caller whose button did
        // nothing would have no way to learn why.
        out.refusals
            .push(format!("nothing in this package names `{from}`"));
    }
    out
}

/// Carry out a rename, or refuse the whole of it.
///
/// **The package first, the disk afterwards**, and the order is the answer to
/// what a half-done rename leaves behind. Everything here changes memory, which
/// reaches the file only on a save, so a failure on the way leaves an unsaved
/// package and a file where it was. Moving the file first and failing afterwards
/// would leave a file nothing points at.
pub fn apply(store: &mut EpcStore, plan: &Rename) -> Result<Applied> {
    if !plan.is_allowed() {
        return Err(Error::Package(format!(
            "this rename was refused: {}",
            plan.refusals.join("; ")
        )));
    }

    // 1 — the fields the objects carry themselves.
    let mut fields = 0;
    for uri in &plan.objects {
        let object = store.edit(uri)?;
        // Read the paths out first: the walk borrows the object, and writing
        // through it needs the borrow back.
        let targets: Vec<(String, String)> = locate(&*object)
            .iter()
            .filter(|found| found.proxy_uuid.is_none() && names_file(&found.file, &plan.from))
            .map(|found| (found.field_path.clone(), renamed(&found.file, &plan.to)))
            .collect();
        for (at, value) in targets {
            let part = path::object_mut(&mut *object, &at)?;
            // `uri` for an eml 2.3 part, `file_uri` for a WITSML channel
            // data in HDF5 — the extension `crate::array` locates.
            let field = crate::array::file_field(part.type_info().name);
            part.set_field(field, OwnedValue::Str(value))?;
            fields += 1;
        }
    }

    // 2 — the names the relationships carry. Both populations, and they are
    // disjoint: a proxy names its file in its own `.rels` (§3.2.3), an ordinary
    // object may carry one too, which this crate honours rather than drops.
    let mut relationships = 0;
    let due: Vec<(String, String)> = store
        .external_file_map()
        .into_iter()
        .chain(store.object_external_file_map())
        .filter(|(_, file)| names_file(file, &plan.from))
        .map(|(uuid, file)| (uuid, renamed(&file, &plan.to)))
        .collect();
    for (uuid, file) in due {
        store.register_external_file(&uuid, file);
        relationships += 1;
    }

    let file = move_file(store, plan)?;
    Ok(Applied {
        fields,
        relationships,
        file,
    })
}

/// Move the file itself, when this store has one to move.
fn move_file(store: &mut EpcStore, plan: &Rename) -> Result<FileMove> {
    if store.external_files_ref().is_virtual() {
        // A store whose files live somewhere this crate cannot reach — a
        // browser holding them in memory. The bytes are the caller's to rename;
        // what belongs here is the store's own bookkeeping, so that a read
        // under the **new** name finds the backend instead of being refused as
        // *file not found* the moment the rename succeeds. The old name is left
        // declared: it is a list of names known to exist, nothing points at it
        // any more, and dropping it would be this deciding what the caller's
        // file map holds.
        store.external_files_mut().declare_present(plan.to.clone());
        return Ok(FileMove::NotOnDisk);
    }
    let Ok(source) = store.external_files_ref().resolve(&plan.from) else {
        return Ok(FileMove::Absent);
    };
    if !source.exists() {
        return Ok(FileMove::Absent);
    }
    let destination = store.external_files_ref().destination_for(&plan.to)?;
    // **What this process holds open, it closes first.** An `.h5` is kept open
    // between reads (`hdf5::close_cached_files`), and on Windows an open handle
    // makes a rename fail outright — so a package that had been read once could
    // not be renamed, which is every package a person would want to rename.
    #[cfg(feature = "hdf5")]
    crate::hdf5::close_cached_files();
    std::fs::rename(&source, &destination).map_err(|why| {
        Error::Package(format!(
            "the package now names `{}`, but the file could not be moved from {} to {}: {why} \
             — the package has not been saved, so nothing on disk is inconsistent yet",
            plan.to,
            source.display(),
            destination.display()
        ))
    })?;
    Ok(FileMove::Moved(destination))
}

/// Whether a declared location names this file.
///
/// Exact, or the **last segment** of a path or URI: eml 2.3 says `Uri`, and
/// producers write anything from `model.h5` to `./data/model.h5`. Comparing the
/// whole string would miss the second; comparing loosely — ignoring case, say —
/// would rename a file the package did not mean, and a zip entry is
/// case-sensitive.
fn names_file(declared: &str, wanted: &str) -> bool {
    !declared.is_empty() && (declared == wanted || last_segment(declared) == wanted)
}

/// The same location with its file name replaced, **keeping whatever led to
/// it**: `./data/model.h5` renamed to `new.h5` is `./data/new.h5`, not a bare
/// name in a package that was pointing into a subdirectory.
fn renamed(declared: &str, to: &str) -> String {
    match declared.rfind(['/', '\\']) {
        Some(at) => format!("{}{to}", &declared[..=at]),
        None => to.to_string(),
    }
}

fn last_segment(declared: &str) -> &str {
    match declared.rfind(['/', '\\']) {
        Some(at) => &declared[at + 1..],
        None => declared,
    }
}
