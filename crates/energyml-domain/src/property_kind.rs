//! Resolving what a property *is*: the kind, its parents, its quantity class.
//!
//! [`crate::property`] reads what the document says — a name, or a reference.
//! This module follows it: into the package, then into a dictionary, up the
//! parent chain to the root, and it says at every step where the answer came
//! from and what it could not follow.
//!
//! # Where a kind is looked for, in order
//!
//! 1. **The package.** The TRG encourages writing a `PropertyKind` object into
//!    the EPC itself, and a kind that is there is authoritative — it is what
//!    the producer meant, not what a dictionary of the same uuid might say.
//! 2. **The dictionary**, which by default is the Energistics one shipped with
//!    this crate (eml 2.3, 3 662 kinds), and which a caller may replace.
//! 3. **Nothing** — and then the refusal names the uuid, the title the
//!    reference cached, both places that were searched, and what to do about
//!    it. "Not found" with no names is what sends someone reading source code.
//!
//! # The dictionary, and how it is loaded
//!
//! Three ways in, by design:
//!
//! - [`builtin`] — the shipped dictionary, **parsed on first use** and then
//!   kept. Nothing is parsed by a program that never resolves a kind.
//! - [`set_default`] — a caller replaces it, once, at startup: a newer
//!   dictionary, a company one, or a trimmed one for a wasm bundle.
//! - [`Dictionary::from_xml`] / [`Dictionary::from_compact`] — an explicit
//!   dictionary handed to [`Resolver::with_dictionary`], which touches no
//!   global state at all.
//!
//! The shipped dictionary is the compacted table `data/property_kinds.tsv`
//! (3 662 kinds, tab-separated lines, generated from the Energistics XML by the
//! private generator). `build.rs` validates it and fails the build when it is
//! absent or empty — never an empty built-in dictionary.
//!
//! # Two vocabularies that are not the same list
//!
//! RESQML 2.0.1 writes a *standard* kind as a name from its own enumeration —
//! `rock volume`, `index`, `cell length`. The eml 2.3 dictionary is a
//! different list with different names, and roughly half of the 2.0.1
//! vocabulary is simply not in it. So a standard kind resolves **by title**
//! when the dictionary happens to hold that title, and otherwise comes back as
//! a named leaf with a note explaining exactly that. It is not an error: the
//! name *is* the meaning in 2.0.1.
//!
//! Resolution by title is used **only** for those standard names. A local kind
//! is never resolved by its title, and the reason is in the corpus: Volve has
//! two kinds titled `X` and two titled `Y`, with different uuids.

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

use energyml_core::value::{Reflect, Scalar};

use crate::scan::{self, RawKind};

// `scan` is private — the reader is an implementation detail — but `RawKind` is
// what every lookup here hands back, so it has to be nameable. Without this a
// caller outside the crate could use the result of `by_uuid` and never write
// its type: no `let kind: &RawKind`, no function taking one, no `Vec<&RawKind>`
// of its own. A public method returning an unnameable type is only half public.
pub use crate::scan::RawKind as DictionaryKind;
use crate::error::{Error, Result};
use crate::property::PropertyKindRef;
use crate::source::ObjectSource;

/// One property kind, whatever it was read from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Kind {
    /// Uuid, empty for a 2.0.1 standard kind, which has none.
    pub uuid: String,
    /// The name a person uses.
    pub title: String,
    /// Uuid of the parent kind, when it has one that is not itself.
    pub parent: Option<String>,
    /// The **uri** of the parent, version included, when the package's own
    /// kind names one — what the walk up reads it by.
    pub parent_uri: Option<String>,
    /// Name of a class in the Energistics unit dictionary.
    ///
    /// Informational when it matches nothing there (common TRG §11.9), which
    /// is why nothing in this crate refuses a property over it.
    pub quantity_class: Option<String>,
    /// An abstract kind must only be a parent, never used directly.
    pub is_abstract: bool,
    /// Set when the dictionary marks it as no longer to be used.
    pub deprecated: Option<String>,
    /// Where this one was found.
    pub found_in: Found,
}

/// Where a kind came from — part of the answer, not a detail.
///
/// An application that colours by kind, or converts units by quantity class,
/// behaves differently when the answer came from the package the user opened
/// than when it came from a dictionary that merely shares a uuid.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Found {
    /// A `PropertyKind` object inside the package.
    Package,
    /// The dictionary in use.
    Dictionary,
    /// The name written in the document, resolved no further.
    StandardName,
}

impl std::fmt::Display for Found {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Found::Package => "the package",
            Found::Dictionary => "the dictionary",
            Found::StandardName => "the name in the document",
        })
    }
}

/// A resolved kind, with everything above it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolution {
    /// The kind itself first, then each parent up to the root.
    pub chain: Vec<Kind>,
    /// What could not be followed, in words meant for a person.
    ///
    /// Never empty-and-silent: if a parent is missing, or a standard name is
    /// not in the dictionary, or the chain was cut short, it is said here.
    pub notes: Vec<String>,
}

impl Resolution {
    /// The kind the property names.
    pub fn kind(&self) -> &Kind {
        &self.chain[0]
    }

    /// The nearest quantity class in the chain — the property's own if it has
    /// one, otherwise the first ancestor that states one.
    pub fn quantity_class(&self) -> Option<&str> {
        self.chain
            .iter()
            .find_map(|kind| kind.quantity_class.as_deref())
    }

    /// Titles from the kind up to the root, for a message or a tree view.
    pub fn path(&self) -> String {
        self.chain
            .iter()
            .map(|kind| kind.title.as_str())
            .collect::<Vec<_>>()
            .join(" ← ")
    }

    /// Whether the kind used directly is abstract — which the schema advises
    /// against, word for word: *"If the Is Abstract flag is set, then this
    /// entry should be used only as the parent of a real property"* (eml 2.3
    /// `PropertyKind.IsAbstract`, the same text in 2.1 and 2.2).
    pub fn uses_an_abstract_kind(&self) -> bool {
        self.kind().is_abstract
    }
}

// ---------------------------------------------------------------------------
// The dictionary
// ---------------------------------------------------------------------------

/// A property kind dictionary, indexed for lookup.
#[derive(Clone, Debug, Default)]
pub struct Dictionary {
    kinds: Vec<RawKind>,
    by_uuid: HashMap<String, usize>,
    /// Lowercased title → every kind carrying it. A `Vec`, because titles are
    /// not unique: the shipped dictionary has two kinds titled
    /// `spring stiffness`, and answering with one of them arbitrarily is how a
    /// resolver becomes untrustworthy.
    by_title: HashMap<String, Vec<usize>>,
    label: String,
}

impl Dictionary {
    /// Build an index over kinds already read.
    fn index(kinds: Vec<RawKind>, label: impl Into<String>) -> Self {
        let mut by_uuid = HashMap::with_capacity(kinds.len());
        let mut by_title: HashMap<String, Vec<usize>> = HashMap::with_capacity(kinds.len());
        for (at, kind) in kinds.iter().enumerate() {
            by_uuid.insert(kind.uuid.to_ascii_lowercase(), at);
            by_title
                .entry(kind.title.to_ascii_lowercase())
                .or_default()
                .push(at);
        }
        Self {
            kinds,
            by_uuid,
            by_title,
            label: label.into(),
        }
    }

    /// Read a `PropertyKindDictionary` document.
    ///
    /// Any version, any namespace prefix. The error says what the document is
    /// when it is not a dictionary, because loading the wrong file otherwise
    /// looks exactly like a dictionary in which nothing is ever found.
    pub fn from_xml(xml: &str) -> Result<Self> {
        let kinds = scan::scan(xml).map_err(|reason| {
            Error::malformed("property kind dictionary", format!("{reason}; nothing was loaded"))
        })?;
        Ok(Self::index(kinds, "a dictionary loaded by this application"))
    }

    /// Read the compact form this crate writes at build time.
    pub fn from_compact(text: &str) -> Self {
        Self::index(
            scan::from_compact(text),
            "a dictionary loaded by this application",
        )
    }

    /// A dictionary with nothing in it, which every message will admit to.
    pub fn empty(label: impl Into<String>) -> Self {
        Self::index(Vec::new(), label)
    }

    /// Name this dictionary carries in messages.
    pub fn labelled(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// How many kinds it holds.
    pub fn len(&self) -> usize {
        self.kinds.len()
    }

    /// Whether it holds none.
    pub fn is_empty(&self) -> bool {
        self.kinds.is_empty()
    }

    /// What it calls itself in a message.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// A kind by uuid.
    pub fn by_uuid(&self, uuid: &str) -> Option<&RawKind> {
        self.by_uuid
            .get(&uuid.to_ascii_lowercase())
            .map(|at| &self.kinds[*at])
    }

    /// Every kind carrying a title, case-insensitively.
    pub fn by_title(&self, title: &str) -> Vec<&RawKind> {
        self.by_title
            .get(&title.to_ascii_lowercase())
            .map(|found| found.iter().map(|at| &self.kinds[*at]).collect())
            .unwrap_or_default()
    }

    /// Everything it holds, in the order the dictionary listed it.
    pub fn kinds(&self) -> &[RawKind] {
        &self.kinds
    }

    /// Kinds whose title contains `query`, best match first.
    ///
    /// # Why the ranking, rather than an alphabetical list
    ///
    /// This is what a chooser is built on, and a chooser is judged on its first
    /// three rows. Typing `pressure` into 3 662 entries matches 89 of them; an
    /// alphabetical list opens on `absolute pressure` and buries `pressure`
    /// itself forty rows down. So: exact title first, then titles *starting*
    /// with the query, then the rest — and alphabetical inside each band, so
    /// the order is stable and a person can learn it.
    ///
    /// An empty query is not an error and not everything: it is the head of the
    /// alphabetical list, which is what a chooser shows before anyone types.
    ///
    /// **Abstract and deprecated kinds are returned.** They are part of the
    /// vocabulary and a person looking one up needs to find it; refusing to
    /// show them would make the dictionary look incomplete. Each carries its
    /// own flag, and the caller decides whether to grey it out — a decision
    /// about display, made where display is decided.
    pub fn search(&self, query: &str, limit: usize) -> Vec<&RawKind> {
        let needle = query.trim().to_ascii_lowercase();
        let mut ranked: Vec<(u8, &str, &RawKind)> = self
            .kinds
            .iter()
            .filter_map(|kind| {
                let title = kind.title.to_ascii_lowercase();
                let band = if needle.is_empty() {
                    2
                } else if title == needle {
                    0
                } else if title.starts_with(&needle) {
                    1
                } else if title.contains(&needle) {
                    2
                } else {
                    return None;
                };
                Some((band, kind.title.as_str(), kind))
            })
            .collect();
        // By band, then by title, then by uuid: two kinds share a title often
        // enough in this dictionary that leaving them in whatever order the
        // filter produced would make the list shuffle between identical calls.
        ranked.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then_with(|| a.1.to_ascii_lowercase().cmp(&b.1.to_ascii_lowercase()))
                .then_with(|| a.2.uuid.cmp(&b.2.uuid))
        });
        ranked.into_iter().take(limit).map(|(_, _, kind)| kind).collect()
    }

    /// How many kinds the same query matches, before any limit.
    ///
    /// The number a list has to show beside a truncated one. `search` takes a
    /// limit and returns no total, so a front end that showed only its output said
    /// *"here are 60 kinds"* about a dictionary of thousands — and a person
    /// looking for one that did not make the cut has no way to know whether it
    /// is absent or merely beyond the sixtieth.
    ///
    /// The predicate is the one `search` filters on, deliberately duplicated
    /// nowhere else: the two numbers disagreeing would be worse than not
    /// showing one.
    pub fn count(&self, query: &str) -> usize {
        let needle = query.trim().to_ascii_lowercase();
        if needle.is_empty() {
            return self.kinds.len();
        }
        self.kinds
            .iter()
            .filter(|kind| kind.title.to_ascii_lowercase().contains(&needle))
            .count()
    }

    /// A kind and everything above it, nearest first.
    ///
    /// Stops on a uuid already seen: the dictionary's root is its own parent,
    /// so a walk that trusted the chain to end would not.
    pub fn ancestry(&self, uuid: &str) -> Vec<&RawKind> {
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut current = self.by_uuid(uuid);
        while let Some(kind) = current {
            if !seen.insert(kind.uuid.to_ascii_lowercase()) {
                break;
            }
            out.push(kind);
            current = kind.parent.as_deref().and_then(|p| self.by_uuid(p));
        }
        out
    }
}

/// The dictionary compacted into this build (`data/property_kinds.tsv`, checked
/// by `build.rs`, which fails the build when it is absent or empty).
const BUILT_IN: &str = include_str!(concat!(env!("OUT_DIR"), "/property_kinds.tsv"));

static DEFAULT: OnceLock<RwLock<Dictionary>> = OnceLock::new();

fn slot() -> &'static RwLock<Dictionary> {
    DEFAULT.get_or_init(|| {
        // The lazy half of the answer: nothing is parsed until something asks
        // for a kind, and then it is parsed once for the whole process.
        let dictionary = Dictionary::from_compact(BUILT_IN).labelled("the built-in Energistics dictionary (eml 2.3)");
        RwLock::new(dictionary)
    })
}

/// Do something with the dictionary currently in use.
///
/// Borrowing rather than cloning: the dictionary is 3 662 entries and callers
/// ask for one kind at a time.
pub fn with_default<T>(f: impl FnOnce(&Dictionary) -> T) -> T {
    let guard = slot().read().expect("the dictionary lock is never held across a panic");
    f(&guard)
}

/// Facts about the dictionary in use, for a message or a `--version` line.
pub fn builtin() -> (usize, String) {
    with_default(|dictionary| (dictionary.len(), dictionary.label().to_string()))
}

/// Replace the dictionary every resolver uses by default.
///
/// For an application that ships a newer Energistics release, a company
/// dictionary, or a trimmed one for a wasm bundle. Takes effect for every
/// later resolution; a resolver built with [`Resolver::with_dictionary`]
/// ignores it entirely.
pub fn set_default(dictionary: Dictionary) {
    let mut guard = slot().write().expect("the dictionary lock is never held across a panic");
    *guard = dictionary;
}

// ---------------------------------------------------------------------------
// The resolver
// ---------------------------------------------------------------------------

/// Follows a [`PropertyKindRef`] to what it means.
///
/// Built over an [`ObjectSource`] — the package — and a dictionary, in that
/// order of authority.
pub struct Resolver<'a> {
    objects: &'a dyn ObjectSource,
    dictionary: Option<&'a Dictionary>,
    depth_limit: usize,
}

impl<'a> Resolver<'a> {
    /// A resolver over a package, using the dictionary set as default.
    pub fn new(objects: &'a dyn ObjectSource) -> Self {
        Self {
            objects,
            dictionary: None,
            depth_limit: 64,
        }
    }

    /// A resolver over a package and one particular dictionary, touching no
    /// global state.
    pub fn with_dictionary(objects: &'a dyn ObjectSource, dictionary: &'a Dictionary) -> Self {
        Self {
            objects,
            dictionary: Some(dictionary),
            depth_limit: 64,
        }
    }

    /// Follow a reference to the kind and all its parents.
    pub fn resolve(&self, reference: &PropertyKindRef) -> Result<Resolution> {
        self.resolve_at(reference, None)
    }

    /// [`Resolver::resolve`] for a reference whose **uri** is known
    /// ([`crate::property::Property::kind_uri`]): the version it names is the
    /// version read, where the uuid alone cannot tell two apart.
    pub fn resolve_at(&self, reference: &PropertyKindRef, uri: Option<&str>) -> Result<Resolution> {
        match self.dictionary {
            Some(dictionary) => self.resolve_with(reference, uri, dictionary),
            None => with_default(|dictionary| self.resolve_with(reference, uri, dictionary)),
        }
    }

    fn resolve_with(&self, reference: &PropertyKindRef, uri: Option<&str>, dictionary: &Dictionary) -> Result<Resolution> {
        let mut notes = Vec::new();
        let first = match reference {
            PropertyKindRef::Object { uuid, title } => {
                self.lookup(uuid, uri, title.as_deref(), dictionary, &mut notes)?
            }
            PropertyKindRef::Standard { name } => self.standard(name, dictionary, &mut notes),
        };

        let mut chain = vec![first];
        let mut seen = vec![chain[0].uuid.to_ascii_lowercase()];
        while let Some(parent) = chain.last().and_then(|kind| kind.parent.clone()) {
            let parent_uri = chain.last().and_then(|kind| kind.parent_uri.clone());
            if seen.contains(&parent.to_ascii_lowercase()) {
                // The dictionary's root is its own parent, so this is the
                // normal way the walk ends — not an anomaly.
                break;
            }
            if chain.len() >= self.depth_limit {
                notes.push(format!(
                    "the parent chain is more than {} deep, which is not a chain but a loop; \
                     it was cut after `{}`",
                    self.depth_limit,
                    chain.last().map(|k| k.title.as_str()).unwrap_or_default()
                ));
                break;
            }
            seen.push(parent.to_ascii_lowercase());
            match self.lookup(&parent, parent_uri.as_deref(), None, dictionary, &mut notes) {
                Ok(kind) => chain.push(kind),
                Err(_) => {
                    notes.push(format!(
                        "the parent `{parent}` of `{}` is in neither the package nor {}, so the chain \
                         stops here and no quantity class could be inherited from above it",
                        chain.last().map(|k| k.title.as_str()).unwrap_or_default(),
                        dictionary.label()
                    ));
                    break;
                }
            }
        }

        Ok(Resolution { chain, notes })
    }

    /// One kind: the package first, then the dictionary. By its uri when the
    /// reference gave one — the package may hold the kind twice — and by the
    /// uuid otherwise; the dictionary knows uuids only (a published kind has
    /// no version).
    fn lookup(
        &self,
        uuid: &str,
        uri: Option<&str>,
        cached_title: Option<&str>,
        dictionary: &Dictionary,
        notes: &mut Vec<String>,
    ) -> Result<Kind> {
        let held = match uri {
            Some(uri) => self.objects.object_at(uri),
            None => self.objects.object(uuid),
        };
        if let Some(object) = held {
            let kind = from_object(object.as_reflect(), uuid);
            if let Some(entry) = dictionary.by_uuid(uuid)
                && !entry.title.eq_ignore_ascii_case(&kind.title)
            {
                notes.push(format!(
                    "`{uuid}` is titled `{}` in the package and `{}` in {}; the package wins, \
                     because it is what this file's producer meant",
                    kind.title,
                    entry.title,
                    dictionary.label()
                ));
            }
            return Ok(kind);
        }
        if let Some(entry) = dictionary.by_uuid(uuid) {
            return Ok(from_raw(entry, Found::Dictionary));
        }

        let named = cached_title
            .map(|title| format!(" (titled `{title}` where it is referenced)"))
            .unwrap_or_default();
        Err(Error::Missing(format!(
            "property kind `{uuid}`{named}: it is not an object of this package, and {} does not \
             hold it either{}. Either the EPC is missing the `PropertyKind` object it references — \
             the specification allows and encourages carrying it — or the dictionary that defines \
             it has to be loaded with `property_kind::set_default`",
            dictionary.label(),
            if dictionary.is_empty() {
                " (that dictionary is empty)"
            } else {
                ""
            }
        )))
    }

    /// A RESQML 2.0.1 standard name.
    ///
    /// Never an error: the name is the meaning in that version. What varies is
    /// how much more could be said about it.
    fn standard(&self, name: &str, dictionary: &Dictionary, notes: &mut Vec<String>) -> Kind {
        let candidates = dictionary.by_title(name);
        match candidates.len() {
            1 => {
                let mut kind = from_raw(candidates[0], Found::Dictionary);
                // The uuid comes from the dictionary, but the document never
                // said one — worth a note, because it is the one place a kind
                // gains an identity it was not written with.
                notes.push(format!(
                    "`{name}` is a RESQML 2.0.1 standard property kind; it was matched by title to \
                     `{}` in {}",
                    kind.uuid,
                    dictionary.label()
                ));
                kind.found_in = Found::Dictionary;
                kind
            }
            0 => {
                notes.push(format!(
                    "`{name}` is a RESQML 2.0.1 standard property kind, and {} has no kind with that \
                     title{}. The 2.0.1 vocabulary and the eml 2.3 dictionary are different lists, so \
                     this is expected for about half of them: the name is the meaning, and no parent \
                     chain or quantity class is available for it",
                    dictionary.label(),
                    if dictionary.is_empty() {
                        " (it is empty)"
                    } else {
                        ""
                    }
                ));
                Kind {
                    uuid: String::new(),
                    title: name.to_string(),
                    parent: None,
                    parent_uri: None,
                    quantity_class: None,
                    is_abstract: false,
                    deprecated: None,
                    found_in: Found::StandardName,
                }
            }
            _ => {
                // Ambiguity is reported, never resolved by picking one.
                notes.push(format!(
                    "`{name}` matches {} kinds of {} by title ({}); none was chosen, because a title \
                     is not an identity — resolution stopped at the name",
                    candidates.len(),
                    dictionary.label(),
                    candidates
                        .iter()
                        .map(|kind| kind.uuid.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
                Kind {
                    uuid: String::new(),
                    title: name.to_string(),
                    parent: None,
                    parent_uri: None,
                    quantity_class: None,
                    is_abstract: false,
                    deprecated: None,
                    found_in: Found::StandardName,
                }
            }
        }
    }
}

fn from_raw(raw: &RawKind, found_in: Found) -> Kind {
    Kind {
        uuid: raw.uuid.clone(),
        title: raw.title.clone(),
        // The dictionary's root is its own parent; dropping that here is what
        // keeps every caller from having to know it.
        parent_uri: None,
        parent: raw
            .parent
            .clone()
            .filter(|parent| !parent.eq_ignore_ascii_case(&raw.uuid)),
        quantity_class: raw.quantity_class.clone(),
        is_abstract: raw.is_abstract,
        deprecated: raw.deprecated.clone(),
        found_in,
    }
}

/// A `PropertyKind` object out of a package, read by shape.
///
/// eml 2.3 spells the parent `Parent`; RESQML 2.0.1 spells it
/// `ParentPropertyKind` and wraps it in a choice between a standard name and a
/// local reference. Both are read here, because a chain that stops at the
/// version boundary is a chain that never reaches a quantity class.
fn from_object(object: &dyn Reflect, uuid: &str) -> Kind {
    let title = object
        .field("Citation")
        .and_then(|f| f.as_object())
        .and_then(|citation| text(citation, "Title"))
        .unwrap_or_default();

    let mut parent_ref: Option<&dyn Reflect> = object
        .field("Parent")
        .and_then(|f| f.as_object())
        .filter(|reference| uuid_of_reference(*reference).is_some());

    if parent_ref.is_none()
        && let Some(inner) = object.field("ParentPropertyKind").and_then(|f| f.as_object())
    {
        parent_ref = Some(inner).filter(|reference| uuid_of_reference(*reference).is_some()).or_else(|| {
            inner
                .field("LocalPropertyKind")
                .and_then(|f| f.as_object())
                .filter(|reference| uuid_of_reference(*reference).is_some())
        });
    }
    let parent = parent_ref.and_then(uuid_of_reference);
    let parent_uri = parent_ref.and_then(energyml_core::identity::uri_of_reflect);

    Kind {
        uuid: uuid.to_string(),
        title,
        parent_uri: parent_uri.filter(|_| parent.as_ref().is_some_and(|p| !p.eq_ignore_ascii_case(uuid))),
        parent: parent.filter(|p| !p.eq_ignore_ascii_case(uuid)),
        quantity_class: text(object, "QuantityClass"),
        is_abstract: matches!(
            object.field("IsAbstract").and_then(|f| f.as_scalar()),
            Some(Scalar::Bool(true))
        ),
        deprecated: text(object, "DeprecationDate"),
        found_in: Found::Package,
    }
}

fn text(object: &dyn Reflect, field: &str) -> Option<String> {
    match object.field(field)?.as_scalar()? {
        Scalar::Str(s) if !s.is_empty() => Some(s.to_string()),
        Scalar::Str(_) => None,
        other => Some(other.to_string()),
    }
}

/// The uuid a reference names — the identity layer's reading, so every
/// spelling of the field is one answer.
fn uuid_of_reference(object: &dyn Reflect) -> Option<String> {
    energyml_core::identity::uuid_of_reflect(object).filter(|uuid| !uuid.is_empty()).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::NoObjects;

    fn dictionary() -> Dictionary {
        Dictionary::from_compact(
            "aaaa\tlength\tbbbb\tlength\t0\t\n\
             bbbb\tproperty\tbbbb\tunitless\t1\t\n\
             cccc\tspring stiffness\tbbbb\tforce per length\t0\t\n\
             dddd\tspring stiffness\tbbbb\tforce per length\t0\t\n\
             eeee\told thing\tbbbb\tunitless\t0\t2020-01-01\n",
        )
        .labelled("the test dictionary")
    }

    #[test]
    fn a_chain_walks_to_the_root_and_stops_there() {
        let dictionary = dictionary();
        let resolver = Resolver::with_dictionary(&NoObjects, &dictionary);
        let resolution = resolver
            .resolve(&PropertyKindRef::Object {
                uuid: "aaaa".into(),
                title: None,
            })
            .expect("it resolves");
        assert_eq!(resolution.path(), "length ← property");
        assert_eq!(resolution.quantity_class(), Some("length"));
        // The root is its own parent in the real dictionary; the walk must end
        // rather than loop.
        assert_eq!(resolution.chain.len(), 2);
        assert!(resolution.notes.is_empty(), "{:?}", resolution.notes);
    }

    #[test]
    fn a_quantity_class_is_inherited_from_the_nearest_ancestor_that_has_one() {
        let dictionary = Dictionary::from_compact("aaaa\tchild\tbbbb\t\t0\t\nbbbb\tparent\t\tarea\t0\t\n");
        let resolver = Resolver::with_dictionary(&NoObjects, &dictionary);
        let resolution = resolver
            .resolve(&PropertyKindRef::Object {
                uuid: "aaaa".into(),
                title: None,
            })
            .unwrap();
        assert_eq!(resolution.quantity_class(), Some("area"));
    }

    #[test]
    fn an_unknown_kind_says_where_it_looked_and_what_to_do() {
        let dictionary = dictionary();
        let resolver = Resolver::with_dictionary(&NoObjects, &dictionary);
        let error = resolver
            .resolve(&PropertyKindRef::Object {
                uuid: "0a5f4400-fa3e-11e5-80a4-0002a5d5c51b".into(),
                title: Some("cellIndex".into()),
            })
            .expect_err("it is nowhere");
        let message = error.to_string();
        for expected in ["0a5f4400", "cellIndex", "the test dictionary", "set_default"] {
            assert!(message.contains(expected), "`{expected}` missing from: {message}");
        }
    }

    #[test]
    fn a_standard_name_the_dictionary_knows_is_matched_and_the_match_is_declared() {
        let dictionary = dictionary();
        let resolver = Resolver::with_dictionary(&NoObjects, &dictionary);
        let resolution = resolver
            .resolve(&PropertyKindRef::Standard {
                name: "length".into(),
            })
            .unwrap();
        assert_eq!(resolution.quantity_class(), Some("length"));
        assert!(
            resolution.notes.iter().any(|note| note.contains("matched by title")),
            "gaining an identity by title has to be said: {:?}",
            resolution.notes
        );
    }

    #[test]
    fn a_standard_name_the_dictionary_lacks_is_a_leaf_and_not_an_error() {
        // Half the RESQML 2.0.1 vocabulary is in this position.
        let dictionary = dictionary();
        let resolver = Resolver::with_dictionary(&NoObjects, &dictionary);
        let resolution = resolver
            .resolve(&PropertyKindRef::Standard {
                name: "rock volume".into(),
            })
            .expect("a name is a meaning in 2.0.1");
        assert_eq!(resolution.kind().title, "rock volume");
        assert_eq!(resolution.kind().found_in, Found::StandardName);
        assert!(resolution.quantity_class().is_none());
        assert!(
            resolution.notes.iter().any(|note| note.contains("different lists")),
            "{:?}",
            resolution.notes
        );
    }

    #[test]
    fn an_ambiguous_title_is_reported_rather_than_picked() {
        // The shipped dictionary really does have two `spring stiffness`.
        let dictionary = dictionary();
        let resolver = Resolver::with_dictionary(&NoObjects, &dictionary);
        let resolution = resolver
            .resolve(&PropertyKindRef::Standard {
                name: "spring stiffness".into(),
            })
            .unwrap();
        assert_eq!(resolution.kind().found_in, Found::StandardName);
        assert!(resolution.quantity_class().is_none(), "no class was borrowed from a guess");
        let note = resolution.notes.join(" ");
        assert!(note.contains("cccc") && note.contains("dddd"), "{note}");
    }

    #[test]
    fn a_missing_parent_stops_the_chain_and_says_so() {
        let dictionary = Dictionary::from_compact("aaaa\tchild\tzzzz\t\t0\t\n").labelled("the test dictionary");
        let resolver = Resolver::with_dictionary(&NoObjects, &dictionary);
        let resolution = resolver
            .resolve(&PropertyKindRef::Object {
                uuid: "aaaa".into(),
                title: None,
            })
            .unwrap();
        assert_eq!(resolution.chain.len(), 1);
        assert!(
            resolution.notes.iter().any(|note| note.contains("zzzz")),
            "{:?}",
            resolution.notes
        );
    }

    /// The three normative rules, each on a fabricated kind: the corpus
    /// exercises two of them and not the third.
    #[test]
    fn an_abstract_kind_used_directly_is_a_defect() {
        let dictionary = Dictionary::from_compact("aaaa\tproperty\t\tunitless\t1\t\n");
        let resolver = Resolver::with_dictionary(&NoObjects, &dictionary);
        let resolution = resolver
            .resolve(&PropertyKindRef::Object {
                uuid: "aaaa".into(),
                title: None,
            })
            .unwrap();
        let issues = check(&resolution, None);
        assert!(
            matches!(issues.as_slice(), [KindIssue::AbstractKindUsed { .. }]),
            "{issues:?}"
        );
        assert!(issues[0].is_a_defect());
    }

    #[test]
    fn a_unit_of_the_wrong_family_is_a_defect_and_one_of_the_right_family_is_not() {
        let dictionary = Dictionary::from_compact("aaaa\tdepth\t\tlength\t0\t\n");
        let resolver = Resolver::with_dictionary(&NoObjects, &dictionary);
        let resolution = resolver
            .resolve(&PropertyKindRef::Object {
                uuid: "aaaa".into(),
                title: None,
            })
            .unwrap();

        assert!(check(&resolution, Some("m")).is_empty(), "a metre is a length");
        assert!(check(&resolution, Some("ft")).is_empty(), "so is a foot");
        let issues = check(&resolution, Some("Pa"));
        assert!(
            matches!(issues.as_slice(), [KindIssue::UomOutsideQuantityClass { .. }]),
            "{issues:?}"
        );
        // A unit outside the table is reported too, and differently: not
        // knowing a unit is not the same as knowing it is the wrong one.
        let unknown = check(&resolution, Some("exoticMeter"));
        assert!(matches!(
            unknown.as_slice(),
            [KindIssue::UomOutsideQuantityClass { found: None, .. }]
        ));
    }

    #[test]
    fn a_class_this_build_cannot_check_says_so_instead_of_passing_silently() {
        // Most classes are here until the Energistics unit dictionary is
        // vendored, and a silent pass would read as "checked and fine".
        let dictionary = Dictionary::from_compact("aaaa\theat\t\tthermodynamic temperature\t0\t\n");
        let resolver = Resolver::with_dictionary(&NoObjects, &dictionary);
        let resolution = resolver
            .resolve(&PropertyKindRef::Object {
                uuid: "aaaa".into(),
                title: None,
            })
            .unwrap();
        let issues = check(&resolution, Some("degC"));
        assert!(
            matches!(issues.as_slice(), [KindIssue::QuantityClassNotCheckable { .. }]),
            "{issues:?}"
        );
        assert!(!issues[0].is_a_defect(), "a limit of this build is not a defect of the file");
    }

    #[test]
    fn a_deprecated_kind_is_reported_but_its_ancestors_are_not() {
        let dictionary = Dictionary::from_compact(
            "aaaa\tcurrent\tbbbb\tlength\t0\t\n\
             bbbb\told parent\t\tlength\t0\t2020-01-01\n",
        );
        let resolver = Resolver::with_dictionary(&NoObjects, &dictionary);
        let resolution = resolver
            .resolve(&PropertyKindRef::Object {
                uuid: "aaaa".into(),
                title: None,
            })
            .unwrap();
        // The property used a kind that is current; what that kind derives
        // from is the dictionary's business, not the file's.
        assert!(check(&resolution, Some("m")).is_empty(), "{:?}", check(&resolution, Some("m")));
    }

    #[test]
    fn the_built_in_dictionary_is_always_there() {
        // `build.rs` refuses to build without `data/property_kinds.tsv`, so an
        // empty built-in dictionary no longer exists.
        let (count, _label) = builtin();
        assert!(count > 3000, "the Energistics dictionary has 3662 kinds, not {count}");
        assert!(with_default(|d| d.by_title("length").len()) == 1);
    }
}

// ---------------------------------------------------------------------------
// The three normative rules
// ---------------------------------------------------------------------------

/// Something a property does that the technical reference guides forbid or
/// discourage.
///
/// These are **validation**, not reading: each one is a file that is perfectly
/// readable and says something it should not. They live here rather than in
/// the read path because a reader that refused them would make the files
/// unusable, and the whole point is to open them and see what is wrong.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum KindIssue {
    /// A kind marked `IsAbstract` used as a property's kind.
    ///
    /// Common TRG 2.3 §11.9, `PropertyKind.IsAbstract`: *"If the Is Abstract
    /// flag is set, then this entry should be used only as the parent of a real
    /// property"*. An abstract kind names a family, not a measurement.
    AbstractKindUsed {
        /// Its title.
        title: String,
        /// Its uuid, empty for a 2.0.1 standard name.
        uuid: String,
    },
    /// A kind the dictionary marks as no longer to be used.
    Deprecated {
        /// Its title.
        title: String,
        /// The date the dictionary gives.
        date: String,
    },
    /// The property's unit is not of the quantity class its kind declares.
    ///
    /// RESQML TRG 2.2 §7.6: *"it contains a unit of measure, which can be
    /// different from the unit of measure of its property type, but it must be
    /// convertible into this unit"*.
    UomOutsideQuantityClass {
        /// The unit the property declares.
        uom: String,
        /// The quantity class the kind declares.
        class: String,
        /// What family the unit is really in, when this build knows it.
        found: Option<String>,
    },
    /// The rule could not be applied, and saying so beats saying nothing.
    ///
    /// Common TRG §11.9 anticipates it: *"if there is no match in the
    /// Energistics Unit of Measure Dictionary, then this attribute is purely
    /// for human information"*. This build carries four unit families, so most
    /// classes fall here until the unit dictionary is vendored.
    QuantityClassNotCheckable {
        /// The class as the kind declares it.
        class: String,
        /// The unit that could therefore not be checked against it.
        uom: String,
    },
}

impl KindIssue {
    /// Whether this is a departure from the guides rather than a limit of this
    /// build.
    pub fn is_a_defect(&self) -> bool {
        !matches!(self, KindIssue::QuantityClassNotCheckable { .. })
    }
}

impl std::fmt::Display for KindIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KindIssue::AbstractKindUsed { title, uuid } => write!(
                f,
                "its property kind `{title}`{} is abstract; the common technical reference guide \
                 §11.9 says such a kind \"should be used only as the parent of a real property\"",
                if uuid.is_empty() {
                    String::new()
                } else {
                    format!(" ({uuid})")
                }
            ),
            KindIssue::Deprecated { title, date } => {
                write!(f, "its property kind `{title}` was deprecated on {date}")
            }
            KindIssue::UomOutsideQuantityClass { uom, class, found } => write!(
                f,
                "its unit `{uom}` is {} where its property kind declares the quantity class \
                 `{class}`; RESQML TRG §7.6 requires the unit to be convertible into the kind's",
                match found {
                    Some(found) => format!("a {found}"),
                    None => "not a unit this build knows".to_string(),
                }
            ),
            KindIssue::QuantityClassNotCheckable { class, uom } => write!(
                f,
                "`{uom}` against the quantity class `{class}` was not checked: this build converts \
                 four families of unit, and `{class}` is not one of them (the Energistics unit \
                 dictionary is not vendored yet)"
            ),
        }
    }
}

/// The quantity classes this build can actually check a unit against.
///
/// Deliberately short. The real table is the Energistics Unit of Measure
/// Dictionary, which is not in the repository; guessing the rest by matching
/// names would produce confident wrong answers on exactly the classes nobody
/// checks by hand.
fn dimension_of_class(class: &str) -> Option<crate::units::Dimension> {
    use crate::units::Dimension;
    match class.trim().to_ascii_lowercase().as_str() {
        "length" => Some(Dimension::Length),
        "plane angle" | "angle" => Some(Dimension::PlaneAngle),
        "pressure" => Some(Dimension::Pressure),
        "time" => Some(Dimension::Time),
        _ => None,
    }
}

/// Check one property against the three rules the guides state.
///
/// `uom` is the property's own unit, absent for a discrete or boolean
/// property. Returns everything that is wrong, and — as a separate variant —
/// what could not be checked at all.
pub fn check(resolution: &Resolution, uom: Option<&str>) -> Vec<KindIssue> {
    use crate::units::Dimension;

    let mut out = Vec::new();
    let kind = resolution.kind();

    if kind.is_abstract {
        out.push(KindIssue::AbstractKindUsed {
            title: kind.title.clone(),
            uuid: kind.uuid.clone(),
        });
    }
    // The deprecation of an *ancestor* is not the property's problem: it used
    // a kind that is current, and what that kind derives from is the
    // dictionary's business.
    if let Some(date) = &kind.deprecated {
        out.push(KindIssue::Deprecated {
            title: kind.title.clone(),
            date: date.clone(),
        });
    }

    if let (Some(uom), Some(class)) = (uom, resolution.quantity_class()) {
        match dimension_of_class(class) {
            Some(expected) => {
                let found = Dimension::of(uom);
                if found != Some(expected) {
                    out.push(KindIssue::UomOutsideQuantityClass {
                        uom: uom.to_string(),
                        class: class.to_string(),
                        found: found.map(|d| format!("{d:?}").to_lowercase()),
                    });
                }
            }
            None => out.push(KindIssue::QuantityClassNotCheckable {
                class: class.to_string(),
                uom: uom.to_string(),
            }),
        }
    }
    out
}
