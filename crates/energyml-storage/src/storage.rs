//! The mother trait, shaped after ETP.
//!
//! Designed from the ETP v1.2 requests rather than from a zip file, for the
//! reason set out in ADR-0002: an abstraction drawn
//! around a file has to be retorted the day a network transport arrives, while
//! one drawn around the protocol takes a file in its stride.
//!
//! What that decision implies, and what the shape below reflects:
//!
//! - **Addressing is by URI**, never by path. `eml:///dataspace('x')/resqml20.obj_Activity(uuid)`.
//! - **Operations are plural.** ETP asks for a hundred objects in one message;
//!   a trait that only offers `get_one` condemns the network implementation to
//!   be slow, and no amount of layering on top can undo that.
//! - **A response is partial by nature.** One object out of a hundred being
//!   unreadable must not fail the other ninety-nine, so the result is a vector
//!   of results, not a result of a vector.
//!
//! # Asynchronous, and why the EPC is not
//!
//! This trait is `async` because ETP is a WebSocket and WebAssembly cannot
//! block. Applications that only ever touch a local file do not have to pay for
//! that: [`crate::epc::EpcStore`] has a complete **synchronous** inherent API,
//! and its implementation of this trait is a one-line wrapper per method.
//!
//! The trade is per application, not per call:
//!
//! | The application wants | It writes | It pays |
//! |---|---|---|
//! | to switch EPC ↔ ETP in one line | `dyn EnergymlStorage` | being async |
//! | never to see `async` | `EpcStore` directly | being tied to EPC |

use async_trait::async_trait;
use energyml_core::reflect::TypeInfo;
use energyml_core::registry::DynObject;

use crate::array::{ArrayData, ArrayLocation, ArrayMetadata};
use crate::error::Result;
use crate::resource::{Dataspace, ObjectHeader, ObjectSummary};

/// `async_trait` without the `Send` bound on wasm, where futures are not `Send`
/// and requiring it would rule out every browser implementation.
#[cfg(target_arch = "wasm32")]
pub use async_trait::async_trait as storage_async_trait;

/// What a store holds by value, beside its [`EnergymlStorage::resources`]: the
/// contained objects and the edges from each to its direct container.
///
/// `#[non_exhaustive]` and built from its default, like the other records of
/// this crate.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Contents {
    /// The contained objects, under their canonical URI. Not the ones that are
    /// also a part (the part is the object), not the same object twice.
    pub objects: Vec<ObjectSummary>,
    /// One edge per (contained object, direct container), `Edge::by_value`, path empty.
    pub edges: Vec<crate::graph::Edge>,
}

/// Reading side of a data store - an EPC file, an ETP dataspace.
///
/// The writing side is [`crate::write::EnergymlWriter`], the level above one
/// dataspace [`crate::dataspaces::EnergymlDataspaces`], the change feed
/// [`crate::changes::EnergymlChanges`]. What a store offers of all that, it says
/// in [`EnergymlStorage::capabilities`].
// `async_trait` marks the boxed future of each default method `#[must_use]`,
// which clippy 1.99 (`double_must_use`, CI 02/10/2026) reads as ours: the
// future is already `must_use`. The attribute is the macro's, not this code's.
#[allow(clippy::double_must_use)]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
pub trait EnergymlStorage {
    /// What this store is called, for diagnostics.
    fn store_kind(&self) -> &'static str;

    /// The dataspaces this store holds - ETP protocol 24, `GetDataspaces`.
    ///
    /// An EPC returns exactly one: a file *is* a dataspace. That is what lets
    /// code walking a remote store walk a file unchanged.
    async fn dataspaces(&self) -> Result<Vec<Dataspace>>;

    /// Every object in the store, identity only - ETP protocol 3,
    /// `GetResources`.
    ///
    /// Cheap by construction: for an EPC this reads `[Content_Types].xml` and
    /// nothing else. No object XML is opened. **The parts only**: the data
    /// objects a part holds by value are [`EnergymlStorage::contents`].
    async fn resources(&self) -> Result<Vec<ObjectSummary>>;

    /// The data objects held **by value** inside the objects of
    /// [`EnergymlStorage::resources`] and not listed by it (a WITSML `Channel`
    /// inside a `ChannelSet` inside a `Log`; ETP 1.2 section 9.1.3, p. 178), with
    /// the edge from each to the object that directly holds it (section
    /// 8.1.1.1.1, p. 156: the contained object is the source).
    ///
    /// **Provided**: empty. A store that knows containment
    /// ([`crate::Capabilities::contained_objects`]) overrides it; an ETP server
    /// already lists its contained objects in `resources()`, so `EtpStore` keeps
    /// the default. An EPC reads the container parts to answer, once.
    async fn contents(&self) -> Result<Contents> {
        Ok(Contents::default())
    }

    /// Citations for the given objects, without deserializing them.
    ///
    /// Level 2 of the three (ADR-0002): the parser
    /// stops as soon as the citation has gone by. This is what a listing shows.
    ///
    /// One result per requested URI, in the same order.
    async fn headers(&self, uris: &[String]) -> Vec<Result<ObjectHeader>>;

    /// The objects themselves - ETP protocol 4, `GetDataObjects`.
    ///
    /// One result per requested URI, in the same order. An unreadable object
    /// does not fail the others.
    async fn get_objects(&self, uris: &[String]) -> Vec<Result<Box<dyn DynObject>>>;

    /// The types this store can produce - ETP protocol 25, `GetSupportedTypes`.
    ///
    /// For an EPC, whatever the registry it was given knows about.
    async fn supported_types(&self) -> Vec<&'static TypeInfo>;

    /// Every array an object points at, resolved to something this store can
    /// read.
    ///
    /// The missing half of [`EnergymlStorage::read_array`]: without it a caller
    /// holding `dyn EnergymlStorage` could read an array it had been handed the
    /// location of, but could not *find* the arrays of an object it had just
    /// deserialized - so every layer that walks an object's data had to name
    /// the concrete store, which is exactly what this trait exists to avoid.
    ///
    /// A list rather than one: eml 2.3 lets a single logical array be spread
    /// over several `ExternalDataArrayPart`s.
    ///
    /// Synchronous, and deliberately: locating is reflection over an object the
    /// caller already holds, plus a lookup in the store's own index - the EPC
    /// resolves an `EpcExternalPartReference` from its inventory, and an ETP
    /// client resolves the same DOR from the resources it has already
    /// discovered. Neither needs to go back to the wire, and making this
    /// `async` would have coloured every geometry reader for nothing.
    fn arrays_of(&self, object: &dyn energyml_core::value::Reflect) -> Vec<ArrayLocation>;

    /// Shape and element type of an array, without reading it - ETP protocol 9,
    /// `GetDataArrayMetadata`.
    async fn array_metadata(&self, location: &ArrayLocation) -> Result<ArrayMetadata>;

    /// An array, or the slice named by the location - ETP protocol 9,
    /// `GetDataArrays` and `GetDataSubarrays`.
    ///
    /// The slice lives in the location itself because that is where the newer
    /// EnergyML form puts it (`Count`, `StartIndex`), and because it is what
    /// the protocol asks for.
    async fn read_array(&self, location: &ArrayLocation) -> Result<ArrayData>;

    /// Everything the store noticed that departs from the specification.
    ///
    /// Never empty by accident: a conformant package produces none, a real one
    /// usually produces a few. See [`crate::error::Diagnostic`].
    ///
    /// Returned by value, not borrowed, because a store that reads lazily
    /// discovers departures **after** it was opened - the fallback taken when
    /// an HDF5 proxy is missing is found the first time an array is located,
    /// not when the archive was indexed. A borrowed slice would have forced
    /// those to be dropped or the whole package to be read eagerly.
    fn diagnostics(&self) -> Vec<crate::error::Diagnostic>;

    /// What this store offers beyond reading, said before it is asked.
    ///
    /// **Provided**: nothing beyond reading. A store that writes, holds several
    /// dataspaces or keeps its deletions says so by overriding this.
    fn capabilities(&self) -> crate::capabilities::Capabilities {
        crate::capabilities::Capabilities::default()
    }

    /// The objects as bytes - ETP protocol 4, `GetDataObjects` with a `format`.
    /// One result per URI, in the same order.
    ///
    /// **Provided**: refused by name, per object. A store that keeps the bytes
    /// (an EPC holds the XML of each part, a server what it was sent) overrides
    /// it and lists the formats in [`EnergymlStorage::capabilities`].
    async fn get_raw(
        &self,
        uris: &[String],
        format: crate::raw::RawFormat,
    ) -> Vec<Result<crate::raw::RawObject>> {
        let _ = format;
        uris.iter()
            .map(|_| {
                Err(crate::error::Error::Unsupported {
                    operation: "reading objects as bytes",
                    store: self.store_kind(),
                })
            })
            .collect()
    }

    /// The objects a context reaches, with the edges between them - ETP
    /// protocol 3, `GetResources` (and `FindResources`, `GetSupportedTypes`
    /// and a notification subscription, which name objects the same way).
    ///
    /// **Provided, and meant to be overridden**, like [`EnergymlStorage::edges`]:
    /// the default walks [`EnergymlStorage::resources`] and
    /// [`EnergymlStorage::edges`] with [`crate::context::discover`] - right
    /// everywhere, and a whole-store read where an indexed store answers from
    /// its index. It refuses a store holding several dataspaces: which one the
    /// listing belongs to is that store's to say.
    async fn resources_in(&self, context: &crate::context::Context) -> Result<crate::context::Found> {
        let dataspaces = self.dataspaces().await?;
        let [dataspace] = dataspaces.as_slice() else {
            return Err(crate::error::Error::Unsupported {
                operation: "walking a context over several dataspaces",
                store: self.store_kind(),
            });
        };
        let mut resources = self.resources().await?;
        let mut edges = self.edges(&[]).await?;
        // The data objects held by value are objects of the dataspace, and
        // their edge to the container is a relationship it states (ETP 1.2
        // section 8.2.2 row 9: a dataspace returns its data objects).
        if self.capabilities().contained_objects {
            let contents = self.contents().await?;
            resources.extend(contents.objects);
            edges.extend(contents.edges);
        }
        crate::context::discover(&dataspace.uri, resources, edges, context)
    }

    /// The objects deleted from a dataspace - ETP protocol 3,
    /// `GetDeletedResources`.
    ///
    /// **Provided**: refused by name. A store that remembers deletions overrides
    /// it and says for how long in [`EnergymlStorage::capabilities`]; a store
    /// that does not must not answer "none deleted".
    async fn deleted_resources(
        &self,
        filter: &crate::context::DeletedFilter,
    ) -> Result<Vec<crate::context::DeletedResource>> {
        let _ = filter;
        Err(crate::error::Error::Unsupported {
            operation: "listing deleted objects",
            store: self.store_kind(),
        })
    }

    /// One object, for the cases where asking for one is what the caller means.
    ///
    /// A convenience over [`EnergymlStorage::get_objects`] - the singular may
    /// exist above the plural, never beneath it.
    async fn get_object(&self, uri: &str) -> Result<Box<dyn DynObject>> {
        let mut results = self.get_objects(std::slice::from_ref(&uri.to_string())).await;
        results
            .pop()
            .unwrap_or_else(|| Err(crate::error::Error::NotFound(uri.to_string())))
    }

    /// One header.
    async fn header(&self, uri: &str) -> Result<ObjectHeader> {
        let mut results = self.headers(std::slice::from_ref(&uri.to_string())).await;
        results
            .pop()
            .unwrap_or_else(|| Err(crate::error::Error::NotFound(uri.to_string())))
    }

    /// What the objects **write**: every reference, as the text of each object
    /// states it (a `DataObjectReference`), with the field it sits in - those
    /// that point at an object the store does not hold included. That is what a
    /// dangling reference is, what a push leaves behind, what a graph shows as an
    /// absent node.
    ///
    /// An empty `uris` means the whole store.
    ///
    /// **Provided**: the objects are read ([`EnergymlStorage::derived_graph`]).
    /// Cheap on a local file; a store that indexes its references (an EPC's
    /// package index) may answer from it, never with less.
    ///
    /// **The source is the object that writes the reference** (ETP 1.2
    /// section 8.1.1.1.1, p. 156). When the store lists what its objects hold
    /// by value ([`crate::Capabilities::contained_objects`]), a DOR written
    /// inside a contained object (a WITSML `Channel` in a `Log`) is that
    /// object's, its path relative to it ([`crate::graph::attribute_to_writers`]).
    /// Asking for an object gives what it writes and what the objects it holds
    /// write; asking for a contained object reads the part that holds it.
    ///
    /// Not [`EnergymlStorage::edges`]: see the design notes
    /// (`deux-sources-un-contrat.md` §3) for the two facts the word "edge" was
    /// covering.
    async fn references(&self, uris: &[String]) -> Result<Vec<crate::graph::Edge>> {
        let contents = if self.capabilities().contained_objects {
            self.contents().await?
        } else {
            Contents::default()
        };
        let by_uri: std::collections::HashMap<&str, &ObjectSummary> = contents
            .objects
            .iter()
            .map(|entry| (entry.uri.as_str(), entry))
            .collect();
        // A contained object is read in the part that holds it.
        let mut parts: Vec<String> = Vec::new();
        for uri in uris {
            match by_uri.get(uri.as_str()) {
                Some(entry) => parts.extend(crate::graph::parts_holding(&by_uri, entry)),
                None => parts.push(uri.clone()),
            }
        }
        if !uris.is_empty() && parts.is_empty() {
            return Ok(Vec::new());
        }
        let held = read_parts(self, &parts).await?;
        let graph = crate::graph::build(
            held.iter()
                .map(|(summary, object)| (summary, object.as_reflect())),
        );
        let written = crate::graph::attribute_to_writers(
            graph.edges,
            held.iter()
                .map(|(summary, object)| (summary, object.as_reflect())),
            &contents.objects,
        );
        if uris.is_empty() {
            return Ok(written);
        }
        // What was asked for, and what the asked objects hold: a part read
        // only for a contained object it holds says nothing of its own.
        Ok(written
            .into_iter()
            .filter(|edge| {
                uris.contains(&edge.from)
                    || by_uri
                        .get(edge.from.as_str())
                        .is_some_and(|entry| uris.iter().any(|uri| crate::graph::within(&by_uri, entry, uri)))
            })
            .collect())
    }

    /// What the store **states**: the relationships between objects it holds -
    /// ETP protocol 3, `GetResources` with `includeEdges`, whose reply is
    /// `GetResourcesEdgesResponse`: both ends are objects of the store, and an
    /// answer carries those with either end among what it found (ETP 1.2
    /// §8.3.1, p. 163, 3.c). This is what discovery walks and counts.
    ///
    /// An empty `uris` means the whole store.
    ///
    /// **Provided, and meant to be overridden.** The default keeps the
    /// [`EnergymlStorage::references`] whose target the store holds and, when
    /// the store provides [`EnergymlStorage::kind_rules`], gives each its kind
    /// and the basis of it (RESQML always primary, ETP 1.2 §8.2.2; WITSML by the
    /// specification's examples, then by a labelled derivation; an ML with no
    /// rule, none: [`crate::kinds`]). Without rules the kind is unknown, since
    /// which relationship is primary is each ML's to say (ETP 1.2 §8.1.1.1.2,
    /// note 1). An ETP server answers natively, from its own index,
    /// without a single object crossing the wire, and states each edge's kind;
    /// its edges include by-value containment and carry no field path.
    ///
    /// A caller that needs every reference as written - a dangling one, what a
    /// push leaves behind - asks [`EnergymlStorage::references`] instead.
    ///
    /// The references are the DORs, not an index built beside them. An EPC's
    /// `.rels` say roughly the same thing and are checked *against* them by
    /// [`crate::graph::check_relationships`], which is why they are not the
    /// source here: the `.rels` speed things up, the DORs are authoritative.
    async fn edges(&self, uris: &[String]) -> Result<Vec<crate::graph::Edge>> {
        let held: std::collections::HashSet<String> = self
            .resources()
            .await?
            .into_iter()
            .map(|summary| summary.uri)
            .collect();
        let held_edges: Vec<crate::graph::Edge> = self
            .references(uris)
            .await?
            .into_iter()
            .filter(|edge| held.contains(&edge.to))
            .collect();
        // The kind is judged here and nowhere else, from the type of the source
        // and the path of the reference: no object is read for it. No rules, no cost.
        Ok(match self.kind_rules() {
            Some(rules) => crate::kinds::judge_edges(rules.as_ref(), held_edges),
            None => held_edges,
        })
    }

    /// The rules that give the kind of an edge this store derives: **provided**,
    /// `None` (the store states its edges' kinds itself, as an ETP server does,
    /// or states none). An implementation that derives its edges from the
    /// objects (`EpcStore`) returns the policy of [`crate::kinds`]; the default
    /// of [`EnergymlStorage::edges`] applies it, in that one place.
    fn kind_rules(&self) -> Option<std::sync::Arc<dyn crate::kinds::KindRules>> {
        None
    }

    /// The reference graph of the store, ready to draw.
    ///
    /// Nodes from [`EnergymlStorage::resources`] and
    /// [`EnergymlStorage::headers`], edges from [`EnergymlStorage::references`]
    /// - what the objects write, so that a reference to an object the store
    /// does not hold is drawn too.
    ///
    /// An empty `uris` means the whole store. Objects referenced but not held
    /// still appear, marked [`crate::graph::Node::present`] `false`.
    async fn graph(&self, uris: &[String]) -> Result<crate::graph::Graph> {
        let all = self.resources().await?;
        let mut wanted: Vec<ObjectSummary> = if uris.is_empty() {
            all
        } else {
            all.into_iter().filter(|s| uris.contains(&s.uri)).collect()
        };
        let mut edges = self.references(uris).await?;
        // The objects held by value are objects the store holds: drawn as
        // present, with their edge to the container, never as absent ones.
        if self.capabilities().contained_objects {
            let contents = self.contents().await?;
            let by_uri: std::collections::HashMap<&str, &ObjectSummary> = contents
                .objects
                .iter()
                .map(|entry| (entry.uri.as_str(), entry))
                .collect();
            let drawn: Vec<ObjectSummary> = contents
                .objects
                .iter()
                .filter(|entry| {
                    uris.is_empty()
                        || uris.contains(&entry.uri)
                        || uris.iter().any(|uri| crate::graph::within(&by_uri, entry, uri))
                })
                .cloned()
                .collect();
            let drawn_uris: std::collections::HashSet<&str> =
                drawn.iter().map(|entry| entry.uri.as_str()).collect();
            edges.extend(
                contents
                    .edges
                    .iter()
                    .filter(|edge| drawn_uris.contains(edge.from.as_str()))
                    .cloned(),
            );
            wanted.extend(drawn);
        }
        let of_wanted: Vec<String> = wanted.iter().map(|s| s.uri.clone()).collect();
        let headers = self.headers(&of_wanted).await;
        let titles: std::collections::HashMap<String, String> = headers
            .into_iter()
            .flatten()
            .filter_map(|header| {
                header
                    .title
                    .filter(|t| !t.trim().is_empty())
                    .map(|title| (header.summary.uri, title))
            })
            .collect();
        Ok(crate::graph::assemble(
            wanted.iter(),
            &|uri: &str| titles.get(uri).cloned(),
            edges,
        ))
    }

    /// The graph derived by reading the objects, whatever the store.
    ///
    /// The floor under [`EnergymlStorage::references`], kept separate so that
    /// an implementation overriding `references` does not accidentally make the
    /// default recurse through `graph`. Callers want `graph`; this is what makes
    /// it work with no help from the store.
    async fn derived_graph(&self, uris: &[String]) -> Result<crate::graph::Graph> {
        let held = read_parts(self, uris).await?;
        Ok(crate::graph::build(
            held.iter()
                .map(|(summary, object)| (summary, object.as_reflect())),
        ))
    }
}

/// The parts at `uris` (every part when empty), each with its object: those
/// that read. An object this store cannot deserialize contributes no edges, and
/// saying so with an error would lose the graph of everything else - the same
/// partiality the plural reads have.
async fn read_parts<S: EnergymlStorage + ?Sized>(
    store: &S,
    uris: &[String],
) -> Result<Vec<(ObjectSummary, Box<dyn DynObject>)>> {
    let all = store.resources().await?;
    let wanted: Vec<ObjectSummary> = if uris.is_empty() {
        all
    } else {
        all.into_iter().filter(|s| uris.contains(&s.uri)).collect()
    };
    let of_wanted: Vec<String> = wanted.iter().map(|s| s.uri.clone()).collect();
    let objects = store.get_objects(&of_wanted).await;
    Ok(wanted
        .into_iter()
        .zip(objects)
        .filter_map(|(summary, object)| object.ok().map(|object| (summary, object)))
        .collect())
}

/// Capabilities that belong to a package on a disk, and to nothing else.
///
/// Adding a PDF to a package makes sense; adding one to a dataspace does not -
/// ETP transports data objects, not arbitrary files. So these operations do not
/// sit on [`EnergymlStorage`]: an application that uses them is local, and the
/// compiler says so at compile time rather than the store saying so at run
/// time with an `Unsupported`.
pub trait LocalPackage {
    /// Non-EnergyML parts carried in the package: PDFs, images, CSV.
    ///
    /// Returns the part names; the bytes come from
    /// [`LocalPackage::read_attachment`].
    fn attachments(&self) -> Vec<String>;

    /// The bytes of one attachment.
    fn read_attachment(&self, part: &str) -> Result<Vec<u8>>;

    /// Relationships recorded for one part, as the package states them.
    ///
    /// An index, not the truth: what an object actually references comes from
    /// its DORs. Useful to check one against the other.
    fn relationships(&self, part: &str) -> Result<Vec<crate::resource::Relationship>>;
}
