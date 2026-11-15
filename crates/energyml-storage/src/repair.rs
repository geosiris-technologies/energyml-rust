//! Writing down where a package's arrays really are.
//!
//! The reading side is permissive on purpose: a renamed package still reads,
//! because the search opens the files beside it and asks which one holds the
//! dataset. That has a cost - every reader pays for the search, and every
//! reader that is not this one fails. This module is the answer: [`survey`]
//! says which file each declared name resolves to, and [`fix`] rewrites the
//! declarations so the package is ordinary again.
//!
//! It lives here, beside [`crate::rename`], so that every face (the command
//! line, the Python bridge, a server) calls the same repair and reads the same
//! verdict; a face prints, it does not repair.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use energyml_core::value::{FieldValue, OwnedValue, Reflect};

use crate::Resolution;
use crate::epc::{EpcStore, SaveOptions};
use crate::error::{Diagnostic, Result};

/// One declared file name, as the package's array pointers use it.
///
/// One row per *declared name*, not per array pointer: a package has thousands
/// of pointers and two or three files, and a person wants the files.
#[derive(Debug, Default)]
pub struct Row {
    /// How many array pointers name it.
    pub pointers: usize,
    /// Where the search found the file, when it did.
    pub found: Option<Resolution>,
    /// The `EpcExternalPartReference` proxies (lower case uuids) that use it.
    pub proxies: BTreeSet<String>,
    /// A dataset path inside the file, to say what was looked for.
    pub example: String,
}

/// Every declared file name of the package and where it resolves.
pub fn survey(store: &EpcStore) -> BTreeMap<String, Row> {
    let mut rows: BTreeMap<String, Row> = BTreeMap::new();
    for summary in store.resources() {
        let Ok(object) = store.get_object(&summary.uri) else {
            continue;
        };
        for location in store.arrays_of(object.as_reflect()) {
            let row = rows.entry(location.file.clone()).or_default();
            row.pointers += 1;
            if let Some(proxy) = &location.proxy_uuid {
                row.proxies.insert(proxy.to_ascii_lowercase());
            }
            if row.found.is_none() {
                row.example = location.path_in_file.clone();
                row.found = store.resolve_array(&location).ok();
            }
        }
    }
    rows
}

/// Ask again for the rows still unresolved, after the caller said where files are.
pub fn resolve_again(store: &EpcStore, rows: &mut BTreeMap<String, Row>) {
    for (declared, row) in rows.iter_mut() {
        if row.found.is_some() {
            continue;
        }
        for summary in store.resources() {
            let Ok(object) = store.get_object(&summary.uri) else {
                continue;
            };
            for location in store.arrays_of(object.as_reflect()) {
                if &location.file == declared && row.found.is_none() {
                    row.found = store.resolve_array(&location).ok();
                }
            }
        }
    }
}

/// How many array pointers name no file at all.
pub fn unresolved_pointers(store: &EpcStore) -> usize {
    store
        .resources()
        .iter()
        .filter_map(|summary| store.get_object(&summary.uri).ok())
        .flat_map(|object| store.arrays_of(object.as_reflect()))
        .filter(|location| location.file.is_empty())
        .count()
}

/// What [`fix`] did.
#[derive(Debug)]
pub struct Fixed {
    /// `(declared, now)` for each name rewritten.
    pub renamed: Vec<(String, String)>,
    /// How many declarations were rewritten (proxies and objects).
    pub declarations: usize,
    /// What the writer had to say.
    pub diagnostics: Vec<Diagnostic>,
    /// Array pointers naming no file before the repair.
    pub before: usize,
}

impl Fixed {
    /// How many pointers named a file before and name none in `written`, the
    /// package as written **read back by the caller**.
    ///
    /// Reopening and counting is the only verdict that does not take the
    /// writer's word for it. Non-zero means the package used an
    /// `externalResource` relationship the writer does not reproduce: the
    /// repaired package still reads, because the search finds the file, but
    /// another tool may not. The repair did not repair.
    pub fn lost(&self, written: &EpcStore) -> usize {
        unresolved_pointers(written).saturating_sub(self.before)
    }
}

/// Write the answer down in `out`: every name whose file the search found
/// under another name is rewritten to the name of the file found.
///
/// Two places carry a file name and both are rewritten, because a package may
/// use either: the `.rels` of an `EpcExternalPartReference` (the writer
/// regenerates those from what the store knows) and, in eml 2.3, the `URI` of
/// every `ExternalDataArrayPart`. Returns `None` when every array names the
/// file it is in (nothing is written).
///
/// The caller reopens `out` with its own reading switches and asks
/// [`Fixed::lost`] for the verdict.
pub fn fix(store: &mut EpcStore, rows: &BTreeMap<String, Row>, out: &Path) -> Result<Option<Fixed>> {
    let mut declarations = 0usize;
    let mut renamed = Vec::new();
    for (declared, row) in rows {
        let Some(found) = &row.found else { continue };
        if found.agrees() {
            continue;
        }
        let name = found.name();

        for proxy in &row.proxies {
            store.register_external_file(proxy, name.clone());
            declarations += 1;
        }

        let users: Vec<String> = store
            .resources()
            .iter()
            .filter(|summary| {
                store
                    .get_object(&summary.uri)
                    .map(|object| {
                        store
                            .arrays_of(object.as_reflect())
                            .iter()
                            .any(|location| &location.file == declared)
                    })
                    .unwrap_or(false)
            })
            .map(|summary| summary.uri.clone())
            .collect();
        for uri in users {
            let object = store.edit(&uri)?;
            if rename_uris(object, declared, &name) > 0 {
                declarations += 1;
            }
        }
        renamed.push((declared.clone(), name));
    }

    if declarations == 0 {
        return Ok(None);
    }

    // Carry over every file name the package *had*. A save regenerates the
    // `.rels` from what the store knows, and what it knows about external files
    // is `declared_files` - so a name only ever read out of a `.rels` is dropped
    // unless it is put back first.
    let known: Vec<(String, String)> = store.external_file_map().into_iter().collect();
    for (uuid, file) in known {
        store.register_external_file(&uuid, file);
    }

    let before = unresolved_pointers(store);
    let diagnostics = store.save_as(out, &SaveOptions::default())?;
    Ok(Some(Fixed {
        renamed,
        declarations,
        diagnostics,
        before,
    }))
}

/// Replace a file name wherever an object writes one, and say how many times.
///
/// Reflection rather than a typed walk: the field is `URI` on an eml 2.3
/// `ExternalDataArrayPart` and `PathInHdfFile` has no name at all in 2.0, and a
/// version this build learns tomorrow is handled by the same code.
pub fn rename_uris(object: &mut dyn Reflect, from: &str, to: &str) -> usize {
    let mut changed = 0usize;
    let fields: Vec<(&'static str, usize)> = object
        .type_info()
        .fields
        .iter()
        .map(|f| {
            let occurrences = object.field(f.name).map(|v| v.occurrences()).unwrap_or(0);
            (f.name, occurrences)
        })
        .collect();

    for (name, occurrences) in fields {
        // The scalar that carries a file name, whatever the release calls the
        // element around it.
        if name.eq_ignore_ascii_case("URI")
            && let Some(FieldValue::Scalar(scalar)) = object.field(name)
        {
            let text = scalar.to_string();
            let bare = text.rsplit(['/', '\\']).next().unwrap_or(&text);
            if text == from || bare == from {
                if object.set_field(name, OwnedValue::Str(to.to_string())).is_ok() {
                    changed += 1;
                }
                continue;
            }
        }
        // Descend. `field_object_mut` is the only mutable way in, so a list is
        // walked by index rather than by iterator.
        match occurrences {
            0 => {
                if let Some(child) = object.field_object_mut(name, None) {
                    changed += rename_uris(child, from, to);
                }
            }
            n => {
                for index in 0..n {
                    if let Some(child) = object.field_object_mut(name, Some(index)) {
                        changed += rename_uris(child, from, to);
                    }
                }
            }
        }
    }
    changed
}
