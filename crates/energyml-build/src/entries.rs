//! What a caller names, as pieces of a bag — a file, an address, an object of a
//! package.
//!
//! This used to live in `energyml-cli/src/create.rs`, and that was logic in
//! a face: the command decided which file is a table and which a GeoJSON, how a
//! slot is read off `top=…`, how two inputs given to one slot are numbered. The
//! recipe runner ([`crate::recipe`]) needs every one of those answers, and a
//! second copy of them would be two readers of one input — the shape of the
//! bugs once found in this workspace. So the answers are here, and the command calls them.
//!
//! # The bytes are the caller's
//!
//! This crate compiles for wasm and reads no disk and no network: [`Reader`] is
//! how a caller hands over the **text** of a file or of an address, and the
//! package a `model.epc#uuid` names. The CLI reads with `std::fs` and `ureq`;
//! the recipe runner reads files relative to its recipe. What the text *is* —
//! a table, a GeoJSON, neither — is decided here, by the same rule for both.

use std::collections::BTreeMap;

use energyml_storage::EpcStore;

use crate::osdu::Template;
use crate::pieces::{Pieces, Source};
use crate::source::{Source as Where, name_of, source_of};
use crate::tabular::{self, Dialect};

/// How the bytes of an input reach this module — the caller's I/O.
pub trait Reader {
    /// The text a source names, and the content type a server declared for it.
    ///
    /// A file declares nothing, which is why the second half is an `Option`
    /// and not an empty string: *no claim* and *a claim of nothing* are two
    /// answers, and only the first should fall back on the name.
    fn text(&mut self, source: &Where) -> Result<(String, Option<String>), String>;

    /// The **bytes** a source names, and the content type a server declared —
    /// what a document piece is made of ([`read_document`]).
    ///
    /// The default goes through [`Reader::text`], so a reader written before
    /// documents existed still hands over a README; a PDF is not text and is
    /// refused there, by that reader's own words. A reader that can do better
    /// — a disk, a network — says so by implementing this.
    fn bytes(&mut self, source: &Where) -> Result<(Vec<u8>, Option<String>), String> {
        self.text(source).map(|(text, declared)| (text.into_bytes(), declared))
    }

    /// The package at that path, opened **once** however many of its objects
    /// are named.
    fn package(&mut self, path: &str) -> Result<&EpcStore, String>;
}

/// Why an input could not be made a piece.
///
/// One case is apart because only the caller can say how to repair it: a table
/// needs a template, and the CLI gives one with `--template` while a recipe
/// gives one per step. The sentence around it is the same for both — see
/// [`Unread::said`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unread {
    /// A table, and nothing to say what its columns mean.
    NoTemplate {
        /// The source, as written.
        source: String,
    },
    /// Anything else, said in full.
    Said(String),
}

impl Unread {
    /// The refusal in words, `give` naming how the caller gives a template —
    /// `` `--template <file.json>` `` for the command line.
    pub fn said(&self, give: &str) -> String {
        match self {
            Unread::NoTemplate { source } => format!(
                "{source}: a table needs {give} — an OSDU ColumnBasedTableTemplate, a File.TabularData, or the bare list of column definitions. What a column means is read from it, never guessed from its heading"
            ),
            Unread::Said(why) => why.clone(),
        }
    }
}

impl From<String> for Unread {
    fn from(why: String) -> Self {
        Unread::Said(why)
    }
}

/// The slot an input names — `top=model.epc#…` — and the input without it.
///
/// A slot is words, digits, spaces, `-` and `_`: what a slot name is made of,
/// and what no path starts with — a drive letter stops at its colon.
pub fn slot_of(input: &str) -> (Option<&str>, &str) {
    match input.split_once('=') {
        Some((slot, rest))
            if !slot.is_empty() && slot.chars().all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_')) =>
        {
            (Some(slot), rest)
        }
        _ => (None, input),
    }
}

/// **A slot named twice is two pieces, `wells` then `wells 2`** — the names the
/// binder ranks (`declare::Binder`) and the front end sends.
///
/// Named alike, every shape was paired with the first companion
/// `wells (object)`: 120 frames given, the same well seen 120 times.
#[derive(Debug, Clone, Default)]
pub struct Numbering {
    seen: BTreeMap<String, usize>,
}

impl Numbering {
    /// The name the next piece given to `slot` takes.
    pub fn next(&mut self, slot: &str) -> String {
        let count = self.seen.entry(slot.to_string()).or_default();
        *count += 1;
        if *count == 1 { slot.to_string() } else { format!("{slot} {count}") }
    }
}

/// Whether this is a table — by its name, or by what the server said.
///
/// Deliberately **not** `.json`: a template is a `.json` too, and a rule that
/// took one for the other would read a producer's column definitions as rows.
/// The separator is not part of this question — `tabular::Dialect::sniff`
/// answers it, on the header and outside quotes.
pub fn reads_as_table(name: &str, declared: Option<&str>) -> bool {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".csv") || lower.ends_with(".tsv") {
        return true;
    }
    let kind = declared.unwrap_or_default().split(';').next().unwrap_or_default().trim().to_ascii_lowercase();
    matches!(kind.as_str(), "text/csv" | "text/tab-separated-values")
}

/// Whether this is a GeoJSON — **by its name, or by what the server said it
/// was**.
///
/// The name decides first, and that is the house rule (`mesh.rs`: the format is
/// decided by the file name, never by the content). The content type is not an
/// exception to it: sniffing would be reading the **bytes** and guessing, while
/// a `Content-Type` is the server *stating* what it sent — the same kind of
/// claim an EPC's `[Content_Types].xml` makes about its parts, which this
/// workspace reads rather than second-guesses.
///
/// It earns its keep on exactly the case that made this worth writing: an API
/// that serves a collection from `.../features?bbox=…`, which names no file and
/// would otherwise be refused for the shape of its address.
pub fn reads_as_geojson(name: &str, declared: Option<&str>) -> bool {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".geojson") || lower.ends_with(".json") {
        return true;
    }
    // The parameters after `;` are the charset and friends, never the type.
    let kind = declared.unwrap_or_default().split(';').next().unwrap_or_default().trim().to_ascii_lowercase();
    matches!(kind.as_str(), "application/geo+json" | "application/json" | "text/json")
}

/// A table template, read through the caller's reader.
///
/// **Read once for the lot**: a template describes a producer's export, not
/// one of its files, and reading it per file would be reading the same JSON a
/// hundred and twenty times to answer the same question.
pub fn template(reader: &mut dyn Reader, source: &Where) -> Result<Template, String> {
    let (text, _) = reader.text(source)?;
    crate::osdu::read(&text).map_err(|why| format!("{}: {why}", source.as_str()))
}

/// The text of one input as a piece — a table when its name or its declared
/// type says so, a GeoJSON otherwise, or the refusal that names both ends.
///
/// `name` is the **file** the source names (`source::name_of`): a collection
/// keeps it, and it is what an operation reads a stage or a date from.
/// `label` is the source as written, for a refusal. What a GeoJSON sets aside
/// feature by feature goes into `set_aside`, named — the rest still reads.
pub fn read_text(
    name: &str,
    text: &str,
    declared: Option<&str>,
    template: Option<&Template>,
    label: &str,
    set_aside: &mut Vec<String>,
) -> Result<Source, Unread> {
    // **A table before a GeoJSON**, because the two are told apart by the
    // file's name and a `.json` is both a template and a collection. The
    // house rule of `mesh.rs` applies: the name decides, never the bytes.
    if reads_as_table(name, declared) {
        let Some(template) = template else {
            return Err(Unread::NoTemplate { source: label.to_string() });
        };
        let dialect = template.dialect.unwrap_or_else(|| Dialect::sniff(text));
        let span = tracing::info_span!("table.parse", file = name, bytes = text.len(), rows = tracing::field::Empty).entered();
        let table = tabular::read(name, text, dialect).map_err(|why| format!("{label}: {why}"))?;
        span.record("rows", table.len());
        drop(span);
        return Ok(Source::Table { name: name.to_string(), table, template: template.clone() });
    }
    if !reads_as_geojson(name, declared) {
        return Err(Unread::Said(format!(
            "{label}: only a GeoJSON can be read here for now — meshes and the objects of a package come next\n                   (it is named `{name}`{}, and a GeoJSON is named `.geojson` or `.json`, or served as `application/geo+json`)",
            declared.map(|kind| format!(", served as `{kind}`")).unwrap_or_default(),
        )));
    }
    let collection = {
        let _span = tracing::info_span!("geojson.parse", file = name, bytes = text.len()).entered();
        crate::geojson::read(text).map_err(|why| format!("{label}: {why}"))?
    };
    for why in &collection.refused {
        set_aside.push(format!("{name}: {why}"));
    }
    Ok(Source::Features { name: name.to_string(), features: collection.features })
}

/// A file as a **document** piece: its bytes, untouched, and the content type
/// its source declared.
///
/// Never decided from the name — a `.json` stays a GeoJSON for every operation
/// that reads one. It is the operation's declaration that asks for a document
/// ([`takes_document`]), and only then is a file read this way.
pub fn read_document(name: &str, bytes: Vec<u8>, declared: Option<&str>) -> Source {
    Source::Document { name: name.to_string(), content_type: declared.map(str::to_string), bytes }
}

/// Whether a file given to `slot` — or to no slot — is to be read as a
/// document, by what the operation **takes** (`Operation::takes`).
///
/// A named slot answers by its own kind. An unnamed file is a document only
/// when the operation takes documents and nothing else a file could fill — a
/// table or a collection of features: guessing between the two would be
/// reading the name of the file, which this module never does.
pub fn takes_document(takes: &[crate::operation::Slot], slot: Option<&str>) -> bool {
    use crate::operation::Need;
    let kind_of = |one: &crate::operation::Slot| match &one.need {
        Need::Input(wants) => Some(wants.kind),
        _ => None,
    };
    match slot {
        Some(slot) => takes.iter().any(|one| one.name == slot && kind_of(one) == Some("document")),
        None => {
            let kinds: Vec<&str> = takes.iter().filter_map(kind_of).collect();
            kinds.contains(&"document") && !kinds.iter().any(|kind| matches!(*kind, "table" | "features"))
        }
    }
}

/// An object of a package, read as a shape — and what the caller then knows
/// of it. One implementation, [`crate::read`], shared with the front end, and so is
/// the naming for a slot. `key` is the object's uri — what the caller holds —;
/// a bare uuid (the `model.epc#<uuid>` of the command line) is accepted at this
/// door and refused, with its candidates, when the package holds it twice.
pub fn object_pieces(store: &EpcStore, key: &str, slot: Option<&str>) -> Result<Vec<(String, Source)>, String> {
    let read = crate::read::pieces_of_object(store, key)?;
    Ok(match slot {
        Some(slot) => crate::read::in_slot(slot, read),
        None => read,
    })
}

/// The bag: one piece per input — two for an object of a package, its shape
/// and what is known of it.
///
/// An input is `[slot=]file`, `[slot=]https://…` or `[slot=]model.epc#<uuid>`.
/// A **table** takes `template`, one for every table given, which is what a
/// set of files exported by one producer has.
pub fn pieces_of(
    inputs: &[String],
    template: Option<&Template>,
    reader: &mut dyn Reader,
    set_aside: &mut Vec<String>,
) -> Result<Pieces, Unread> {
    pieces_for(inputs, template, reader, set_aside, &[])
}

/// [`pieces_of`], for an operation whose declaration is known: a file given
/// to a slot that takes a **document** is read as bytes ([`takes_document`]).
/// With no slot declared, exactly [`pieces_of`].
pub fn pieces_for(
    inputs: &[String],
    template: Option<&Template>,
    reader: &mut dyn Reader,
    set_aside: &mut Vec<String>,
    takes: &[crate::operation::Slot],
) -> Result<Pieces, Unread> {
    let mut pieces = Pieces::empty();
    let mut numbering = Numbering::default();
    for input in inputs {
        let (slot, input) = slot_of(input);
        // The slot as declared, before `wells 2` numbers it: what decides
        // whether a file is a document.
        let declared_slot = slot;
        let numbered = slot.map(|slot| numbering.next(slot));
        let slot = numbered.as_deref();
        // **What it is, before anything is cut off it.** A `#` means *this uuid
        // of that package* for a path — and it means a **fragment** in a URL,
        // so testing for it first would send `https://a/x.json#part` off to
        // open a package called `https://a/x.json`. The scheme decides, and it
        // decides first.
        let source = source_of(input);
        if matches!(source, Where::File(_))
            && let Some((package, uuid)) = input.rsplit_once('#')
        {
            let store = reader.package(package)?;
            let read = object_pieces(store, uuid, slot).map_err(|why| format!("{package}: {why}"))?;
            for (name, source) in read {
                pieces = pieces.with(name, source);
            }
            continue;
        }
        // The name is the **file** a source names, whether it came off a disk
        // or off a network — `crate::source`, one implementation, so that
        // `sigeol-units` reads the same stage from
        // `.../GeologicUnit_Bartonien_pretty.json` either way.
        let name = name_of(&source);
        // The piece takes the slot's name; the collection keeps the source's,
        // which is what an operation reads a stage or a date from.
        let piece = slot.map(str::to_string).unwrap_or_else(|| name.clone());
        if takes_document(takes, declared_slot) {
            let (bytes, declared) = reader.bytes(&source)?;
            pieces = pieces.with(piece, read_document(&name, bytes, declared.as_deref()));
            continue;
        }
        let (text, declared) = reader.text(&source)?;
        let read = read_text(&name, &text, declared.as_deref(), template, source.as_str(), set_aside)?;
        pieces = pieces.with(piece, read);
    }
    Ok(pieces)
}
