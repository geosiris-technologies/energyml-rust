//! Static facts about where one data object holds another, or points at one.
//!
//! A [`TypeInfo`] says what a type is made of. It does not say, without a walk
//! over every field, **where in a document** a data object of another type may
//! sit inside this one, or where its references to other objects are written.
//! Those two questions are asked at open time on raw bytes, before any object
//! is built, so the answer has to be a plain table of element names. This
//! module holds the types of that table and the index a
//! [`Registry`](crate::registry::Registry) keeps it
//! in; the tables themselves are written by the generator, one `relations.rs`
//! per generated crate, from the same schema that produced the types.
//!
//! # Two kinds of slot
//!
//! - A [`ContainedSlot`] is a place where a type holds a **data object by
//!   value**: an element whose type is itself a data object with a `uuid`, and
//!   not a reference. A WITSML `Log` holds its `ChannelSet`s that way.
//! - A [`DorSlot`] is a place where a type writes a **reference** to a data
//!   object (a `DataObjectReference`), with the cardinality the schema gives it.
//!
//! A slot is reached by a [`Step`] path from the object that carries it. The
//! path crosses the nested, non-object elements in between
//! (`JobPerforationClusters/StimPerforationCluster`) and, where the schema
//! lets a subtype stand in for a declared type, says which one by `xsi:type`.
//!
//! # What the table does not say
//!
//! - **No transitive closure.** A slot says "here is a `ChannelSet`"; that a
//!   `ChannelSet` holds `Channel`s is said by the `ChannelSet` entry. A reader
//!   composes the two, which also covers a recursive container.
//! - **No policy.** Whether a relationship is primary or secondary is a
//!   decision of the exchange protocol, not a fact of the schema, and lives
//!   with the storage layer.
//! - **No second source of truth.** The same facts can be recomputed by walking
//!   [`TypeInfo::fields`]; the table is the form a byte-level scanner can use,
//!   and a test holds the two against each other.

use std::collections::{HashMap, HashSet};

use crate::naming;
use crate::reflect::TypeInfo;
use crate::registry::normalize_qualified_type;

/// One step of the path from an object to a slot.
///
/// `name` is the local name of the XML element. `when_type` is set only where
/// the schema lets several types stand in for the declared one and the slot
/// exists for one of them alone: PRODML's `Disposition` is declared as the
/// abstract `AbstractDisposition`, and only a `TransferDisposition` holds a
/// `Transfer`. It then carries the qualified type the `xsi:type` of that
/// element must name (`prodml23.TransferDisposition`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    /// Local name of the XML element.
    pub name: &'static str,
    /// The qualified type the element's `xsi:type` must name, when the slot
    /// belongs to one subtype of the declared type only.
    pub when_type: Option<&'static str>,
}

impl Step {
    /// A step with no condition on the element's type.
    pub const fn new(name: &'static str) -> Step {
        Step {
            name,
            when_type: None,
        }
    }

    /// A step that only applies when the element's `xsi:type` names
    /// `when_type`.
    pub const fn when(name: &'static str, when_type: &'static str) -> Step {
        Step {
            name,
            when_type: Some(when_type),
        }
    }
}

/// A place where a type holds a data object by value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContainedSlot {
    /// From the carrying object to the contained element, for example
    /// `[JobPerforationClusters, StimPerforationCluster]`. Never empty.
    pub path: &'static [Step],
    /// The qualified type the element is declared as (`witsml21.ChannelSet`).
    ///
    /// For a slot declared by an abstract type ([`abstract_bound`]) it names
    /// that abstract type (`eml23.AbstractObject`), which is not itself a
    /// data object: only `xsi:type` says which one is there.
    ///
    /// [`abstract_bound`]: ContainedSlot::abstract_bound
    pub declared: &'static str,
    /// Further data object types a document may write here instead of
    /// `declared`, selected by `xsi:type` (`witsml21.PPFGChannelSet`). For an
    /// abstract slot, every concrete data object type the generating crate
    /// knows; other crates may know more.
    pub alternatives: &'static [&'static str],
    /// Whether `declared` is abstract, so that only `xsi:type` names the type.
    pub abstract_bound: bool,
    /// `minOccurs` of the contained element.
    pub min_occurs: u32,
    /// `maxOccurs` of the contained element; `None` is `unbounded`.
    pub max_occurs: Option<u32>,
    /// Where the schema declares the element: the schema file and the
    /// `Type.Element` that declares it, as the generator knows them. No line
    /// number.
    pub source: &'static str,
}

impl ContainedSlot {
    /// Whether a data object of `qualified_type` may sit in this slot.
    ///
    /// Compared as the generator wrote the names; a caller holding a loose
    /// spelling asks [`Registry::may_be_contained`] or normalises first.
    ///
    /// [`Registry::may_be_contained`]: crate::registry::Registry::may_be_contained
    pub fn admits(&self, qualified_type: &str) -> bool {
        (!self.abstract_bound && self.declared == qualified_type)
            || self.alternatives.contains(&qualified_type)
    }

    /// The path as dotted element names, without the type conditions.
    pub fn dotted_path(&self) -> String {
        dotted(self.path)
    }
}

/// A place where a type writes a reference to a data object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DorSlot {
    /// From the carrying object to the reference element. Never empty.
    pub path: &'static [Step],
    /// Whether the reference must be present for the object to be valid:
    /// the **effective** `minOccurs`, true only when the element and every
    /// optional ancestor on the path are mandatory. An element written inside
    /// an `xs:choice` or an optional group is not.
    pub required: bool,
    /// `minOccurs` of the reference element itself, as the schema reads it
    /// (zero inside a choice or an optional group).
    pub min_occurs: u32,
    /// `maxOccurs` of the reference element; `None` is `unbounded`.
    pub max_occurs: Option<u32>,
    /// Where the schema declares the element; see [`ContainedSlot::source`].
    pub source: &'static str,
}

impl DorSlot {
    /// The path as dotted element names, without the type conditions.
    pub fn dotted_path(&self) -> String {
        dotted(self.path)
    }
}

/// Everything the generator knows about the slots of one top-level type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypeRelations {
    /// Package short name (`witsml`).
    pub package: &'static str,
    /// Package version as the directory spells it (`2.1`, `2.2_dev3`): the
    /// same string as [`TypeInfo::version`], so that a release and its
    /// development drop are two tables.
    pub version: &'static str,
    /// Rust type name, as [`TypeInfo::name`] spells it (`Log`).
    pub name: &'static str,
    /// Qualified type (`witsml21.Log`).
    pub qualified_type: &'static str,
    /// Where this type holds data objects by value. Empty for a type that
    /// holds none.
    pub contains: &'static [ContainedSlot],
    /// Where this type writes references. Emitted only for the packages whose
    /// exchange rules read them; empty otherwise, which then means "not
    /// computed", not "none".
    pub refers: &'static [DorSlot],
}

impl TypeRelations {
    /// Whether this type holds at least one data object by value.
    pub fn is_container(&self) -> bool {
        !self.contains.is_empty()
    }
}

/// Element names of a path, joined by dots (`ReportingEntityVolumes.Disposition.Transfer`).
pub fn dotted(path: &[Step]) -> String {
    let mut out = String::new();
    for (i, step) in path.iter().enumerate() {
        if i > 0 {
            out.push('.');
        }
        out.push_str(step.name);
    }
    out
}

/// Element names of a path with their type conditions, as `Disposition{prodml23.TransferDisposition}`.
pub fn dotted_typed(path: &[Step]) -> String {
    let mut out = String::new();
    for (i, step) in path.iter().enumerate() {
        if i > 0 {
            out.push('.');
        }
        out.push_str(step.name);
        if let Some(when) = step.when_type {
            out.push('{');
            out.push_str(when);
            out.push('}');
        }
    }
    out
}

/// The tables a [`Registry`] holds, indexed for the questions asked of them.
///
/// Crate-private: [`Registry`] is the door. Every lookup is a hash lookup; none
/// reads a table slot.
///
/// [`Registry`]: crate::registry::Registry
#[derive(Default)]
pub(crate) struct RelationsIndex {
    tables: Vec<&'static TypeRelations>,
    /// Keyed like `Registry::by_name`: package and name lower-cased, version
    /// verbatim, so a release and its development drop do not meet.
    by_name: HashMap<(String, String, String), usize>,
    /// By normalised qualified type. A development drop never displaces a
    /// release here, for the reason `Registry::register` gives.
    by_qualified: HashMap<String, usize>,
    /// Normalised qualified types that appear as the declared or an alternative
    /// type of some slot.
    contained: HashSet<String>,
}

impl RelationsIndex {
    /// Add one crate's tables. Adding the same tables twice changes nothing.
    pub(crate) fn add(&mut self, tables: &'static [TypeRelations]) {
        for table in tables {
            let key = name_key(table.package, table.version, table.name);
            if let Some(held) = self.by_name.get(&key)
                && std::ptr::eq(self.tables[*held], table)
            {
                continue;
            }
            let idx = self.tables.len();
            self.tables.push(table);
            self.by_name.insert(key, idx);

            let qualified = normalize_qualified_type(table.qualified_type);
            let displaced_release = naming::is_prerelease(table.version)
                && self
                    .by_qualified
                    .get(&qualified)
                    .is_some_and(|held| !naming::is_prerelease(self.tables[*held].version));
            if !displaced_release {
                self.by_qualified.insert(qualified, idx);
            }

            for slot in table.contains {
                self.contained.insert(normalize_qualified_type(slot.declared));
                for alternative in slot.alternatives {
                    self.contained.insert(normalize_qualified_type(alternative));
                }
            }
        }
    }

    /// The table of the type `package version name` (a [`TypeInfo`]'s own three
    /// strings).
    pub(crate) fn of_name(&self, package: &str, version: &str, name: &str) -> Option<&'static TypeRelations> {
        self.by_name
            .get(&name_key(package, version, name))
            .map(|i| self.tables[*i])
    }

    /// The table of a type, by its [`TypeInfo`].
    pub(crate) fn of(&self, info: &TypeInfo) -> Option<&'static TypeRelations> {
        self.of_name(info.package, info.version, info.name)
    }

    /// The table of a type, by qualified type.
    pub(crate) fn for_qualified(&self, qualified_type: &str) -> Option<&'static TypeRelations> {
        self.by_qualified
            .get(&normalize_qualified_type(qualified_type))
            .map(|i| self.tables[*i])
    }

    /// Whether some slot admits the type.
    pub(crate) fn may_be_contained(&self, qualified_type: &str) -> bool {
        self.contained.contains(&normalize_qualified_type(qualified_type))
    }

    /// Every table, in registration order.
    pub(crate) fn iter(&self) -> impl Iterator<Item = &'static TypeRelations> + '_ {
        self.tables.iter().copied()
    }
}

fn name_key(package: &str, version: &str, name: &str) -> (String, String, String) {
    (
        package.to_ascii_lowercase(),
        version.to_string(),
        name.to_ascii_lowercase(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    static CHANNEL: &[&str] = &["witsml21.PPFGChannel"];

    static CHANNEL_SET_SLOTS: &[ContainedSlot] = &[ContainedSlot {
        path: &[Step::new("Channel")],
        declared: "witsml21.Channel",
        alternatives: CHANNEL,
        abstract_bound: false,
        min_occurs: 0,
        max_occurs: None,
        source: "witsml/v2.1/xsd_schemas/Log.xsd#ChannelSet.Channel",
    }];

    static TABLES: &[TypeRelations] = &[
        TypeRelations {
            package: "witsml",
            version: "2.1",
            name: "ChannelSet",
            qualified_type: "witsml21.ChannelSet",
            contains: CHANNEL_SET_SLOTS,
            refers: &[],
        },
        TypeRelations {
            package: "witsml",
            version: "2.1",
            name: "Wellbore",
            qualified_type: "witsml21.Wellbore",
            contains: &[],
            refers: &[],
        },
    ];

    static DROP: &[TypeRelations] = &[TypeRelations {
        package: "resqml",
        version: "2.2_dev3",
        name: "Thing",
        qualified_type: "resqml22.Thing",
        contains: &[],
        refers: &[],
    }];

    static RELEASE: &[TypeRelations] = &[TypeRelations {
        package: "resqml",
        version: "2.2",
        name: "Thing",
        qualified_type: "resqml22.Thing",
        contains: CHANNEL_SET_SLOTS,
        refers: &[],
    }];

    fn index() -> RelationsIndex {
        let mut index = RelationsIndex::default();
        index.add(TABLES);
        index
    }

    /// Break it: key `by_name` on the name alone, and `of_name("witsml",
    /// "2.0", "ChannelSet")` finds the 2.1 table.
    #[test]
    fn a_table_is_found_by_its_package_version_and_name() {
        let index = index();
        assert!(index.of_name("witsml", "2.1", "ChannelSet").is_some());
        assert!(index.of_name("WITSML", "2.1", "channelset").is_some());
        assert!(index.of_name("witsml", "2.0", "ChannelSet").is_none());
        assert!(index.of_name("resqml", "2.1", "ChannelSet").is_none());
    }

    /// Break it: drop the normalisation in `for_qualified`, and `witsml21.`
    /// written with an `obj_` prefix or in another case finds nothing.
    #[test]
    fn a_table_is_found_by_qualified_type_in_the_spellings_the_registry_accepts() {
        let index = index();
        assert!(index.for_qualified("witsml21.ChannelSet").is_some());
        assert!(index.for_qualified("witsml21.obj_ChannelSet").is_some());
        assert!(index.for_qualified("witsml21.Nothing").is_none());
    }

    /// Break it: register the slot's `declared` only, and a `PPFGChannel` is
    /// not a type that may be contained.
    #[test]
    fn a_type_may_be_contained_if_a_slot_declares_or_allows_it() {
        let index = index();
        assert!(index.may_be_contained("witsml21.Channel"));
        assert!(index.may_be_contained("witsml21.PPFGChannel"));
        assert!(!index.may_be_contained("witsml21.ChannelSet"));
        assert!(!index.may_be_contained("witsml21.Wellbore"));
    }

    /// Break it: let `contains` be absent from the container test, and a type
    /// with an empty table is a container.
    #[test]
    fn only_a_table_with_slots_is_a_container() {
        let index = index();
        assert!(index.for_qualified("witsml21.ChannelSet").unwrap().is_container());
        assert!(!index.for_qualified("witsml21.Wellbore").unwrap().is_container());
    }

    /// The qualified type is shared by a release and its development drop; the
    /// release must keep it whatever the order. Break it: drop the
    /// `displaced_release` test and the drop registered last wins.
    #[test]
    fn a_development_drop_never_displaces_a_release() {
        let mut index = RelationsIndex::default();
        index.add(RELEASE);
        index.add(DROP);
        let held = index.for_qualified("resqml22.Thing").unwrap();
        assert_eq!(held.version, "2.2");
        assert!(held.is_container());
        // Both are still reachable by their own version.
        assert!(index.of_name("resqml", "2.2_dev3", "Thing").is_some());

        let mut reversed = RelationsIndex::default();
        reversed.add(DROP);
        reversed.add(RELEASE);
        assert_eq!(reversed.for_qualified("resqml22.Thing").unwrap().version, "2.2");
    }

    /// Registering a crate twice (a caller that builds two registries from one
    /// set of tables, or one that calls `register` again) must not double the
    /// tables. Break it: remove the `ptr::eq` test in `add`.
    #[test]
    fn adding_the_same_tables_twice_changes_nothing() {
        let mut index = index();
        index.add(TABLES);
        assert_eq!(index.iter().count(), TABLES.len());
    }

    #[test]
    fn a_slot_admits_its_declared_type_and_its_alternatives_but_not_an_abstract_declaration() {
        let slot = CHANNEL_SET_SLOTS[0];
        assert!(slot.admits("witsml21.Channel"));
        assert!(slot.admits("witsml21.PPFGChannel"));
        assert!(!slot.admits("witsml21.Log"));

        let abstract_slot = ContainedSlot {
            declared: "eml23.AbstractObject",
            alternatives: &["eml23.PropertyKind"],
            abstract_bound: true,
            ..slot
        };
        assert!(abstract_slot.admits("eml23.PropertyKind"));
        assert!(
            !abstract_slot.admits("eml23.AbstractObject"),
            "an abstract type is not a data object"
        );
    }

    #[test]
    fn paths_print_with_and_without_their_type_conditions() {
        let path = [
            Step::new("ReportingEntityVolumes"),
            Step::when("Disposition", "prodml23.TransferDisposition"),
            Step::new("Transfer"),
        ];
        assert_eq!(dotted(&path), "ReportingEntityVolumes.Disposition.Transfer");
        assert_eq!(
            dotted_typed(&path),
            "ReportingEntityVolumes.Disposition{prodml23.TransferDisposition}.Transfer"
        );
    }
}
