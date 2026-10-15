//! The contract every store of `energyml-storage` answers the same way: an EPC
//! file, an ETP dataspace, whatever comes next.
//!
//! Written once, played on each implementation (design note
//! `deux-sources-un-contrat.md` §4). A store's own tests check what the store
//! does; this module checks what the **traits promise**, so that two sources
//! asked the same question cannot answer it differently without a test saying
//! which rule broke.
//!
//! # How to play it
//!
//! Every group is an `async fn` taking a **factory** that builds a *fresh* store
//! holding exactly the [`model`] below, and a registry that reads the model's
//! XML ([`ModelObject::read`]). The caller brings its own executor, so this
//! module adds no dependency:
//!
//! ```ignore
//! let make = || my_store_holding_the_model();
//! futures_executor::block_on(energyml_storage::contract::objects(&make, &registry));
//! futures_executor::block_on(energyml_storage::contract::all(&make, &registry));
//! ```
//!
//! A group that fails **panics** with the rule that broke ([`Rule::id`]), the
//! text of the rule and where it comes from ([`Rule::source`]). [`rules::ALL`]
//! lists them.
//!
//! The bounds `Send + Sync` are those of the `async_trait` methods the groups
//! call on a generic store (the provided methods of the traits need them).
//!
//! # What a rule may claim
//!
//! - Only what the ETP 1.2 specification (Doc v1.1) or the design note states,
//!   cited at the rule. Where either is open, the rule checks what is stated and
//!   the gap is named in the doc of the group.
//! - A fact the store does not state is **absent**, and a request that depends on
//!   it is **refused by name** ([`Error::Unsupported`]), never answered empty.
//!   [`EnergymlStorage::capabilities`] says which: the groups read it and assert
//!   the refusal when a capability is off and the behaviour when it is on.
//! - A rule never asks a store to check itself: expectations come from the
//!   [`model`] (a description the store did not write), or from a second route
//!   to the same fact in the store (`edges()` against `resources_in()` counts).
//!
//! # Known gaps (what the suite does not assert, and why)
//!
//! - **Containment** (WITSML containers, `prune_contained = true`): there is no
//!   capability that says a store knows containment, so [`containment`] assumes
//!   none of the stores of this workspace does. The day one does, a capability
//!   flag is needed first.
//! - **Active status**: no capability flag either; the rule reads the summaries
//!   (a status absent from a summary makes the filter refused).
//! - **Edges of a subset** (`edges(&[uri])`, `references(&[uri])`): whether the
//!   argument selects the objects holding the reference, or any edge touching
//!   them, is not stated; only the whole-store form (`&[]`) is asserted.
//! - **Step 4 of §8.1.1.1.4** reads "remove edges that are not connected to a node
//!   in the final set": literally, one end suffices. The design note (§3) and
//!   [`Found::edges`] read it as both ends; the rule follows the design note.
//! - **A type of a schema version the session did not negotiate** is an error in
//!   ETP (§8.2.2, EDATAOBJECTTYPE_NOTSUPPORTED), so "a wildcard of another version
//!   is empty" is not asserted.
//! - **A misplaced star** in a type (`res*ml22.Foo`) is not defined by §23.34.15,
//!   which only names `version.*`.
//! - **Arrays**: only the refusals a capability promises (a flag off means
//!   `Unsupported`). The flag-on behaviour needs a store-specific array location.
//! - **Discovery of an absent object or another dataspace** (`NotFound`): the
//!   specification does not say which error a store raises.
//! - **Notifications and the change feed** ([`crate::changes`]).
//! - **A refused commit** (the transaction is over, §18.1.1.1 step 8.b.ii) and a
//!   refused rollback (`TODO(verify)` step 6.b): they need a scripted refusal, so
//!   they stay in the store's own tests.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Display;

use energyml_core::identifiers::EtpUri;
use energyml_core::registry::{DynObject, Registry};

use crate::array::{ArrayData, ArrayLocation, ArrayMetadata, ArrayValues};
use crate::context::{Context, DeletedFilter, Found, Navigable, Scope, types_of};
use crate::dataspaces::EnergymlDataspaces;
use crate::error::{Error, Result};
use crate::graph::Edge;
use crate::raw::{RawFormat, RawObject};
use crate::resource::{ActiveStatus, Dataspace, ObjectSummary};
use crate::storage::EnergymlStorage;
use crate::write::{EnergymlWriter, Transaction, TransactionRequest};

// ---------------------------------------------------------------------------
// The rules
// ---------------------------------------------------------------------------

/// One rule of the contract: what it says, and where it comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rule {
    /// A short stable name, `OBJ-1`-like, printed when the rule breaks.
    pub id: &'static str,
    /// Where the rule comes from: the ETP 1.2 section, the design note, or the
    /// documentation of the trait that states it.
    pub source: &'static str,
    /// The rule, in a sentence.
    pub text: &'static str,
}

macro_rules! rules {
    ($($name:ident = ($id:literal, $source:literal, $text:literal);)+) => {
        $(
            #[doc = $text]
            pub const $name: Rule = Rule { id: $id, source: $source, text: $text };
        )+
        /// Every rule of the contract.
        pub const ALL: &[Rule] = &[$($name),+];
    };
}

/// The rules, by name.
pub mod rules {
    use super::Rule;

    rules! {
        OBJ_WRITES_OFF = ("OBJ-0",
            "capabilities.rs (writes); write.rs (what a store cannot do it refuses by name)",
            "A store whose capabilities say it does not write refuses put_objects and delete_objects by name (Unsupported).");
        OBJ_PUT_URI = ("OBJ-1",
            "ETP 1.2 section 9.2.1.2 step 2 and PutResponse (section 23.34.9); write.rs Put::uri",
            "put_objects answers, for each object, the full URI it is now known under (type and uuid, in the store's own dataspace), and the listing holds exactly that URI.");
        OBJ_READBACK = ("OBJ-2",
            "ETP 1.2 section 9.2.1.1; storage.rs headers and get_objects",
            "An object that was put is listed, its header and the object itself are read under the URI put answered, and it is of the type it was written as.");
        OBJ_UPSERT = ("OBJ-3",
            "ETP 1.2 section 9.1.1 and section 9.2.1.2 step 2.b",
            "Putting an object that is held replaces it completely: it is listed once, and what is read back is the new version.");
        OBJ_PLURAL = ("OBJ-4",
            "write.rs put_objects (one result per object, in order); ETP 1.2 section 3.7.3 (plural messages)",
            "put_objects answers one result per object, in the order given.");
        OBJ_DELETE_URI = ("OBJ-5",
            "ETP 1.2 section 9.2.1.3 step 2.c; write.rs delete_objects; design note section 4",
            "delete_objects answers, for a deleted object, the URIs actually deleted, among them the URI the object had; the object is no longer listed and nothing else is lost.");
        OBJ_DELETE_PARTIAL = ("OBJ-6",
            "ETP 1.2 section 9.2.1.3 step 3 (ENOT_FOUND per URI); section 3.7.3",
            "Deleting a URI no object has fails for that URI alone (NotFound); the others in the call are deleted, one result per URI, in order.");
        OBJ_GET_ABSENT = ("OBJ-7",
            "ETP 1.2 section 9.2.1.1 step 3 (ENOT_FOUND per URI); storage.rs get_objects (partial by nature)",
            "Reading a URI no object has fails for that URI alone (NotFound); the others in the call are read, one result per URI, in order.");
        OBJ_PUT_RAW = ("OBJ-8",
            "write.rs put_raw (refused by default); raw.rs",
            "put_raw either writes the object (then it is listed under the URI answered) or is refused by name (Unsupported); it never answers success and writes nothing.");
        CONT_PRUNE = ("CONT-1",
            "write.rs put_objects (prune_contained); ETP 1.2 section 9.1.3.2",
            "A store that does not know containment refuses prune_contained = true by name (Unsupported) rather than ignoring it, and writes or deletes nothing.");
        TX_OFF = ("TX-0",
            "capabilities.rs (transactions); write.rs (refused by name)",
            "A store whose capabilities say it has no transactions refuses begin by name (Unsupported) and holds no open transaction.");
        TX_ONE = ("TX-1",
            "ETP 1.2 section 18.1.1.1 step 1.c.i (only one transaction; EMAX_TRANSACTIONS_EXCEEDED); write.rs begin",
            "A second begin while a transaction is open is refused (Error::Transaction) and the first stays open.");
        TX_HOLDER = ("TX-2",
            "write.rs check_open; ETP 1.2 section 18.1.1.1 step 8.a (a transaction that is not valid is not found)",
            "Only the transaction begin returned ends the open one: another one, commit or rollback, is refused (Error::Transaction) and the open one stays open.");
        TX_ROLLBACK = ("TX-3",
            "ETP 1.2 section 18.1.1.1 step 6.a (the store is in the same state as before the transaction)",
            "A rollback undoes what the transaction put and deleted: afterwards the store lists what it listed before.");
        TX_COMMIT = ("TX-4",
            "ETP 1.2 section 18.1.1.1 step 8.b.i",
            "A commit keeps what the transaction put and deleted.");
        TX_ENDED = ("TX-5",
            "write.rs (open_transaction; one at a time); ETP 1.2 section 18.1.1.1 step 8.a",
            "After a commit or a rollback no transaction is open, the ended one cannot end again (Error::Transaction), and another can begin.");
        TX_READ_ONLY = ("TX-6",
            "write.rs TransactionRequest::read_only (a transaction that only reads); ETP 1.2 section 18.1.1.1 step 1.b only recommends flagging it, so this is the trait's rule, not the specification's",
            "A read-only transaction refuses every write (Error::Transaction) and the store is unchanged after it ends.");
        TX_DATASPACE = ("TX-7",
            "write.rs (a writer is one dataspace); dataspaces.rs open_dataspace (a URI naming no dataspace of the store is NotFound)",
            "A transaction over a dataspace the store does not hold is refused (NotFound) and none is open; over the store's own dataspace it opens.");
        RAW_ANSWER = ("RAW-1",
            "ETP 1.2 section 9.2.1.1 and section 9.3.1 (GetDataObjects format); storage.rs get_raw; design note section 2 (objects as bytes)",
            "For a format the capabilities list in raw_formats, get_raw answers one result per URI, in order, each under the full URI asked, in that format, with bytes (the XML names the uuid).");
        RAW_REFUSED = ("RAW-2",
            "storage.rs get_raw (refused by name, per object); raw.rs",
            "For a format absent from raw_formats, get_raw refuses every URI by name (Unsupported), one result per URI.");
        RAW_ABSENT = ("RAW-3",
            "ETP 1.2 section 9.2.1.1 step 3 (ENOT_FOUND per URI)",
            "In a format the store answers, a URI no object has is NotFound for that URI alone.");
        DISC_DATASPACE = ("DISC-1",
            "ETP 1.2 section 8.2.2 row 9 and section 23.34.15 (uri): discovering a dataspace URI, scope and depth are ignored",
            "A context on the dataspace URI answers the dataspace's whole content whatever the scope and the depth.");
        DISC_LISTING = ("DISC-2",
            "ETP 1.2 section 8.2.2 (dataspaces in discovery operations); design note section 2 (list dataspaces, objects of a dataspace)",
            "The store's own dataspace is among its dataspaces, and the content of a dataspace context is the listing of resources().");
        DISC_TARGETS = ("DISC-3",
            "ETP 1.2 section 8.1.1.1.1 (the object holding a DOR is the source, the object it points to the target) and section 8.1.1.1.4 step 1; section 23.34.15 (depth)",
            "Scope Targets reaches the objects an object references, as far as the depth says; TargetsOrSelf adds the object itself.");
        DISC_SOURCES = ("DISC-4",
            "ETP 1.2 section 8.1.1.1.1 and section 8.1.1.1.4 step 1; section 23.34.15 (depth)",
            "Scope Sources reaches the objects that reference an object, as far as the depth says; SourcesOrSelf adds the object itself.");
        DISC_ITSELF = ("DISC-5",
            "ETP 1.2 section 8.3.1 and section 23.34.15: if scope is self, depth is ignored",
            "Scope Itself answers the object alone whatever the depth.");
        DISC_TYPES = ("DISC-6",
            "ETP 1.2 section 8.1.1.1.4 step 3 (types removed after the candidate set is built); design note section 2",
            "The types filter applies after the walk: an object of another type is walked through, not stopped at.");
        DISC_WILDCARD = ("DISC-7",
            "ETP 1.2 section 23.34.15 (dataObjectTypes): version.* means every data object of that schema version",
            "A type ending in .* keeps every type of that schema version; an exact name keeps that type only.");
        DISC_EDGES = ("DISC-8",
            "ETP 1.2 section 8.1.1.1.4 step 4, spelled out in section 8.3.1 (p. 163, GetResourcesEdgesResponse row, 3.c)",
            "The edges a discovery answers are the store's edges with either end among the objects it answers; over the model they are the references between held objects.");
        DISC_DANGLING = ("DISC-9",
            "design note section 3 (a stated relationship is between objects the store holds)",
            "A reference to an object the store does not hold adds no object to what a walk reaches.");
        DISC_COUNTS = ("DISC-10",
            "ETP 1.2 section 8.3.1 (countObjects: counts of sources and targets for each resource); design note section 2; resource.rs ObjectSummary::source_count",
            "With count_objects the sources and targets of each object found are the distinct objects at the other end of the store's edges(); without it they are unstated.");
        DISC_SUPPORTED_TYPES = ("DISC-11",
            "ETP 1.2 section 25 (GetSupportedTypes with counts); design note section 2",
            "types_of over a discovery counts the objects of each type that discovery reached.");
        DISC_STATUS = ("DISC-12",
            "ETP 1.2 section 8.3.1 (activeStatusFilter); design note section 0.3 and Q1 (an unstated fact is refused by name)",
            "A filter on the active status is refused by name (Unsupported) when an object does not state its status, and answered from the stated status otherwise.");
        DISC_TIMES = ("DISC-13",
            "ETP 1.2 section 8.3.1 (storeLastWriteFilter); capabilities.rs store_times; resource.rs (no date is invented)",
            "A store that keeps the store times states them on every object and filters on them; one that does not leaves them unstated and refuses the filter by name (Unsupported).");
        DISC_KINDS = ("DISC-14",
            "ETP 1.2 section 8.1.1.1.1 note 1 and section 8.1.1.1.2; capabilities.rs edge_kinds; design note section 2",
            "Following relationships by kind (Primary, Secondary, the secondary expansions) is refused by name (Unsupported) when the store does not state the kind of its edges, and answered, with every edge's kind stated, when it does.");
        REF_HELD = ("REF-1",
            "design note section 3 (edges are between objects the store holds); ETP 1.2 section 8.1.1.1.4 step 4",
            "Every entry of edges() has both ends among the objects the store holds.");
        REF_SUBSET = ("REF-2",
            "design note section 3 (edges are the references whose target is held); the model has no containment",
            "Over the model, every entry of edges() is among references() (same from and to).");
        REF_DANGLING = ("REF-3",
            "design note section 3 (a reference may point outside the store, an edge may not)",
            "A reference to an object the store does not hold is in references() and not in edges().");
        REF_WRITTEN = ("REF-4",
            "design note section 3 (references are what the objects write, with the path of the field); the model's own description of its references",
            "references() are exactly the references the model's objects write, each with the path of its field; edges() are those whose target is held.");
        REF_GRAPH = ("REF-5",
            "storage.rs graph (objects referenced but not held still appear, marked absent); graph.rs Node::present",
            "graph() has a present node for each held object and an absent node for a referenced object the store does not hold.");
        CAP_DELETIONS = ("CAP-1",
            "ETP 1.2 section 9.2.1.3 step 2.a.i (tombstones); capabilities.rs deletions_kept_for; storage.rs deleted_resources",
            "A store that does not keep deletions refuses deleted_resources by name (Unsupported), never an empty list; one that does lists the object it deleted.");
        CAP_DATASPACES = ("CAP-2",
            "ETP 1.2 section 21.2.1.1 to section 21.2.1.3; capabilities.rs several_dataspaces; dataspaces.rs",
            "A store that does not hold several dataspaces refuses to create or delete one by name (Unsupported); one that does lists what it created and no longer lists what it deleted.");
        CAP_DOOR = ("CAP-3",
            "dataspaces.rs open_dataspace; ETP 1.2 section 21.2.1.1 (GetDataspaces)",
            "list_dataspaces and dataspaces list the store's own dataspace; open_dataspace reaches it, and answers NotFound for a dataspace the store does not list and for an object URI.");
        CAP_ARRAYS = ("CAP-4",
            "capabilities.rs subarrays and uninitialized_arrays; write.rs (refused by default)",
            "A store whose capabilities say it cannot write part of an array, or declare an array before writing it, refuses by name (Unsupported).");
    }
}

use rules::*;

/// A rule broke: say which, what it says, where it comes from, and what was seen.
fn broken(rule: &Rule, detail: impl Display) -> ! {
    panic!(
        "contract rule {} broken: {}\n  source: {}\n  seen:   {}",
        rule.id, rule.text, rule.source, detail
    )
}

/// The factory did not build the store the groups need: not a rule, a setup error.
fn setup(detail: impl Display) -> ! {
    panic!("contract setup: {detail}")
}

macro_rules! rule {
    ($rule:expr, $cond:expr, $($detail:tt)+) => {
        if !$cond {
            broken($rule, format!($($detail)+));
        }
    };
}

fn brief<T>(result: &Result<T>) -> String {
    match result {
        Ok(_) => "Ok(..)".to_string(),
        Err(error) => format!("Err({error})"),
    }
}

fn briefs<T>(results: &[Result<T>]) -> String {
    format!("[{}]", results.iter().map(brief).collect::<Vec<_>>().join(", "))
}

fn must<T>(rule: &Rule, what: &str, result: Result<T>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => broken(rule, format!("{what} failed: {error}")),
    }
}

// ---------------------------------------------------------------------------
// The model
// ---------------------------------------------------------------------------

/// A BoundaryFeature.
pub const FEATURE: &str = "7f000000-0000-4000-8000-000000000001";
/// A HorizonInterpretation of [`FEATURE`].
pub const HORIZON_1: &str = "7f000000-0000-4000-8000-000000000002";
/// Another HorizonInterpretation of [`FEATURE`].
pub const HORIZON_2: &str = "7f000000-0000-4000-8000-000000000003";
/// A StratigraphicColumn whose rank refers to [`HORIZON_1`].
pub const COLUMN: &str = "7f000000-0000-4000-8000-000000000004";
/// A HorizonInterpretation of [`ABSENT`], a feature the model does not hold.
pub const DANGLING: &str = "7f000000-0000-4000-8000-000000000005";
/// A BoundaryFeature the model does **not** hold, referenced by [`DANGLING`].
pub const ABSENT: &str = "7f000000-0000-4000-8000-0000000000ff";
/// A BoundaryFeature that is free for the groups to put (not in the model).
pub const NEW_1: &str = "7f000000-0000-4000-8000-000000000091";
/// Another one.
pub const NEW_2: &str = "7f000000-0000-4000-8000-000000000092";

const RESQML: &str = "http://www.energistics.org/energyml/data/resqmlv2";
const EML: &str = "http://www.energistics.org/energyml/data/commonv2";

/// One object of the model: its identity, and its XML to load into a store.
///
/// The expected URI is the store's to give (an EPC and a dataspace spell it
/// differently); the groups find an object by its [`ModelObject::uuid`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelObject {
    /// Its uuid, lowercase.
    pub uuid: String,
    /// `resqml22.BoundaryFeature`.
    pub qualified_type: &'static str,
    /// The content type of its EPC part, for [`Registry::read_xml`].
    pub content_type: &'static str,
    /// Its citation title.
    pub title: String,
    /// Its XML document, as a store takes it (`schemaVersion="2.2"`).
    pub xml: String,
}

impl ModelObject {
    /// The object, deserialized by the registry the caller gives (it must know
    /// RESQML 2.2 and the EML 2.3 it is built on).
    ///
    /// # Panics
    /// When the registry cannot read it: a setup error, not a rule.
    pub fn read(&self, registry: &Registry) -> Box<dyn DynObject> {
        registry
            .read_xml(self.content_type, self.xml.as_bytes())
            .unwrap_or_else(|error| setup(format!("the registry cannot read `{}`: {error}", self.uuid)))
    }

    /// The object as bytes, addressed by `uri` (for `put_raw`).
    pub fn raw(&self, uri: impl Into<String>) -> RawObject {
        RawObject::new(uri, RawFormat::Xml, self.xml.clone().into_bytes())
    }

    /// The file name an EPC would give its part, `BoundaryFeature_<uuid>.xml`.
    pub fn part_name(&self) -> String {
        let name = self
            .qualified_type
            .split_once('.')
            .map_or(self.qualified_type, |(_, name)| name);
        format!("{name}_{}.xml", self.uuid)
    }
}

/// What a store must hold for the groups to run, described without any store.
///
/// A fixture for a reference graph, **not** a valid RESQML model: the column's
/// rank points at a horizon interpretation where a model would use a rank
/// interpretation. What matters is the shape of the references.
///
/// ```text
///   column --(Ranks)--> horizon-1 --(InterpretedFeature)--> feature <-- horizon-2
///   dangling --(InterpretedFeature)--> absent        (absent is not held)
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model {
    /// The objects the store holds, five of them.
    pub objects: Vec<ModelObject>,
    /// The references the objects write, as (uuid of the holder, uuid of the
    /// target), including the one to [`ABSENT`].
    pub references: Vec<(&'static str, &'static str)>,
}

impl Model {
    /// The uuids of the held objects.
    pub fn held(&self) -> BTreeSet<String> {
        self.objects.iter().map(|object| object.uuid.clone()).collect()
    }

    /// The references whose target the model holds: what a store states as edges.
    pub fn stated(&self) -> BTreeSet<(String, String)> {
        let held = self.held();
        self.references
            .iter()
            .filter(|(_, to)| held.contains(*to))
            .map(|(from, to)| (from.to_string(), to.to_string()))
            .collect()
    }

    /// Every reference written, as pairs of uuids.
    pub fn written(&self) -> BTreeSet<(String, String)> {
        self.references
            .iter()
            .map(|(from, to)| (from.to_string(), to.to_string()))
            .collect()
    }

    /// For a held object, how many distinct objects point at it and how many it
    /// points at, over the stated references.
    pub fn counts(&self, uuid: &str) -> (u32, u32) {
        let stated = self.stated();
        let sources = stated.iter().filter(|(_, to)| to == uuid).count();
        let targets = stated.iter().filter(|(from, _)| from == uuid).count();
        (sources as u32, targets as u32)
    }
}

fn citation(title: &str) -> String {
    format!(
        "<eml:Citation><eml:Title>{title}</eml:Title><eml:Originator>contract</eml:Originator>\
         <eml:Creation>2026-10-14T00:00:00Z</eml:Creation><eml:Format>contract</eml:Format></eml:Citation>"
    )
}

fn open(element: &str, uuid: &str) -> String {
    format!(
        "<resqml:{element} xmlns:resqml=\"{RESQML}\" xmlns:eml=\"{EML}\" uuid=\"{uuid}\" schemaVersion=\"2.2\">"
    )
}

fn reference(element: &str, uuid: &str, qualified_type: &str, title: &str) -> String {
    format!(
        "<resqml:{element}><eml:Uuid>{uuid}</eml:Uuid><eml:QualifiedType>{qualified_type}</eml:QualifiedType>\
         <eml:Title>{title}</eml:Title></resqml:{element}>"
    )
}

/// A BoundaryFeature, which references nothing. Any uuid: [`NEW_1`] and
/// [`NEW_2`] are free for the groups to put.
pub fn feature(uuid: &str, title: &str) -> ModelObject {
    ModelObject {
        uuid: uuid.to_string(),
        qualified_type: "resqml22.BoundaryFeature",
        content_type: "application/x-resqml+xml;version=2.2;type=BoundaryFeature",
        title: title.to_string(),
        xml: format!(
            "{}{}<resqml:IsWellKnown>false</resqml:IsWellKnown></resqml:BoundaryFeature>",
            open("BoundaryFeature", uuid),
            citation(title)
        ),
    }
}

/// A HorizonInterpretation of the feature `of`.
pub fn horizon(uuid: &str, title: &str, of: &str) -> ModelObject {
    ModelObject {
        uuid: uuid.to_string(),
        qualified_type: "resqml22.HorizonInterpretation",
        content_type: "application/x-resqml+xml;version=2.2;type=HorizonInterpretation",
        title: title.to_string(),
        xml: format!(
            "{}{}<resqml:Domain>depth</resqml:Domain>{}</resqml:HorizonInterpretation>",
            open("HorizonInterpretation", uuid),
            citation(title),
            reference("InterpretedFeature", of, "resqml22.BoundaryFeature", "a boundary")
        ),
    }
}

/// A StratigraphicColumn with one rank, the object `rank`.
pub fn column(uuid: &str, title: &str, rank: &str) -> ModelObject {
    ModelObject {
        uuid: uuid.to_string(),
        qualified_type: "resqml22.StratigraphicColumn",
        content_type: "application/x-resqml+xml;version=2.2;type=StratigraphicColumn",
        title: title.to_string(),
        xml: format!(
            "{}{}{}</resqml:StratigraphicColumn>",
            open("StratigraphicColumn", uuid),
            citation(title),
            reference("Ranks", rank, "resqml22.HorizonInterpretation", "a horizon")
        ),
    }
}

/// The model every group expects the factory's store to hold: five objects, four
/// references written, three of them between held objects.
pub fn model() -> Model {
    Model {
        objects: vec![
            feature(FEATURE, "a boundary"),
            horizon(HORIZON_1, "a horizon", FEATURE),
            horizon(HORIZON_2, "another horizon", FEATURE),
            column(COLUMN, "a column", HORIZON_1),
            horizon(DANGLING, "a horizon of nothing held", ABSENT),
        ],
        references: vec![
            (HORIZON_1, FEATURE),
            (HORIZON_2, FEATURE),
            (COLUMN, HORIZON_1),
            (DANGLING, ABSENT),
        ],
    }
}

// ---------------------------------------------------------------------------
// What a group reads of the store
// ---------------------------------------------------------------------------

/// The store as the factory built it: what it lists, and its dataspace.
struct Held {
    dataspace: String,
    summaries: Vec<ObjectSummary>,
}

impl Held {
    fn uri(&self, uuid: &str) -> String {
        self.summaries
            .iter()
            .find(|summary| summary.uuid.eq_ignore_ascii_case(uuid))
            .map(|summary| summary.uri.clone())
            .unwrap_or_else(|| {
                setup(format!(
                    "the store does not list the model object {}",
                    label(uuid)
                ))
            })
    }

    /// The URI of an object of the same type as `like`, with another uuid: how a
    /// group names an object the store does not hold.
    fn uri_like(&self, like: &str, uuid: &str) -> String {
        self.uri(like).replace(like, uuid)
    }

    fn uris(&self) -> BTreeSet<String> {
        self.summaries.iter().map(|summary| summary.uri.clone()).collect()
    }
}

async fn held<S: EnergymlStorage + Sync>(store: &S) -> Held {
    let summaries = store
        .resources()
        .await
        .unwrap_or_else(|error| setup(format!("resources() failed on the fresh store: {error}")));
    let want = model().held();
    let got: BTreeSet<String> = summaries
        .iter()
        .map(|summary| summary.uuid.to_ascii_lowercase())
        .collect();
    if want != got {
        setup(format!(
            "the factory must build a store holding exactly the contract model; it lists {} and the model is {}",
            show(&got),
            show(&want)
        ));
    }
    let first = &summaries[0].uri;
    Held {
        dataspace: dataspace_of(first),
        summaries,
    }
}

async fn listing<S: EnergymlStorage + Sync>(store: &S, rule: &Rule) -> Vec<ObjectSummary> {
    must(rule, "resources()", store.resources().await)
}

fn dataspace_of(uri: &str) -> String {
    match EtpUri::parse(uri).ok().and_then(|parsed| parsed.dataspace) {
        Some(name) => format!("eml:///dataspace('{}')", name.replace('\'', "''")),
        None => "eml:///".to_string(),
    }
}

fn uuid_of(uri: &str) -> String {
    EtpUri::parse(uri)
        .ok()
        .and_then(|parsed| parsed.uuid)
        .unwrap_or_else(|| uri.to_string())
}

fn label(uuid: &str) -> String {
    let lower = uuid.to_ascii_lowercase();
    for (name, known) in [
        ("feature", FEATURE),
        ("horizon-1", HORIZON_1),
        ("horizon-2", HORIZON_2),
        ("column", COLUMN),
        ("dangling", DANGLING),
        ("absent", ABSENT),
        ("new-1", NEW_1),
        ("new-2", NEW_2),
    ] {
        if lower == known {
            return name.to_string();
        }
    }
    lower
}

fn show(uuids: &BTreeSet<String>) -> String {
    format!(
        "{{{}}}",
        uuids
            .iter()
            .map(|uuid| label(uuid))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn set(uuids: &[&str]) -> BTreeSet<String> {
    uuids.iter().map(|uuid| uuid.to_ascii_lowercase()).collect()
}

fn uuids_of(found: &Found) -> BTreeSet<String> {
    found
        .resources
        .iter()
        .map(|summary| summary.uuid.to_ascii_lowercase())
        .collect()
}

fn pairs(edges: &[Edge]) -> BTreeSet<(String, String)> {
    edges
        .iter()
        .map(|edge| (uuid_of(&edge.from), uuid_of(&edge.to)))
        .collect()
}

fn show_pairs(pairs: &BTreeSet<(String, String)>) -> String {
    format!(
        "{{{}}}",
        pairs
            .iter()
            .map(|(from, to)| format!("{} -> {}", label(from), label(to)))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn wanted_pairs(pairs: &[(&str, &str)]) -> BTreeSet<(String, String)> {
    pairs
        .iter()
        .map(|(from, to)| (from.to_string(), to.to_string()))
        .collect()
}

// ---------------------------------------------------------------------------
// Objects
// ---------------------------------------------------------------------------

/// **Objects**: put, read back, upsert, delete, in the plural and partial the
/// way ETP's Store protocol is (ETP 1.2 section 9).
///
/// A store that does not write ([`crate::Capabilities::writes`] off) must refuse
/// by name; one that does is asked for the behaviour. Pruning is in
/// [`containment`], raw puts are in this group as a refusal-or-write rule.
pub async fn objects<S: EnergymlWriter + Send + Sync>(make: &dyn Fn() -> S, registry: &Registry) {
    if !make().capabilities().writes {
        writes_off(make, registry).await;
        return;
    }
    put_answers_the_full_uri(make, registry).await;
    put_twice_replaces(make, registry).await;
    put_is_plural(make, registry).await;
    delete_answers_the_uri_it_had(make).await;
    delete_is_partial(make).await;
    read_is_partial(make).await;
    put_raw_writes_or_refuses(make).await;
}

async fn writes_off<S: EnergymlWriter + Send + Sync>(make: &dyn Fn() -> S, registry: &Registry) {
    let mut store = make();
    let state = held(&store).await;
    let put = store
        .put_objects(vec![feature(NEW_1, "a new boundary").read(registry)], false)
        .await;
    rule!(
        &OBJ_WRITES_OFF,
        matches!(put.as_slice(), [Err(Error::Unsupported { .. })]),
        "put_objects answered {}",
        briefs(&put)
    );
    let deleted = store.delete_objects(&[state.uri(COLUMN)], false).await;
    rule!(
        &OBJ_WRITES_OFF,
        matches!(deleted.as_slice(), [Err(Error::Unsupported { .. })]),
        "delete_objects answered {}",
        briefs(&deleted)
    );
}

async fn put_answers_the_full_uri<S: EnergymlWriter + Send + Sync>(
    make: &dyn Fn() -> S,
    registry: &Registry,
) {
    let mut store = make();
    let before = held(&store).await;
    let new = feature(NEW_1, "a new boundary");
    let put = store.put_objects(vec![new.read(registry)], false).await;
    let [Ok(put)] = put.as_slice() else {
        broken(
            &OBJ_PUT_URI,
            format!("one object, one success expected, got {}", briefs(&put)),
        )
    };
    let parsed = EtpUri::parse(&put.uri)
        .unwrap_or_else(|error| broken(&OBJ_PUT_URI, format!("`{}` is not an ETP URI: {error}", put.uri)));
    rule!(
        &OBJ_PUT_URI,
        parsed.uuid.as_deref() == Some(NEW_1) && parsed.object_type.is_some(),
        "`{}` does not name the object put by its type and uuid",
        put.uri
    );
    rule!(
        &OBJ_PUT_URI,
        dataspace_of(&put.uri) == before.dataspace,
        "`{}` is not in the store's dataspace `{}`",
        put.uri,
        before.dataspace
    );
    let after = listing(&store, &OBJ_PUT_URI).await;
    rule!(
        &OBJ_PUT_URI,
        after.iter().filter(|summary| summary.uri == put.uri).count() == 1,
        "the listing does not hold `{}` exactly once",
        put.uri
    );
    rule!(
        &OBJ_PUT_URI,
        after.len() == before.summaries.len() + 1,
        "the listing went from {} to {} objects after one put",
        before.summaries.len(),
        after.len()
    );

    // Read back.
    let header = must(&OBJ_READBACK, "header()", store.header(&put.uri).await);
    rule!(
        &OBJ_READBACK,
        header.title.as_deref() == Some("a new boundary"),
        "the header's title is {:?}",
        header.title
    );
    let object = must(&OBJ_READBACK, "get_object()", store.get_object(&put.uri).await);
    rule!(
        &OBJ_READBACK,
        object.as_reflect().type_info().qualified_type == Some("resqml22.BoundaryFeature"),
        "the object read back is of type {:?}",
        object.as_reflect().type_info().qualified_type
    );
}

async fn put_twice_replaces<S: EnergymlWriter + Send + Sync>(make: &dyn Fn() -> S, registry: &Registry) {
    let mut store = make();
    let before = held(&store).await;
    let replaced = feature(FEATURE, "a replaced title");
    let put = store.put_objects(vec![replaced.read(registry)], false).await;
    let [Ok(put)] = put.as_slice() else {
        broken(
            &OBJ_UPSERT,
            format!("one object, one success expected, got {}", briefs(&put)),
        )
    };
    let after = listing(&store, &OBJ_UPSERT).await;
    rule!(
        &OBJ_UPSERT,
        after.len() == before.summaries.len()
            && after
                .iter()
                .filter(|summary| summary.uuid.eq_ignore_ascii_case(FEATURE))
                .count()
                == 1,
        "after replacing a held object the listing has {} objects ({} before) and {} of that uuid",
        after.len(),
        before.summaries.len(),
        after
            .iter()
            .filter(|summary| summary.uuid.eq_ignore_ascii_case(FEATURE))
            .count()
    );
    let header = must(&OBJ_UPSERT, "header()", store.header(&put.uri).await);
    rule!(
        &OBJ_UPSERT,
        header.title.as_deref() == Some("a replaced title"),
        "the title read back is {:?}, not the replacement's",
        header.title
    );
}

async fn put_is_plural<S: EnergymlWriter + Send + Sync>(make: &dyn Fn() -> S, registry: &Registry) {
    let mut store = make();
    held(&store).await;
    let puts = store
        .put_objects(
            vec![
                feature(NEW_1, "first").read(registry),
                feature(NEW_2, "second").read(registry),
            ],
            false,
        )
        .await;
    rule!(
        &OBJ_PLURAL,
        puts.len() == 2,
        "two objects, {} results",
        puts.len()
    );
    for (put, uuid) in puts.iter().zip([NEW_1, NEW_2]) {
        match put {
            Ok(put) => rule!(
                &OBJ_PLURAL,
                uuid_of(&put.uri) == uuid,
                "result for {} names `{}`: the order is not kept",
                label(uuid),
                put.uri
            ),
            Err(error) => broken(&OBJ_PLURAL, format!("{} was not put: {error}", label(uuid))),
        }
    }
}

async fn delete_answers_the_uri_it_had<S: EnergymlWriter + Send + Sync>(make: &dyn Fn() -> S) {
    let mut store = make();
    let before = held(&store).await;
    let uri = before.uri(COLUMN);
    let deleted = store.delete_objects(std::slice::from_ref(&uri), false).await;
    let [Ok(deleted)] = deleted.as_slice() else {
        broken(
            &OBJ_DELETE_URI,
            format!("one URI, one success expected, got {}", briefs(&deleted)),
        )
    };
    rule!(
        &OBJ_DELETE_URI,
        deleted.contains(&uri),
        "`{uri}` is not among the URIs deleted: {deleted:?}"
    );
    let after = listing(&store, &OBJ_DELETE_URI).await;
    let mut want = before.uris();
    want.remove(&uri);
    let got: BTreeSet<String> = after.into_iter().map(|summary| summary.uri).collect();
    rule!(
        &OBJ_DELETE_URI,
        got == want,
        "after deleting `{uri}` the listing is not the others: {} listed, {} expected",
        got.len(),
        want.len()
    );
}

async fn delete_is_partial<S: EnergymlWriter + Send + Sync>(make: &dyn Fn() -> S) {
    let mut store = make();
    let before = held(&store).await;
    let absent = before.uri_like(FEATURE, ABSENT);
    let column = before.uri(COLUMN);
    let results = store
        .delete_objects(&[absent.clone(), column.clone()], false)
        .await;
    rule!(
        &OBJ_DELETE_PARTIAL,
        results.len() == 2
            && matches!(results[0], Err(Error::NotFound(_)))
            && matches!(&results[1], Ok(deleted) if deleted.contains(&column)),
        "deleting [absent, column] answered {}",
        briefs(&results)
    );
    let after = listing(&store, &OBJ_DELETE_PARTIAL).await;
    rule!(
        &OBJ_DELETE_PARTIAL,
        after.len() == before.summaries.len() - 1 && !after.iter().any(|summary| summary.uri == column),
        "the held URI of the call was not deleted alone"
    );
}

async fn read_is_partial<S: EnergymlStorage + Send + Sync>(make: &dyn Fn() -> S) {
    let store = make();
    let state = held(&store).await;
    let feature = state.uri(FEATURE);
    let absent = state.uri_like(FEATURE, ABSENT);
    let read = store.get_objects(&[feature, absent]).await;
    rule!(
        &OBJ_GET_ABSENT,
        read.len() == 2 && read[0].is_ok() && matches!(read[1], Err(Error::NotFound(_))),
        "reading [held, absent] answered {}",
        briefs(&read)
    );
}

async fn put_raw_writes_or_refuses<S: EnergymlWriter + Send + Sync>(make: &dyn Fn() -> S) {
    let mut store = make();
    let before = held(&store).await;
    let uri = before.uri_like(FEATURE, NEW_1);
    let puts = store
        .put_raw(vec![feature(NEW_1, "a raw boundary").raw(uri)], false)
        .await;
    match puts.as_slice() {
        [Ok(put)] => {
            let after = listing(&store, &OBJ_PUT_RAW).await;
            rule!(
                &OBJ_PUT_RAW,
                after.iter().any(|summary| summary.uri == put.uri),
                "put_raw answered success under `{}` and the listing does not hold it",
                put.uri
            );
        }
        [Err(Error::Unsupported { .. })] => {
            let after = listing(&store, &OBJ_PUT_RAW).await;
            rule!(
                &OBJ_PUT_RAW,
                after.len() == before.summaries.len(),
                "put_raw was refused and the listing changed"
            );
        }
        other => broken(&OBJ_PUT_RAW, format!("put_raw answered {}", briefs(other))),
    }
}

/// **Containment**: a store that does not know containment
/// ([`crate::Capabilities::contained_objects`] off) refuses
/// `prune_contained = true` by name and writes or deletes nothing. A store
/// that knows it (an ETP server: the request travels as asked) is not checked
/// here - what it prunes is the server's to say, and the triangulation's to
/// confront (design note `deux-sources-un-contrat.md` §5).
pub async fn containment<S: EnergymlWriter + Send + Sync>(make: &dyn Fn() -> S, registry: &Registry) {
    let mut store = make();
    if store.capabilities().contained_objects {
        return;
    }
    let before = held(&store).await;
    let put = store
        .put_objects(vec![feature(NEW_1, "a new boundary").read(registry)], true)
        .await;
    rule!(
        &CONT_PRUNE,
        matches!(put.as_slice(), [Err(Error::Unsupported { .. })]),
        "put_objects with prune_contained answered {}",
        briefs(&put)
    );
    let uri = before.uri(COLUMN);
    let deleted = store.delete_objects(std::slice::from_ref(&uri), true).await;
    rule!(
        &CONT_PRUNE,
        matches!(deleted.as_slice(), [Err(Error::Unsupported { .. })]),
        "delete_objects with prune_contained answered {}",
        briefs(&deleted)
    );
    let after = listing(&store, &CONT_PRUNE).await;
    rule!(
        &CONT_PRUNE,
        after.iter().map(|summary| &summary.uri).collect::<BTreeSet<_>>()
            == before
                .summaries
                .iter()
                .map(|summary| &summary.uri)
                .collect::<BTreeSet<_>>(),
        "a refused request changed the listing"
    );
}

// ---------------------------------------------------------------------------
// Transactions
// ---------------------------------------------------------------------------

/// **Transactions** (ETP 1.2 section 18): one at a time, ended only by the one
/// that was opened, a rollback undoes, a commit keeps, a read-only one writes
/// nothing. A store without transactions refuses `begin` by name.
pub async fn transactions<S: EnergymlWriter + Send + Sync>(make: &dyn Fn() -> S, registry: &Registry) {
    let mut store = make();
    held(&store).await;
    if !store.capabilities().transactions {
        let refused = store.begin(TransactionRequest::new()).await;
        rule!(
            &TX_OFF,
            matches!(refused, Err(Error::Unsupported { .. })),
            "begin answered {}",
            brief(&refused)
        );
        rule!(
            &TX_OFF,
            store.open_transaction().is_none(),
            "a transaction is open"
        );
        return;
    }
    drop(store);
    one_at_a_time(make).await;
    only_the_holder_ends(make).await;
    rollback_undoes(make, registry).await;
    commit_keeps(make, registry).await;
    ended_is_over(make).await;
    read_only_writes_nothing(make, registry).await;
    other_dataspace_refused(make).await;
}

async fn one_at_a_time<S: EnergymlWriter + Send + Sync>(make: &dyn Fn() -> S) {
    let mut store = make();
    held(&store).await;
    let first = must(
        &TX_ONE,
        "begin",
        store
            .begin(TransactionRequest::new().with_message("contract: first"))
            .await,
    );
    rule!(
        &TX_ONE,
        store.open_transaction().as_ref() == Some(&first),
        "the transaction begin returned is not the open one"
    );
    let second = store.begin(TransactionRequest::new()).await;
    rule!(
        &TX_ONE,
        matches!(second, Err(Error::Transaction { .. })),
        "a second begin answered {}",
        brief(&second)
    );
    rule!(
        &TX_ONE,
        store.open_transaction().as_ref() == Some(&first),
        "the refused second begin disturbed the first"
    );
    must(&TX_ONE, "rollback of the first", store.rollback(&first).await);
}

async fn only_the_holder_ends<S: EnergymlWriter + Send + Sync>(make: &dyn Fn() -> S) {
    let mut store = make();
    held(&store).await;
    let first = must(&TX_HOLDER, "begin", store.begin(TransactionRequest::new()).await);
    let stranger = Transaction::new("not-the-open-one", TransactionRequest::new());
    let commit = store.commit(&stranger).await;
    rule!(
        &TX_HOLDER,
        matches!(commit, Err(Error::Transaction { .. })),
        "committing another transaction answered {}",
        brief(&commit)
    );
    let rollback = store.rollback(&stranger).await;
    rule!(
        &TX_HOLDER,
        matches!(rollback, Err(Error::Transaction { .. })),
        "rolling back another transaction answered {}",
        brief(&rollback)
    );
    rule!(
        &TX_HOLDER,
        store.open_transaction().as_ref() == Some(&first),
        "a refused end closed or changed the open transaction"
    );
    must(&TX_HOLDER, "rollback by its holder", store.rollback(&first).await);
}

async fn rollback_undoes<S: EnergymlWriter + Send + Sync>(make: &dyn Fn() -> S, registry: &Registry) {
    let mut store = make();
    let before = held(&store).await;
    let transaction = must(
        &TX_ROLLBACK,
        "begin",
        store.begin(TransactionRequest::new()).await,
    );
    let put = store
        .put_objects(vec![feature(NEW_1, "in a transaction").read(registry)], false)
        .await;
    rule!(
        &TX_ROLLBACK,
        put.len() == 1 && put[0].is_ok(),
        "put inside the transaction answered {}",
        briefs(&put)
    );
    let deleted = store.delete_objects(&[before.uri(COLUMN)], false).await;
    rule!(
        &TX_ROLLBACK,
        deleted.len() == 1 && deleted[0].is_ok(),
        "delete inside the transaction answered {}",
        briefs(&deleted)
    );
    must(&TX_ROLLBACK, "rollback", store.rollback(&transaction).await);
    let after: BTreeSet<String> = listing(&store, &TX_ROLLBACK)
        .await
        .into_iter()
        .map(|summary| summary.uri)
        .collect();
    rule!(
        &TX_ROLLBACK,
        after == before.uris(),
        "after the rollback the store lists {} objects, it listed {} before",
        after.len(),
        before.summaries.len()
    );
}

async fn commit_keeps<S: EnergymlWriter + Send + Sync>(make: &dyn Fn() -> S, registry: &Registry) {
    let mut store = make();
    let before = held(&store).await;
    let transaction = must(&TX_COMMIT, "begin", store.begin(TransactionRequest::new()).await);
    let put = store
        .put_objects(vec![feature(NEW_1, "in a transaction").read(registry)], false)
        .await;
    rule!(
        &TX_COMMIT,
        put.len() == 1 && put[0].is_ok(),
        "put inside the transaction answered {}",
        briefs(&put)
    );
    let column = before.uri(COLUMN);
    let deleted = store.delete_objects(std::slice::from_ref(&column), false).await;
    rule!(
        &TX_COMMIT,
        deleted.len() == 1 && deleted[0].is_ok(),
        "delete inside the transaction answered {}",
        briefs(&deleted)
    );
    must(&TX_COMMIT, "commit", store.commit(&transaction).await);
    let after = listing(&store, &TX_COMMIT).await;
    rule!(
        &TX_COMMIT,
        after
            .iter()
            .any(|summary| summary.uuid.eq_ignore_ascii_case(NEW_1))
            && !after.iter().any(|summary| summary.uri == column),
        "after the commit the store does not hold the put and lack the deleted"
    );
}

async fn ended_is_over<S: EnergymlWriter + Send + Sync>(make: &dyn Fn() -> S) {
    let mut store = make();
    held(&store).await;
    rule!(
        &TX_ENDED,
        store.open_transaction().is_none(),
        "a transaction is open on a fresh store"
    );
    let first = must(&TX_ENDED, "begin", store.begin(TransactionRequest::new()).await);
    must(&TX_ENDED, "commit", store.commit(&first).await);
    rule!(
        &TX_ENDED,
        store.open_transaction().is_none(),
        "a transaction is still open after a commit"
    );
    let again = store.commit(&first).await;
    rule!(
        &TX_ENDED,
        matches!(again, Err(Error::Transaction { .. })),
        "an ended transaction ended again, answering {}",
        brief(&again)
    );
    let second = must(
        &TX_ENDED,
        "begin after a commit",
        store.begin(TransactionRequest::new()).await,
    );
    must(&TX_ENDED, "rollback", store.rollback(&second).await);
    rule!(
        &TX_ENDED,
        store.open_transaction().is_none(),
        "a transaction is still open after a rollback"
    );
    let again = store.rollback(&second).await;
    rule!(
        &TX_ENDED,
        matches!(again, Err(Error::Transaction { .. })),
        "a rolled back transaction ended again, answering {}",
        brief(&again)
    );
}

async fn read_only_writes_nothing<S: EnergymlWriter + Send + Sync>(
    make: &dyn Fn() -> S,
    registry: &Registry,
) {
    let mut store = make();
    let before = held(&store).await;
    let transaction = must(
        &TX_READ_ONLY,
        "begin",
        store.begin(TransactionRequest::new().read_only()).await,
    );
    let put = store
        .put_objects(vec![feature(NEW_1, "refused").read(registry)], false)
        .await;
    rule!(
        &TX_READ_ONLY,
        matches!(put.as_slice(), [Err(Error::Transaction { .. })]),
        "put in a read-only transaction answered {}",
        briefs(&put)
    );
    let deleted = store.delete_objects(&[before.uri(COLUMN)], false).await;
    rule!(
        &TX_READ_ONLY,
        matches!(deleted.as_slice(), [Err(Error::Transaction { .. })]),
        "delete in a read-only transaction answered {}",
        briefs(&deleted)
    );
    must(&TX_READ_ONLY, "rollback", store.rollback(&transaction).await);
    let after: BTreeSet<String> = listing(&store, &TX_READ_ONLY)
        .await
        .into_iter()
        .map(|summary| summary.uri)
        .collect();
    rule!(
        &TX_READ_ONLY,
        after == before.uris(),
        "the store changed under a read-only transaction"
    );
}

async fn other_dataspace_refused<S: EnergymlWriter + Send + Sync>(make: &dyn Fn() -> S) {
    let mut store = make();
    let state = held(&store).await;
    let elsewhere = "eml:///dataspace('contract-elsewhere')";
    let refused = store.begin(TransactionRequest::new().over([elsewhere])).await;
    rule!(
        &TX_DATASPACE,
        matches!(refused, Err(Error::NotFound(_))),
        "a transaction over another dataspace answered {}",
        brief(&refused)
    );
    rule!(
        &TX_DATASPACE,
        store.open_transaction().is_none(),
        "a refused begin left a transaction open"
    );
    let own = must(
        &TX_DATASPACE,
        "begin over the store's own dataspace",
        store
            .begin(TransactionRequest::new().over([state.dataspace.clone()]))
            .await,
    );
    must(&TX_DATASPACE, "rollback", store.rollback(&own).await);
}

// ---------------------------------------------------------------------------
// Raw objects
// ---------------------------------------------------------------------------

/// **Raw objects** (ETP 1.2 section 9.3.1, `format`): a format the capabilities
/// list answers bytes under the full URI; a format they do not list is refused
/// by name, per object.
///
/// Gap: the JSON form is `TODO(verify)` (which JSON), so for it only a
/// non-empty answer is asserted.
pub async fn raw_objects<S: EnergymlStorage + Sync>(make: &dyn Fn() -> S) {
    let store = make();
    let state = held(&store).await;
    let capabilities = store.capabilities();
    let uris: Vec<String> = state
        .summaries
        .iter()
        .map(|summary| summary.uri.clone())
        .collect();
    for format in [RawFormat::Xml, RawFormat::Json] {
        let answers = store.get_raw(&uris, format).await;
        if capabilities.raw_formats.contains(&format) {
            rule!(
                &RAW_ANSWER,
                answers.len() == uris.len(),
                "{} URIs asked in {format:?}, {} results",
                uris.len(),
                answers.len()
            );
            for (uri, answer) in uris.iter().zip(&answers) {
                let raw = match answer {
                    Ok(raw) => raw,
                    Err(error) => broken(
                        &RAW_ANSWER,
                        format!("get_raw({format:?}) of `{uri}` failed: {error}"),
                    ),
                };
                rule!(
                    &RAW_ANSWER,
                    &raw.uri == uri,
                    "asked `{uri}`, answered under `{}`",
                    raw.uri
                );
                rule!(
                    &RAW_ANSWER,
                    raw.format == format,
                    "asked {format:?}, answered {:?}",
                    raw.format
                );
                rule!(
                    &RAW_ANSWER,
                    !raw.bytes.is_empty(),
                    "`{uri}` answered no bytes in {format:?}"
                );
                if format == RawFormat::Xml {
                    let text = String::from_utf8_lossy(&raw.bytes).to_ascii_lowercase();
                    rule!(
                        &RAW_ANSWER,
                        text.contains(&uuid_of(uri)),
                        "the XML answered for `{uri}` does not name its uuid"
                    );
                }
            }
            let absent = state.uri_like(FEATURE, ABSENT);
            let mixed = store.get_raw(&[state.uri(FEATURE), absent], format).await;
            rule!(
                &RAW_ABSENT,
                mixed.len() == 2 && mixed[0].is_ok() && matches!(mixed[1], Err(Error::NotFound(_))),
                "[held, absent] in {format:?} answered {}",
                briefs(&mixed)
            );
        } else {
            rule!(
                &RAW_REFUSED,
                answers.len() == uris.len()
                    && answers
                        .iter()
                        .all(|answer| matches!(answer, Err(Error::Unsupported { .. }))),
                "{format:?} is not in raw_formats and get_raw answered {}",
                briefs(&answers)
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

fn context(uri: String, scope: Scope, depth: u32) -> Context {
    Context::of(uri, scope).with_depth(depth)
}

fn typed(mut context: Context, types: &[&str]) -> Context {
    context.types = types.iter().map(|t| t.to_string()).collect();
    context
}

/// Ask, and check the objects reached are exactly `want`, and the edges inside.
async fn reaches<S: EnergymlStorage + Sync>(
    store: &S,
    rule: &Rule,
    context: Context,
    want: &[&str],
) -> Found {
    let found = match store.resources_in(&context).await {
        Ok(found) => found,
        Err(error) => broken(rule, format!("{context:?} was refused: {error}")),
    };
    let got = uuids_of(&found);
    let want = set(want);
    rule!(
        rule,
        got == want,
        "{context:?} reached {}, expected {}",
        show(&got),
        show(&want)
    );
    let inside: BTreeSet<&str> = found.resources.iter().map(|r| r.uri.as_str()).collect();
    for edge in &found.edges {
        rule!(
            &DISC_EDGES,
            inside.contains(edge.from.as_str()) || inside.contains(edge.to.as_str()),
            "{context:?}: the edge {} -> {} has neither end among the objects found",
            edge.from,
            edge.to
        );
    }
    found
}

/// **Discovery** over the model (ETP 1.2 section 8): a dataspace URI ignores
/// scope and depth, targets and sources to a depth, the types after the walk, a
/// wildcard, edges inside the answer, counts, and the refusal of a filter on a
/// fact the store does not state.
pub async fn discovery<S: EnergymlStorage + Sync>(make: &dyn Fn() -> S) {
    let store = make();
    let state = held(&store).await;
    let ds = state.dataspace.clone();
    let everything = [FEATURE, HORIZON_1, HORIZON_2, COLUMN, DANGLING];

    // The dataspace and its listing.
    let dataspaces = must(&DISC_LISTING, "dataspaces()", store.dataspaces().await);
    rule!(
        &DISC_LISTING,
        dataspaces.iter().any(|d| d.uri == ds),
        "the dataspace of the held objects, `{ds}`, is not among {:?}",
        dataspaces.iter().map(|d| &d.uri).collect::<Vec<_>>()
    );
    let plain = reaches(&store, &DISC_LISTING, Context::dataspace(ds.clone()), &everything).await;
    let listed: BTreeSet<String> = state.summaries.iter().map(|s| s.uri.clone()).collect();
    rule!(
        &DISC_LISTING,
        plain
            .resources
            .iter()
            .map(|s| s.uri.clone())
            .collect::<BTreeSet<_>>()
            == listed,
        "the dataspace context and resources() list different URIs"
    );
    let stated = model().stated();
    rule!(
        &DISC_EDGES,
        pairs(&plain.edges) == stated,
        "the edges of the dataspace are {}, the model's are {}",
        show_pairs(&pairs(&plain.edges)),
        show_pairs(&stated)
    );

    // A dataspace URI ignores scope and depth.
    for (scope, depth) in [
        (Scope::Itself, 1),
        (Scope::Sources, 1),
        (Scope::Targets, 5),
        (Scope::SourcesOrSelf, 2),
        (Scope::TargetsOrSelf, 3),
    ] {
        reaches(
            &store,
            &DISC_DATASPACE,
            context(ds.clone(), scope, depth),
            &everything,
        )
        .await;
    }

    // Targets, to a depth.
    let column = state.uri(COLUMN);
    reaches(
        &store,
        &DISC_TARGETS,
        context(column.clone(), Scope::Targets, 1),
        &[HORIZON_1],
    )
    .await;
    let two = reaches(
        &store,
        &DISC_TARGETS,
        context(column.clone(), Scope::Targets, 2),
        &[HORIZON_1, FEATURE],
    )
    .await;
    // One end found is enough (ETP 1.2 section 8.3.1, p. 163, 3.c): the column
    // is the start, not in a `Targets` answer, yet its edge to the horizon is
    // answered; so is the second horizon's edge to the feature found.
    let walked = wanted_pairs(&[(COLUMN, HORIZON_1), (HORIZON_1, FEATURE), (HORIZON_2, FEATURE)]);
    rule!(
        &DISC_EDGES,
        pairs(&two.edges) == walked,
        "the edges between what a depth 2 walk from the column reached are {}, expected {}",
        show_pairs(&pairs(&two.edges)),
        show_pairs(&walked)
    );
    reaches(
        &store,
        &DISC_TARGETS,
        context(column.clone(), Scope::Targets, 5),
        &[HORIZON_1, FEATURE],
    )
    .await;
    reaches(
        &store,
        &DISC_TARGETS,
        context(column.clone(), Scope::TargetsOrSelf, 1),
        &[COLUMN, HORIZON_1],
    )
    .await;

    // Sources, to a depth.
    let feature_uri = state.uri(FEATURE);
    reaches(
        &store,
        &DISC_SOURCES,
        context(feature_uri.clone(), Scope::Sources, 1),
        &[HORIZON_1, HORIZON_2],
    )
    .await;
    reaches(
        &store,
        &DISC_SOURCES,
        context(feature_uri.clone(), Scope::Sources, 2),
        &[HORIZON_1, HORIZON_2, COLUMN],
    )
    .await;
    reaches(
        &store,
        &DISC_SOURCES,
        context(feature_uri.clone(), Scope::SourcesOrSelf, 1),
        &[FEATURE, HORIZON_1, HORIZON_2],
    )
    .await;
    reaches(
        &store,
        &DISC_SOURCES,
        context(column.clone(), Scope::Sources, 3),
        &[],
    )
    .await;

    // Itself, whatever the depth.
    let horizon = state.uri(HORIZON_1);
    reaches(
        &store,
        &DISC_ITSELF,
        context(horizon, Scope::Itself, 5),
        &[HORIZON_1],
    )
    .await;

    // A reference to an object the store does not hold reaches nothing.
    let dangling = state.uri(DANGLING);
    reaches(
        &store,
        &DISC_DANGLING,
        context(dangling.clone(), Scope::Targets, 2),
        &[],
    )
    .await;
    reaches(
        &store,
        &DISC_DANGLING,
        context(dangling, Scope::TargetsOrSelf, 2),
        &[DANGLING],
    )
    .await;

    // The types, after the walk.
    reaches(
        &store,
        &DISC_TYPES,
        typed(
            context(column.clone(), Scope::Targets, 2),
            &["resqml22.BoundaryFeature"],
        ),
        &[FEATURE],
    )
    .await;
    reaches(
        &store,
        &DISC_TYPES,
        typed(
            context(feature_uri.clone(), Scope::Sources, 2),
            &["resqml22.HorizonInterpretation"],
        ),
        &[HORIZON_1, HORIZON_2],
    )
    .await;
    reaches(
        &store,
        &DISC_TYPES,
        typed(
            context(feature_uri.clone(), Scope::Sources, 2),
            &["resqml22.StratigraphicColumn"],
        ),
        &[COLUMN],
    )
    .await;

    // The wildcard.
    reaches(
        &store,
        &DISC_WILDCARD,
        typed(Context::dataspace(ds.clone()), &["resqml22.*"]),
        &everything,
    )
    .await;
    reaches(
        &store,
        &DISC_WILDCARD,
        typed(
            Context::dataspace(ds.clone()),
            &["resqml22.HorizonInterpretation"],
        ),
        &[HORIZON_1, HORIZON_2, DANGLING],
    )
    .await;
    reaches(
        &store,
        &DISC_WILDCARD,
        typed(
            Context::dataspace(ds.clone()),
            &["resqml22.TectonicBoundaryFeature"],
        ),
        &[],
    )
    .await;
    reaches(
        &store,
        &DISC_WILDCARD,
        typed(context(feature_uri.clone(), Scope::Sources, 2), &["resqml22.*"]),
        &[HORIZON_1, HORIZON_2, COLUMN],
    )
    .await;

    counts(&store, &state).await;

    // types_of counts what a context reached.
    let types = types_of(&plain);
    let expected = vec![
        ("resqml22.BoundaryFeature".to_string(), 1),
        ("resqml22.HorizonInterpretation".to_string(), 3),
        ("resqml22.StratigraphicColumn".to_string(), 1),
    ];
    rule!(
        &DISC_SUPPORTED_TYPES,
        types == expected,
        "types_of answered {types:?}, expected {expected:?}"
    );

    unstated_facts(&store, &state).await;
}

async fn counts<S: EnergymlStorage + Sync>(store: &S, state: &Held) {
    let model = model();
    let edges = must(&DISC_COUNTS, "edges()", store.edges(&[]).await);
    let mut sources: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut targets: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for edge in &edges {
        sources
            .entry(uuid_of(&edge.to))
            .or_default()
            .insert(uuid_of(&edge.from));
        targets
            .entry(uuid_of(&edge.from))
            .or_default()
            .insert(uuid_of(&edge.to));
    }

    let mut asked = Context::dataspace(state.dataspace.clone());
    asked.count_objects = true;
    let found = reaches(
        store,
        &DISC_COUNTS,
        asked,
        &[FEATURE, HORIZON_1, HORIZON_2, COLUMN, DANGLING],
    )
    .await;
    for summary in &found.resources {
        let uuid = summary.uuid.to_ascii_lowercase();
        let from_edges = (
            sources.get(&uuid).map_or(0, BTreeSet::len) as u32,
            targets.get(&uuid).map_or(0, BTreeSet::len) as u32,
        );
        let from_model = model.counts(&uuid);
        rule!(
            &DISC_COUNTS,
            (summary.source_count, summary.target_count) == (Some(from_model.0), Some(from_model.1)),
            "{}: counted {:?} sources and {:?} targets, the model's references say {:?}",
            label(&uuid),
            summary.source_count,
            summary.target_count,
            from_model
        );
        rule!(
            &DISC_COUNTS,
            from_edges == from_model,
            "{}: edges() give {:?} (sources, targets), the model's references {:?}",
            label(&uuid),
            from_edges,
            from_model
        );
    }

    // The counts are the object's own, not those inside the answer.
    let mut walk = context(state.uri(COLUMN), Scope::Targets, 1);
    walk.count_objects = true;
    let found = reaches(store, &DISC_COUNTS, walk, &[HORIZON_1]).await;
    let horizon = &found.resources[0];
    rule!(
        &DISC_COUNTS,
        (horizon.source_count, horizon.target_count) == (Some(1), Some(1)),
        "the horizon reached from the column has {:?} sources and {:?} targets, expected one and one",
        horizon.source_count,
        horizon.target_count
    );

    // Not asked, not stated.
    let plain = reaches(
        store,
        &DISC_COUNTS,
        Context::dataspace(state.dataspace.clone()),
        &[FEATURE, HORIZON_1, HORIZON_2, COLUMN, DANGLING],
    )
    .await;
    rule!(
        &DISC_COUNTS,
        plain
            .resources
            .iter()
            .all(|r| r.source_count.is_none() && r.target_count.is_none()),
        "counts were stated though the context did not ask for them"
    );
}

async fn unstated_facts<S: EnergymlStorage + Sync>(store: &S, state: &Held) {
    let capabilities = store.capabilities();
    let ds = state.dataspace.clone();

    // The active status (no capability says it: read from the summaries).
    let mut by_status = Context::dataspace(ds.clone());
    by_status.active_status = Some(ActiveStatus::Inactive);
    let answer = store.resources_in(&by_status).await;
    if state.summaries.iter().any(|s| s.active_status.is_none()) {
        rule!(
            &DISC_STATUS,
            matches!(answer, Err(Error::Unsupported { .. })),
            "an object states no active status and the filter answered {}",
            brief(&answer)
        );
    } else {
        let found = must(&DISC_STATUS, "the filter on a stated status", answer);
        rule!(
            &DISC_STATUS,
            found
                .resources
                .iter()
                .all(|r| r.active_status == Some(ActiveStatus::Inactive)),
            "the filter on Inactive returned an object that is not"
        );
    }

    // The store times.
    let stated_times = |s: &ObjectSummary| s.store_created.is_some() && s.store_last_write.is_some();
    let invented = |s: &ObjectSummary| s.store_created.is_some() || s.store_last_write.is_some();
    let mut by_time = Context::dataspace(ds.clone());
    by_time.store_last_write_after = Some(0);
    let answer = store.resources_in(&by_time).await;
    if capabilities.store_times {
        rule!(
            &DISC_TIMES,
            state.summaries.iter().all(stated_times),
            "the store keeps times and an object states none"
        );
        let found = must(&DISC_TIMES, "the filter on the store's last write", answer);
        rule!(
            &DISC_TIMES,
            found
                .resources
                .iter()
                .all(|r| r.store_last_write.is_some_and(|t| t > 0)),
            "the filter returned an object not written after 0"
        );
    } else {
        rule!(
            &DISC_TIMES,
            !state.summaries.iter().any(invented),
            "the store keeps no times and an object states one"
        );
        rule!(
            &DISC_TIMES,
            matches!(answer, Err(Error::Unsupported { .. })),
            "the store keeps no times and the filter answered {}",
            brief(&answer)
        );
    }

    // The kinds of the edges.
    let start = state.uri(COLUMN);
    for (what, kinded) in [
        ("Navigable::Primary", {
            let mut c = context(start.clone(), Scope::Targets, 2);
            c.navigable = Navigable::Primary;
            c
        }),
        ("Navigable::Secondary", {
            let mut c = context(start.clone(), Scope::Targets, 2);
            c.navigable = Navigable::Secondary;
            c
        }),
        ("include_secondary_targets", {
            let mut c = context(start.clone(), Scope::Targets, 2);
            c.include_secondary_targets = true;
            c
        }),
        ("include_secondary_sources", {
            let mut c = context(start.clone(), Scope::Targets, 2);
            c.include_secondary_sources = true;
            c
        }),
    ] {
        let answer = store.resources_in(&kinded).await;
        if capabilities.edge_kinds {
            let found = must(&DISC_KINDS, what, answer);
            rule!(
                &DISC_KINDS,
                found.edges.iter().all(|edge| edge.kind.is_some()),
                "{what}: the store states edge kinds and an edge of the answer has none"
            );
        } else {
            rule!(
                &DISC_KINDS,
                matches!(answer, Err(Error::Unsupported { .. })),
                "{what}: the store states no edge kind and the request answered {}",
                brief(&answer)
            );
        }
    }
}

// ---------------------------------------------------------------------------
// References and edges
// ---------------------------------------------------------------------------

/// **References and edges** (design note section 3): `references()` is what the
/// objects write, a reference to an absent object included; `edges()` is the
/// relationships between objects the store holds.
///
/// Gap: only the whole-store form (`&[]`) is asserted, see the module doc.
pub async fn references<S: EnergymlStorage + Sync>(make: &dyn Fn() -> S) {
    let store = make();
    let state = held(&store).await;
    let model = model();
    let written = must(&REF_WRITTEN, "references()", store.references(&[]).await);
    let stated = must(&REF_HELD, "edges()", store.edges(&[]).await);

    let held_uris = state.uris();
    for edge in &stated {
        rule!(
            &REF_HELD,
            held_uris.contains(&edge.from) && held_uris.contains(&edge.to),
            "the edge {} -> {} has an end the store does not hold",
            edge.from,
            edge.to
        );
    }

    let written_pairs = pairs(&written);
    let stated_pairs = pairs(&stated);
    rule!(
        &REF_SUBSET,
        stated_pairs.is_subset(&written_pairs),
        "edges {} are not all among references {}",
        show_pairs(&stated_pairs),
        show_pairs(&written_pairs)
    );

    rule!(
        &REF_DANGLING,
        written_pairs.contains(&(DANGLING.to_string(), ABSENT.to_string())),
        "the reference to the absent object is not in references(): {}",
        show_pairs(&written_pairs)
    );
    rule!(
        &REF_DANGLING,
        !stated_pairs.iter().any(|(_, to)| to == ABSENT),
        "an edge leads to the absent object: {}",
        show_pairs(&stated_pairs)
    );

    rule!(
        &REF_WRITTEN,
        written_pairs == model.written(),
        "references() are {}, the model writes {}",
        show_pairs(&written_pairs),
        show_pairs(&model.written())
    );
    rule!(
        &REF_WRITTEN,
        stated_pairs == model.stated(),
        "edges() are {}, the model states {}",
        show_pairs(&stated_pairs),
        show_pairs(&model.stated())
    );
    for edge in &written {
        rule!(
            &REF_WRITTEN,
            !edge.path.trim().is_empty(),
            "the reference {} -> {} carries no field path",
            edge.from,
            edge.to
        );
    }

    let graph = must(&REF_GRAPH, "graph()", store.graph(&[]).await);
    for object in &model.objects {
        let node = graph
            .nodes
            .iter()
            .find(|node| node.uuid.eq_ignore_ascii_case(&object.uuid));
        rule!(
            &REF_GRAPH,
            node.is_some_and(|node| node.present),
            "the held object {} has no present node",
            label(&object.uuid)
        );
    }
    let absent = graph
        .nodes
        .iter()
        .find(|node| node.uuid.eq_ignore_ascii_case(ABSENT));
    rule!(
        &REF_GRAPH,
        absent.is_some_and(|node| !node.present),
        "the referenced absent object has no node, or a present one"
    );
}

// ---------------------------------------------------------------------------
// Capabilities agree with behaviour
// ---------------------------------------------------------------------------

/// **Capabilities agree with behaviour**: deletions, dataspaces, the dataspace
/// door, arrays. Each runs as its own group too ([`deletions`], [`dataspace_creation`],
/// [`dataspace_door`], [`array_refusals`]) so a store can name the one it fails.
pub async fn capabilities<S: EnergymlWriter + EnergymlDataspaces + Send + Sync>(
    make: &dyn Fn() -> S,
    registry: &Registry,
) {
    deletions(make).await;
    dataspace_creation(make).await;
    dataspace_door(make, registry).await;
    array_refusals(make).await;
}

/// `deletions_kept_for` off: [`EnergymlStorage::deleted_resources`] is refused by
/// name, never an empty list. On: it lists the object just deleted.
pub async fn deletions<S: EnergymlWriter + Send + Sync>(make: &dyn Fn() -> S) {
    let mut store = make();
    let state = held(&store).await;
    let filter = DeletedFilter::of(state.dataspace.clone());
    if store.capabilities().deletions_kept_for.is_none() {
        let answer = store.deleted_resources(&filter).await;
        rule!(
            &CAP_DELETIONS,
            matches!(answer, Err(Error::Unsupported { .. })),
            "deletions are not kept and deleted_resources answered {}",
            brief(&answer)
        );
        return;
    }
    let column = state.uri(COLUMN);
    if store.capabilities().writes {
        let deleted = store.delete_objects(std::slice::from_ref(&column), false).await;
        rule!(
            &CAP_DELETIONS,
            deleted.len() == 1 && deleted[0].is_ok(),
            "delete answered {}",
            briefs(&deleted)
        );
    }
    let listed = must(
        &CAP_DELETIONS,
        "deleted_resources",
        store.deleted_resources(&filter).await,
    );
    rule!(
        &CAP_DELETIONS,
        listed.iter().any(|d| d.uri == column),
        "the deleted object `{column}` is not among the deletions kept"
    );
}

/// `several_dataspaces` off: creating and deleting a dataspace are refused by
/// name. On: a created dataspace is listed, a deleted one is not.
pub async fn dataspace_creation<S: EnergymlDataspaces + EnergymlStorage + Send + Sync>(make: &dyn Fn() -> S) {
    let mut store = make();
    let state = held(&store).await;
    let new = "eml:///dataspace('contract-new')";
    if !store.capabilities().several_dataspaces {
        let created = store
            .put_dataspaces(vec![Dataspace::new(new, "contract-new")])
            .await;
        rule!(
            &CAP_DATASPACES,
            matches!(created.as_slice(), [Err(Error::Unsupported { .. })]),
            "the store holds one dataspace and creating another answered {}",
            briefs(&created)
        );
        let deleted = store
            .delete_dataspaces(std::slice::from_ref(&state.dataspace))
            .await;
        rule!(
            &CAP_DATASPACES,
            matches!(deleted.as_slice(), [Err(Error::Unsupported { .. })]),
            "the store holds one dataspace and deleting it answered {}",
            briefs(&deleted)
        );
        return;
    }
    let created = store
        .put_dataspaces(vec![Dataspace::new(new, "contract-new")])
        .await;
    rule!(
        &CAP_DATASPACES,
        created.len() == 1 && created[0].is_ok(),
        "creating answered {}",
        briefs(&created)
    );
    let listed = must(&CAP_DATASPACES, "list_dataspaces", store.list_dataspaces().await);
    rule!(
        &CAP_DATASPACES,
        listed.iter().any(|d| d.uri == new),
        "the created dataspace is not listed"
    );
    let deleted = store.delete_dataspaces(&[new.to_string()]).await;
    rule!(
        &CAP_DATASPACES,
        deleted.len() == 1 && deleted[0].is_ok(),
        "deleting answered {}",
        briefs(&deleted)
    );
    let listed = must(&CAP_DATASPACES, "list_dataspaces", store.list_dataspaces().await);
    rule!(
        &CAP_DATASPACES,
        !listed.iter().any(|d| d.uri == new) && listed.iter().any(|d| d.uri == state.dataspace),
        "after deleting the created dataspace the store lists {:?}",
        listed.iter().map(|d| &d.uri).collect::<Vec<_>>()
    );
}

/// `open_dataspace` reaches the store's own dataspace (and the writer it gives
/// writes into it), and is `NotFound` for a dataspace the store does not list
/// and for an object URI.
pub async fn dataspace_door<S: EnergymlWriter + EnergymlDataspaces + Send + Sync>(
    make: &dyn Fn() -> S,
    registry: &Registry,
) {
    let mut store = make();
    let state = held(&store).await;
    let listed = must(&CAP_DOOR, "list_dataspaces", store.list_dataspaces().await);
    rule!(
        &CAP_DOOR,
        listed.iter().any(|d| d.uri == state.dataspace),
        "list_dataspaces does not list the store's own `{}`: {:?}",
        state.dataspace,
        listed.iter().map(|d| &d.uri).collect::<Vec<_>>()
    );
    let of_storage = must(&CAP_DOOR, "dataspaces", store.dataspaces().await);
    let a: BTreeSet<&String> = listed.iter().map(|d| &d.uri).collect();
    let b: BTreeSet<&String> = of_storage.iter().map(|d| &d.uri).collect();
    rule!(
        &CAP_DOOR,
        a == b,
        "list_dataspaces and dataspaces list different dataspaces"
    );

    let elsewhere = "eml:///dataspace('contract-elsewhere')";
    if !listed.iter().any(|d| d.uri == elsewhere) {
        let refused = store.open_dataspace(elsewhere).err();
        rule!(
            &CAP_DOOR,
            matches!(refused, Some(Error::NotFound(_))),
            "open_dataspace of a dataspace the store does not list answered {:?}",
            refused.map(|e| e.to_string())
        );
    }
    let object = state.uri(FEATURE);
    let refused = store.open_dataspace(&object).err();
    rule!(
        &CAP_DOOR,
        matches!(refused, Some(Error::NotFound(_))),
        "open_dataspace of an object URI answered {:?}",
        refused.map(|e| e.to_string())
    );

    if !store.capabilities().writes {
        let own = store.open_dataspace(&state.dataspace).is_ok();
        rule!(
            &CAP_DOOR,
            own,
            "open_dataspace of the store's own dataspace was refused"
        );
        return;
    }
    let put = match store.open_dataspace(&state.dataspace) {
        Ok(writer) => {
            writer
                .put_objects(vec![feature(NEW_1, "through the door").read(registry)], false)
                .await
        }
        Err(error) => broken(
            &CAP_DOOR,
            format!("open_dataspace of the store's own dataspace: {error}"),
        ),
    };
    rule!(
        &CAP_DOOR,
        put.len() == 1 && put[0].is_ok(),
        "the writer of the own dataspace answered {}",
        briefs(&put)
    );
    let after = listing(&store, &CAP_DOOR).await;
    rule!(
        &CAP_DOOR,
        after.iter().any(|s| s.uuid.eq_ignore_ascii_case(NEW_1)),
        "an object put through open_dataspace is not in the store"
    );
}

/// `subarrays` or `uninitialized_arrays` off: writing part of an array, or
/// declaring one before it is written, is refused by name.
///
/// Gap: with the flag on, the behaviour needs a store-specific
/// [`ArrayLocation`]; it is not asserted.
pub async fn array_refusals<S: EnergymlWriter + Send + Sync>(make: &dyn Fn() -> S) {
    let mut store = make();
    held(&store).await;
    let capabilities = store.capabilities();
    if !capabilities.writes {
        return;
    }
    let data = ArrayData {
        metadata: ArrayMetadata::new(vec![2], "double"),
        values: ArrayValues::F64(vec![1.0, 2.0]),
    };
    let whole = ArrayLocation {
        file: "contract.h5".into(),
        path_in_file: "/a".into(),
        ..ArrayLocation::default()
    };
    if !capabilities.subarrays {
        let part = ArrayLocation {
            start: vec![0],
            count: vec![1],
            ..whole.clone()
        };
        let written = store.put_arrays(&[(part, data.clone())]).await;
        rule!(
            &CAP_ARRAYS,
            matches!(written.as_slice(), [Err(Error::Unsupported { .. })]),
            "subarrays are off and writing part of an array answered {}",
            briefs(&written)
        );
    }
    if !capabilities.uninitialized_arrays {
        let declared = store.declare_arrays(&[(whole, data.metadata.clone())]).await;
        rule!(
            &CAP_ARRAYS,
            matches!(declared.as_slice(), [Err(Error::Unsupported { .. })]),
            "uninitialized arrays are off and declaring one answered {}",
            briefs(&declared)
        );
    }
}

// ---------------------------------------------------------------------------
// Everything
// ---------------------------------------------------------------------------

/// Every group, in turn, on stores the factory builds fresh each time.
pub async fn all<S: EnergymlWriter + EnergymlDataspaces + Send + Sync>(
    make: &dyn Fn() -> S,
    registry: &Registry,
) {
    objects(make, registry).await;
    containment(make, registry).await;
    transactions(make, registry).await;
    raw_objects(make).await;
    discovery(make).await;
    references(make).await;
    capabilities(make, registry).await;
}
