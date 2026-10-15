//! Runtime type registry.
//!
//! Lets callers create and read objects by name, which is what a generic UI and
//! an EPC reader both need: `[Content_Types].xml` gives a content type string,
//! and the registry turns it into a live object.
//!
//! Registration is explicit - each generated crate exposes a `register`
//! function - rather than link-time collection. Link-time tricks are fragile
//! under `wasm32` and make the set of known types depend on linker behaviour.

use std::collections::HashMap;

use quick_xml::events::BytesStart;

use crate::error::{Error, Result};
use crate::identifiers::QualifiedType;
use crate::identity::{ObjectKey, parse_content_type};
use crate::naming;
use crate::reflect::TypeInfo;
use crate::relations::{RelationsIndex, TypeRelations};
use crate::validate::Validate;
use crate::value::Reflect;
use crate::xml::{XmlDeserialize, XmlReader, XmlSerialize};

/// Everything the dynamic layers need from a generated object.
///
/// `Send + Sync` is part of the bargain: an EnergyML object is plain data, and
/// a store that holds one must stay usable from an `async` task whose future
/// has to be `Send`. Every generated type satisfies it without trying.
pub trait DynObject: Reflect + XmlSerialize + Validate + Send + Sync {
    /// Reflection view.
    fn as_reflect(&self) -> &dyn Reflect;
    /// Mutable reflection view.
    fn as_reflect_mut(&mut self) -> &mut dyn Reflect;
    /// Owned reflection view, for handing a freshly built object to a setter.
    ///
    /// `Box<dyn DynObject>` to `Box<dyn Reflect>` is trait upcasting, stable
    /// only from Rust 1.86; this crate supports 1.85, so the conversion is
    /// spelled out in the blanket impl where the concrete type is still known.
    fn into_reflect(self: Box<Self>) -> Box<dyn Reflect>;
    /// A deep copy, without naming the type.
    ///
    /// What an in-memory layer over a package needs: it holds the modified
    /// objects, and every caller asking for one gets its own. Every generated
    /// type derives `Clone`, so this costs nothing at the definition site.
    fn clone_box(&self) -> Box<dyn DynObject>;
    /// Serialization view.
    fn as_xml(&self) -> &dyn XmlSerialize;
    /// Validation view.
    fn as_validate(&self) -> &dyn Validate;
}

impl<T: Reflect + XmlSerialize + Validate + Clone + Send + Sync> DynObject for T {
    fn as_reflect(&self) -> &dyn Reflect {
        self
    }
    fn as_reflect_mut(&mut self) -> &mut dyn Reflect {
        self
    }
    fn into_reflect(self: Box<Self>) -> Box<dyn Reflect> {
        self
    }
    fn clone_box(&self) -> Box<dyn DynObject> {
        Box::new(self.clone())
    }
    fn as_xml(&self) -> &dyn XmlSerialize {
        self
    }
    fn as_validate(&self) -> &dyn Validate {
        self
    }
}

/// How to build and read one registered type.
#[derive(Clone, Copy)]
pub struct TypeEntry {
    /// Static description.
    pub info: &'static TypeInfo,
    /// Build an empty instance, for dynamic construction from a UI.
    pub make_default: fn() -> Box<dyn DynObject>,
    /// Read an instance whose start tag has already been consumed.
    pub read_xml: fn(&mut XmlReader<'_>, &BytesStart<'_>) -> Result<Box<dyn DynObject>>,
}

impl TypeEntry {
    /// Build an entry for a generated type.
    ///
    /// One shape whether or not the `json` feature is on: JSON is written from
    /// reflection (see [`crate::json`]), not from serde, so nothing about the
    /// concrete type has to be captured here for it.
    pub fn of<T>() -> TypeEntry
    where
        T: DynObject + XmlDeserialize + Default + crate::reflect::EnergymlType + 'static,
    {
        TypeEntry {
            info: <T as crate::reflect::EnergymlType>::type_info(),
            make_default: || Box::new(T::default()),
            read_xml: |r, start| Ok(Box::new(T::xml_read(r, start)?)),
        }
    }
}

/// A set of known types, usually one per generated crate, merged together.
#[derive(Default)]
pub struct Registry {
    entries: Vec<TypeEntry>,
    by_qualified_type: HashMap<String, usize>,
    by_content_type: HashMap<String, usize>,
    by_package_version_name: HashMap<(String, String, String), usize>,
    /// Every version of one concept, keyed by `type_key` lowercased. This is
    /// what makes "all the versions of Activity" an O(1) lookup instead of a
    /// scan, and it is the index a version translator works from.
    by_type_key: HashMap<String, Vec<usize>>,
    /// Versions the project knows and this build does not carry, each with the
    /// sentence that refuses it. See [`Registry::declare_absent`].
    absent: Vec<AbsentVersion>,
    /// The containment and reference tables of the registered crates. See
    /// [`Registry::register_relations`].
    relations: RelationsIndex,
}

/// One schema version that exists but is not registered in this build.
struct AbsentVersion {
    /// The package, lower case: `resqml`.
    package: String,
    /// Every compact spelling of the release, dots removed: `20` and `201` for
    /// RESQML 2.0.1.
    compact: Vec<String>,
    /// The development drop, when the version is one (`dev3`).
    prerelease: Option<String>,
    /// The refusal, as the caller worded it.
    why: String,
}

impl AbsentVersion {
    /// Whether this row is what `(package, version)` names, in any spelling.
    fn names(&self, package: &str, version: &str) -> bool {
        if !self.package.eq_ignore_ascii_case(package.trim()) {
            return false;
        }
        let (release, pre) = naming::split_prerelease(version);
        let same_drop = match (pre, self.prerelease.as_deref()) {
            (None, None) => true,
            (Some(a), Some(b)) => naming::prerelease_eq(a, b),
            _ => false,
        };
        same_drop
            && self
                .compact
                .iter()
                .any(|c| *c == naming::compact_version(release))
    }
}

/// How much a reader is willing to put right.
///
/// Two questions, not one, and telling them apart is what lets a tool read a
/// real package without a tool that writes one inventing data.
///
/// | | absent required item | value present and unreadable |
/// |---|---|---|
/// | [`Strict`][ReadMode::Strict] | refuses | refuses |
/// | [`Tolerant`][ReadMode::Tolerant] | default, reported | refuses |
/// | [`Lenient`][ReadMode::Lenient] | default, reported | default, reported |
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ReadMode {
    /// Only what the document says. What a caller that will **write the object
    /// back** wants, and what the round-trip check uses.
    Strict,
    /// An item the document does not carry takes its default and is reported.
    /// A value that is there and unreadable still refuses.
    ///
    /// The default, because silence and nonsense are different news: a producer
    /// that omits `Citation.Originator` has written a package whose geometry,
    /// references and titles are all intact, and refusing it hands the caller
    /// nothing. A producer that writes `hello` where a number belongs has
    /// written something, and dropping it loses what the file said.
    #[default]
    Tolerant,
    /// Both are defaulted and reported.
    Lenient,
}

/// Everything a read had to put in the document's place.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Departures {
    /// Required items that were absent and were filled with a default.
    pub defaulted: Vec<crate::xml::Defaulted>,
    /// Values that were present and unreadable, and what replaced them.
    pub repaired: Vec<crate::xml::Repaired>,
}

impl Departures {
    /// Whether the document needed nothing at all.
    pub fn is_empty(&self) -> bool {
        self.defaulted.is_empty() && self.repaired.is_empty()
    }

    /// Every departure as one line of text, cardinality first.
    pub fn lines(&self) -> Vec<String> {
        self.defaulted
            .iter()
            .map(ToString::to_string)
            .chain(self.repaired.iter().map(ToString::to_string))
            .collect()
    }
}

/// Put `idx` under `key`, unless that would let a development drop displace a
/// release already holding it.
///
/// Free rather than a method so it borrows one map and the entry list, not the
/// whole `Registry`.
fn claim(map: &mut HashMap<String, usize>, key: String, idx: usize, entries: &[TypeEntry], is_drop: bool) {
    if is_drop
        && let Some(held) = map.get(&key)
        && !naming::is_prerelease(entries[*held].info.version)
    {
        return;
    }
    map.insert(key, idx);
}

/// The read itself, shared by every entry point so the modes cannot drift.
fn read_entry(
    registry: &Registry,
    entry: &TypeEntry,
    bytes: &[u8],
    mode: ReadMode,
) -> Result<(Box<dyn DynObject>, Departures)> {
    let mut cursor = bytes;
    let mut reader = XmlReader::new(&mut cursor);
    reader = match mode {
        ReadMode::Strict => reader.strict(),
        ReadMode::Tolerant => reader,
        ReadMode::Lenient => reader.lenient(),
    };
    let root = reader.read_root()?;
    let entry = registry.substitute(entry, &root)?;
    let object = (entry.read_xml)(&mut reader, &root)?;
    Ok((
        object,
        Departures {
            defaulted: reader.take_defaulted(),
            repaired: reader.take_repaired(),
        },
    ))
}

impl Registry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add one type. Later registrations of the same key win, which lets a
    /// caller override a generated type with its own - with one exception, and
    /// it is load-bearing: **a development drop never displaces a release.**
    ///
    /// `resqml 2.2_dev3` computes the *same* qualified type as `resqml 2.2`.
    /// The drop's schemas declare `version="2.2"`, so `resqml22.Activity` names
    /// a type in both crates while these maps hold one index per key. Left to
    /// registration order, adding the drop would silently re-point every
    /// `resqml22.` lookup - every DOR resolution, every JSON `$type` - of the
    /// *released* 2.2 at the drop's types, and nothing would fail: the objects
    /// are plausible, and mostly identical. Which release a document is read as
    /// is decided by the reader, over ordered candidates, not by whoever called
    /// `register` last.
    pub fn register(&mut self, entry: TypeEntry) {
        let idx = self.entries.len();
        let info = entry.info;
        self.entries.push(entry);

        // Every type has a qualified type, not only the top-level ones:
        // `eml20.Citation` names a nested type as surely as
        // `resqml20.obj_Activity` names an EPC part, and JSON `$type` uses both.
        // The declared one is written second so it wins where the two differ.
        let drop = naming::is_prerelease(info.version);
        claim(
            &mut self.by_qualified_type,
            normalize_qualified_type(&computed_qualified_type(info)),
            idx,
            &self.entries,
            drop,
        );
        if let Some(qt) = info.qualified_type {
            claim(
                &mut self.by_qualified_type,
                normalize_qualified_type(qt),
                idx,
                &self.entries,
                drop,
            );
        }
        if let Some(ct) = info.content_type {
            claim(
                &mut self.by_content_type,
                normalize_content_type(ct),
                idx,
                &self.entries,
                drop,
            );
        }
        // A release whose version and schema version differ answers to both
        // spellings. RESQML 2.0.1 stamps `schemaVersion="2.0"`, so its
        // qualified type is `resqml20.…` - and fesapi writes
        // `version=2.0.1` in some of its references (an activity's
        // `ActivityDescriptor`), which compacts to `resqml201.…`. Either
        // spelling names this type; refusing the second would leave a
        // reference the writer meant unresolved.
        if naming::compact_version(info.version) != naming::compact_version(info.schema_version) {
            claim(
                &mut self.by_qualified_type,
                normalize_qualified_type(&format!(
                    "{}{}.{}",
                    info.package,
                    naming::compact_version(info.version),
                    info.object_type
                )),
                idx,
                &self.entries,
                drop,
            );
            if let Some(ct) = info.content_type
                && let Ok(parts) = parse_content_type(ct)
            {
                claim(
                    &mut self.by_content_type,
                    normalize_content_type(&format!(
                        "application/x-{}+xml;version={};type={}",
                        parts.domain, info.version, parts.object_type
                    )),
                    idx,
                    &self.entries,
                    drop,
                );
            }
        }
        self.by_package_version_name.insert(
            (
                info.package.to_ascii_lowercase(),
                info.version.to_string(),
                info.name.to_ascii_lowercase(),
            ),
            idx,
        );
        self.by_type_key
            .entry(info.type_key.to_ascii_lowercase())
            .or_default()
            .push(idx);
    }

    /// Every registered type sharing one concept, whatever the package version
    /// and whatever the `obj_` prefix: `types_by_key("Activity")` returns both
    /// `resqml20.obj_Activity` and `resqml22.Activity`.
    pub fn types_by_key(&self, type_key: &str) -> Vec<&'static TypeInfo> {
        let key = naming::strip_obj_prefix(type_key.trim()).to_ascii_lowercase();
        self.by_type_key
            .get(&key)
            .map(|idx| idx.iter().map(|i| self.entries[*i].info).collect())
            .unwrap_or_default()
    }

    /// Tolerant lookup for user input and command lines: `activity`,
    /// `Activity` and `obj_Activity` all find the same types.
    ///
    /// Deliberately separate from the exact lookups - being loose is a
    /// decision the caller makes, never a default.
    pub fn find_type_loose(&self, name: &str) -> Vec<&'static TypeInfo> {
        self.types_by_key(name)
    }

    /// Build a reference to `key` in one eml version, without naming its Rust
    /// type - the dynamic counterpart of `DataObjectReference::from_key`.
    ///
    /// The version decides which fields exist, so this is what a UI or a
    /// translator uses when the target version is only known at runtime.
    pub fn make_object_ref(
        &self,
        package: &str,
        version: &str,
        key: &ObjectKey<'_>,
    ) -> Result<Box<dyn DynObject>> {
        let entry = self
            .object_ref_type(package, version)
            .ok_or_else(|| Error::UnknownType(format!("{package}/{version} DataObjectReference")))?;
        let mut object = (entry.make_default)();
        crate::identity::set_target_reflect(object.as_reflect_mut(), key)?;
        Ok(object)
    }

    /// The reference type of one package version, if it is registered.
    pub fn object_ref_type(&self, package: &str, version: &str) -> Option<&TypeEntry> {
        self.entries.iter().find(|e| {
            e.info.is_object_ref
                && e.info.package.eq_ignore_ascii_case(package)
                && naming::version_eq(e.info.version, version)
        })
    }

    /// Every registered type, in registration order.
    pub fn types(&self) -> impl Iterator<Item = &'static TypeInfo> + '_ {
        self.entries.iter().map(|e| e.info)
    }

    /// Every registered top-level object type - what a "new object" menu lists.
    ///
    /// Includes the types [`TypeInfo::is_nestable`] flags: they are legal EPC
    /// parts too, and whether to show a type that *can* be inlined is the
    /// caller's decision - `.filter(|i| !i.is_nestable)` when listing only the
    /// ones that are never children.
    pub fn top_level_types(&self) -> impl Iterator<Item = &'static TypeInfo> + '_ {
        self.entries.iter().map(|e| e.info).filter(|i| i.is_top_level)
    }

    /// Add the relations tables of one generated crate: for each of its
    /// top-level types, where it holds data objects by value and (WITSML) where
    /// it writes references. Called by the crate's `register`, after its types.
    ///
    /// Registering the same tables twice changes nothing, and a development
    /// drop never displaces a release in the by-qualified-type lookups, for the
    /// reason [`Registry::register`] gives.
    pub fn register_relations(&mut self, tables: &'static [TypeRelations]) {
        self.relations.add(tables);
    }

    /// The relations table of a registered type, if its crate has one.
    ///
    /// Keyed on package, version and name, so a release and its development
    /// drop have a table each.
    pub fn relations_of(&self, info: &TypeInfo) -> Option<&'static TypeRelations> {
        self.relations.of(info)
    }

    /// The relations table of a type by qualified type (`witsml21.Log`), in the
    /// spellings the other lookups accept. A release wins over a development
    /// drop that shares the qualified type.
    pub fn relations_for(&self, qualified_type: &str) -> Option<&'static TypeRelations> {
        self.relations.for_qualified(qualified_type)
    }

    /// Whether the type holds at least one data object by value: a hash lookup
    /// and a length test, no table is read. False for a type with no table.
    pub fn is_container(&self, qualified_type: &str) -> bool {
        self.relations_for(qualified_type)
            .is_some_and(|t| t.is_container())
    }

    /// Whether some registered slot admits the type as a contained data object.
    /// A hash lookup. A subset of [`TypeInfo::is_nestable`]: that flag says the
    /// type is reached by some field, this one that it is reached by a slot of
    /// a type that is itself a part.
    pub fn may_be_contained(&self, qualified_type: &str) -> bool {
        self.relations.may_be_contained(qualified_type)
    }

    /// Every registered table, in registration order.
    pub fn relations(&self) -> impl Iterator<Item = &'static TypeRelations> + '_ {
        self.relations.iter()
    }

    /// Look up by qualified type, e.g. `resqml22.TriangulatedSetRepresentation`.
    pub fn by_qualified_type(&self, qualified_type: &str) -> Option<&TypeEntry> {
        self.by_qualified_type
            .get(&normalize_qualified_type(qualified_type))
            .map(|i| &self.entries[*i])
    }

    /// Look up by EPC content type. Whitespace and `obj_` prefixes are ignored,
    /// because both spellings occur in real EPC files.
    pub fn by_content_type(&self, content_type: &str) -> Option<&TypeEntry> {
        self.by_content_type
            .get(&normalize_content_type(content_type))
            .map(|i| &self.entries[*i])
    }

    /// A type by name, **anywhere the model can be reached from here** -
    /// registered or not.
    ///
    /// # Why a lookup is not enough
    ///
    /// The registry holds what can be a *part* of a package: top-level objects.
    /// Everything else hangs off their fields - `Citation`, `TrianglePatch`,
    /// and **every enumeration**, which is registered nowhere on purpose since
    /// nothing is ever a part of one. A caller asking about one of those gets
    /// `None` from every `by_*` method here, and the honest answer is not "this
    /// build does not know that type": it knows it perfectly well, it simply
    /// does not index it.
    ///
    /// So the fallback walks out from the registered types through fields,
    /// choice variants, subtypes and bases, visiting each type once - the model
    /// is deeply cyclic, and a walk that did not remember would not terminate.
    ///
    /// # Two spellings, and telling them apart by the dot
    ///
    /// A command line says `TrianglePatch`; a front end says
    /// `resqml22.TrianglePatch`, because that is what a document writes as
    /// `$type` and the only name that is unambiguous across releases. Both have
    /// to work, and mixing the two rules would let a bare match in one release
    /// answer a question asked about another. A name containing a dot is
    /// therefore matched as a qualified type and nothing else.
    ///
    /// This lived in `energyml-cli` until 04/08, which meant the command line
    /// could describe a type the web could not - the CLI knowing something the
    /// library does not, which is a bug in the layering rather than a feature
    /// of the CLI.
    pub fn find_type(&self, name: &str) -> Option<&'static TypeInfo> {
        if let Some(entry) = self.by_qualified_type(name) {
            return Some(entry.info);
        }
        let name = name.trim();
        if name.is_empty() {
            return None;
        }
        if name.contains('.') {
            let wanted = normalize_qualified_type(name);
            self.walk(|info| normalize_qualified_type(&computed_qualified_type(info)) == wanted)
        } else {
            self.walk(|info| info.name.eq_ignore_ascii_case(name) || info.xml_name.eq_ignore_ascii_case(name))
        }
    }

    /// The first type reachable from the registry that satisfies `wanted`.
    ///
    /// Breadth-first from every registered type, each visited once. Identity is
    /// by pointer: `TypeInfo` is a `static`, so two references to the same type
    /// are the same address, and comparing names would conflate the `Activity`
    /// of `resqml20` with the one of `eml23`.
    fn walk(&self, wanted: impl Fn(&'static TypeInfo) -> bool) -> Option<&'static TypeInfo> {
        let mut seen: Vec<&'static TypeInfo> = Vec::new();
        let mut queue: Vec<&'static TypeInfo> = self.types().collect();
        while let Some(info) = queue.pop() {
            if seen.iter().any(|known| std::ptr::eq(*known, info)) {
                continue;
            }
            seen.push(info);
            if wanted(info) {
                return Some(info);
            }
            for field in info.fields {
                if let Some(inner) = field.ty.type_info() {
                    queue.push(inner);
                }
            }
            for variant in info.variants {
                if let Some(ty) = variant.ty {
                    queue.push(ty());
                }
            }
            for subtype in info.subtypes {
                queue.push(subtype());
            }
            if let Some(base) = info.base {
                queue.push(base());
            }
        }
        None
    }

    /// Look up by package, version and type name.
    pub fn by_name(&self, package: &str, version: &str, name: &str) -> Option<&TypeEntry> {
        self.by_package_version_name
            .get(&(
                package.to_ascii_lowercase(),
                version.to_string(),
                name.to_ascii_lowercase(),
            ))
            .map(|i| &self.entries[*i])
    }

    /// The entry a root is read as: `entry` itself, or - when the root's
    /// `xsi:type` names one of its subtypes - that subtype.
    ///
    /// A part declared as the element's type (`type=FlowTestActivity`) whose
    /// root says `xsi:type="prodml:InjectionFlowTest"` **is** the injection
    /// test; read as the declared type, everything the subtype adds would be
    /// dropped without a word. See [`TypeInfo::root_element`]. An `xsi:type` naming
    /// the declared type itself changes nothing, and one naming no subtype of
    /// it is refused by name: the fallback exists for documents that write
    /// none, not to paper over a type nobody knows.
    fn substitute<'e>(&'e self, entry: &'e TypeEntry, root: &BytesStart<'_>) -> Result<&'e TypeEntry> {
        let Some(named) = crate::xml::xsi_type(root)? else {
            return Ok(entry);
        };
        let local = naming::strip_obj_prefix(crate::xml::local_part(&named));
        // Any of the type's own names: producers write the **complex type**
        // there as often as not - `h+g_PaleoGTM.epc` roots a
        // `<resqml2:LocalDepth3dCrs>` with `xsi:type="resqml2:obj_LocalDepth3dCrs"`,
        // the type naming itself.
        let names = |info: &'static TypeInfo| {
            [info.xml_name, info.name, info.object_type]
                .iter()
                .any(|name| naming::strip_obj_prefix(name).eq_ignore_ascii_case(local))
        };
        if names(entry.info) {
            return Ok(entry);
        }
        // `TypeInfo::subtypes` is not filled for structs, so the subtype is
        // found the other way: a registered type whose `base` chain reaches
        // the declared one. Identity by pointer - every `TypeInfo` is a static.
        let descends = |info: &'static TypeInfo| {
            let mut at = info.base.map(|base| base());
            while let Some(ancestor) = at {
                if std::ptr::eq(ancestor, entry.info) {
                    return true;
                }
                at = ancestor.base.map(|base| base());
            }
            false
        };
        if let Some(found) = self.entries.iter().find(|candidate| {
            candidate.info.package == entry.info.package
                && candidate.info.version == entry.info.version
                && names(candidate.info)
                && descends(candidate.info)
        }) {
            return Ok(found);
        }
        Err(Error::UnknownType(format!(
            "the root says `xsi:type=\"{named}\"`, which is no subtype of `{}`",
            entry.info.qualified_type.unwrap_or(entry.info.name)
        )))
    }

    /// Say that a schema version **exists but is not registered here**, and why.
    ///
    /// The registry cannot tell an absent type from a type nobody ever heard of:
    /// both come back `None`. The caller that built it knows the difference - it
    /// chose not to compile a version it ships - and this is where it says so.
    /// A reader that finds a document declaring such a version **refuses it by
    /// `why`** instead of reading it as another release of the same package (a
    /// 2.0.1 part read as 2.2 does not fail, it yields a plausible object holding
    /// the fields the two share). A version never declared here stays an unknown
    /// one, and keeps the reader's fallbacks.
    ///
    /// `version` is the registry's own spelling (`2.0.1`, `2.2_dev3`); `compact`
    /// adds the other compact spellings a qualified type may carry (`20` for
    /// RESQML 2.0.1, whose `schemaVersion` is `2.0`). Pure data: nothing here
    /// registers a type or depends on a schema crate.
    pub fn declare_absent(&mut self, package: &str, version: &str, compact: &[&str], why: impl Into<String>) {
        let (release, prerelease) = naming::split_prerelease(version);
        let mut spellings = vec![naming::compact_version(release)];
        spellings.extend(compact.iter().map(|c| naming::compact_version(c)));
        spellings.dedup();
        self.absent.push(AbsentVersion {
            package: package.trim().to_ascii_lowercase(),
            compact: spellings,
            prerelease: prerelease.map(str::to_string),
            why: why.into(),
        });
    }

    /// The refusal for `(package, version)` when that version was declared
    /// absent - `2.0` and `2.0.1` are one version, `2.2` the release and
    /// `2.2_dev3` the drop.
    pub fn absent(&self, package: &str, version: &str) -> Option<&str> {
        self.absent
            .iter()
            .find(|row| row.names(package, version))
            .map(|row| row.why.as_str())
    }

    /// [`Registry::absent`] for a qualified type (`resqml20.obj_Activity`,
    /// `resqml201.obj_Activity`). A qualified type cannot name a development
    /// drop, so only releases answer.
    pub fn absent_qualified_type(&self, qualified_type: &str) -> Option<&str> {
        let parts = crate::identity::parse_qualified_type(qualified_type).ok()?;
        self.absent
            .iter()
            .find(|row| {
                row.prerelease.is_none()
                    && row.package.eq_ignore_ascii_case(parts.domain)
                    && row.compact.iter().any(|c| c == parts.version)
            })
            .map(|row| row.why.as_str())
    }

    /// [`Registry::absent`] for a content type
    /// (`application/x-resqml+xml;version=2.0.1;type=obj_Activity`).
    pub fn absent_content_type(&self, content_type: &str) -> Option<&str> {
        let parts = parse_content_type(content_type).ok()?;
        self.absent(parts.domain, parts.version)
    }

    /// The error for a qualified type nothing registered answers to: the named
    /// refusal when the version was declared absent, the unknown name otherwise.
    fn missing_qualified_type(&self, qualified_type: &str) -> Error {
        match self.absent_qualified_type(qualified_type) {
            Some(why) => Error::VersionAbsent(why.to_string()),
            None => Error::UnknownType(qualified_type.to_string()),
        }
    }

    /// The same for a content type.
    fn missing_content_type(&self, content_type: &str) -> Error {
        match self.absent_content_type(content_type) {
            Some(why) => Error::VersionAbsent(why.to_string()),
            None => Error::UnknownType(content_type.to_string()),
        }
    }

    /// Create an empty instance of a type given its qualified type.
    pub fn create(&self, qualified_type: &str) -> Result<Box<dyn DynObject>> {
        let entry = self
            .by_qualified_type(qualified_type)
            .ok_or_else(|| self.missing_qualified_type(qualified_type))?;
        Ok((entry.make_default)())
    }

    /// Read an object from bytes, using its content type to pick the concrete
    /// type. This is the entry point an EPC reader uses per part.
    pub fn read_xml(&self, content_type: &str, bytes: &[u8]) -> Result<Box<dyn DynObject>> {
        Ok(self.read_xml_as(content_type, bytes, ReadMode::Tolerant)?.0)
    }

    /// Read an object in a chosen mode, with everything the mode had to put
    /// there.
    ///
    /// The one entry point the three others are written in terms of, so the
    /// modes cannot drift apart.
    pub fn read_xml_as(
        &self,
        content_type: &str,
        bytes: &[u8],
        mode: ReadMode,
    ) -> Result<(Box<dyn DynObject>, Departures)> {
        let entry = self
            .by_content_type(content_type)
            .ok_or_else(|| self.missing_content_type(content_type))?;
        read_entry(self, entry, bytes, mode)
    }

    /// Read an object **as a named type**, rather than as whatever a content
    /// type happens to resolve to.
    ///
    /// The difference only shows when two registered types share a content
    /// type, and exactly one thing makes them: a development drop and the
    /// release it precedes. `resqml 2.2_dev3` declares `version="2.2"`, so it
    /// computes `application/x-resqml+xml;version=2.2;type=BoundaryFeature` -
    /// the released 2.2's own string.
    ///
    /// A caller walking ordered candidates must dispatch on the candidate, not
    /// on that string. Going back through [`Registry::by_content_type`] hands
    /// every candidate of such a pair the *same* entry, so the walk still runs,
    /// still reports a fallback, and still reads the same type each time -
    /// which looks exactly like a walk that worked. Measured: with the drop
    /// registered and the read dispatched by content type, a document the
    /// release cannot parse came back as the release anyway.
    pub fn read_xml_as_named(
        &self,
        info: &'static TypeInfo,
        bytes: &[u8],
        mode: ReadMode,
    ) -> Result<(Box<dyn DynObject>, Departures)> {
        let entry = self
            .by_name(info.package, info.version, info.name)
            .ok_or_else(|| Error::UnknownType(format!("{} {} {}", info.package, info.version, info.name)))?;
        read_entry(self, entry, bytes, mode)
    }

    /// Read an object, filling absent required items with their defaults
    /// rather than refusing the part. See [`crate::xml::XmlReader::lenient`].
    ///
    /// The second half of the pair is what had to be invented. It is returned,
    /// never logged here: this crate has no opinion on where a diagnostic
    /// goes, and the storage layer above turns each one into a `Diagnostic`.
    pub fn read_xml_lenient(
        &self,
        content_type: &str,
        bytes: &[u8],
    ) -> Result<(Box<dyn DynObject>, Vec<crate::xml::Defaulted>)> {
        let (object, departures) = self.read_xml_repairing(content_type, bytes)?;
        Ok((object, departures.defaulted))
    }

    /// The same, with **everything** leniency had to do - what was invented
    /// for an absent item, and what was replaced for a value that was there
    /// and wrong.
    ///
    /// Two lists rather than one, because the two are different news: a
    /// missing element is a producer forgetting, a value of `hello` where a
    /// number belongs is a producer writing something wrong, and only the
    /// second carries a string worth showing a person.
    pub fn read_xml_repairing(
        &self,
        content_type: &str,
        bytes: &[u8],
    ) -> Result<(Box<dyn DynObject>, Departures)> {
        self.read_xml_as(content_type, bytes, ReadMode::Lenient)
    }

    /// The entry describing an object's concrete type.
    pub fn entry_of(&self, object: &dyn Reflect) -> Option<&TypeEntry> {
        let info = object.type_info();
        self.by_name(info.package, info.version, info.name)
    }

    /// Write any registered object as the EnergyML JSON encoding.
    ///
    /// A convenience over [`crate::json::write_string`]; the encoding needs no
    /// registry at all, since reflection carries everything it wants.
    #[cfg(feature = "json")]
    pub fn write_json(&self, object: &dyn Reflect, pretty: bool) -> Result<String> {
        crate::json::write_string(object, pretty)
    }

    /// Read an object from the EnergyML JSON encoding.
    ///
    /// No qualified type is passed in: `$type` is in the document, which is the
    /// whole point of the encoding. Objects carried under `_data` are dropped
    /// here - use [`Registry::read_json_document`] to keep them.
    #[cfg(feature = "json")]
    pub fn read_json(&self, text: &str) -> Result<Box<dyn DynObject>> {
        Ok(crate::json::read_string(text, self)?.root)
    }

    /// Read a JSON document, keeping the objects its references carried.
    #[cfg(feature = "json")]
    pub fn read_json_document(&self, text: &str) -> Result<crate::json::JsonDocument> {
        crate::json::read_string(text, self)
    }
}

/// Serialize an object as a standalone EPC part.
///
/// Everything the low-level [`crate::xml::to_xml_string`] asks for - the tag,
/// its namespace, the declarations to put on the root - is already in
/// [`TypeInfo`], so a part writer should not have to spell them again. This is
/// the call an EPC or ETP writer makes once per object.
pub fn to_part(object: &dyn DynObject, opts: crate::xml::XmlWriteOptions) -> Result<String> {
    let info = object.as_reflect().type_info();
    let namespace = crate::ns::Namespace::well_known(info.package)
        .ok_or_else(|| Error::UnknownType(format!("no namespace for package `{}`", info.package)))?;

    // `eml` is declared for the inherited citation fields, `xsi` for the
    // `xsi:type` of any substituted child. Declaring the same one twice would
    // be invalid, so the package's own namespace goes in only once.
    let mut declare = vec![namespace];
    for extra in [crate::ns::EML, crate::ns::XSI] {
        if !declare.iter().any(|n| n.uri == extra.uri) {
            declare.push(extra);
        }
    }

    // **A type with no global element of its own is written under its
    // ancestor's, saying what it is** - see [`TypeInfo::root_element`].
    match info.root_element {
        Some(element) => {
            let typed = Substituted {
                inner: object.as_xml(),
                xsi_type: format!("{}:{}", namespace.prefix, info.xml_name),
            };
            crate::xml::to_xml_string(&typed, namespace, element, &declare, opts)
        }
        None => crate::xml::to_xml_string(object.as_xml(), namespace, info.xml_name, &declare, opts),
    }
}

/// An object written under another element's name, saying what it is.
///
/// PRODML 2.3 declares a global element for `FlowTestActivity` and for none of
/// its subtypes, though its documentation says *"This object is abstract; you
/// must choose one of the available types"* (FlowTestActivity.xsd l. 186-190).
/// A document can only name an `InjectionFlowTest` as
/// `<prodml:FlowTestActivity xsi:type="prodml:InjectionFlowTest">` - the form
/// of Energistics' own example (`xml_examples/FlowTest/FlowTestActivity.xml`
/// l. 2). The part's **identity** stays the concrete type - content type
/// `type=InjectionFlowTest`, qualified type `prodml23.InjectionFlowTest`:
/// the project's rule, the specifications being silent on it. Readers accept
/// both declarations (see [`Registry::read_xml_as`]).
struct Substituted<'a> {
    inner: &'a dyn XmlSerialize,
    xsi_type: String,
}

impl XmlSerialize for Substituted<'_> {
    fn xml_attributes(&self, tag: &mut quick_xml::events::BytesStart<'_>) -> Result<()> {
        crate::xml::push_xsi_type(tag, &self.xsi_type);
        self.inner.xml_attributes(tag)
    }

    fn xml_children(&self, w: &mut crate::xml::XmlWriter<'_>) -> Result<()> {
        self.inner.xml_children(w)
    }

    fn xml_text(&self) -> Option<std::borrow::Cow<'_, str>> {
        self.inner.xml_text()
    }
}

/// Lookup key for a qualified type, tolerant in the same way and for the same
/// reason as [`normalize_content_type`].
/// The qualified type of any type, including the nested ones the generator does
/// not stamp with one. Kept in step with [`crate::json::qualified_type_of`],
/// which is the same rule - duplicated only so the `json` feature can be off.
fn computed_qualified_type(info: &'static TypeInfo) -> String {
    format!(
        "{}{}.{}",
        info.package,
        naming::compact_version(info.schema_version),
        info.object_type
    )
}

pub(crate) fn normalize_qualified_type(value: &str) -> String {
    match QualifiedType::parse(value) {
        Ok(qt) => format!(
            "{}|{}|{}",
            qt.domain.to_ascii_lowercase(),
            qt.version,
            naming::strip_obj_prefix(&qt.object_type).to_ascii_lowercase()
        ),
        Err(_) => value.trim().to_ascii_lowercase(),
    }
}

/// Lookup key for a content type.
///
/// Writers disagree about whitespace around `;` and about the `obj_` prefix, so
/// matching is deliberately tolerant - but only here, on the way *in*. What
/// gets written back out is always the verbatim string from `TypeInfo`.
fn normalize_content_type(value: &str) -> String {
    match parse_content_type(value) {
        Ok(parts) => format!(
            "{}|{}|{}",
            parts.domain.to_ascii_lowercase(),
            naming::compact_version(parts.version),
            naming::strip_obj_prefix(parts.object_type).to_ascii_lowercase()
        ),
        Err(_) => value.trim().to_ascii_lowercase(),
    }
}
