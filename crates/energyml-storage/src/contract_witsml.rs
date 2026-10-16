//! The storage contract for **objects contained by value** (WITSML containers),
//! written from the ETP 1.2 specification and the WITSML 2.1 schemas, not from
//! the code it judges.
//!
//! [`crate::contract`] has a RESQML model with no containment. This module adds
//! a second model, [`model_witsml`], and the groups that play on it the rules of
//! containment (design note `genre-des-aretes-et-objets-contenus.md`, sections
//! 4 and 5.1). Like the first contract it is written once and played on each
//! implementation: a factory builds a *fresh* store holding exactly the model.
//!
//! # The model
//!
//! Written by hand from the XSD (`witsml/v2.1`, EML 2.3), each mandatory element
//! cited at the builder that writes it. In short:
//!
//! ```text
//!   Well <-- Wellbore                                  (parts)
//!   Log  (a part) holds by value:
//!     ChannelSet            holds Channel A, Channel SHARED
//!     PPFGChannelSet        holds Channel B, Channel SHARED   (xsi:type)
//!   Channel SHARED --(Wellbore, a DOR)--> Wellbore
//! ```
//!
//! Three objects are parts ([`WELL`], [`WELLBORE`], [`LOG`]); five are contained
//! ([`SET_A`], [`SET_PPFG`], [`CHANNEL_ONLY_A`], [`CHANNEL_ONLY_B`],
//! [`CHANNEL_SHARED`]). A `ChannelSet` is both contained (by the `Log`) and a
//! container (of `Channel`s); [`CHANNEL_SHARED`] is in two containers, which the
//! specification allows (ETP 1.2 section 9.1.3, p. 178). The model describes what
//! it contains independently of any store ([`WitsmlModel`]); the groups compare
//! the store with that description and never with the store itself.
//!
//! A second model, [`model_witsml_with_part`], is the same plus a standalone
//! `Channel` part with the uuid of [`CHANNEL_ONLY_A`] (an object held both as a
//! part and by value).
//!
//! # The groups
//!
//! Each group is an `async fn` taking factories, as in [`crate::contract`]; a
//! rule that breaks **panics** with its id, text and source ([`rules::ALL`]).
//! Every group reads [`crate::Capabilities`] and asserts the refusal when a
//! capability is off, the behaviour when it is on:
//!
//! | Group | Rules | Plays when |
//! |---|---|---|
//! | [`contents`] | CONT-2, CONT-8 | `contained_objects` |
//! | [`containment_edges`] | CONT-3, CONT-4, KIND-2 | `contained_objects` |
//! | [`contained_reads`] | CONT-5, CONT-6, CONT-7 | `contained_objects` |
//! | [`contained_references`] | REF-6 | `contained_objects` |
//! | [`contained_part`] | CONT-9 | `contained_objects` |
//! | [`contained_cost`] | CONT-10 | always (the scan counter is the caller's) |
//! | [`contained_unaware`] | CONT-11 | not `contained_objects` |
//! | [`contained_writes`] | CONT-12 | `contained_objects`, writes, not `prune_contained` |
//! | [`witsml`] | all of them | |
//!
//! # Known gaps
//!
//! - **The list of containers and contents** is the guide of each ML (ETP 1.2
//!   section 9.1.3, p. 178: not published for 1.2); the model holds the
//!   `Log`, `ChannelSet`, `Channel` triple, the one the specification names.
//! - **Pruning** (`prune_contained` on): what a server prunes is the server's to
//!   say; only the refusal while the capability is off is asserted
//!   ([`contained_writes`], [`crate::contract::containment`]).
//! - **`get_raw` of a contained object** is asserted only for the bytes it
//!   returns when it answers; a refusal by name is accepted ([`CONT_INLINE`]).
//! - **Which copy wins** when an object is both a part and held by value is a
//!   rule of the design note, not of the specification ([`CONT_ALSO_PART`]).
//! - **Several containers in several parts** (the same `Channel` in two `Log`
//!   parts) is not modelled: the specification does not say which copy a read
//!   returns.
//! - **`schemaVersion`** is written `2.1`, as [`crate::contract`] writes `2.2`;
//!   the XSD describes it as "the first 2 digits of an ML version"
//!   (`Abstract.xsd:307`). `TODO(verify)` on a real package.

use std::collections::BTreeSet;
use std::fmt::Display;

use energyml_core::identifiers::EtpUri;
use energyml_core::identity::{title_of_reflect, uuid_of_reflect};
use energyml_core::registry::Registry;

use crate::context::{Context, Found, Navigable, Scope};
use crate::contract::{ModelObject, Rule};
use crate::error::{Error, Result};
use crate::graph::{Edge, EdgeKind};
use crate::kinds::KindBasis;
use crate::raw::RawFormat;
use crate::resource::ObjectSummary;
use crate::storage::EnergymlStorage;
use crate::write::EnergymlWriter;

// ---------------------------------------------------------------------------
// The rules
// ---------------------------------------------------------------------------

macro_rules! rules {
    ($($name:ident = ($id:literal, $source:literal, $text:literal);)+) => {
        $(
            #[doc = $text]
            pub const $name: Rule = Rule { id: $id, source: $source, text: $text };
        )+
        /// Every rule of the containment contract.
        pub const ALL: &[Rule] = &[$($name),+];
    };
}

/// The rules, by name.
pub mod rules {
    use crate::contract::Rule;

    rules! {
        CONT_LISTED = ("CONT-2",
            "ETP 1.2 section 9.1.3 (p. 178) and section 9.1.3.1 (p. 179: contained objects are standalone data objects in their own right); design note genre-des-aretes section 4.3 and 4.4 (D1b-1: contents() lists them, resources() does not)",
            "contents() lists every object the model holds by value, once, under the canonical URI of its type and uuid (spelled as a part's) and with the URIs of its direct containers in contained_in; resources() lists the parts only; a dataspace context answers parts and contents.");
        CONT_EDGE = ("CONT-3",
            "ETP 1.2 section 8.1.1.1.1 (p. 156: for ByValue containment the contained object is the source, the container the target) and section 8.1.1.1.2 (p. 157: by-value is a characteristic of a primary relationship); design note section 4.5",
            "For each contained object and each of its direct containers there is exactly one edge from the object to the container, and none to a container of a container; when the store states edge kinds it is Primary, on a stated basis, the specification's example for a Log or ChannelSet container, or by-value for another.");
        CONT_WALK = ("CONT-4",
            "ETP 1.2 section 8.1.1.1.4 step 1 (p. 158) and section 8.2.2 row 9, 1.a.i (p. 167: an endpoint must inspect the DOR and ByValue references)",
            "A walk follows the containment edges: Targets from a Channel reaches its ChannelSet at depth 1 and the Log at depth 2; Sources from the Log reaches its ChannelSets, then their Channels.");
        CONT_GET = ("CONT-5",
            "ETP 1.2 section 9.1.3.1 (p. 179), section 9.2.1.1 steps 2 and 3 (one result per URI, ENOT_FOUND per URI) and section 9.2.2 row 15 (p. 188)",
            "get_objects of a contained object answers that object, with its own uuid, its own type and its own citation, never the container's; the answer has one result per URI in order, and a URI no object has fails for that URI alone.");
        CONT_HEADER = ("CONT-6",
            "ETP 1.2 section 9.1.3.1 (p. 179: a contained object is a data object in its own right); storage.rs header and headers; design note section 4.6",
            "headers and header of a contained object answer the citation of that object (title, uuid, type, under the URI asked), not the citation of its container.");
        CONT_INLINE = ("CONT-7",
            "ETP 1.2 section 9.2.2 row 15 (p. 188): a store must return contained objects in a container object, as defined by the container data object schema",
            "get_objects of a container answers the container; its XML, where the store answers one, holds the contained objects of the model inline, and a ChannelSet holds its own Channels and no other.");
        CONT_SHARED = ("CONT-8",
            "ETP 1.2 section 9.1.3 (p. 178: an object may be included in one or more container data objects); section 8.3.1 (p. 163: sourceCount and targetCount); design note section 4.8",
            "The same Channel in two ChannelSets is one resource and two edges: listed once, with both containers in contained_in, one edge to each, and counted once as a source of each container and once as a target of each.");
        CONT_ALSO_PART = ("CONT-9",
            "ETP 1.2 section 9.1.3.1 (p. 179: standalone data objects in their own right); design note section 4.8 (the part wins) - a rule of the note, not of the specification",
            "An object that is both a part and held by value is one resource, the part: one URI, its header and its object are the part's, and its edge to its container is still stated.");
        CONT_COST = ("CONT-10",
            "design note genre-des-aretes section 4.2 and decision D1b-2 (containment is discovered on demand: a listing never reads every object; a dataspace holding no container type opens no part more)",
            "resources() and headers() walk no part (the scan counter stays at zero); contents() walks the container parts once and a second call walks no more; a dataspace holding no container type is walked by nothing.");
        CONT_UNAWARE = ("CONT-11",
            "capabilities.rs contained_objects; design note section 4.3 (contents() is empty or refused when the store does not know containment); the principle that an unstated fact is refused by name",
            "A store that does not know containment lists no contained object, in any form (resources_in, contents, contained_in), answers no edge by value, and does not read a contained object by its URI (NotFound or Unsupported); it never answers a partial containment.");
        CONT_WRITES = ("CONT-12",
            "ETP 1.2 section 9.1.3.2 and section 9.2.2 row 12 (p. 186-187), rows 19 and 25 (p. 188, 191-192); design note section 4.9 (decisions D1b-8 and D1b-9: refused by name while the store cannot honour them)",
            "While prune_contained is off, writing alone an object that exists only by value, and deleting a container without prune, are refused by name (Unsupported) and change nothing.");
        KIND_EXAMPLES = ("KIND-2",
            "ETP 1.2 section 8.1.1.1.2 (p. 157): organizing Channels into ChannelSets or ChannelSets into Logs is primary; the reference from a Channel to a Wellbore is secondary",
            "When the store states edge kinds for WITSML 2.1: ChannelSet to Channel and Log to ChannelSet containments are Primary and Channel to Wellbore is Secondary, each on a stated basis (a server, or the rule of the specification), never a derived one; otherwise the edges have no kind and a request by kind that meets them is refused by name.");
        REF_CONTAINED = ("REF-6",
            "ETP 1.2 section 8.1.1.1.1 (p. 156: the data object that contains the DOR is the source); design note section 4.5 (the source of a DOR written in a contained object is that object)",
            "A DOR written inside a contained object has that object for its source: references() holds it from the Channel, never from a container, and every edge that is not a containment is among the references written.");
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

fn must<T>(rule: &Rule, what: &str, result: Result<T>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => broken(rule, format!("{what} failed: {error}")),
    }
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

// ---------------------------------------------------------------------------
// The model
// ---------------------------------------------------------------------------

/// A Well, a part.
pub const WELL: &str = "7f100000-0000-4000-8000-000000000001";
/// A Wellbore of [`WELL`], a part.
pub const WELLBORE: &str = "7f100000-0000-4000-8000-000000000002";
/// A Log, a part, holding [`SET_A`] and [`SET_PPFG`] by value.
pub const LOG: &str = "7f100000-0000-4000-8000-000000000003";
/// A ChannelSet held by [`LOG`], holding [`CHANNEL_ONLY_A`] and [`CHANNEL_SHARED`].
pub const SET_A: &str = "7f100000-0000-4000-8000-000000000004";
/// A PPFGChannelSet held by [`LOG`] (a ChannelSet written with `xsi:type`),
/// holding [`CHANNEL_ONLY_B`] and [`CHANNEL_SHARED`].
pub const SET_PPFG: &str = "7f100000-0000-4000-8000-000000000005";
/// A Channel held by [`SET_A`] only.
pub const CHANNEL_ONLY_A: &str = "7f100000-0000-4000-8000-000000000006";
/// A Channel held by [`SET_PPFG`] only.
pub const CHANNEL_ONLY_B: &str = "7f100000-0000-4000-8000-000000000007";
/// A Channel held by [`SET_A`] **and** by [`SET_PPFG`], with a DOR to [`WELLBORE`].
pub const CHANNEL_SHARED: &str = "7f100000-0000-4000-8000-000000000008";
/// A PropertyKind the model does **not** hold, referenced by every Channel.
pub const ABSENT_PROPERTY_KIND: &str = "7f100000-0000-4000-8000-0000000000fe";
/// A BusinessAssociate the model does **not** hold, referenced by every Channel.
pub const ABSENT_COMPANY: &str = "7f100000-0000-4000-8000-0000000000fd";
/// A Channel the model does **not** hold: how a group names an absent contained object.
pub const ABSENT_CHANNEL: &str = "7f100000-0000-4000-8000-0000000000fc";

/// The title of the standalone part of [`model_witsml_with_part`], which is not
/// the title of the inline copy.
pub const STANDALONE_TITLE: &str = "a depth channel read on its own";

const WITSML: &str = "http://www.energistics.org/energyml/data/witsmlv2";
const EML: &str = "http://www.energistics.org/energyml/data/commonv2";
const XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";

/// An object the model holds by value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelContent {
    /// Its uuid, lowercase.
    pub uuid: &'static str,
    /// `witsml21.Channel`: the type it is written as (`xsi:type` included).
    pub qualified_type: &'static str,
    /// Its citation title, as written inline.
    pub title: &'static str,
    /// The uuids of its **direct** containers (a Channel is not contained by
    /// the Log, only by its ChannelSets).
    pub containers: Vec<&'static str>,
}

/// A DOR the model writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelReference {
    /// The uuid of the object that **holds** the DOR in its text. A contained
    /// object holds its own DOR (REF-6).
    pub holder: &'static str,
    /// The XSD element that carries it (the last step of the path).
    pub element: &'static str,
    /// The uuid it points to.
    pub target: &'static str,
}

/// What a store must hold for the WITSML groups to run, described without any
/// store: the parts, the objects held by value with their direct containers, and
/// the DORs written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WitsmlModel {
    /// The parts: what `resources()` lists.
    pub parts: Vec<ModelObject>,
    /// The objects held by value inside those parts.
    pub contents: Vec<ModelContent>,
    /// The DORs written, including those to objects the model does not hold.
    pub references: Vec<ModelReference>,
}

impl WitsmlModel {
    /// The uuids of the parts.
    pub fn part_uuids(&self) -> BTreeSet<String> {
        self.parts.iter().map(|part| part.uuid.clone()).collect()
    }

    /// The uuids of the objects held by value.
    pub fn content_uuids(&self) -> BTreeSet<String> {
        self.contents.iter().map(|c| c.uuid.to_string()).collect()
    }

    /// The uuids of every object the model holds, parts and contents.
    pub fn held_uuids(&self) -> BTreeSet<String> {
        let mut all = self.part_uuids();
        all.extend(self.content_uuids());
        all
    }

    /// Whether the object is a part of its own right.
    pub fn is_part(&self, uuid: &str) -> bool {
        self.parts.iter().any(|part| part.uuid == uuid)
    }

    /// The qualified type of an object of the model.
    ///
    /// # Panics
    /// When the model does not hold it: a setup error.
    pub fn type_of(&self, uuid: &str) -> &'static str {
        if let Some(part) = self.parts.iter().find(|part| part.uuid == uuid) {
            return part.qualified_type;
        }
        self.contents
            .iter()
            .find(|c| c.uuid == uuid)
            .map(|c| c.qualified_type)
            .unwrap_or_else(|| setup(format!("the model does not hold {}", label(uuid))))
    }

    /// The title an object is read with: the part's when it is a part (the
    /// part wins), the inline title otherwise.
    ///
    /// # Panics
    /// When the model does not hold it: a setup error.
    pub fn title_of(&self, uuid: &str) -> String {
        if let Some(part) = self.parts.iter().find(|part| part.uuid == uuid) {
            return part.title.clone();
        }
        self.contents
            .iter()
            .find(|c| c.uuid == uuid)
            .map(|c| c.title.to_string())
            .unwrap_or_else(|| setup(format!("the model does not hold {}", label(uuid))))
    }

    /// The uuids of the direct containers of an object (empty for a part that
    /// is not held by value).
    pub fn containers_of(&self, uuid: &str) -> Vec<&'static str> {
        self.contents
            .iter()
            .find(|c| c.uuid == uuid)
            .map(|c| c.containers.clone())
            .unwrap_or_default()
    }

    /// The containment relationships: (contained, container), direct only.
    pub fn containment(&self) -> BTreeSet<(String, String)> {
        let mut pairs = BTreeSet::new();
        for content in &self.contents {
            for container in &content.containers {
                pairs.insert((content.uuid.to_string(), container.to_string()));
            }
        }
        pairs
    }

    /// Every DOR written, as (holder, target).
    pub fn written(&self) -> BTreeSet<(String, String)> {
        self.references
            .iter()
            .map(|r| (r.holder.to_string(), r.target.to_string()))
            .collect()
    }

    /// The relationships a store states: the containments and the DORs whose
    /// target the model holds.
    pub fn stated(&self) -> BTreeSet<(String, String)> {
        let held = self.held_uuids();
        let mut pairs = self.containment();
        pairs.extend(self.written().into_iter().filter(|(_, to)| held.contains(to)));
        pairs
    }

    /// For a held object, how many distinct objects point at it and how many it
    /// points at, over the stated relationships (containments included).
    pub fn counts(&self, uuid: &str) -> (u32, u32) {
        let stated = self.stated();
        let sources = stated.iter().filter(|(_, to)| to == uuid).count();
        let targets = stated.iter().filter(|(from, _)| from == uuid).count();
        (sources as u32, targets as u32)
    }
}

fn declarations() -> String {
    format!("xmlns:witsml=\"{WITSML}\" xmlns:eml=\"{EML}\" xmlns:xsi=\"{XSI}\"")
}

/// `eml:Citation`: Title, Originator, Creation and Format are mandatory
/// (`Abstract.xsd:363-393`, EML 2.3).
fn citation(title: &str) -> String {
    format!(
        "<eml:Citation><eml:Title>{title}</eml:Title><eml:Originator>contract</eml:Originator>\
         <eml:Creation>2026-10-14T00:00:00Z</eml:Creation><eml:Format>contract</eml:Format></eml:Citation>"
    )
}

/// The head every WITSML object shares: `Citation` is mandatory
/// (`Abstract.xsd:276`) and so is `ActiveStatus` for an `AbstractActiveObject`
/// (`Abstract.xsd:169`), which `Well`, `Wellbore`, `Log`, `ChannelSet` and
/// `Channel` all extend (`Well.xsd:325`, `Wellbore.xsd:130`, `Log.xsd:217`,
/// `:537`, `:696`).
fn head(title: &str) -> String {
    format!("{}<eml:ActiveStatus>active</eml:ActiveStatus>", citation(title))
}

/// A DOR: `Uuid`, `QualifiedType` and `Title` are mandatory (`ObjectReference.xsd:70-95`).
fn dor(element: &str, uuid: &str, qualified_type: &str, title: &str) -> String {
    format!(
        "<witsml:{element}><eml:Uuid>{uuid}</eml:Uuid><eml:QualifiedType>{qualified_type}</eml:QualifiedType>\
         <eml:Title>{title}</eml:Title></witsml:{element}>"
    )
}

fn title_of_channel(uuid: &str) -> &'static str {
    match uuid {
        CHANNEL_ONLY_A => "a depth channel",
        CHANNEL_ONLY_B => "a pressure channel",
        CHANNEL_SHARED => "a channel in two sets",
        _ => setup(format!("no channel of the model has the uuid {uuid}")),
    }
}

/// A Channel element. Mandatory in the XSD, in this order: `Mnemonic`
/// (`Log.xsd:219`), `GlobalMnemonic` (`:226`), `DataKind` (`:231`), `Uom`
/// (`:238`), `ChannelPropertyKind` (a DOR, `:255`), `LoggingCompany` (a DOR,
/// `:293`) and at least one `Index` (`:391`); an `Index` needs `IndexKind`,
/// `Uom`, `Direction` and `Mnemonic` (`Log.xsd:436-466`). `Wellbore` is
/// optional (`:401`): only the shared channel writes it, the secondary example
/// of ETP 1.2 section 8.1.1.1.2 (p. 157).
fn channel_body(uuid: &str, title: &str) -> String {
    let wellbore = if uuid == CHANNEL_SHARED {
        dor("Wellbore", WELLBORE, "witsml21.Wellbore", "a wellbore")
    } else {
        String::new()
    };
    format!(
        "{}<witsml:Mnemonic>MD</witsml:Mnemonic><witsml:GlobalMnemonic>MD</witsml:GlobalMnemonic>\
         <witsml:DataKind>double</witsml:DataKind><witsml:Uom>m</witsml:Uom>{}{}\
         <witsml:Index><witsml:IndexKind>measured depth</witsml:IndexKind><witsml:Uom>m</witsml:Uom>\
         <witsml:Direction>increasing</witsml:Direction><witsml:Mnemonic>MD</witsml:Mnemonic></witsml:Index>{}",
        head(title),
        dor(
            "ChannelPropertyKind",
            ABSENT_PROPERTY_KIND,
            "eml23.PropertyKind",
            "a property kind"
        ),
        dor(
            "LoggingCompany",
            ABSENT_COMPANY,
            "eml23.BusinessAssociate",
            "a logging company"
        ),
        wellbore
    )
}

/// A Channel written inside a ChannelSet (`Log.xsd:650`).
fn channel_inline(uuid: &str) -> String {
    format!(
        "<witsml:Channel uuid=\"{uuid}\" schemaVersion=\"2.1\">{}</witsml:Channel>",
        channel_body(uuid, title_of_channel(uuid))
    )
}

/// A ChannelSet written inside the Log (`Log.xsd:822`). The PPFG one is written
/// with `xsi:type`, since `PPFGChannelSet` extends `ChannelSet` (`PPFG.xsd:477`)
/// and the element is declared with the base type.
fn set_inline(uuid: &str, title: &str, ppfg: bool, channels: &[&str]) -> String {
    let xsi_type = if ppfg {
        " xsi:type=\"witsml:PPFGChannelSet\""
    } else {
        ""
    };
    let inside: String = channels.iter().map(|c| channel_inline(c)).collect();
    format!(
        "<witsml:ChannelSet{xsi_type} uuid=\"{uuid}\" schemaVersion=\"2.1\">{}{inside}</witsml:ChannelSet>",
        head(title)
    )
}

fn part(
    uuid: &str,
    qualified_type: &'static str,
    content_type: &'static str,
    title: &str,
    element: &str,
    body: &str,
) -> ModelObject {
    ModelObject {
        uuid: uuid.to_string(),
        qualified_type,
        content_type,
        title: title.to_string(),
        xml: format!(
            "<witsml:{element} {} uuid=\"{uuid}\" schemaVersion=\"2.1\">{}{body}</witsml:{element}>",
            declarations(),
            head(title)
        ),
    }
}

/// The Well (`Well.xsd:325`: nothing mandatory beyond the head).
pub fn well() -> ModelObject {
    part(
        WELL,
        "witsml21.Well",
        "application/x-witsml+xml;version=2.1;type=Well",
        "a well",
        "Well",
        "",
    )
}

/// The Wellbore: `Well` is a mandatory DOR (`Wellbore.xsd:333`).
pub fn wellbore() -> ModelObject {
    part(
        WELLBORE,
        "witsml21.Wellbore",
        "application/x-witsml+xml;version=2.1;type=Wellbore",
        "a wellbore",
        "Wellbore",
        &dor("Well", WELL, "witsml21.Well", "a well"),
    )
}

/// The Log, with the ChannelSet [`SET_A`] and the PPFGChannelSet [`SET_PPFG`]
/// by value, holding the channels given (uuids of the model's channels).
/// `Wellbore` is optional on a Log (`Log.xsd:823`) and left out, so that the only
/// DOR to the Wellbore is the Channel's.
pub fn log_with(in_set_a: &[&str], in_set_ppfg: &[&str]) -> ModelObject {
    let sets = format!(
        "{}{}",
        set_inline(SET_A, "a channel set", false, in_set_a),
        set_inline(SET_PPFG, "a PPFG channel set", true, in_set_ppfg)
    );
    part(
        LOG,
        "witsml21.Log",
        "application/x-witsml+xml;version=2.1;type=Log",
        "a log",
        "Log",
        &sets,
    )
}

/// A Channel as a part of its own right (root element `Channel`).
pub fn standalone_channel(uuid: &str, title: &str) -> ModelObject {
    ModelObject {
        uuid: uuid.to_string(),
        qualified_type: "witsml21.Channel",
        content_type: "application/x-witsml+xml;version=2.1;type=Channel",
        title: title.to_string(),
        xml: format!(
            "<witsml:Channel {} uuid=\"{uuid}\" schemaVersion=\"2.1\">{}</witsml:Channel>",
            declarations(),
            channel_body(uuid, title)
        ),
    }
}

fn contents_of_model() -> Vec<ModelContent> {
    vec![
        ModelContent {
            uuid: SET_A,
            qualified_type: "witsml21.ChannelSet",
            title: "a channel set",
            containers: vec![LOG],
        },
        ModelContent {
            uuid: SET_PPFG,
            qualified_type: "witsml21.PPFGChannelSet",
            title: "a PPFG channel set",
            containers: vec![LOG],
        },
        ModelContent {
            uuid: CHANNEL_ONLY_A,
            qualified_type: "witsml21.Channel",
            title: "a depth channel",
            containers: vec![SET_A],
        },
        ModelContent {
            uuid: CHANNEL_ONLY_B,
            qualified_type: "witsml21.Channel",
            title: "a pressure channel",
            containers: vec![SET_PPFG],
        },
        ModelContent {
            uuid: CHANNEL_SHARED,
            qualified_type: "witsml21.Channel",
            title: "a channel in two sets",
            containers: vec![SET_A, SET_PPFG],
        },
    ]
}

fn references_of_model() -> Vec<ModelReference> {
    let mut references = vec![
        ModelReference {
            holder: WELLBORE,
            element: "Well",
            target: WELL,
        },
        ModelReference {
            holder: CHANNEL_SHARED,
            element: "Wellbore",
            target: WELLBORE,
        },
    ];
    for channel in [CHANNEL_ONLY_A, CHANNEL_ONLY_B, CHANNEL_SHARED] {
        references.push(ModelReference {
            holder: channel,
            element: "ChannelPropertyKind",
            target: ABSENT_PROPERTY_KIND,
        });
        references.push(ModelReference {
            holder: channel,
            element: "LoggingCompany",
            target: ABSENT_COMPANY,
        });
    }
    references
}

/// The model: a Well, a Wellbore and a Log (the parts), the Log holding by
/// value a ChannelSet and a PPFGChannelSet, which hold the Channels, one of them
/// in both; and a Channel that references the Wellbore.
pub fn model_witsml() -> WitsmlModel {
    WitsmlModel {
        parts: vec![
            well(),
            wellbore(),
            log_with(
                &[CHANNEL_ONLY_A, CHANNEL_SHARED],
                &[CHANNEL_ONLY_B, CHANNEL_SHARED],
            ),
        ],
        contents: contents_of_model(),
        references: references_of_model(),
    }
}

/// [`model_witsml`] plus a standalone `Channel` part with the uuid of
/// [`CHANNEL_ONLY_A`] and the title [`STANDALONE_TITLE`] (the inline copy keeps
/// its own title).
pub fn model_witsml_with_part() -> WitsmlModel {
    let mut model = model_witsml();
    model
        .parts
        .push(standalone_channel(CHANNEL_ONLY_A, STANDALONE_TITLE));
    model
}

// ---------------------------------------------------------------------------
// What a group reads of the store
// ---------------------------------------------------------------------------

fn label(uuid: &str) -> String {
    let lower = uuid.to_ascii_lowercase();
    for (name, known) in [
        ("well", WELL),
        ("wellbore", WELLBORE),
        ("log", LOG),
        ("set-a", SET_A),
        ("set-ppfg", SET_PPFG),
        ("channel-only-a", CHANNEL_ONLY_A),
        ("channel-only-b", CHANNEL_ONLY_B),
        ("channel-shared", CHANNEL_SHARED),
        ("absent-property-kind", ABSENT_PROPERTY_KIND),
        ("absent-company", ABSENT_COMPANY),
        ("absent-channel", ABSENT_CHANNEL),
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
        .to_ascii_lowercase()
}

fn pairs(edges: &[Edge]) -> BTreeSet<(String, String)> {
    edges
        .iter()
        .map(|edge| (uuid_of(&edge.from), uuid_of(&edge.to)))
        .collect()
}

fn uuids_of(found: &Found) -> BTreeSet<String> {
    found
        .resources
        .iter()
        .map(|summary| summary.uuid.to_ascii_lowercase())
        .collect()
}

/// A containment edge carries no field path: it is the inverse of the nesting
/// of the text, not a reference written in it (design note section 4.5).
fn is_containment(edge: &Edge) -> bool {
    edge.path.trim().is_empty()
}

/// The store as the factory built it: its dataspace and its parts.
struct Held {
    dataspace: String,
    parts: Vec<ObjectSummary>,
}

impl Held {
    fn part_uri(&self, uuid: &str) -> String {
        self.parts
            .iter()
            .find(|summary| summary.uuid.eq_ignore_ascii_case(uuid))
            .map(|summary| summary.uri.clone())
            .unwrap_or_else(|| setup(format!("the store does not list the part {}", label(uuid))))
    }

    /// The canonical URI of an object of this type and uuid in this store,
    /// spelled as the store spells a part's (the dataspace and the form are the
    /// store's; the type and the uuid are the model's).
    fn canonical(&self, uuid: &str, qualified_type: &str) -> String {
        self.part_uri(LOG)
            .replace(LOG, uuid)
            .replace("witsml21.Log", qualified_type)
    }

    /// The URI the store must answer an object of the model under: the part's
    /// when it is a part, the canonical one otherwise.
    fn uri(&self, model: &WitsmlModel, uuid: &str) -> String {
        if model.is_part(uuid) {
            self.part_uri(uuid)
        } else {
            self.canonical(uuid, model.type_of(uuid))
        }
    }
}

async fn held<S: EnergymlStorage + Sync>(store: &S, model: &WitsmlModel) -> Held {
    let parts = store
        .resources()
        .await
        .unwrap_or_else(|error| setup(format!("resources() failed on the fresh store: {error}")));
    let got: BTreeSet<String> = parts.iter().map(|s| s.uuid.to_ascii_lowercase()).collect();
    let want = model.part_uuids();
    if got != want {
        let extra: BTreeSet<String> = got.difference(&want).cloned().collect();
        if !extra.is_empty() && extra.is_subset(&model.content_uuids()) && want.is_subset(&got) {
            broken(
                &CONT_LISTED,
                format!(
                    "resources() lists contained objects {}; it lists the parts only",
                    show(&extra)
                ),
            );
        }
        setup(format!(
            "the factory must build a store holding exactly the WITSML model; it lists {} and the parts are {}",
            show(&got),
            show(&want)
        ));
    }
    let dataspace = dataspace_of(&parts[0].uri);
    Held { dataspace, parts }
}

/// Ask a context, and check the objects it reaches are exactly `want`.
async fn reach<S: EnergymlStorage + Sync>(store: &S, rule: &Rule, context: Context, want: &[&str]) -> Found {
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
    found
}

fn uuid_list(uuids: &BTreeSet<String>) -> Vec<&str> {
    uuids.iter().map(String::as_str).collect()
}

/// The edges of the store, from both of its routes: `edges()` and the edges
/// `contents()` states, each once.
async fn union_edges<S: EnergymlStorage + Sync>(store: &S, rule: &Rule) -> Vec<Edge> {
    let mut edges = must(rule, "edges()", store.edges(&[]).await);
    let contents = must(rule, "contents()", store.contents().await);
    for edge in contents.edges {
        let known = edges
            .iter()
            .any(|e| e.from == edge.from && e.to == edge.to && e.path == edge.path);
        if !known {
            edges.push(edge);
        }
    }
    edges
}

fn knows_containment<S: EnergymlStorage>(store: &S) -> bool {
    store.capabilities().contained_objects
}

/// Whether the store states the kind of the edges of WITSML 2.1 objects.
fn states_witsml<S: EnergymlStorage>(store: &S) -> bool {
    let capabilities = store.capabilities();
    capabilities.edge_kinds && capabilities.edge_kind_coverage.covers("witsml21")
}

// ---------------------------------------------------------------------------
// Contents
// ---------------------------------------------------------------------------

/// **Contents** (CONT-2, CONT-8): `contents()` lists the objects held by value
/// with their canonical URI and their direct containers; `resources()` lists the
/// parts only; the same Channel in two ChannelSets is one resource and two edges.
///
/// Plays when the store knows containment; [`contained_unaware`] otherwise.
pub async fn contents<S: EnergymlStorage + Sync>(make: &dyn Fn() -> S) {
    let store = make();
    let model = model_witsml();
    let state = held(&store, &model).await;
    if !knows_containment(&store) {
        return;
    }

    // CONT-2: what is listed, and under which URI.
    let listed = must(&CONT_LISTED, "contents()", store.contents().await).objects;
    let got: BTreeSet<String> = listed.iter().map(|o| o.uuid.to_ascii_lowercase()).collect();
    let want = model.content_uuids();
    rule!(
        &CONT_LISTED,
        got == want && listed.len() == want.len(),
        "contents() lists {} ({} entries), the model holds {} by value",
        show(&got),
        listed.len(),
        show(&want)
    );
    for object in &listed {
        let uuid = object.uuid.to_ascii_lowercase();
        let expected = state.uri(&model, &uuid);
        rule!(
            &CONT_LISTED,
            object.uri == expected,
            "{} is listed under `{}`, its canonical URI is `{expected}`",
            label(&uuid),
            object.uri
        );
        rule!(
            &CONT_LISTED,
            object.qualified_type == model.type_of(&uuid),
            "{} is listed as `{}`, the model writes it as `{}`",
            label(&uuid),
            object.qualified_type,
            model.type_of(&uuid)
        );
        let wanted_in: BTreeSet<String> = model
            .containers_of(&uuid)
            .iter()
            .map(|container| state.uri(&model, container))
            .collect();
        let got_in: BTreeSet<String> = object.contained_in.iter().cloned().collect();
        rule!(
            &CONT_LISTED,
            got_in == wanted_in && object.contained_in.len() == got_in.len(),
            "{} is contained in {:?}, the model's direct containers are {:?}",
            label(&uuid),
            object.contained_in,
            wanted_in
        );
    }
    for part in &state.parts {
        rule!(
            &CONT_LISTED,
            part.contained_in.is_empty(),
            "the part {} states containers {:?}",
            label(&part.uuid),
            part.contained_in
        );
    }
    let everything = model.held_uuids();
    let found = reach(
        &store,
        &CONT_LISTED,
        Context::dataspace(state.dataspace.clone()),
        &uuid_list(&everything),
    )
    .await;

    // CONT-8: the same Channel in two ChannelSets.
    let shared = CHANNEL_SHARED;
    rule!(
        &CONT_SHARED,
        listed
            .iter()
            .filter(|o| o.uuid.eq_ignore_ascii_case(shared))
            .count()
            == 1,
        "the channel held by two sets is not listed exactly once by contents()"
    );
    rule!(
        &CONT_SHARED,
        found
            .resources
            .iter()
            .filter(|r| r.uuid.eq_ignore_ascii_case(shared))
            .count()
            == 1,
        "the channel held by two sets is not listed exactly once by the dataspace context"
    );
    let edges = union_edges(&store, &CONT_SHARED).await;
    let from_shared: BTreeSet<String> = edges
        .iter()
        .filter(|e| is_containment(e) && uuid_of(&e.from) == shared)
        .map(|e| uuid_of(&e.to))
        .collect();
    let two = set(&[SET_A, SET_PPFG]);
    rule!(
        &CONT_SHARED,
        from_shared == two,
        "the channel held by two sets has containment edges to {}, expected {}",
        show(&from_shared),
        show(&two)
    );
    rule!(
        &CONT_SHARED,
        edges
            .iter()
            .filter(|e| is_containment(e) && uuid_of(&e.from) == shared)
            .count()
            == 2,
        "the channel held by two sets has other than two containment edges"
    );

    // The counts (ETP 1.2 section 8.3.1): each container counted once.
    let mut asked = Context::dataspace(state.dataspace.clone());
    asked.count_objects = true;
    let counted = reach(&store, &CONT_SHARED, asked, &uuid_list(&everything)).await;
    for summary in &counted.resources {
        let uuid = summary.uuid.to_ascii_lowercase();
        let expected = model.counts(&uuid);
        rule!(
            &CONT_SHARED,
            (summary.source_count, summary.target_count) == (Some(expected.0), Some(expected.1)),
            "{}: counted {:?} sources and {:?} targets, the model's relationships say {:?}",
            label(&uuid),
            summary.source_count,
            summary.target_count,
            expected
        );
    }
}

// ---------------------------------------------------------------------------
// Edges, walks, kinds
// ---------------------------------------------------------------------------

/// Whether the basis is the one a server or the specification gives for the
/// three examples of ETP 1.2 section 8.1.1.1.2 (p. 157).
fn official_example(basis: Option<KindBasis>) -> bool {
    matches!(
        basis,
        Some(KindBasis::Stated | KindBasis::EtpSpec { page: 157, .. })
    )
}

/// **Containment edges** (CONT-3, CONT-4, KIND-2): the edge from a contained
/// object to its container, a walk through it, and its kind.
///
/// Plays when the store knows containment.
pub async fn containment_edges<S: EnergymlStorage + Sync>(make: &dyn Fn() -> S) {
    let store = make();
    let model = model_witsml();
    let state = held(&store, &model).await;
    if !knows_containment(&store) {
        return;
    }
    let edges = union_edges(&store, &CONT_EDGE).await;

    // CONT-3: exactly the model's direct containments.
    let by_value: Vec<&Edge> = edges.iter().filter(|e| is_containment(e)).collect();
    let got: BTreeSet<(String, String)> = by_value
        .iter()
        .map(|e| (uuid_of(&e.from), uuid_of(&e.to)))
        .collect();
    let want = model.containment();
    rule!(
        &CONT_EDGE,
        got == want,
        "the containment edges are {}, the model's direct containments are {}",
        show_pairs(&got),
        show_pairs(&want)
    );
    for (content, container) in &want {
        let n = by_value
            .iter()
            .filter(|e| uuid_of(&e.from) == *content && uuid_of(&e.to) == *container)
            .count();
        rule!(
            &CONT_EDGE,
            n == 1,
            "{} -> {}: {n} edges, expected exactly one",
            label(content),
            label(container)
        );
    }
    let log_uri = state.part_uri(LOG);
    rule!(
        &CONT_EDGE,
        !by_value
            .iter()
            .any(|e| e.from.contains(CHANNEL_ONLY_A) && e.to == log_uri),
        "a channel has an edge to the log, a container of its container"
    );

    // The second route to the same fact, inside the store: contained_in.
    let contents = must(&CONT_EDGE, "contents()", store.contents().await);
    let mut from_fields: BTreeSet<(String, String)> = BTreeSet::new();
    for object in &contents.objects {
        for container in &object.contained_in {
            from_fields.insert((uuid_of(&object.uri), uuid_of(container)));
        }
    }
    rule!(
        &CONT_EDGE,
        from_fields == want,
        "contained_in states {}, the model's direct containments are {}",
        show_pairs(&from_fields),
        show_pairs(&want)
    );

    // Kinds.
    if states_witsml(&store) {
        for edge in &by_value {
            let container = uuid_of(&edge.to);
            let container_type = model.type_of(&container);
            if matches!(container_type, "witsml21.Log" | "witsml21.ChannelSet") {
                rule!(
                    &KIND_EXAMPLES,
                    edge.kind == Some(EdgeKind::Primary) && official_example(edge.basis),
                    "{} -> {} ({container_type}) is {:?} on {:?}, expected Primary on a stated basis",
                    label(&uuid_of(&edge.from)),
                    label(&container),
                    edge.kind,
                    edge.basis
                );
            } else {
                rule!(
                    &CONT_EDGE,
                    edge.kind == Some(EdgeKind::Primary)
                        && matches!(
                            edge.basis,
                            Some(KindBasis::Stated | KindBasis::EtpSpec { .. } | KindBasis::DerivedByValue)
                        ),
                    "{} -> {} ({container_type}) is {:?} on {:?}, expected Primary, stated or by-value",
                    label(&uuid_of(&edge.from)),
                    label(&container),
                    edge.kind,
                    edge.basis
                );
            }
        }
        let secondary = edges
            .iter()
            .find(|e| !is_containment(e) && uuid_of(&e.from) == CHANNEL_SHARED && uuid_of(&e.to) == WELLBORE);
        let Some(secondary) = secondary else {
            broken(
                &KIND_EXAMPLES,
                "the reference from the channel to the wellbore is not among the edges",
            )
        };
        rule!(
            &KIND_EXAMPLES,
            secondary.kind == Some(EdgeKind::Secondary) && official_example(secondary.basis),
            "the reference from a Channel to a Wellbore is {:?} on {:?}, expected Secondary on a stated basis",
            secondary.kind,
            secondary.basis
        );
    } else {
        for edge in &edges {
            rule!(
                &KIND_EXAMPLES,
                edge.kind.is_none(),
                "the store states no kind for witsml21 and the edge {} -> {} has {:?}",
                edge.from,
                edge.to,
                edge.kind
            );
        }
        let mut by_kind = Context::of(state.uri(&model, WELL), Scope::Sources).with_depth(2);
        by_kind.navigable = Navigable::Primary;
        let answer = store.resources_in(&by_kind).await;
        rule!(
            &KIND_EXAMPLES,
            matches!(
                answer,
                Err(Error::Unsupported { .. } | Error::UnstatedKind { .. })
            ),
            "the store states no kind for witsml21 and a request by kind that meets its edges answered {}",
            brief(&answer)
        );
    }

    // CONT-4: walks through the containment edges.
    let channel = state.uri(&model, CHANNEL_ONLY_A);
    reach(
        &store,
        &CONT_WALK,
        Context::of(channel.clone(), Scope::Targets).with_depth(1),
        &[SET_A],
    )
    .await;
    let two = reach(
        &store,
        &CONT_WALK,
        Context::of(channel.clone(), Scope::Targets).with_depth(2),
        &[SET_A, LOG],
    )
    .await;
    let walked = pairs(&two.edges);
    for wanted in [(CHANNEL_ONLY_A, SET_A), (SET_A, LOG)] {
        rule!(
            &CONT_WALK,
            walked.contains(&(wanted.0.to_string(), wanted.1.to_string())),
            "the edges of a depth 2 walk from the channel are {}, the edge {} -> {} is missing",
            show_pairs(&walked),
            label(wanted.0),
            label(wanted.1)
        );
    }
    reach(
        &store,
        &CONT_WALK,
        Context::of(channel.clone(), Scope::Targets).with_depth(5),
        &[SET_A, LOG],
    )
    .await;
    reach(
        &store,
        &CONT_WALK,
        Context::of(channel.clone(), Scope::TargetsOrSelf).with_depth(1),
        &[CHANNEL_ONLY_A, SET_A],
    )
    .await;
    let log = state.uri(&model, LOG);
    reach(
        &store,
        &CONT_WALK,
        Context::of(log.clone(), Scope::Sources).with_depth(1),
        &[SET_A, SET_PPFG],
    )
    .await;
    reach(
        &store,
        &CONT_WALK,
        Context::of(log, Scope::Sources).with_depth(2),
        &[SET_A, SET_PPFG, CHANNEL_ONLY_A, CHANNEL_ONLY_B, CHANNEL_SHARED],
    )
    .await;
    reach(
        &store,
        &CONT_WALK,
        Context::of(state.uri(&model, CHANNEL_SHARED), Scope::Targets).with_depth(1),
        &[SET_A, SET_PPFG, WELLBORE],
    )
    .await;
}

// ---------------------------------------------------------------------------
// Reading a contained object
// ---------------------------------------------------------------------------

/// **Reading** (CONT-5, CONT-6, CONT-7): a contained object is read, and its
/// header read, as itself; a container is read with its contents inline.
///
/// Plays when the store knows containment.
pub async fn contained_reads<S: EnergymlStorage + Sync>(make: &dyn Fn() -> S) {
    let store = make();
    let model = model_witsml();
    let state = held(&store, &model).await;
    if !knows_containment(&store) {
        return;
    }
    let asked = [CHANNEL_ONLY_A, SET_PPFG, LOG, CHANNEL_SHARED, SET_A];
    let uris: Vec<String> = asked.iter().map(|uuid| state.uri(&model, uuid)).collect();

    // CONT-5: the object, not its container, in order.
    let read = store.get_objects(&uris).await;
    rule!(
        &CONT_GET,
        read.len() == uris.len(),
        "{} URIs asked, {} results",
        uris.len(),
        read.len()
    );
    for (uuid, result) in asked.iter().zip(&read) {
        let object = match result {
            Ok(object) => object,
            Err(error) => broken(&CONT_GET, format!("{} was not read: {error}", label(uuid))),
        };
        let reflect = object.as_reflect();
        rule!(
            &CONT_GET,
            uuid_of_reflect(reflect).is_some_and(|read| read.eq_ignore_ascii_case(uuid)),
            "reading {} answered the object {:?}",
            label(uuid),
            uuid_of_reflect(reflect)
        );
        rule!(
            &CONT_GET,
            reflect.type_info().qualified_type == Some(model.type_of(uuid)),
            "reading {} answered an object of type {:?}, the model writes it as {}",
            label(uuid),
            reflect.type_info().qualified_type,
            model.type_of(uuid)
        );
        let title = model.title_of(uuid);
        rule!(
            &CONT_GET,
            title_of_reflect(reflect) == Some(title.as_str()),
            "reading {} answered the citation {:?}, the object's own is `{title}`",
            label(uuid),
            title_of_reflect(reflect)
        );
    }
    let absent = state.canonical(ABSENT_CHANNEL, "witsml21.Channel");
    let mixed = store
        .get_objects(&[uris[0].clone(), absent, uris[2].clone()])
        .await;
    rule!(
        &CONT_GET,
        mixed.len() == 3
            && mixed[0].is_ok()
            && matches!(mixed[1], Err(Error::NotFound(_)))
            && mixed[2].is_ok(),
        "reading [contained, absent contained, part] answered {}",
        briefs(&mixed)
    );

    // CONT-6: the citation of the contained object.
    for (uuid, uri) in asked.iter().zip(&uris) {
        let header = must(&CONT_HEADER, "header()", store.header(uri).await);
        let title = model.title_of(uuid);
        rule!(
            &CONT_HEADER,
            header.title.as_deref() == Some(title.as_str()),
            "the header of {} has the title {:?}, the object's own is `{title}`",
            label(uuid),
            header.title
        );
        rule!(
            &CONT_HEADER,
            header.summary.uuid.eq_ignore_ascii_case(uuid)
                && header.summary.qualified_type == model.type_of(uuid)
                && &header.summary.uri == uri,
            "the header of {} names `{}` ({}, `{}`)",
            label(uuid),
            header.summary.uri,
            header.summary.uuid,
            header.summary.qualified_type
        );
    }
    let headers = store.headers(&uris).await;
    rule!(
        &CONT_HEADER,
        headers.len() == uris.len(),
        "{} URIs asked, {} headers",
        uris.len(),
        headers.len()
    );
    for (uuid, header) in asked.iter().zip(&headers) {
        let title = model.title_of(uuid);
        rule!(
            &CONT_HEADER,
            matches!(header, Ok(h) if h.title.as_deref() == Some(title.as_str())),
            "headers() of {} answered {}",
            label(uuid),
            brief(header)
        );
    }

    // CONT-7: the container, contents inline.
    if !store.capabilities().raw_formats.contains(&RawFormat::Xml) {
        return;
    }
    let raws = store
        .get_raw(
            &[state.uri(&model, LOG), state.uri(&model, SET_A)],
            RawFormat::Xml,
        )
        .await;
    let text = |index: usize, what: &str| -> Option<String> {
        match raws.get(index) {
            Some(Ok(raw)) => Some(String::from_utf8_lossy(&raw.bytes).to_ascii_lowercase()),
            Some(Err(Error::Unsupported { .. })) if index == 1 => None,
            other => broken(
                &CONT_INLINE,
                format!(
                    "get_raw of {what} answered {:?}",
                    other.map(|r| r.as_ref().map(|_| ()).map_err(|e| e.to_string()))
                ),
            ),
        }
    };
    let log_text = text(0, "the log").unwrap_or_default();
    for uuid in model.content_uuids() {
        rule!(
            &CONT_INLINE,
            log_text.contains(&uuid),
            "the XML of the log does not hold its contained object {} inline",
            label(&uuid)
        );
    }
    if let Some(set_text) = text(1, "a channel set") {
        for inside in [CHANNEL_ONLY_A, CHANNEL_SHARED] {
            rule!(
                &CONT_INLINE,
                set_text.contains(inside),
                "the XML of the channel set does not hold its channel {}",
                label(inside)
            );
        }
        for outside in [CHANNEL_ONLY_B, SET_PPFG, LOG] {
            rule!(
                &CONT_INLINE,
                !set_text.contains(outside),
                "the XML of the channel set holds {}, which it does not contain",
                label(outside)
            );
        }
    }
}

// ---------------------------------------------------------------------------
// References
// ---------------------------------------------------------------------------

/// **References** (REF-6): a DOR written in a contained object has that object
/// for its source.
///
/// Plays when the store knows containment.
pub async fn contained_references<S: EnergymlStorage + Sync>(make: &dyn Fn() -> S) {
    let store = make();
    let model = model_witsml();
    let state = held(&store, &model).await;
    if !knows_containment(&store) {
        return;
    }
    let written = must(&REF_CONTAINED, "references()", store.references(&[]).await);
    let got = pairs(&written);
    let want = model.written();
    rule!(
        &REF_CONTAINED,
        got == want,
        "references() are {}, the model's objects write {} (a DOR in a contained object is its own)",
        show_pairs(&got),
        show_pairs(&want)
    );
    for edge in &written {
        rule!(
            &REF_CONTAINED,
            !edge.path.trim().is_empty(),
            "the reference {} -> {} carries no field path",
            edge.from,
            edge.to
        );
    }
    let channel_to_wellbore = written
        .iter()
        .find(|e| uuid_of(&e.from) == CHANNEL_SHARED && uuid_of(&e.to) == WELLBORE);
    let Some(channel_to_wellbore) = channel_to_wellbore else {
        broken(
            &REF_CONTAINED,
            "the DOR from the channel to the wellbore is not among references()",
        )
    };
    rule!(
        &REF_CONTAINED,
        channel_to_wellbore.path.rsplit('.').next() == Some("Wellbore"),
        "the path of the DOR from the channel is `{}`, it ends at the element Wellbore",
        channel_to_wellbore.path
    );
    for container in [LOG, SET_A, SET_PPFG] {
        rule!(
            &REF_CONTAINED,
            !written.iter().any(|e| uuid_of(&e.from) == container),
            "the container {} is the source of a reference the model writes in its contents",
            label(container)
        );
    }

    // Every edge that is not a containment is a reference written.
    let edges = union_edges(&store, &REF_CONTAINED).await;
    for edge in edges.iter().filter(|e| !is_containment(e)) {
        rule!(
            &REF_CONTAINED,
            got.contains(&(uuid_of(&edge.from), uuid_of(&edge.to))),
            "the edge {} -> {} is no containment and no reference written",
            edge.from,
            edge.to
        );
    }
    rule!(
        &REF_CONTAINED,
        !got.iter().any(|pair| model.containment().contains(pair)),
        "a containment is among the references written"
    );

    // The wellbore is pointed at by the channel, not by the log.
    reach(
        &store,
        &REF_CONTAINED,
        Context::of(state.uri(&model, WELLBORE), Scope::Sources).with_depth(1),
        &[CHANNEL_SHARED],
    )
    .await;
}

// ---------------------------------------------------------------------------
// An object that is both a part and held by value
// ---------------------------------------------------------------------------

/// **Both a part and held by value** (CONT-9): one resource, the part.
///
/// The factory builds the store holding [`model_witsml_with_part`]. Plays when
/// the store knows containment.
pub async fn contained_part<S: EnergymlStorage + Sync>(make: &dyn Fn() -> S) {
    let store = make();
    let model = model_witsml_with_part();
    if !knows_containment(&store) {
        return;
    }
    let state = held(&store, &model).await;
    let part_uri = state.part_uri(CHANNEL_ONLY_A);

    let everything = model.held_uuids();
    let found = reach(
        &store,
        &CONT_ALSO_PART,
        Context::dataspace(state.dataspace.clone()),
        &uuid_list(&everything),
    )
    .await;
    let urls: Vec<&String> = found
        .resources
        .iter()
        .filter(|r| r.uuid.eq_ignore_ascii_case(CHANNEL_ONLY_A))
        .map(|r| &r.uri)
        .collect();
    rule!(
        &CONT_ALSO_PART,
        urls.len() == 1 && *urls[0] == part_uri,
        "the object held as a part and by value is listed {} times, as {:?}; the part is `{part_uri}`",
        urls.len(),
        urls
    );
    let contents = must(&CONT_ALSO_PART, "contents()", store.contents().await);
    for object in contents
        .objects
        .iter()
        .filter(|o| o.uuid.eq_ignore_ascii_case(CHANNEL_ONLY_A))
    {
        rule!(
            &CONT_ALSO_PART,
            object.uri == part_uri,
            "contents() lists the object under `{}`, not under the part's `{part_uri}`",
            object.uri
        );
    }

    let header = must(&CONT_ALSO_PART, "header()", store.header(&part_uri).await);
    rule!(
        &CONT_ALSO_PART,
        header.title.as_deref() == Some(STANDALONE_TITLE),
        "the header has the title {:?}, the part's is `{STANDALONE_TITLE}`",
        header.title
    );
    let object = must(&CONT_ALSO_PART, "get_object()", store.get_object(&part_uri).await);
    rule!(
        &CONT_ALSO_PART,
        title_of_reflect(object.as_reflect()) == Some(STANDALONE_TITLE),
        "the object read has the title {:?}, the part's is `{STANDALONE_TITLE}`",
        title_of_reflect(object.as_reflect())
    );

    let edges = union_edges(&store, &CONT_ALSO_PART).await;
    rule!(
        &CONT_ALSO_PART,
        pairs(&edges).contains(&(CHANNEL_ONLY_A.to_string(), SET_A.to_string())),
        "the edge from the object to its container is not stated though it is also a part: {}",
        show_pairs(&pairs(&edges))
    );
}

// ---------------------------------------------------------------------------
// Cost
// ---------------------------------------------------------------------------

/// **Cost** (CONT-10): `resources()` walks nothing; `contents()` walks the
/// container parts once; a dataspace holding no container type is walked by
/// nothing.
///
/// `scans` reads a counter of the store: **how many parts it has walked to find
/// contained objects** (zero on a store just opened). It is the caller's, since
/// how a store counts is its own; a store that cannot count cannot play this
/// group. `make` holds [`model_witsml`], `make_plain` holds the RESQML model of
/// [`crate::contract::model`], which has no container type.
pub async fn contained_cost<S: EnergymlStorage + Sync>(
    make: &dyn Fn() -> S,
    make_plain: &dyn Fn() -> S,
    scans: &dyn Fn(&S) -> u64,
) {
    let store = make();
    let model = model_witsml();
    rule!(
        &CONT_COST,
        scans(&store) == 0,
        "a store just opened has walked {} part(s)",
        scans(&store)
    );
    let state = held(&store, &model).await;
    rule!(
        &CONT_COST,
        scans(&store) == 0,
        "resources() walked {} part(s) to find contained objects",
        scans(&store)
    );
    let uris: Vec<String> = state.parts.iter().map(|p| p.uri.clone()).collect();
    for header in store.headers(&uris).await {
        must(&CONT_COST, "headers()", header);
    }
    rule!(
        &CONT_COST,
        scans(&store) == 0,
        "headers() walked {} part(s) to find contained objects",
        scans(&store)
    );

    if !knows_containment(&store) {
        return;
    }
    must(&CONT_COST, "contents()", store.contents().await);
    let first = scans(&store);
    rule!(
        &CONT_COST,
        first > 0,
        "contents() walked no part though the model holds a Log; the counter cannot tell, or the contents are not read"
    );
    must(&CONT_COST, "contents() again", store.contents().await);
    rule!(
        &CONT_COST,
        scans(&store) == first,
        "a second contents() walked {} more part(s)",
        scans(&store) - first
    );

    // A dataspace with no container type.
    let plain = make_plain();
    let listed = must(
        &CONT_COST,
        "resources() of the plain store",
        plain.resources().await,
    );
    rule!(
        &CONT_COST,
        !listed.is_empty(),
        "the plain factory built an empty store"
    );
    let dataspace = dataspace_of(&listed[0].uri);
    let content = must(
        &CONT_COST,
        "contents() of the plain store",
        plain.contents().await,
    );
    rule!(
        &CONT_COST,
        content.objects.is_empty() && content.edges.is_empty(),
        "a dataspace with no container type has {} contained objects",
        content.objects.len()
    );
    must(
        &CONT_COST,
        "resources_in() of the plain store",
        plain.resources_in(&Context::dataspace(dataspace)).await,
    );
    must(&CONT_COST, "edges() of the plain store", plain.edges(&[]).await);
    rule!(
        &CONT_COST,
        scans(&plain) == 0,
        "a dataspace holding no container type was walked ({} part(s))",
        scans(&plain)
    );
}

// ---------------------------------------------------------------------------
// A store that does not know containment
// ---------------------------------------------------------------------------

/// **Without containment** (CONT-11): a store whose capabilities do not say it
/// knows containment lists no contained object, answers no containment edge, and
/// refuses by name, or does not find, what depends on it.
///
/// Plays when `contained_objects` is off.
pub async fn contained_unaware<S: EnergymlStorage + Sync>(make: &dyn Fn() -> S) {
    let store = make();
    let model = model_witsml();
    let state = held(&store, &model).await;
    if knows_containment(&store) {
        return;
    }
    let parts = model.part_uuids();
    reach(
        &store,
        &CONT_UNAWARE,
        Context::dataspace(state.dataspace.clone()),
        &uuid_list(&parts),
    )
    .await;
    for part in &state.parts {
        rule!(
            &CONT_UNAWARE,
            part.contained_in.is_empty(),
            "the part {} states containers {:?} though the store does not know containment",
            label(&part.uuid),
            part.contained_in
        );
    }
    match store.contents().await {
        Ok(contents) => rule!(
            &CONT_UNAWARE,
            contents.objects.is_empty() && contents.edges.is_empty(),
            "contents() lists {} objects and {} edges though the store does not know containment",
            contents.objects.len(),
            contents.edges.len()
        ),
        Err(Error::Unsupported { .. }) => {}
        Err(other) => broken(&CONT_UNAWARE, format!("contents() answered Err({other})")),
    }
    let edges = must(&CONT_UNAWARE, "edges()", store.edges(&[]).await);
    rule!(
        &CONT_UNAWARE,
        edges.iter().all(|e| !is_containment(e)),
        "an edge by value is stated though the store does not know containment"
    );
    let content = state.canonical(CHANNEL_ONLY_A, "witsml21.Channel");
    let read = store.get_objects(&[content]).await;
    rule!(
        &CONT_UNAWARE,
        matches!(
            read.as_slice(),
            [Err(Error::NotFound(_) | Error::Unsupported { .. })]
        ),
        "a contained object was asked of a store that does not know containment and it answered {}",
        briefs(&read)
    );
}

// ---------------------------------------------------------------------------
// Writes
// ---------------------------------------------------------------------------

/// **Writes that need containment** (CONT-12): while `prune_contained` is off,
/// writing alone an object that exists only by value, and deleting a container
/// without prune, are refused by name and change nothing.
///
/// Plays when the store writes, knows containment and does not honour
/// `prune_contained` ([`crate::contract::containment`] covers the flag itself).
pub async fn contained_writes<S: EnergymlWriter + Send + Sync>(make: &dyn Fn() -> S, registry: &Registry) {
    let mut store = make();
    let model = model_witsml();
    let state = held(&store, &model).await;
    let capabilities = store.capabilities();
    if !capabilities.writes || !capabilities.contained_objects || capabilities.prune_contained {
        return;
    }
    let before_uris: BTreeSet<String> = state.parts.iter().map(|p| p.uri.clone()).collect();
    let before_contents: BTreeSet<String> = must(&CONT_WRITES, "contents()", store.contents().await)
        .objects
        .iter()
        .map(|o| o.uri.clone())
        .collect();

    // Writing alone an object that exists only by value.
    let alone = standalone_channel(CHANNEL_ONLY_B, "written alone");
    let put = store.put_objects(vec![alone.read(registry)], false).await;
    rule!(
        &CONT_WRITES,
        matches!(put.as_slice(), [Err(Error::Unsupported { .. })]),
        "putting alone {}, which the store holds only by value, answered {}",
        label(CHANNEL_ONLY_B),
        briefs(&put)
    );

    // Deleting a container without prune.
    let deleted = store.delete_objects(&[state.part_uri(LOG)], false).await;
    rule!(
        &CONT_WRITES,
        matches!(deleted.as_slice(), [Err(Error::Unsupported { .. })]),
        "deleting the log without prune answered {}",
        briefs(&deleted)
    );

    // Nothing changed.
    let after_uris: BTreeSet<String> = must(&CONT_WRITES, "resources()", store.resources().await)
        .into_iter()
        .map(|s| s.uri)
        .collect();
    let after_contents: BTreeSet<String> = must(&CONT_WRITES, "contents()", store.contents().await)
        .objects
        .iter()
        .map(|o| o.uri.clone())
        .collect();
    rule!(
        &CONT_WRITES,
        after_uris == before_uris && after_contents == before_contents,
        "a refused write changed the store: {} parts and {} contained objects listed, {} and {} before",
        after_uris.len(),
        after_contents.len(),
        before_uris.len(),
        before_contents.len()
    );
}

// ---------------------------------------------------------------------------
// Everything
// ---------------------------------------------------------------------------

/// Every group of the containment contract, in turn, on stores the factories
/// build fresh each time.
///
/// - `make`: a store holding [`model_witsml`];
/// - `make_with_part`: a store holding [`model_witsml_with_part`];
/// - `make_plain`: a store holding the RESQML model of [`crate::contract::model`];
/// - `scans`: the store's counter of walked parts, see [`contained_cost`].
///
/// [`crate::contract::all`] cannot call these: its factory holds the RESQML
/// model, and the WITSML model needs stores of its own.
pub async fn witsml<S: EnergymlWriter + Send + Sync>(
    make: &dyn Fn() -> S,
    make_with_part: &dyn Fn() -> S,
    make_plain: &dyn Fn() -> S,
    registry: &Registry,
    scans: &dyn Fn(&S) -> u64,
) {
    contents(make).await;
    containment_edges(make).await;
    contained_reads(make).await;
    contained_references(make).await;
    contained_part(make_with_part).await;
    contained_cost(make, make_plain, scans).await;
    contained_unaware(make).await;
    contained_writes(make, registry).await;
}
