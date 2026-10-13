//! The colour cascade under the inheritance order (ADR-0009).
//!
//! [`crate::inherit::Walker`] knows the order (the object, its collections, its
//! parents, theirs, up the FIRP chain, then the model) and nothing about
//! colours; this module gives it the question — *"does this object carry a
//! default graphical information for the whole object?"* — and keeps what the
//! answer needs to be a colour: the [`Decision`] with its level, **and the path
//! that found it**.
//!
//! # What changes against [`Cascade::resolve`]
//!
//! `resolve` and `chain` read three parent fields from the objects and know no
//! collection and no model; they stay for a caller that has no package index
//! (they are the order of 05/09, and the doc of `color` says so). This module
//! is the order of 02/10, and it needs the index: memberships are read on the
//! edges, not on the objects.
//!
//! * a **representation** (or any object): itself, its collections, its
//!   parents and theirs, the model — nothing found is `None`, and the caller
//!   keeps the hue of the uuid;
//! * a **property** (§2.2): the property, its kind, the kind's parents, a
//!   collection at each step — and **nothing beyond**: no representation, no
//!   model. Nothing found is the view's choice.
//!
//! The level of an answer is read from the **path**, not from the rank: the
//! last step says whether the carrier is a collection, the model, or a parent —
//! and a parent is told apart by the type the index holds for it, like
//! `color::level_of_reference` does for a reference.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::color::{Cascade, Decision, Level, Request, Scope, Step as Rung};
use crate::color_walks::{Ask, ColourCache, Scoped, drawn_by};
use crate::error::{Error, Result};
use crate::inherit::{Resolution, Step, Via, Walked, Walker, identity_of, uuid_of_identity};
use crate::package_index::PackageIndex;
use crate::source::ObjectSource;

/// A colour ladder under the inheritance order: the rungs a person can choose
/// from, which one the order takes, and the walk that took it.
#[derive(Clone, Debug)]
pub struct Ladder {
    /// The rungs, the object first. The parents (or the kind chain) in the
    /// order of the chain, and the collections or the model **on the path** that
    /// answered, each placed right after the rung it was reached from.
    pub steps: Vec<Rung>,
    /// The rung that answered, or `None`: the same answer [`Cascade::inherited`]
    /// gives.
    pub chosen: Option<usize>,
    /// The walk itself: rank, path, ties, notes. `walked.resolution`'s decision
    /// is the one `chosen` points at.
    pub walked: Walked<Decision>,
    /// **The colour per target** the object's geometry draws
    /// ([`crate::color_walks::drawn_by`]), each walked from the same walk as
    /// `walked` — the cascade runs once per object, a target the walk had not
    /// yet answered resumes it. Empty for a property and for an object whose
    /// kind draws no target of its own.
    ///
    /// An edge colour of a surface is **its faces'** (`merged_into`): the front end
    /// draws a surface's wireframe with its faces, and says so.
    pub targets: Vec<Scoped>,
}

/// What the walk asks of the set, and what it remembers of the question.
struct Probe<'c, 'a> {
    cascade: &'c Cascade<'a>,
    /// The first error a colour map reading gave: the predicate cannot return it.
    error: RefCell<Option<Error>>,
    /// Why an object that carries nothing carried nothing (a viewer kind that
    /// is not ours…), by lowercase uuid — attached to the answers whose path
    /// goes through it.
    skipped: RefCell<HashMap<String, Vec<String>>>,
}

impl Probe<'_, '_> {
    fn new<'c, 'a>(cascade: &'c Cascade<'a>) -> Probe<'c, 'a> {
        Probe {
            cascade,
            error: RefCell::default(),
            skipped: RefCell::default(),
        }
    }

    /// A `ColorInformation` on the object — what a property, its kind or a
    /// collection of either carries.
    fn colour(&self, objects: &dyn ObjectSource, uuid: &str) -> Option<Decision> {
        let mut notes = Vec::new();
        match self
            .cascade
            .color_information_on(uuid, Level::Property, objects, &mut notes)
        {
            Ok(found) => {
                if found.is_none() && !notes.is_empty() {
                    self.skipped.borrow_mut().insert(uuid.to_ascii_lowercase(), notes);
                }
                found
            }
            Err(why) => {
                self.error.borrow_mut().get_or_insert(why);
                None
            }
        }
    }

    fn failed(&self) -> Result<()> {
        match self.error.borrow_mut().take() {
            Some(why) => Err(why),
            None => Ok(()),
        }
    }
}

/// What kind of object a uuid is, as the level it would answer at.
fn parent_level(index: &PackageIndex, uuid: &str, steps_up: usize) -> Level {
    let key = index.node(uuid).map(|node| node.type_key.clone());
    match key.as_deref() {
        Some(key) if key.ends_with("Interpretation") => Level::Interpretation,
        Some(key) if key.ends_with("Feature") => Level::Feature,
        // Not in the package, or a type that is neither: what the step count
        // says, as `color::Cascade` did before the type was read.
        _ => {
            if steps_up <= 1 {
                Level::Interpretation
            } else {
                Level::Feature
            }
        }
    }
}

/// The level an answer comes at, from its path.
pub(crate) fn level_of(index: &PackageIndex, property: bool, carrier: &str, path: &[Step]) -> Level {
    match path.last() {
        None if property => Level::Property,
        None => Level::Representation,
        Some(last) => match last.via {
            Via::Collection => Level::Collection,
            Via::Model => Level::Model,
            Via::Parent if property => Level::PropertyKind,
            Via::Parent => {
                let ups = path.iter().filter(|step| step.via == Via::Parent).count();
                parent_level(index, carrier, ups)
            }
        },
    }
}

fn relevel(index: &PackageIndex, property: bool, resolution: &mut Resolution<Decision>) {
    resolution.value.level = level_of(index, property, &resolution.carrier_uri, &resolution.path);
    for tie in &mut resolution.ties {
        tie.value.level = level_of(index, property, &tie.carrier_uri, &tie.path);
    }
}

impl<'a> Cascade<'a> {
    /// What one object inherits, by the order of ADR-0009.
    ///
    /// `Ok` with `resolution: None` is an answer — the walk found nothing and
    /// says why in `notes`; the caller keeps the hue of the uuid.
    pub fn inherited(
        &self,
        index: &PackageIndex,
        objects: &dyn ObjectSource,
        address: &str,
    ) -> Result<Walked<Decision>> {
        let mut all = self.inherited_all(index, objects, &[address])?;
        Ok(all.pop().expect("one answer for one object"))
    }

    /// [`Cascade::inherited`] for a population, **one walker for all**: a
    /// collection that two thousand wells share is asked once
    /// (what is per package is paid once per package).
    ///
    /// The whole object, through [`ColourCache`] — the one implementation of the
    /// walk of an object's colour; here the cache lives for the call.
    pub fn inherited_all<S: AsRef<str>>(
        &self,
        index: &PackageIndex,
        objects: &dyn ObjectSource,
        addresses: &[S],
    ) -> Result<Vec<Walked<Decision>>> {
        let mut cache = ColourCache::new();
        let scopes = [Scope::WholeObject];
        let asks: Vec<Ask<'_>> = addresses
            .iter()
            .map(|address| Ask {
                uri: address.as_ref(),
                scopes: &scopes,
            })
            .collect();
        let answers = cache.resolve(self, index, objects, &asks);
        Ok(answers.into_iter().map(|mut one| one.remove(0).walked).collect())
    }

    /// What a **property** inherits (§2.2): itself, its kind, the kind's
    /// parents, a collection at each step, and nothing beyond. `kinds` is the
    /// kind then its parents, nearest first — `Request::kind_chain`.
    pub fn inherited_property(
        &self,
        index: &PackageIndex,
        objects: &dyn ObjectSource,
        property: &str,
        kinds: &[String],
    ) -> Result<Walked<Decision>> {
        let probe = Probe::new(self);
        let has = |identity: &str| probe.colour(objects, identity);
        let walker = Walker::new(index, objects, &has);
        let mut walked = if self.inherit {
            walker.resolve_property(property, kinds)
        } else {
            self.own_only(&walker.identity(property), |identity| {
                probe.colour(objects, identity)
            })
        };
        Self::settle(index, &probe, true, &mut walked);
        probe.failed()?;
        Ok(walked)
    }

    /// `inherit = false`: only what the object carries itself — and the walk's
    /// notes say that nothing else was looked at.
    fn own_only(&self, identity: &str, has: impl Fn(&str) -> Option<Decision>) -> Walked<Decision> {
        match has(identity) {
            Some(value) => {
                let resolution = Resolution {
                    value,
                    rank: 1,
                    carrier: uuid_of_identity(identity),
                    carrier_uri: identity.to_string(),
                    path: Vec::new(),
                    ties: Vec::new(),
                    notes: Vec::new(),
                };
                Walked {
                    notes: Vec::new(),
                    resolution: Some(resolution),
                }
            }
            None => Walked {
                resolution: None,
                notes: vec![
                    "inheritance is switched off, so the collections, the parents and the model were not \
                     looked at"
                        .to_string(),
                ],
            },
        }
    }

    /// The level of the answer, and the reasons the objects on its path were
    /// skipped before.
    fn settle(index: &PackageIndex, probe: &Probe<'_, '_>, property: bool, walked: &mut Walked<Decision>) {
        let Some(resolution) = walked.resolution.as_mut() else {
            return;
        };
        relevel(index, property, resolution);
        let skipped = probe.skipped.borrow();
        let on_path = std::iter::once(resolution.carrier_uri.clone()).chain(
            resolution
                .path
                .iter()
                .flat_map(|step| [step.from_uri.clone(), step.to_uri.clone()]),
        );
        for identity in on_path {
            if let Some(notes) = skipped.get(&identity.to_ascii_lowercase()) {
                for note in notes {
                    if !resolution.notes.contains(note) {
                        resolution.notes.push(note.clone());
                    }
                }
            }
        }
        walked.notes = resolution.notes.clone();
    }

    /// The ladder of each request — rungs to choose from, the order's pick, and
    /// the path — **one walker for the batch**.
    ///
    /// A request with a `property` is walked as a property (§2.2) and its rungs
    /// are the property and its kinds; any other is walked as an object, and its
    /// rungs are the object and what it represents, as [`Cascade::chain`] lists
    /// them, plus the collections and the model **on the path that answered** —
    /// each right after the rung it was reached from, so the order of the list
    /// is still the order the walk went.
    ///
    /// The cache lives for the call; [`Cascade::ladders_cached`] keeps it.
    pub fn ladders(
        &self,
        index: &PackageIndex,
        objects: &dyn ObjectSource,
        requests: &[Request<'_>],
    ) -> Result<Vec<Ladder>> {
        self.ladders_cached(&mut ColourCache::new(), index, objects, requests)
    }

    /// [`Cascade::ladders`] over a [`ColourCache`] the caller keeps between
    /// calls: **the walk of an object is done once**.
    ///
    /// Each object request is asked for the whole object **and** for the
    /// targets its geometry draws ([`drawn_by`], by the type the index holds for
    /// it): a polyline asks nodes and edges, never faces. The walk is resumed
    /// for a target the tiers already read do not answer, and for that target
    /// only. A surface's edges are not asked: they are drawn with its faces,
    /// and `targets` says so — and says when the file gave them another colour,
    /// if the walk had read it on the way.
    pub fn ladders_cached(
        &self,
        cache: &mut ColourCache,
        index: &PackageIndex,
        objects: &dyn ObjectSource,
        requests: &[Request<'_>],
    ) -> Result<Vec<Ladder>> {
        // The objects first, all at once: one walker, and what is shared is read
        // once.
        let mut wanted: Vec<Vec<Scope>> = Vec::new();
        let mut asked: Vec<usize> = Vec::new();
        let mut shapes_of = Vec::new();
        for (at, request) in requests.iter().enumerate() {
            if request.property.is_some() {
                continue;
            }
            let shape = drawn_by(
                index
                    .node(request.representation)
                    .map(|node| &*node.type_key)
                    .unwrap_or(""),
            );
            let mut scopes = vec![Scope::WholeObject];
            scopes.extend_from_slice(shape.scopes);
            wanted.push(scopes);
            asked.push(at);
            shapes_of.push(shape);
        }
        let asks: Vec<Ask<'_>> = asked
            .iter()
            .zip(&wanted)
            .map(|(at, scopes)| Ask {
                uri: requests[*at].representation,
                scopes,
            })
            .collect();
        let mut answers = cache.resolve(self, index, objects, &asks).into_iter();
        let mut shapes = shapes_of.into_iter();

        let probe = Probe::new(self);
        let mut out = Vec::with_capacity(requests.len());
        for request in requests {
            let property = request.property.is_some();
            let (walked, targets) = match request.property {
                Some(property) => (
                    self.inherited_property(index, objects, property, request.kind_chain)?,
                    Vec::new(),
                ),
                None => {
                    let mut scoped = answers.next().expect("one answer per object request");
                    let shape = shapes.next().expect("one shape per object request");
                    let whole = scoped.remove(0);
                    let mut targets = scoped;
                    if shape.edges_follow_faces {
                        let edges = merged_edges(cache, index, request.representation, &targets);
                        targets.push(edges);
                    }
                    (whole.walked, targets)
                }
            };
            // The rungs go by identity, the walk's: every address of the request
            // becomes the uri of the node the index holds for it before the
            // chain is listed, so that the rung that carried is found by
            // comparing uris and nothing else.
            let representation = identity_of(index, request.representation);
            let property_uri = request.property.map(|property| identity_of(index, property));
            let kinds: Vec<String> = request
                .kind_chain
                .iter()
                .map(|kind| identity_of(index, kind))
                .collect();
            let canonical = Request {
                representation: &representation,
                property: property_uri.as_deref(),
                kind_chain: &kinds,
                element: request.element,
            };
            let mut steps = self.chain(&canonical, objects)?;
            for rung in &mut steps {
                rung.uri = identity_of(index, &rung.uri);
            }
            if property {
                steps.retain(|rung| matches!(rung.level, Level::Property | Level::PropertyKind));
            } else {
                for rung in steps.iter_mut().skip(1) {
                    rung.level = parent_level_of_rung(index, &rung.uri, rung.level);
                }
            }
            let chosen = self.place(index, objects, property, &walked, &mut steps)?;
            let mut walked = walked;
            // One word for one answer: the rung the front end marks and the decision
            // it reads beside it say the same level.
            if let (Some(at), Some(resolution)) = (chosen, walked.resolution.as_mut()) {
                resolution.value.level = steps[at].level;
            }
            out.push(Ladder {
                steps,
                chosen,
                walked,
                targets,
            });
        }
        probe.failed()?;
        Ok(out)
    }

    /// Put the nodes of the answering path that the chain does not list, and
    /// find the carrier's rung.
    fn place(
        &self,
        index: &PackageIndex,
        objects: &dyn ObjectSource,
        property: bool,
        walked: &Walked<Decision>,
        steps: &mut Vec<Rung>,
    ) -> Result<Option<usize>> {
        let Some(resolution) = walked.resolution.as_ref() else {
            return Ok(None);
        };
        // By uri: the identity of a rung, which two versions of one object do
        // not share.
        let find =
            |steps: &[Rung], uri: &str| steps.iter().position(|rung| rung.uri.eq_ignore_ascii_case(uri));
        for (n, step) in resolution.path.iter().enumerate() {
            if find(steps, &step.to_uri).is_some() {
                continue;
            }
            let after = find(steps, &step.from_uri).unwrap_or(0);
            let level = level_of(index, property, &step.to_uri, &resolution.path[..=n]);
            let mut notes = Vec::new();
            let carries = if property {
                self.color_information_on(&step.to_uri, level, objects, &mut notes)?
            } else {
                self.default_information_on(&step.to_uri, level, Scope::WholeObject, &mut notes)
            };
            steps.insert(
                after + 1,
                Rung {
                    level,
                    uuid: step.to.clone(),
                    uri: step.to_uri.clone(),
                    carries,
                },
            );
        }
        Ok(find(steps, &resolution.carrier_uri))
    }
}

/// The edges of a surface, which are drawn with its faces: the faces'
/// answer, named for the edges — and, when the walk had already read another
/// colour for the edges, **which one**, so it is not silently overridden.
fn merged_edges(cache: &ColourCache, index: &PackageIndex, address: &str, targets: &[Scoped]) -> Scoped {
    let uuid = uuid_of_identity(address);
    let faces = targets.iter().find(|one| one.scope == Scope::Faces);
    let mut edges = match faces {
        Some(faces) => faces.clone(),
        None => Scoped {
            scope: Scope::Edges,
            walked: Walked {
                resolution: None,
                notes: Vec::new(),
            },
            exact: true,
            merged_into: None,
            merged_note: None,
        },
    };
    edges.scope = Scope::Edges;
    edges.merged_into = Some(Scope::Faces);
    // What the file says for the edges, from the tiers the walk has read — a peek:
    // the walk is not resumed for a colour that is not drawn.
    if let Some(own) = cache.peek(index, address, Scope::Edges)
        && let Some(theirs) = own.decision()
        && own.exact
        && faces.and_then(Scoped::decision).map(|d| (&d.paint, d.visible))
            != Some((&theirs.paint, theirs.visible))
    {
        edges.merged_note = Some(format!(
            "the file gives the edges of {uuid} a colour of their own (on {}); they are drawn with the colour of \
             the faces, as the scene draws a surface's wireframe",
            theirs.found_on
        ));
    }
    edges
}

/// The level a parent rung is told by: the type the package holds for it, the
/// chain's own reading when it holds none.
fn parent_level_of_rung(index: &PackageIndex, address: &str, chain_level: Level) -> Level {
    match chain_level {
        Level::Interpretation | Level::Feature => {
            let key = index.node(address).map(|node| node.type_key.clone());
            match key.as_deref() {
                Some(key) if key.ends_with("Interpretation") => Level::Interpretation,
                Some(key) if key.ends_with("Feature") => Level::Feature,
                _ => chain_level,
            }
        }
        other => other,
    }
}
