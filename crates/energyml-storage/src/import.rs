//! Bringing part of another package — or all of it — into the one that is open.
//!
//! Asked for as: import part of another package — or all of it — an epc + h5
//! into the one already open, check for object conflicts so that the user
//! decides which epc keeps the final object, and likewise for the datasets:
//! whether the h5 files are merged or kept apart. The design, with each
//! decision argued, is kept with the design notes.
//!
//! # Why not [`crate::extract::copy_into`]
//!
//! It already copies a set and its closure from one package into another, and
//! it **writes over** whatever the target holds under the same uuid — *a uuid
//! is an identity*. True of the identity, false of the content: the corpus
//! holds one grid under one uuid in two packages, 12.5 m cells in one and a
//! 19 629 173 km axis in the other. The
//! import asks instead, and says what each answer entails. `copy_into` keeps its
//! contract; the Python bridge relies on it.
//!
//! # A quote, then the writing — and the bytes are a second half
//!
//! `plan` reads both packages and writes nothing; `apply_plan` carries the
//! quote out **in memory** — objects put, pointers rewritten, proxies declared —
//! and hands back the dataset copies and file carries it could not do itself
//! without a writing backend. `copy_datasets` and `carry_files` do them on a
//! disk; a browser does them through h5wasm, which is the only HDF5 it has.
//! Objects first, bytes second, and every failure named by its path.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use energyml_core::path;
use energyml_core::registry::DynObject;
use energyml_core::value::{FieldValue, OwnedValue, Reflect, Scalar};

use crate::array::ArrayLocation;
use crate::epc::EpcStore;
use crate::error::{Error, Result};
use crate::graph;
use crate::resource::ObjectSummary;

// ---------------------------------------------------------------------------
// What the person asks
// ---------------------------------------------------------------------------

/// Which of the two versions of one uuid the package ends up holding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keep {
    /// The object the open package already holds. Nothing is written.
    Current,
    /// The imported object, written over the current one.
    Imported,
}

/// What becomes of the numbers the imported objects point at.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Datasets {
    /// Not decided yet. Enough when nothing written names an array; a
    /// question otherwise, and [`Plan::is_allowed`] waits for its answer.
    #[default]
    Undecided,
    /// The imported data file joins the package as a file of its own.
    ///
    /// `names` maps a file as the imported package names it to the name it
    /// takes here, overriding the proposal of the plan — which keeps the name
    /// unless the open package already uses it.
    Separate {
        /// Imported file name → name here.
        names: BTreeMap<String, String>,
    },
    /// Every dataset the imported objects name is copied into a data file of
    /// the open package — the one it names, when it names only one.
    Merge {
        /// The file to copy into. Required when the package names none, or
        /// several.
        into: Option<String>,
    },
}

/// Everything a person decides about an import.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Request {
    /// URIs in the imported package. **Empty means everything.** Closed here: what they need comes along, and the plan says what did.
    pub wanted: Vec<String>,
    /// One decision for every conflict that has none of its own.
    pub all: Option<Keep>,
    /// Decisions by uuid (any case). Win over [`Request::all`].
    pub each: BTreeMap<String, Keep>,
    /// What becomes of the numbers.
    pub datasets: Datasets,
}

impl Request {
    fn decision_for(&self, uuid: &str) -> Option<Keep> {
        let key = uuid.to_ascii_lowercase();
        self.each
            .iter()
            .find(|(k, _)| k.to_ascii_lowercase() == key)
            .map(|(_, keep)| *keep)
            .or(self.all)
    }
}

// ---------------------------------------------------------------------------
// What the plan answers
// ---------------------------------------------------------------------------

/// What happens to one object of the imported set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fate {
    /// The open package does not hold its uuid: it is written.
    New,
    /// Held here, the same structure: nothing to do.
    Identical,
    /// Held here, different, and the imported version was chosen.
    Replaces,
    /// Held here, and the current version stays — by choice, or because it is
    /// a proxy whose fate follows the datasets.
    KeptCurrent,
    /// Held here, different, and nobody has said which to keep.
    Undecided,
    /// It cannot come in; the plan's refusals say why.
    Refused,
}

/// One object of the imported set, and what happens to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Incoming {
    /// Its URI in the imported package.
    pub uri: String,
    /// Its uuid.
    pub uuid: String,
    /// `resqml22.TriangulatedSetRepresentation`.
    pub qualified_type: String,
    /// `Citation.Title`, when it has one.
    pub title: Option<String>,
    /// Whether it was asked for, or came with the closure.
    pub asked: bool,
    /// What happens to it.
    pub fate: Fate,
}

/// How two objects under one uuid differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictKind {
    /// Same qualified type, different content: either may be kept.
    Differs,
    /// The same concept in two releases: only the current one may be kept.
    Release,
    /// Two concepts under one uuid: neither answer is honest, so refused.
    Concept,
}

/// One uuid both packages hold, differently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    /// The uuid.
    pub uuid: String,
    /// Its URI in the open package.
    pub here: String,
    /// Its URI in the imported package.
    pub there: String,
    /// The two titles, which are often what a person recognises.
    pub title_here: Option<String>,
    /// See [`Conflict::title_here`].
    pub title_there: Option<String>,
    /// Which of the three.
    pub kind: ConflictKind,
    /// Where the two differ, as dotted paths — the first [`DIFFERENCES`].
    pub differences: Vec<String>,
    /// How many differences there are in all.
    pub total: usize,
    /// What the request decided, if it did.
    pub decision: Option<Keep>,
    /// Imported objects naming it: they point at the current version if it is
    /// kept.
    pub pointed_by_imported: Vec<String>,
    /// Current objects naming it: they point at the imported version if that
    /// one is taken.
    pub pointed_by_current: Vec<String>,
}

/// How many differing paths a conflict carries. The total is always given.
pub const DIFFERENCES: usize = 20;

/// One array pointer of a written object, and where it ends up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pointer {
    /// The object holding it, by its URI in the imported package.
    pub object: String,
    /// Where inside the object: `Geometry.Points.Coordinates`.
    pub field_path: String,
    /// The proxy it goes through, for the eml 2.0–2.2 forms.
    pub proxy: Option<String>,
    /// The file and dataset it names in the imported package.
    pub from_file: String,
    /// See [`Pointer::from_file`].
    pub from_path: String,
    /// The file and dataset it will name here.
    pub to_file: String,
    /// See [`Pointer::to_file`].
    pub to_path: String,
    /// The new value of the file field (`Uri`, eml 2.3), when it changes.
    pub new_uri: Option<String>,
    /// The new value of the path field, when the path was relocated.
    pub new_path: Option<String>,
}

/// One dataset to copy, for [`Datasets::Merge`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatasetCopy {
    /// The file and dataset in the imported package.
    pub from_file: String,
    /// See [`DatasetCopy::from_file`].
    pub from_path: String,
    /// The file and dataset here.
    pub to_file: String,
    /// See [`DatasetCopy::to_file`].
    pub to_path: String,
    /// Whether the path had to move because the one it had was taken.
    pub relocated: bool,
}

/// One data file to carry across, for [`Datasets::Separate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileCarry {
    /// The file as the imported objects name it.
    pub from: String,
    /// Where its bytes are: a name the imported store was given (a browser),
    /// or a path on its disk.
    pub held: String,
    /// The name it takes here.
    pub to: String,
}

/// What becomes of the numbers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DatasetPlan {
    /// The request's mode, unchanged.
    pub mode: Datasets,
    /// Whether nothing is left to decide: a mode was given, or nothing written
    /// names an array.
    pub decided: bool,
    /// The data files the open package names.
    pub files_here: Vec<String>,
    /// The data files the written objects name in the imported package.
    pub files_there: Vec<String>,
    /// For a merge: the file everything goes into, once known.
    pub target: Option<String>,
    /// For a separate import: each imported file and the name it would take,
    /// before the request's overrides — what the front end offers to edit.
    pub proposed: BTreeMap<String, String>,
    /// Every pointer of every written object.
    pub pointers: Vec<Pointer>,
    /// The copies a merge makes, one per dataset.
    pub copies: Vec<DatasetCopy>,
    /// The files a separate import carries.
    pub carries: Vec<FileCarry>,
    /// Proxies to declare: uuid → the file they stand for here.
    pub proxies: BTreeMap<String, String>,
    /// Proxy uuids both packages hold that written objects go through.
    pub shared_proxies: Vec<String>,
    /// Datasets of replaced objects that nothing will name any more, as
    /// `file:path`. Listed, never deleted.
    pub orphaned: Vec<String>,
    /// Pointers the imported package itself cannot resolve to a file, carried
    /// as they were by a separate import — the imported package's state, not
    /// the import's doing.
    pub inherited: Vec<String>,
}

/// What an import would do.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    /// The imported set — the request closed — in the imported package's order.
    pub objects: Vec<Incoming>,
    /// Asked for, and the imported package does not hold it.
    pub unknown: Vec<String>,
    /// Not asked for: what the closure brought.
    pub added: Vec<String>,
    /// Named by the imported set, not held by the imported package, **held
    /// here**: the import completes them.
    pub resolved_here: Vec<String>,
    /// Named by the imported set and held by neither package.
    pub missing: Vec<String>,
    /// Every uuid both hold differently.
    pub conflicts: Vec<Conflict>,
    /// Imported objects that come in **beside** an object the open package holds:
    /// the same uuid and type under another `objectVersion` (ADR-0010: importing
    /// the same uuid under another version takes a new slot).
    /// Not conflicts — nothing is replaced — and listed so that a person sees that
    /// the package will hold two versions of them. Their fate is [`Fate::New`].
    pub new_versions: Vec<String>,
    /// The numbers.
    pub datasets: DatasetPlan,
    /// Why this import cannot be carried out, in words a person can act on.
    pub refusals: Vec<String>,
}

impl Plan {
    /// Conflicts nobody has decided, plus one when the datasets need a mode.
    pub fn undecided(&self) -> usize {
        self.objects
            .iter()
            .filter(|one| one.fate == Fate::Undecided)
            .count()
            + usize::from(!self.datasets.decided)
    }

    /// How many objects would be written.
    pub fn to_write(&self) -> usize {
        self.objects
            .iter()
            .filter(|one| matches!(one.fate, Fate::New | Fate::Replaces))
            .count()
    }

    /// Whether [`apply_plan`] will carry it out.
    pub fn is_allowed(&self) -> bool {
        self.refusals.is_empty() && self.undecided() == 0
    }
}

// ---------------------------------------------------------------------------
// The plan
// ---------------------------------------------------------------------------

/// Work out what importing `request` from `from` into `into` would do.
///
/// Reads every object of both packages — the closure needs the imported graph,
/// the consequences need the current one — and probes the target data file for
/// the paths a merge would write. Writes nothing.
pub fn plan(from: &EpcStore, into: &EpcStore, request: &Request) -> Plan {
    let mut out = Plan::default();

    let (there, unreadable_there) = from.read_objects(None);
    let (here, unreadable_here) = into.read_objects(None);
    let graph_there = graph::build(there.iter().map(|(s, o)| (s, o.as_reflect())));
    let graph_here = graph::build(here.iter().map(|(s, o)| (s, o.as_reflect())));

    let object_there: HashMap<&str, (&ObjectSummary, &dyn DynObject)> = there
        .iter()
        .map(|(s, o)| (s.uri.as_str(), (s, o.as_ref())))
        .collect();
    // Every entry of the open package, by uuid: a uuid may be held several times
    // (two `objectVersion`s, two releases), and an imported object is compared
    // with the one that has its identity, not with whichever came last.
    let mut here_all: HashMap<String, Vec<HereEntry<'_>>> = HashMap::new();
    for (s, o) in &here {
        here_all.entry(s.uuid.to_ascii_lowercase()).or_default().push((
            s,
            o.as_ref(),
            version_of(o.as_reflect()),
        ));
    }
    let here_by_uuid: HashMap<String, (&ObjectSummary, &dyn DynObject)> = here_all
        .iter()
        .filter_map(|(uuid, held)| held.first().map(|(s, o, _)| (uuid.clone(), (*s, *o))))
        .collect();

    // 1 — the set, closed.
    let listed_there: Vec<ObjectSummary> = from.resources();
    let listed: HashSet<&str> = listed_there.iter().map(|s| s.uri.as_str()).collect();
    let seeds: Vec<String> = if request.wanted.is_empty() {
        there.iter().map(|(s, _)| s.uri.clone()).collect()
    } else {
        request.wanted.clone()
    };
    for uri in &request.wanted {
        if !listed.contains(uri.as_str()) {
            out.unknown.push(uri.clone());
            out.refusals.push(format!("{uri} is not in the imported package"));
        } else if !object_there.contains_key(uri.as_str()) {
            out.refusals.push(format!(
                "{uri} is in the imported package and cannot be read, so it cannot be imported"
            ));
        }
    }
    let closed = graph::closure(&graph_there, &seeds);
    let asked: HashSet<&str> = seeds.iter().map(String::as_str).collect();
    out.added = closed
        .needed
        .iter()
        .filter(|uri| !asked.contains(uri.as_str()))
        .cloned()
        .collect();

    // What the set names and the imported package does not hand over.
    let uuid_of_node: HashMap<&str, &str> = graph_there
        .nodes
        .iter()
        .map(|n| (n.uri.as_str(), n.uuid.as_str()))
        .collect();
    for uri in &closed.missing {
        let uuid = uuid_of_node.get(uri.as_str()).copied().unwrap_or_default();
        if from.find_by_uuid(uuid).is_some() {
            // Held and unreadable: the import would create a dangling reference
            // the imported package did not have.
            out.refusals.push(format!(
                "{uri} is needed by what is imported, and the imported package holds it but \
                 cannot read it{}",
                unreadable_there
                    .iter()
                    .find(|why| why.contains(uuid))
                    .map(|why| format!(" ({why})"))
                    .unwrap_or_default()
            ));
        } else if here_by_uuid.contains_key(&uuid.to_ascii_lowercase()) {
            out.resolved_here.push(uri.clone());
        } else {
            out.missing.push(uri.clone());
        }
    }

    // 2 — who points at what, on both sides, for the consequences.
    let uuid_of_here: HashMap<&str, &str> = graph_here
        .nodes
        .iter()
        .map(|n| (n.uri.as_str(), n.uuid.as_str()))
        .collect();
    let mut referrers_here: HashMap<String, Vec<String>> = HashMap::new();
    for edge in &graph_here.edges {
        if let Some(uuid) = uuid_of_here.get(edge.to.as_str()) {
            push_once(
                referrers_here.entry(uuid.to_ascii_lowercase()).or_default(),
                &edge.from,
            );
        }
    }
    let needed: HashSet<&str> = closed.needed.iter().map(String::as_str).collect();
    let mut referrers_there: HashMap<String, Vec<String>> = HashMap::new();
    for edge in &graph_there.edges {
        if !needed.contains(edge.from.as_str()) {
            continue;
        }
        if let Some(uuid) = uuid_of_node.get(edge.to.as_str()) {
            push_once(
                referrers_there.entry(uuid.to_ascii_lowercase()).or_default(),
                &edge.from,
            );
        }
    }

    // 3 — each object of the set, and the conflicts.
    let type_key = |qualified: &str| -> String {
        graph_there
            .nodes
            .iter()
            .chain(graph_here.nodes.iter())
            .find(|n| n.qualified_type == qualified)
            .map(|n| n.type_key.clone())
            .unwrap_or_else(|| qualified.to_string())
    };
    for uri in &closed.needed {
        let Some((summary, object)) = object_there.get(uri.as_str()).copied() else {
            continue;
        };
        let key = summary.uuid.to_ascii_lowercase();
        let title = energyml_core::identity::title_of_reflect(object.as_reflect()).map(str::to_string);
        let mut incoming = Incoming {
            uri: summary.uri.clone(),
            uuid: summary.uuid.clone(),
            qualified_type: summary.qualified_type.clone(),
            title: title.clone(),
            asked: asked.contains(uri.as_str()),
            fate: Fate::New,
        };
        if !here_by_uuid.contains_key(&key) && into.find_by_uuid(&summary.uuid).is_some() {
            // Held here and unreadable: nothing can say whether the import
            // would replace a different object — the rule `rename` applies to
            // a part it cannot read.
            out.refusals.push(format!(
                "{} is held by the open package and cannot be read there, so nothing can say \
                 whether importing {} would replace a different object{}",
                summary.uuid,
                summary.uri,
                unreadable_here
                    .iter()
                    .find(|why| why.contains(&summary.uuid))
                    .map(|why| format!(" ({why})"))
                    .unwrap_or_default()
            ));
            incoming.fate = Fate::Refused;
        } else if let Some((current, current_object)) = {
            // The entry of the open package this one is **the same object as**:
            // same uuid, same type, same `objectVersion` — an absent one is a
            // value like another. When the open package holds the uuid and type
            // under other versions only, nothing here is this object: it comes in
            // beside them, as a new version. When it holds the uuid under
            // another type only, the old rule stands (a release, or two concepts).
            let held = here_all.get(&key).map(Vec::as_slice).unwrap_or_default();
            let version_there = version_of(object.as_reflect());
            let same_type: Vec<_> = held
                .iter()
                .filter(|(s, _, _)| s.qualified_type == summary.qualified_type)
                .collect();
            match same_type
                .iter()
                .find(|(_, _, held_version)| *held_version == version_there)
            {
                Some((s, o, _)) => Some((*s, *o)),
                None if !same_type.is_empty() => {
                    out.new_versions.push(summary.uri.clone());
                    None
                }
                None => here_by_uuid.get(&key).copied(),
            }
        } {
            if is_proxy(&summary.qualified_type) && is_proxy(&current.qualified_type) {
                // A proxy's content is next to nothing and its identity is a
                // file: its fate follows the datasets, never a question.
                incoming.fate = Fate::KeptCurrent;
            } else {
                let kind = if current.qualified_type == summary.qualified_type {
                    ConflictKind::Differs
                } else if type_key(&current.qualified_type) == type_key(&summary.qualified_type) {
                    ConflictKind::Release
                } else {
                    ConflictKind::Concept
                };
                let (differences, total) = if kind == ConflictKind::Differs {
                    differences(current_object.as_reflect(), object.as_reflect(), DIFFERENCES)
                } else {
                    (
                        vec![format!("{} → {}", current.qualified_type, summary.qualified_type)],
                        1,
                    )
                };
                if total == 0 {
                    incoming.fate = Fate::Identical;
                } else {
                    let decision = request.decision_for(&summary.uuid);
                    incoming.fate = match (kind, decision) {
                        (ConflictKind::Concept, _) => {
                            out.refusals.push(format!(
                                "{} is a {} here and a {} in the imported package: two concepts \
                                 under one uuid, and neither can stand for the other",
                                summary.uuid, current.qualified_type, summary.qualified_type
                            ));
                            Fate::Refused
                        }
                        (ConflictKind::Release, Some(Keep::Imported)) => {
                            out.refusals.push(format!(
                                "{} is a {} here and a {} in the imported package: a release is \
                                 not replaced in place (a translation mints a new uuid), so only \
                                 the current one can be kept",
                                summary.uuid, current.qualified_type, summary.qualified_type
                            ));
                            Fate::Refused
                        }
                        (_, Some(Keep::Current)) => Fate::KeptCurrent,
                        (_, Some(Keep::Imported)) => Fate::Replaces,
                        (_, None) => Fate::Undecided,
                    };
                    out.conflicts.push(Conflict {
                        uuid: summary.uuid.clone(),
                        here: current.uri.clone(),
                        there: summary.uri.clone(),
                        title_here: energyml_core::identity::title_of_reflect(current_object.as_reflect())
                            .map(str::to_string),
                        title_there: title,
                        kind,
                        differences,
                        total,
                        decision,
                        pointed_by_imported: referrers_there.get(&key).cloned().unwrap_or_default(),
                        pointed_by_current: referrers_here.get(&key).cloned().unwrap_or_default(),
                    });
                }
            }
        }
        out.objects.push(incoming);
    }

    // 4 — the numbers.
    out.datasets = plan_datasets(
        from,
        into,
        request,
        &out.objects,
        &object_there,
        &here,
        &mut out.refusals,
    );

    if out.refusals.is_empty() && out.undecided() == 0 && out.to_write() == 0 {
        out.refusals.push(
            "nothing to import: every object of the imported set is already here, identical \
             or kept"
                .to_string(),
        );
    }
    out
}

/// An object of the open package, with the `objectVersion` it states.
type HereEntry<'a> = (&'a ObjectSummary, &'a dyn DynObject, Option<String>);

fn push_once(list: &mut Vec<String>, value: &str) {
    if !list.iter().any(|one| one == value) {
        list.push(value.to_string());
    }
}

/// The `objectVersion` an object states, as an identity: blank is none.
fn version_of(object: &dyn Reflect) -> Option<String> {
    energyml_core::identity::object_version_of_reflect(object)
        .map(str::trim)
        .filter(|version| !version.is_empty())
        .map(str::to_string)
}

fn is_proxy(qualified_type: &str) -> bool {
    qualified_type.contains("EpcExternalPartReference")
}

// ---------------------------------------------------------------------------
// The numbers
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn plan_datasets(
    from: &EpcStore,
    into: &EpcStore,
    request: &Request,
    objects: &[Incoming],
    object_there: &HashMap<&str, (&ObjectSummary, &dyn DynObject)>,
    here: &[(ObjectSummary, Box<dyn DynObject>)],
    refusals: &mut Vec<String>,
) -> DatasetPlan {
    let mut out = DatasetPlan {
        mode: request.datasets.clone(),
        ..Default::default()
    };

    // What the open package names, and which of its datasets stay named.
    let replaced: HashSet<String> = objects
        .iter()
        .filter(|one| one.fate == Fate::Replaces)
        .map(|one| one.uuid.to_ascii_lowercase())
        .collect();
    let mut files_here: Vec<String> = Vec::new();
    let mut taken: HashSet<(String, String)> = HashSet::new();
    let mut kept_paths: HashSet<(String, String)> = HashSet::new();
    let mut replaced_paths: Vec<(String, String)> = Vec::new();
    for (summary, object) in here {
        for location in into.arrays_of(object.as_reflect()) {
            if location.file.is_empty() {
                continue;
            }
            let file = bare(&location.file).to_string();
            push_once(&mut files_here, &file);
            let key = (file, location.path_in_file.clone());
            taken.insert(key.clone());
            if replaced.contains(&summary.uuid.to_ascii_lowercase()) {
                replaced_paths.push(key);
            } else {
                kept_paths.insert(key);
            }
        }
    }
    for file in into.external_file_map().into_values() {
        push_once(&mut files_here, bare(&file));
    }
    out.files_here = files_here;
    for (file, path) in replaced_paths {
        if !kept_paths.contains(&(file.clone(), path.clone())) {
            push_once(&mut out.orphaned, &format!("{file}:{path}"));
        }
    }

    // The pointers of what will be written.
    struct Raw {
        object: String,
        location: ArrayLocation,
    }
    let mut raw: Vec<Raw> = Vec::new();
    for one in objects
        .iter()
        .filter(|one| matches!(one.fate, Fate::New | Fate::Replaces))
    {
        let Some((_, object)) = object_there.get(one.uri.as_str()).copied() else {
            continue;
        };
        for location in from.arrays_of(object.as_reflect()) {
            raw.push(Raw {
                object: one.uri.clone(),
                location,
            });
        }
    }
    for pointer in &raw {
        if !pointer.location.file.is_empty() {
            push_once(&mut out.files_there, &pointer.location.file);
        }
    }
    if raw.is_empty() {
        out.decided = true;
        return out;
    }

    let held_here = |uuid: &str| into.find_by_uuid(uuid).is_some();
    for pointer in &raw {
        if let Some(proxy) = &pointer.location.proxy_uuid
            && held_here(proxy)
        {
            push_once(&mut out.shared_proxies, &proxy.to_ascii_lowercase());
        }
    }

    match &request.datasets {
        Datasets::Undecided => {
            out.decided = false;
        }
        Datasets::Separate { names } => {
            out.decided = true;
            separate(
                from,
                into,
                names,
                &raw.iter()
                    .map(|r| (r.object.as_str(), &r.location))
                    .collect::<Vec<_>>(),
                &mut out,
                refusals,
            );
        }
        Datasets::Merge { into: chosen } => {
            out.decided = true;
            merge(
                from,
                into,
                chosen.as_deref(),
                &raw.iter()
                    .map(|r| (r.object.as_str(), &r.location))
                    .collect::<Vec<_>>(),
                &taken,
                &mut out,
                refusals,
            );
        }
    }
    out
}

/// The imported files become files of this package.
fn separate(
    from: &EpcStore,
    into: &EpcStore,
    names: &BTreeMap<String, String>,
    raw: &[(&str, &ArrayLocation)],
    out: &mut DatasetPlan,
    refusals: &mut Vec<String>,
) {
    for proxy in &out.shared_proxies {
        refusals.push(format!(
            "the proxy {proxy} is in both packages and imported objects go through it: kept \
             apart, one proxy would stand for two files — merge the datasets instead"
        ));
    }

    let used_here = |name: &str| -> bool {
        out.files_here.iter().any(|one| bare(one) == bare(name))
            || into
                .external_files_ref()
                .present()
                .iter()
                .any(|one| bare(one) == bare(name))
            || (!into.external_files_ref().is_virtual()
                && into
                    .external_files_ref()
                    .destination_for(bare(name))
                    .is_ok_and(|path| path.exists()))
    };

    // One name per imported file: the request's, or the proposal.
    let mut chosen: BTreeMap<String, String> = BTreeMap::new();
    let mut claimed: BTreeSet<String> = BTreeSet::new();
    for file in out.files_there.clone() {
        let proposal = if used_here(&file) {
            free_name(bare(&file), |candidate| {
                used_here(candidate) || claimed.contains(candidate)
            })
        } else {
            bare(&file).to_string()
        };
        out.proposed.insert(file.clone(), proposal.clone());
        let name = names
            .iter()
            .find(|(k, _)| k.as_str() == file || bare(k) == bare(&file))
            .map(|(_, v)| v.clone())
            .unwrap_or(proposal);
        if name.trim().is_empty() {
            refusals.push(format!("the name given to `{file}` is empty"));
        } else if name.contains('/') || name.contains('\\') {
            refusals.push(format!(
                "`{name}` is a path; EPC §3.2.3 names an external file by a bare name"
            ));
        } else if used_here(&name) {
            refusals.push(format!(
                "`{name}` is already a data file of the open package: two files under one name \
                 would become one pointer"
            ));
        } else if !claimed.insert(name.clone()) {
            refusals.push(format!("two imported files would both be called `{name}`"));
        }

        // Where the bytes are.
        let files = from.external_files_ref();
        let held = if files.is_virtual() {
            let present = files.present();
            present
                .iter()
                .find(|one| bare(one) == bare(&file))
                .cloned()
                .or_else(|| (present.len() == 1).then(|| present[0].clone()))
        } else {
            files.resolve(&file).ok().map(|path| path.display().to_string())
        };
        match held {
            Some(held) => out.carries.push(FileCarry {
                from: file.clone(),
                held,
                to: name.clone(),
            }),
            None => refusals.push(format!(
                "`{file}` is named by the imported objects and is not beside the imported package"
            )),
        }
        chosen.insert(file, name);
    }

    for (object, location) in raw {
        if location.file.is_empty() {
            push_once(
                &mut out.inherited,
                &format!(
                    "{object}: {} names no file the imported package can resolve",
                    location.field_path
                ),
            );
            continue;
        }
        let to_file = chosen
            .get(&location.file)
            .cloned()
            .unwrap_or_else(|| location.file.clone());
        let new_uri = (location.proxy_uuid.is_none() && bare(&location.file) != to_file)
            .then(|| renamed(&location.file, &to_file));
        if let Some(proxy) = &location.proxy_uuid
            && !out.shared_proxies.contains(&proxy.to_ascii_lowercase())
        {
            out.proxies.insert(proxy.to_ascii_lowercase(), to_file.clone());
        }
        out.pointers.push(Pointer {
            object: object.to_string(),
            field_path: location.field_path.clone(),
            proxy: location.proxy_uuid.clone(),
            from_file: location.file.clone(),
            from_path: location.path_in_file.clone(),
            to_file,
            to_path: location.path_in_file.clone(),
            new_uri,
            new_path: None,
        });
    }
}

/// Every dataset is copied into one file of this package.
fn merge(
    from: &EpcStore,
    into: &EpcStore,
    chosen: Option<&str>,
    raw: &[(&str, &ArrayLocation)],
    taken: &HashSet<(String, String)>,
    out: &mut DatasetPlan,
    refusals: &mut Vec<String>,
) {
    let target = match chosen.map(str::trim).filter(|one| !one.is_empty()) {
        Some(name) if name.contains('/') || name.contains('\\') => {
            refusals.push(format!(
                "`{name}` is a path; EPC §3.2.3 names an external file by a bare name"
            ));
            return;
        }
        Some(name) => name.to_string(),
        None => match out.files_here.as_slice() {
            [only] => only.clone(),
            [] => {
                refusals.push(
                    "the open package names no data file: name one to merge the datasets into".to_string(),
                );
                return;
            }
            several => {
                refusals.push(format!(
                    "the open package names {} data files ({}): say which one to merge into",
                    several.len(),
                    several.join(", ")
                ));
                return;
            }
        },
    };
    out.target = Some(target.clone());

    // Where each pointer's numbers go: the target, except through a proxy both
    // packages hold, which keeps the file the open package gives it.
    let file_for = |location: &ArrayLocation| -> String {
        match &location.proxy_uuid {
            Some(proxy) if out.shared_proxies.contains(&proxy.to_ascii_lowercase()) => into
                .external_file_of(proxy)
                .map(|file| bare(&file).to_string())
                .unwrap_or_else(|| target.clone()),
            _ => target.clone(),
        }
    };

    // One copy per dataset, and the paths already taken in its target.
    let mut copies: Vec<DatasetCopy> = Vec::new();
    let mut index: HashMap<(String, String), usize> = HashMap::new();
    for (object, location) in raw {
        if index.contains_key(&(location.file.clone(), location.path_in_file.clone())) {
            continue;
        }
        let probe = ArrayLocation {
            file: location.file.clone(),
            path_in_file: location.path_in_file.clone(),
            mime: location.mime.clone(),
            ..Default::default()
        };
        if location.file.is_empty() {
            refusals.push(format!(
                "{object}: {} names no file the imported package can resolve, so there is \
                 nothing to copy",
                location.field_path
            ));
            continue;
        }
        if let Err(why) = from.array_metadata(&probe) {
            refusals.push(format!(
                "{object}: the imported package cannot read `{}:{}` ({why}), and merging would \
                 point at numbers never copied",
                location.file, location.path_in_file
            ));
            continue;
        }
        index.insert(
            (location.file.clone(), location.path_in_file.clone()),
            copies.len(),
        );
        copies.push(DatasetCopy {
            from_file: location.file.clone(),
            from_path: location.path_in_file.clone(),
            to_file: file_for(location),
            to_path: location.path_in_file.clone(),
            relocated: false,
        });
    }

    // A taken path moves under `/import-<n>`, never over what is there.
    // Asked of the target file alone (`holds_array`): a search would answer
    // from a neighbouring file and leave a diagnostic in the open package.
    let probed_taken = |file: &str, path_in_file: &str| -> bool {
        taken.contains(&(file.to_string(), path_in_file.to_string()))
            || into.holds_array(&ArrayLocation {
                file: file.to_string(),
                path_in_file: path_in_file.to_string(),
                ..Default::default()
            })
    };
    let mut assigned: HashSet<(String, String)> = HashSet::new();
    let mut colliding: Vec<usize> = Vec::new();
    for (at, copy) in copies.iter().enumerate() {
        let key = (copy.to_file.clone(), copy.to_path.clone());
        if probed_taken(&copy.to_file, &copy.to_path) || !assigned.insert(key) {
            colliding.push(at);
        }
    }
    if !colliding.is_empty() {
        let mut n = 1usize;
        let relocated = |n: usize, copy: &DatasetCopy| prefixed(n, &copy.to_path);
        while colliding.iter().any(|at| {
            let copy = &copies[*at];
            let moved = relocated(n, copy);
            probed_taken(&copy.to_file, &moved) || assigned.contains(&(copy.to_file.clone(), moved))
        }) {
            n += 1;
        }
        for at in colliding {
            let moved = relocated(n, &copies[at]);
            copies[at].to_path = moved;
            copies[at].relocated = true;
        }
    }

    for (object, location) in raw {
        let Some(at) = index.get(&(location.file.clone(), location.path_in_file.clone())) else {
            continue;
        };
        let copy = &copies[*at];
        let new_uri =
            (location.proxy_uuid.is_none() && location.file != copy.to_file).then(|| copy.to_file.clone());
        if let Some(proxy) = &location.proxy_uuid
            && !out.shared_proxies.contains(&proxy.to_ascii_lowercase())
        {
            out.proxies
                .insert(proxy.to_ascii_lowercase(), copy.to_file.clone());
        }
        out.pointers.push(Pointer {
            object: object.to_string(),
            field_path: location.field_path.clone(),
            proxy: location.proxy_uuid.clone(),
            from_file: location.file.clone(),
            from_path: location.path_in_file.clone(),
            to_file: copy.to_file.clone(),
            to_path: copy.to_path.clone(),
            new_uri,
            new_path: copy.relocated.then(|| copy.to_path.clone()),
        });
    }
    out.copies = copies;
}

/// `/import-<n>` in front of a path — a prefix, never a reading of the path.
fn prefixed(n: usize, path_in_file: &str) -> String {
    if path_in_file.starts_with('/') {
        format!("/import-{n}{path_in_file}")
    } else {
        format!("/import-{n}/{path_in_file}")
    }
}

/// `model-import.h5`, then `model-import-2.h5`, … — the first nobody uses.
fn free_name(name: &str, used: impl Fn(&str) -> bool) -> String {
    let (stem, extension) = match name.rfind('.') {
        Some(at) if at > 0 => (&name[..at], &name[at..]),
        _ => (name, ""),
    };
    let mut n = 1usize;
    loop {
        let candidate = if n == 1 {
            format!("{stem}-import{extension}")
        } else {
            format!("{stem}-import-{n}{extension}")
        };
        if !used(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// The last segment of a name or a URI.
fn bare(name: &str) -> &str {
    name.rsplit(['/', '\\']).next().unwrap_or(name)
}

/// The same location with its file name replaced, keeping what led to it —
/// the rule of `rename.rs`: `./data/model.h5` renamed is `./data/new.h5`.
fn renamed(declared: &str, to: &str) -> String {
    match declared.rfind(['/', '\\']) {
        Some(at) => format!("{}{to}", &declared[..=at]),
        None => to.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Comparing two objects
// ---------------------------------------------------------------------------

/// Where two objects differ, as dotted paths — the first `limit`, and the total.
///
/// **Structure, not bytes**: both objects are walked field by field
/// through [`Reflect`], so a namespace prefix, an attribute order or `2.0`
/// written `2` is no difference at all, and a value that changed is one. `NaN`
/// equals `NaN`: the corpus uses it as a sentinel, and `==` would call two
/// identical objects different. Nothing is ignored — a `Citation.LastUpdate` is
/// a difference, named so the person sees whether it is the only one.
pub fn differences(a: &dyn Reflect, b: &dyn Reflect, limit: usize) -> (Vec<String>, usize) {
    let mut found = Vec::new();
    let mut total = 0usize;
    walk_objects(a, b, String::new(), limit, &mut found, &mut total, 0);
    (found, total)
}

fn note(path: String, limit: usize, found: &mut Vec<String>, total: &mut usize) {
    *total += 1;
    if found.len() < limit {
        found.push(if path.is_empty() {
            "(the object itself)".to_string()
        } else {
            path
        });
    }
}

fn walk_objects(
    a: &dyn Reflect,
    b: &dyn Reflect,
    path: String,
    limit: usize,
    found: &mut Vec<String>,
    total: &mut usize,
    depth: u32,
) {
    // Objects contain each other only by value; the bound is a seatbelt.
    if depth > 64 {
        return;
    }
    let (ta, tb) = (a.type_info(), b.type_info());
    if ta.name != tb.name || ta.schema_version != tb.schema_version {
        note(path, limit, found, total);
        return;
    }
    for field in ta.fields {
        let at = if path.is_empty() {
            field.xml_name.to_string()
        } else {
            format!("{path}.{}", field.xml_name)
        };
        walk_values(
            a.field(field.name),
            b.field(field.name),
            at,
            limit,
            found,
            total,
            depth,
        );
    }
}

fn walk_values(
    a: Option<FieldValue<'_>>,
    b: Option<FieldValue<'_>>,
    path: String,
    limit: usize,
    found: &mut Vec<String>,
    total: &mut usize,
    depth: u32,
) {
    let empty = |v: &Option<FieldValue<'_>>| v.as_ref().is_none_or(FieldValue::is_empty);
    if empty(&a) && empty(&b) {
        return;
    }
    match (a, b) {
        (Some(FieldValue::Scalar(x)), Some(FieldValue::Scalar(y))) => {
            if !same_scalar(x, y) {
                note(path, limit, found, total);
            }
        }
        (Some(FieldValue::Object(x)), Some(FieldValue::Object(y))) => {
            walk_objects(x, y, path, limit, found, total, depth + 1);
        }
        (Some(FieldValue::List(xs)), Some(FieldValue::List(ys))) => {
            if xs.len() != ys.len() {
                note(
                    format!("{path} ({} → {} entries)", xs.len(), ys.len()),
                    limit,
                    found,
                    total,
                );
                return;
            }
            for (i, (x, y)) in xs.into_iter().zip(ys).enumerate() {
                walk_values(
                    Some(x),
                    Some(y),
                    format!("{path}[{i}]"),
                    limit,
                    found,
                    total,
                    depth,
                );
            }
        }
        _ => note(path, limit, found, total),
    }
}

fn same_scalar(a: Scalar<'_>, b: Scalar<'_>) -> bool {
    match (a, b) {
        (Scalar::Double(x), Scalar::Double(y)) => x == y || (x.is_nan() && y.is_nan()),
        (x, y) => x == y,
    }
}

// ---------------------------------------------------------------------------
// Carrying it out
// ---------------------------------------------------------------------------

/// What [`apply_plan`] did to the open package, and what is left for the bytes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Applied {
    /// URIs written, new ones and replacements alike.
    pub written: Vec<String>,
    /// Of those, the ones that replaced an object of the open package.
    pub replaced: Vec<String>,
    /// Of those, the ones that came in as **another version** of an object the open
    /// package already held — same uuid and type, another `objectVersion` — and
    /// now stand beside it.
    pub new_versions: Vec<String>,
    /// Pointer fields rewritten (a file name, a relocated path).
    pub fields: usize,
    /// Proxies declared.
    pub proxies: usize,
    /// The datasets to copy — [`copy_datasets`] on a disk, h5wasm in a front end.
    pub copies: Vec<DatasetCopy>,
    /// The files to carry — [`carry_files`] on a disk, h5wasm in a front end.
    pub carries: Vec<FileCarry>,
}

/// Plan, check the plan against the one somebody saw, and carry it out.
///
/// `shown` is the plan the person accepted. When the packages changed since —
/// an edit, a dataset appearing at a target path — the recomputed plan differs
/// and this **refuses**: carrying out a stale quote is carrying out something
/// nobody accepted.
pub fn apply(
    from: &EpcStore,
    into: &mut EpcStore,
    request: &Request,
    shown: Option<&Plan>,
) -> Result<Applied> {
    let quote = plan(from, into, request);
    if shown.is_some_and(|shown| *shown != quote) {
        return Err(Error::Package(
            "the packages changed since this import was planned: plan it again".to_string(),
        ));
    }
    apply_plan(from, into, &quote)
}

/// Carry out a plan, in memory: objects put, pointers rewritten, proxies
/// declared. The bytes are what it hands back.
pub fn apply_plan(from: &EpcStore, into: &mut EpcStore, quote: &Plan) -> Result<Applied> {
    if !quote.is_allowed() {
        let mut why = quote.refusals.clone();
        let undecided = quote
            .objects
            .iter()
            .filter(|one| one.fate == Fate::Undecided)
            .count();
        if undecided > 0 {
            why.push(format!("{undecided} conflict(s) not decided"));
        }
        if !quote.datasets.decided {
            why.push("what becomes of the datasets is not decided".to_string());
        }
        return Err(Error::Package(format!(
            "this import was refused: {}",
            why.join("; ")
        )));
    }

    let mut done = Applied::default();
    let mut by_object: HashMap<&str, Vec<&Pointer>> = HashMap::new();
    for pointer in &quote.datasets.pointers {
        by_object
            .entry(pointer.object.as_str())
            .or_default()
            .push(pointer);
    }

    for one in quote
        .objects
        .iter()
        .filter(|one| matches!(one.fate, Fate::New | Fate::Replaces))
    {
        let mut object = from.get_object(&one.uri)?;
        for pointer in by_object.get(one.uri.as_str()).into_iter().flatten() {
            done.fields += rewrite(object.as_reflect_mut(), pointer)?;
        }
        // An object that comes in beside one of the same uuid and type — another
        // `objectVersion` — goes through the gate that adds a version: an ordinary
        // put would replace the one that is there.
        if quote.new_versions.contains(&one.uri) {
            into.put_as_new_version(object)?;
            done.new_versions.push(one.uri.clone());
        } else {
            into.put(object)?;
        }
        done.written.push(one.uri.clone());
        if one.fate == Fate::Replaces {
            done.replaced.push(one.uri.clone());
        }
    }
    // Declared **after** the proxies are in, and before any save — the save is
    // what writes the `.rels` that name them.
    for (proxy, file) in &quote.datasets.proxies {
        into.register_external_file(proxy, file.clone());
        done.proxies += 1;
    }
    // A browser store resolves by the names it was told about: the files this
    // import brings, or merges into, are among them from now on.
    if into.external_files_ref().is_virtual() {
        for carry in &quote.datasets.carries {
            into.external_files_mut().declare_present(carry.to.clone());
        }
        if let Some(target) = &quote.datasets.target {
            into.external_files_mut().declare_present(target.clone());
        }
    }
    done.copies = quote.datasets.copies.clone();
    done.carries = quote.datasets.carries.clone();
    Ok(done)
}

/// Rewrite one pointer inside an object; how many fields changed.
fn rewrite(object: &mut dyn Reflect, pointer: &Pointer) -> Result<usize> {
    if pointer.new_uri.is_none() && pointer.new_path.is_none() {
        return Ok(0);
    }
    let part = path::object_mut(object, &pointer.field_path)?;
    // One table of which field holds what, shared with the locator — a WITSML
    // channel data in HDF5 names its file in `file_uri` and its dataset in
    // `data` (`crate::array`).
    let path_field = crate::array::path_field(part.type_info().name);
    let file_field = crate::array::file_field(part.type_info().name);
    let mut fields = 0;
    if let Some(uri) = &pointer.new_uri {
        part.set_field(file_field, OwnedValue::Str(uri.clone()))?;
        fields += 1;
    }
    if let Some(moved) = &pointer.new_path {
        part.set_field(path_field, OwnedValue::Str(moved.clone()))?;
        fields += 1;
    }
    Ok(fields)
}

/// Copy datasets from one package's files into another's, on a disk.
///
/// Read whole, written whole: the type the file holds is the type written. One
/// answer per copy, in order — `None` when it went, the reason when it did not.
/// A copy that fails does not stop the others: one refusal is not the batch's.
///
/// A store whose files live elsewhere — a browser's, known by names only —
/// has nowhere to write, and each copy says so rather than *file not found*:
/// there, the front end copies through h5wasm.
pub fn copy_datasets(from: &EpcStore, into: &EpcStore, copies: &[DatasetCopy]) -> Vec<Option<String>> {
    copies
        .iter()
        .map(|copy| {
            if into.external_files_ref().is_virtual() {
                return Some(format!(
                    "{}:{} → {}:{}: the open package has no disk; whoever holds its files copies \
                     the dataset",
                    copy.from_file, copy.from_path, copy.to_file, copy.to_path
                ));
            }
            let source = ArrayLocation {
                file: copy.from_file.clone(),
                path_in_file: copy.from_path.clone(),
                ..Default::default()
            };
            from.read_array(&source)
                .and_then(|data| into.write_array_to(&copy.to_file, &copy.to_path, &data))
                .err()
                .map(|why| {
                    format!(
                        "{}:{} → {}:{}: {why}",
                        copy.from_file, copy.from_path, copy.to_file, copy.to_path
                    )
                })
        })
        .collect()
}

/// What [`carry_files`] did with one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Carried {
    /// The open package has no disk: whoever holds its files copies the bytes.
    NotOnDisk,
    /// Copied, and where to.
    Copied(std::path::PathBuf),
}

/// Copy the imported data files beside the open package, on a disk.
///
/// Never over an existing file: the plan chose a name nobody uses, and a file
/// appearing there since is refused rather than replaced.
pub fn carry_files(into: &EpcStore, carries: &[FileCarry]) -> Result<Vec<Carried>> {
    let mut out = Vec::new();
    for carry in carries {
        if into.external_files_ref().is_virtual() {
            out.push(Carried::NotOnDisk);
            continue;
        }
        let destination = into.external_files_ref().destination_for(&carry.to)?;
        if destination.exists() {
            return Err(Error::Package(format!(
                "{} exists already; the import will not write over it",
                destination.display()
            )));
        }
        std::fs::copy(&carry.held, &destination).map_err(|why| {
            Error::Package(format!(
                "copying {} to {}: {why}",
                carry.held,
                destination.display()
            ))
        })?;
        // The journal says a data file arrived, at the place it says what became
        // of the objects: a cache that read through this name reads another file
        // now.
        into.data_file_written(&carry.to);
        out.push(Carried::Copied(destination));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_free_name_keeps_the_extension() {
        assert_eq!(free_name("model.h5", |_| false), "model-import.h5");
        assert_eq!(
            free_name("model.h5", |n| n == "model-import.h5"),
            "model-import-2.h5"
        );
        assert_eq!(free_name("model", |_| false), "model-import");
    }

    #[test]
    fn a_relocation_is_a_prefix() {
        assert_eq!(prefixed(1, "/RESQML/abc/points"), "/import-1/RESQML/abc/points");
        assert_eq!(prefixed(2, "points"), "/import-2/points");
    }

    #[test]
    fn a_rename_keeps_what_led_to_the_file() {
        assert_eq!(renamed("./data/model.h5", "new.h5"), "./data/new.h5");
        assert_eq!(renamed("model.h5", "new.h5"), "new.h5");
    }

    #[test]
    fn nan_equals_nan_and_nothing_else_does() {
        assert!(same_scalar(Scalar::Double(f64::NAN), Scalar::Double(f64::NAN)));
        assert!(!same_scalar(Scalar::Double(1.0), Scalar::Double(f64::NAN)));
        assert!(same_scalar(Scalar::Str("a"), Scalar::Str("a")));
        assert!(!same_scalar(Scalar::Int(1), Scalar::Int(2)));
    }
}
