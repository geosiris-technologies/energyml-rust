//! What one package keeps between two calculations — so that a face which
//! recomputes (the front end, on every change of trace or selection) reads each
//! object once.
//!
//! The calculation can exploit the optimisations the front end already caches, so
//! as not to recompute what the front end already has. The front end's own caches — its
//! package index, its colour walks — come in through
//! [`crate::read::PackageContext`]; this is the section's own, kept by the face
//! next to the package (one per package, as the wasm `Session` is).
//!
//! # What depends on what
//!
//! | kept | depends on | so a change of trace… |
//! |---|---|---|
//! | a well, read ([`WellRead`]) | the package, the scene's system | reads nothing again: wells are projected in `assemble` |
//! | a surface, placed ([`Prepared`]) | the package, the scene's system | reads nothing again |
//! | a surface's cut along one line | the above **and that line** (its polyline, nothing else) | cuts only the lines it has not cut: another section, a cut; a width, a key, an order, none |
//! | a well's head ([`HeadSlot`]) and the spatial index over the heads | the package, the scene's system | reads nothing again: the sections only choose which wells to read |
//!
//! The package is told by its **revision** (`Asked::revision`, the store's):
//! another revision, another store id or another scene and everything is
//! dropped — a stale well is a well drawn where the file no longer puts it.
//! A cut depends on the **line** and on nothing else of the section — not its
//! corridor, not its key, not the other sections — so it is kept under
//! `(surface, line)`, several per surface (ADR-0010),
//! and going A → B → A cuts nothing the third time. A line the surface misses
//! is kept too: it is an answer. What is kept carries no section key; the key
//! of the section asking is written on it when it is served.
//!
//! **Only what read is kept**: a refusal is said again, never remembered (the
//! rule of the wasm `PieceCache`), so a backend loaded since can answer.
//!
//! [`Work`] counts what was done, so the tests count work and never time
//! (`energyml-domain`'s `color_scopes` does the same).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::model::{SectionTrace, SurfaceCut, SurfaceRead, WellRead};
use crate::read::surfaces::Prepared;
use crate::read::wells::HeadRead;
use crate::spatial::{Index, Rect};

/// What a calculation did, summed since the cache was made.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Work {
    /// Wells read from the package.
    pub wells_read: usize,
    /// Surfaces read from the package and placed in the scene.
    pub surfaces_read: usize,
    /// Surfaces cut along at least one line they had not been cut along.
    pub surfaces_cut: usize,
    /// Lines cut, one surface each: a surface along three new lines is
    /// `surfaces_cut + 1` and `sections_cut + 3`.
    pub sections_cut: usize,
    /// Objects answered from the cache without any of the above.
    pub reused: usize,
    /// Heads read (no array) to decide which wells the corridor asks for.
    pub heads_computed: usize,
    /// Windows asked of the spatial index over the heads.
    pub spatial_queries: usize,
}

/// What the cache knows of one well's head.
#[derive(Clone, Debug)]
pub(crate) enum HeadSlot {
    /// The head, read, and where the well sits in the spatial index.
    Head { at: usize, head: Box<HeadRead> },
    /// The head could not be told (a refusal, a non-finite position): the
    /// well is read in full, and the full read says what is wrong.
    Unusable,
}

/// What the cache is exact for: the package's identity and revision, and the
/// scene its objects were placed in.
#[derive(Clone, Debug, PartialEq)]
struct Key {
    store: String,
    revision: u64,
    epsg: Option<u64>,
    unit: Option<String>,
    time_factor: Option<u64>,
}

/// A section's line, as the cut's identity: the bits of its polyline's `f64`s —
/// an exact key, never a rounded one.
type Line = Vec<[u64; 2]>;

fn line_of(section: &SectionTrace) -> Line {
    section.trace.iter().map(|p| [p[0].to_bits(), p[1].to_bits()]).collect()
}

/// One package's reads, kept — see the module.
#[derive(Default)]
pub struct PackageCache {
    key: Option<Key>,
    wells: HashMap<String, WellRead>,
    surfaces: HashMap<String, Arc<Prepared>>,
    /// The cuts of each surface by the line they were made along; `None` is
    /// "the line misses the surface". See [`Line`].
    cuts: HashMap<(String, Line), Option<SurfaceCut>>,
    work: Work,
    /// Every well's head, by uri.
    heads: HashMap<String, HeadSlot>,
    /// The heads as points; the payload is the position in `head_uris`.
    spatial: Index<usize>,
    head_uris: Vec<String>,
}

impl PackageCache {
    /// An empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// What was done since the cache was made.
    pub fn work(&self) -> Work {
        self.work
    }

    /// Drop everything — what a face does when it closes the package.
    pub fn clear(&mut self) {
        self.key = None;
        self.wells.clear();
        self.surfaces.clear();
        self.cuts.clear();
        self.heads.clear();
        self.spatial = Index::new();
        self.head_uris.clear();
    }

    /// Keep what is exact for this package, revision and scene; drop the rest.
    pub(crate) fn hold_for(&mut self, store: &str, revision: u64, scene: &crate::read::Scene<'_>) {
        let key = Key {
            store: store.to_string(),
            revision,
            epsg: scene.system.map(|s| s.epsg),
            unit: scene.system.map(|s| s.unit.clone()),
            time_factor: scene.time_factor.map(f64::to_bits),
        };
        if self.key.as_ref() != Some(&key) {
            self.clear();
            self.key = Some(key);
        }
    }

    /// A well, read once.
    pub(crate) fn well<E>(&mut self, uri: &str, read: impl FnOnce() -> Result<WellRead, E>) -> Result<WellRead, E> {
        if let Some(kept) = self.wells.get(uri) {
            self.work.reused += 1;
            return Ok(kept.clone());
        }
        let well = read()?;
        self.work.wells_read += 1;
        self.wells.insert(uri.to_string(), well.clone());
        Ok(well)
    }

    /// The head of a well, computed once (`read` tells it; no array is read).
    pub(crate) fn head(&mut self, uri: &str, read: impl FnOnce() -> Option<HeadRead>) -> &HeadSlot {
        if !self.heads.contains_key(uri) {
            self.work.heads_computed += 1;
            let slot = match read() {
                Some(head) if head.head.iter().all(|v| v.is_finite()) => {
                    let at = self.head_uris.len();
                    self.head_uris.push(uri.to_string());
                    if let Some(rect) = Rect::point([head.head[0], head.head[1]]) {
                        self.spatial.insert(rect, at);
                    }
                    HeadSlot::Head { at, head: Box::new(head) }
                }
                _ => HeadSlot::Unusable,
            };
            self.heads.insert(uri.to_string(), slot);
        }
        &self.heads[uri]
    }

    /// The positions (`HeadSlot::Head::at`) of the heads inside any of `windows`.
    pub(crate) fn heads_in(&mut self, windows: &[Rect]) -> HashSet<usize> {
        let mut found = HashSet::new();
        for window in windows {
            self.work.spatial_queries += 1;
            self.spatial.visit(window, &mut |at| {
                found.insert(*at);
            });
        }
        found
    }

    /// A surface cut along `sections`: placed once, and cut once **per line** —
    /// only the lines it has not been cut along are cut now. The answer is the
    /// one a fresh cache gives (the cuts in the order of `sections`, a missed
    /// line left out, each labelled with the key of the section asking).
    pub(crate) fn surface<E>(
        &mut self,
        uri: &str,
        sections: &[SectionTrace],
        prepare: impl FnOnce() -> Result<Prepared, E>,
    ) -> Result<SurfaceRead, E> {
        let lines: Vec<Line> = sections.iter().map(line_of).collect();
        // One representative per line the surface has not been cut along.
        let mut seen: HashSet<&Line> = HashSet::new();
        let missing: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter(|(_, line)| !self.cuts.contains_key(&(uri.to_string(), (*line).clone())) && seen.insert(line))
            .map(|(at, _)| at)
            .collect();

        let prepared = match self.surfaces.get(uri) {
            Some(kept) => Arc::clone(kept),
            None => {
                let placed = Arc::new(prepare()?);
                self.work.surfaces_read += 1;
                self.surfaces.insert(uri.to_string(), Arc::clone(&placed));
                placed
            }
        };
        if missing.is_empty() {
            self.work.reused += 1;
        } else {
            let asked: Vec<SectionTrace> = missing.iter().map(|at| sections[*at].clone()).collect();
            let made = crate::read::surfaces::cut_prepared(&prepared, &asked);
            self.work.surfaces_cut += 1;
            self.work.sections_cut += asked.len();
            for (at, one) in missing.iter().zip(&asked) {
                let found = made.cuts.iter().find(|cut| cut.section == one.key).cloned();
                self.cuts.insert((uri.to_string(), lines[*at].clone()), found);
            }
        }

        let cuts = sections
            .iter()
            .zip(&lines)
            .filter_map(|(section, line)| {
                let kept = self.cuts.get(&(uri.to_string(), line.clone()))?.as_ref()?;
                Some(SurfaceCut { section: section.key, ..kept.clone() })
            })
            .collect();
        Ok(SurfaceRead {
            object: prepared.object.clone(),
            role: prepared.role,
            time: prepared.time,
            colour: prepared.colour.clone(),
            age: prepared.age,
            feature: prepared.feature.clone(),
            cuts,
        })
    }
}
