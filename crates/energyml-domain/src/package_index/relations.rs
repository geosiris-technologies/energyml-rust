//! The relations the inheritance walks read, **laid in the same pass as the
//! references** (ADR-0010).
//!
//! The link optimisation of the inheritance walk also applies to attachments and
//! similar relations, and the relations are built in addition at
//! graph-construction time, rather than by traversing the whole graph
//! afterwards.
//!
//! The index reads every object **once**, for its references. While it holds the
//! object it also writes down, from the references it just took and a field or
//! two it can see, what the walks of [`crate::inherit`] used to rediscover on
//! every call by filtering the whole of an object's edges:
//!
//! | table | filled by | read by |
//! |---|---|---|
//! | **parents** — the three FIRP fields of a node | every node | `Walker::parents_of`, the models' parents |
//! | **associations** — the collections an association set names, with the §8.3 repeat decided, and the members of each | a `CollectionsToDataobjectsAssociationSet` | `Walker::associations_of` |
//! | **held** — the other direction: the memberships of an object | the same | `Walker::links_of` |
//! | **projects** — the collections whose `Kind` is `project` | a `DataobjectCollection` (its `Kind` field, no second read) | `Walker::is_project` |
//! | **modelled** — who is referenced by a `Model` or an `EarthModelInterpretation` | those two types | `Walker::model_candidates` |
//! | **decorated** — who an entry of a `GraphicalInformationSet` targets, and what kind of entry | a `GraphicalInformationSet` | *who carries a graphical information*, [`PackageIndex::graphical_entries_of`] |
//! | **attached** — which `Attachment` objects name an object (`ObjectReference`) | an `eml23.Attachment` | `attachments::attachments_of` |
//!
//! **Edges and tables, never resolutions.** Nothing here walks: a table is a
//! row per reference that already existed, so the build costs one pass over the
//! edges it took anyway — no object is read for it, and the only two things
//! taken from an object are a `Kind` and the type names of the entries of a
//! graphical set. A resolution (a walk, a colour, a listing) is still computed on
//! demand and kept by its own cache, and **dropped by the same disturbance** the
//! references report: [`PackageIndex::update_with`] re-lays the rows of each
//! object it re-reads, at the place it re-lays its references.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use super::{IndexNode, Interner, NodeId, NodeState, PackageIndex, PathId};
use crate::inherit::{PARENT_FIELDS, Part, parse_association};

const ASSOCIATION_SET: &str = "CollectionsToDataobjectsAssociationSet";
const ATTACHMENT: &str = "eml23.Attachment";

/// What the index reads **from the object** while it holds it, beyond its
/// references: two facts, no second read.
#[derive(Default)]
pub(super) struct Facts {
    /// A `DataobjectCollection` whose `Kind` is `project`.
    pub project: bool,
    /// For a `GraphicalInformationSet`: the type name of each entry, in order
    /// (`DefaultGraphicalInformation`, `ColorInformation`…).
    pub entry_types: Vec<Arc<str>>,
}

#[derive(Clone, Debug)]
struct Row {
    index: u64,
    collection: NodeId,
    /// Decided when the row was laid: common TRG 2.3 §8.3 — the same collection
    /// named by an earlier association of the same set.
    repeated_of: Option<u64>,
    /// `(position in the association, member)`.
    members: Box<[(u64, NodeId)]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Held {
    holder: NodeId,
    association: u64,
    position: u64,
}

/// The tables. Private: the index answers, by uuid, through the accessors below.
#[derive(Default)]
pub(super) struct Relations {
    parents: HashMap<NodeId, Box<[(u8, NodeId)]>>,
    sets: HashMap<NodeId, Box<[Row]>>,
    held: HashMap<NodeId, Vec<Held>>,
    projects: HashSet<NodeId>,
    modelled: HashMap<NodeId, Vec<(NodeId, PathId)>>,
    /// target → `(set, entry)`.
    decorated: HashMap<NodeId, Vec<(NodeId, usize)>>,
    /// set → the type name of each entry.
    graphical_sets: BTreeMap<NodeId, Vec<Arc<str>>>,
    attached: HashMap<NodeId, Vec<NodeId>>,
}

/// `GraphicalInformation[i].TargetObject` or `….TargetObject[k]` → `i`.
fn entry_of_target(path: &str) -> Option<usize> {
    let rest = path.strip_prefix("GraphicalInformation[")?;
    let close = rest.find(']')?;
    let entry: usize = rest[..close].parse().ok()?;
    let tail = rest[close + 1..].strip_prefix(".TargetObject")?;
    (tail.is_empty() || (tail.starts_with('[') && tail.ends_with(']'))).then_some(entry)
}

fn insert_sorted<T: Ord>(list: &mut Vec<T>, item: T) {
    let at = list.partition_point(|held| held < &item);
    list.insert(at, item);
}

impl Relations {
    /// Lay the rows of node `id`, from the references it just took.
    pub(super) fn take(
        &mut self,
        nodes: &[IndexNode],
        strings: &Interner,
        id: NodeId,
        out: &[(NodeId, PathId)],
        facts: Facts,
    ) {
        let node = &nodes[id as usize];

        let mut parents: Vec<(u8, NodeId)> = out
            .iter()
            .filter_map(|&(target, path)| {
                let at = PARENT_FIELDS
                    .iter()
                    .position(|field| *field == strings.get(path))?;
                Some((at as u8, target))
            })
            .collect();
        if !parents.is_empty() {
            parents.sort_unstable();
            self.parents.insert(id, parents.into_boxed_slice());
        }

        if &*node.type_key == ASSOCIATION_SET {
            self.lay_set(nodes, strings, id, out);
        }
        if facts.project {
            self.projects.insert(id);
        }
        if matches!(&*node.type_key, "Model" | "EarthModelInterpretation") {
            for &(target, path) in out {
                if target != id {
                    insert_sorted(self.modelled.entry(target).or_default(), (id, path));
                }
            }
        }
        if &*node.type_key == "GraphicalInformationSet" {
            for &(target, path) in out {
                if let Some(entry) = entry_of_target(strings.get(path)) {
                    insert_sorted(self.decorated.entry(target).or_default(), (id, entry));
                }
            }
            self.graphical_sets.insert(id, facts.entry_types);
        }
        if &*node.qualified_type == ATTACHMENT {
            for &(target, path) in out {
                if strings.get(path) == "ObjectReference" {
                    insert_sorted(self.attached.entry(target).or_default(), id);
                }
            }
        }
    }

    fn lay_set(&mut self, nodes: &[IndexNode], strings: &Interner, id: NodeId, out: &[(NodeId, PathId)]) {
        let mut collections: Vec<(u64, NodeId)> = Vec::new();
        let mut members: BTreeMap<u64, Vec<(u64, NodeId)>> = BTreeMap::new();
        for &(target, path) in out {
            match parse_association(strings.get(path)) {
                Some((i, Part::Collection)) => collections.push((i, target)),
                Some((i, Part::Member(j))) if target != id => members.entry(i).or_default().push((j, target)),
                _ => {}
            }
        }
        // By association, then by the collection's uuid as it is written — the
        // order `Walker::associations_of` always gave.
        collections.sort_by(|a, b| (a.0, &nodes[a.1 as usize].uuid).cmp(&(b.0, &nodes[b.1 as usize].uuid)));
        let mut first: HashMap<String, u64> = HashMap::new();
        let mut rows: Vec<Row> = Vec::with_capacity(collections.len());
        for (index, collection) in collections {
            let key = nodes[collection as usize].uuid.to_ascii_lowercase();
            let repeated_of = match first.get(&key) {
                Some(earlier) => Some(*earlier),
                None => {
                    first.insert(key, index);
                    None
                }
            };
            let mut listed = members.get(&index).cloned().unwrap_or_default();
            listed.sort_unstable();
            rows.push(Row {
                index,
                collection,
                repeated_of,
                members: listed.into_boxed_slice(),
            });
        }
        // A membership in an association that names **no** collection is still a
        // membership edge in the document; the walk says "not a membership".
        // Keep it findable: a row without a collection would need an invented id,
        // so those members are kept apart under the association's index only.
        for (index, listed) in &members {
            if rows.iter().any(|row| row.index == *index) {
                continue;
            }
            for &(j, member) in listed {
                let held = Held {
                    holder: id,
                    association: *index,
                    position: j,
                };
                insert_sorted(self.held.entry(member).or_default(), held);
            }
        }
        for row in &rows {
            for &(j, member) in row.members.iter() {
                let held = Held {
                    holder: id,
                    association: row.index,
                    position: j,
                };
                insert_sorted(self.held.entry(member).or_default(), held);
            }
        }
        if !rows.is_empty() {
            self.sets.insert(id, rows.into_boxed_slice());
        }
    }

    /// Take back every row node `id` laid. `out` is what it referenced **before**
    /// it was detached: the tables keyed by target are cleaned through it.
    pub(super) fn forget(&mut self, id: NodeId, out: &[(NodeId, PathId)], strings: &Interner) {
        self.parents.remove(&id);
        self.projects.remove(&id);
        self.graphical_sets.remove(&id);
        let mut members: Vec<NodeId> = Vec::new();
        for &(target, path) in out {
            if matches!(parse_association(strings.get(path)), Some((_, Part::Member(_)))) {
                members.push(target);
            }
        }
        self.sets.remove(&id);
        members.sort_unstable();
        members.dedup();
        for member in members {
            if let Some(list) = self.held.get_mut(&member) {
                list.retain(|held| held.holder != id);
                if list.is_empty() {
                    self.held.remove(&member);
                }
            }
        }
        let mut targets: Vec<NodeId> = out.iter().map(|&(target, _)| target).collect();
        targets.sort_unstable();
        targets.dedup();
        for target in targets {
            if let Some(list) = self.modelled.get_mut(&target) {
                list.retain(|&(source, _)| source != id);
                if list.is_empty() {
                    self.modelled.remove(&target);
                }
            }
            if let Some(list) = self.decorated.get_mut(&target) {
                list.retain(|&(set, _)| set != id);
                if list.is_empty() {
                    self.decorated.remove(&target);
                }
            }
            if let Some(list) = self.attached.get_mut(&target) {
                list.retain(|&source| source != id);
                if list.is_empty() {
                    self.attached.remove(&target);
                }
            }
        }
    }

    /// How many rows the tables hold: a figure for a gauge.
    pub(super) fn len(&self) -> usize {
        self.parents.len()
            + self.sets.values().map(|rows| rows.len()).sum::<usize>()
            + self.held.values().map(Vec::len).sum::<usize>()
            + self.projects.len()
            + self.modelled.values().map(Vec::len).sum::<usize>()
            + self.decorated.values().map(Vec::len).sum::<usize>()
            + self.attached.values().map(Vec::len).sum::<usize>()
    }
}

// ---------------------------------------------------------------------------
// What the walks read
// ---------------------------------------------------------------------------

/// One membership of an object: it is listed as member `position` of
/// association `association` of the set `holder`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Membership<'a> {
    /// Uuid of the `CollectionsToDataobjectsAssociationSet`, as written.
    pub holder: &'a str,
    /// Its uri — the identity the walks key by.
    pub holder_uri: &'a str,
    /// `i` of `SingleCollectionAssociation[i]`.
    pub association: u64,
    /// `j` of `Dataobject[j]`.
    pub position: u64,
}

/// One association of a set: its collection, and whether §8.3 discards it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssociationRow<'a> {
    /// `i` of `SingleCollectionAssociation[i]`.
    pub index: u64,
    /// Uuid of the collection, as written.
    pub collection: &'a str,
    /// Its uri — the identity the walks key by.
    pub collection_uri: &'a str,
    /// Common TRG 2.3 §8.3: the same collection named by an earlier association
    /// of the set — the index of that one.
    pub repeated_of: Option<u64>,
}

/// An object that references another, and is a `Model` or an
/// `EarthModelInterpretation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelRef<'a> {
    /// Its uuid, as written.
    pub uuid: &'a str,
    /// Its uri — the identity the walks key by.
    pub uri: &'a str,
    /// `Model` or `EarthModelInterpretation`.
    pub type_key: &'a str,
    /// The path of the reference in it.
    pub path: &'a str,
}

/// An entry of a `GraphicalInformationSet` that targets an object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GraphicalRow<'a> {
    /// Uuid of the set, as written.
    pub set: &'a str,
    /// Position of the entry **in that set**.
    pub entry: usize,
    /// The entry's type: `DefaultGraphicalInformation`, `ColorInformation`…
    pub kind: &'a str,
}

impl GraphicalRow<'_> {
    /// Whether the entry is a `DefaultGraphicalInformation` — the one the
    /// inheritance of a colour reads.
    pub fn is_default(&self) -> bool {
        self.kind.contains("DefaultGraphicalInformation")
    }
}

impl PackageIndex {
    /// The node an address names — [`PackageIndex::find`]: a uri, or a bare
    /// uuid at the entry.
    fn lay(&self, address: &str) -> Option<NodeId> {
        self.find(address)
    }

    fn row_read(&self, rows: usize) {
        self.work.rows.fetch_add(rows as u64, Ordering::Relaxed);
    }

    /// The parents of an object — the three fields `RepresentedObject`,
    /// `RepresentedInterpretation`, `InterpretedFeature`, as `(rank of the field
    /// in that list, the parent's node)`, in that order. The node carries the
    /// parent's uri, which is what the walks go on by.
    pub fn parent_rows(&self, address: &str) -> Vec<(usize, &IndexNode)> {
        let Some(id) = self.lay(address) else {
            return Vec::new();
        };
        let rows = self
            .relations
            .parents
            .get(&id)
            .map(|rows| &**rows)
            .unwrap_or_default();
        self.row_read(rows.len());
        rows.iter()
            .map(|&(at, target)| (at as usize, &self.nodes[target as usize]))
            .collect()
    }

    /// The associations of a `CollectionsToDataobjectsAssociationSet`, in
    /// association order, with §8.3 decided.
    pub fn associations_of(&self, holder: &str) -> Vec<AssociationRow<'_>> {
        let Some(id) = self.lay(holder) else {
            return Vec::new();
        };
        let rows = self
            .relations
            .sets
            .get(&id)
            .map(|rows| &**rows)
            .unwrap_or_default();
        self.row_read(rows.len());
        rows.iter()
            .map(|row| AssociationRow {
                index: row.index,
                collection: &self.nodes[row.collection as usize].uuid,
                collection_uri: &self.nodes[row.collection as usize].uri,
                repeated_of: row.repeated_of,
            })
            .collect()
    }

    /// The objects an association of a set lists, in order — the **members** of
    /// the collection that association names. The way *from* a collection *to*
    /// what it holds (`memberships_of` is the way back).
    pub fn members_of_association(&self, holder: &str, association: u64) -> Vec<&IndexNode> {
        let Some(id) = self.lay(holder) else {
            return Vec::new();
        };
        let Some(rows) = self.relations.sets.get(&id) else {
            return Vec::new();
        };
        let Some(row) = rows.iter().find(|row| row.index == association) else {
            return Vec::new();
        };
        self.row_read(row.members.len());
        row.members
            .iter()
            .map(|&(_, member)| &self.nodes[member as usize])
            .collect()
    }

    /// The associations (set, index) that name this object as their
    /// **collection** — the sets that hold it.
    pub fn collection_named_by(&self, collection: &str) -> Vec<(&IndexNode, u64)> {
        let Some(wanted) = self.lay(collection) else {
            return Vec::new();
        };
        let mut out: Vec<(NodeId, u64)> = Vec::new();
        for (&holder, rows) in &self.relations.sets {
            for row in rows.iter().filter(|row| row.collection == wanted) {
                out.push((holder, row.index));
            }
        }
        self.row_read(out.len());
        out.sort_by(|a, b| (&self.nodes[a.0 as usize].uri, a.1).cmp(&(&self.nodes[b.0 as usize].uri, b.1)));
        out.into_iter()
            .map(|(holder, index)| (&self.nodes[holder as usize], index))
            .collect()
    }

    /// The collections an object is a member of, one hop: the set and the
    /// association that list it, by `(holder, association, position)`.
    pub fn memberships_of(&self, uuid: &str) -> Vec<Membership<'_>> {
        let Some(id) = self.lay(uuid) else {
            return Vec::new();
        };
        let rows = self
            .relations
            .held
            .get(&id)
            .map(Vec::as_slice)
            .unwrap_or_default();
        self.row_read(rows.len());
        rows.iter()
            .map(|held| Membership {
                holder: &self.nodes[held.holder as usize].uuid,
                holder_uri: &self.nodes[held.holder as usize].uri,
                association: held.association,
                position: held.position,
            })
            .collect()
    }

    /// Whether this object is a `DataobjectCollection` of `Kind` `project` — the
    /// model's own collection, which the walk does not follow.
    pub fn is_project(&self, uuid: &str) -> bool {
        self.lay(uuid)
            .is_some_and(|id| self.relations.projects.contains(&id))
    }

    /// The `Model`s and `EarthModelInterpretation`s that reference this object.
    pub fn models_referencing(&self, uuid: &str) -> Vec<ModelRef<'_>> {
        let Some(id) = self.lay(uuid) else {
            return Vec::new();
        };
        let rows = self
            .relations
            .modelled
            .get(&id)
            .map(Vec::as_slice)
            .unwrap_or_default();
        self.row_read(rows.len());
        rows.iter()
            .filter(|&&(source, _)| self.nodes[source as usize].state == NodeState::Present)
            .map(|&(source, path)| {
                let node = &self.nodes[source as usize];
                ModelRef {
                    uuid: &node.uuid,
                    uri: &node.uri,
                    type_key: &node.type_key,
                    path: self.strings.get(path),
                }
            })
            .collect()
    }

    /// The `GraphicalInformationSet`s of the package, in node order (inventory
    /// order for a build) — **what a caller reading them all asks the index**
    /// instead of scanning the store's listing.
    pub fn graphical_sets(&self) -> Vec<&IndexNode> {
        self.relations
            .graphical_sets
            .keys()
            .map(|&id| &self.nodes[id as usize])
            .filter(|node| node.state == NodeState::Present)
            .collect()
    }

    /// How many entries each set of [`PackageIndex::graphical_sets`] holds, in
    /// the same order.
    pub fn graphical_entry_counts(&self) -> Vec<usize> {
        self.relations
            .graphical_sets
            .iter()
            .filter(|&(&id, _)| self.nodes[id as usize].state == NodeState::Present)
            .map(|(_, kinds)| kinds.len())
            .collect()
    }

    /// The entries of a `GraphicalInformationSet` that **target** this object —
    /// who carries a graphical information, and what kind. Empty: nothing in the
    /// package decorates it.
    pub fn graphical_entries_of(&self, uuid: &str) -> Vec<GraphicalRow<'_>> {
        let Some(id) = self.lay(uuid) else {
            return Vec::new();
        };
        let rows = self
            .relations
            .decorated
            .get(&id)
            .map(Vec::as_slice)
            .unwrap_or_default();
        self.row_read(rows.len());
        rows.iter()
            .filter_map(|&(set, entry)| {
                let kind = self.relations.graphical_sets.get(&set)?.get(entry)?;
                Some(GraphicalRow {
                    set: &self.nodes[set as usize].uuid,
                    entry,
                    kind,
                })
            })
            .collect()
    }

    /// Whether any entry of any set targets this object.
    pub fn is_graphically_decorated(&self, uuid: &str) -> bool {
        self.lay(uuid)
            .is_some_and(|id| self.relations.decorated.contains_key(&id))
    }

    /// The `eml23.Attachment` objects that name this object by `ObjectReference`,
    /// lowercase uuids, sorted, without duplicates.
    pub fn attachments_naming(&self, uuid: &str) -> Vec<String> {
        let Some(id) = self.lay(uuid) else {
            return Vec::new();
        };
        let rows = self
            .relations
            .attached
            .get(&id)
            .map(Vec::as_slice)
            .unwrap_or_default();
        self.row_read(rows.len());
        let mut out: Vec<String> = rows
            .iter()
            .filter(|&&source| self.nodes[source as usize].state == NodeState::Present)
            .map(|&source| self.nodes[source as usize].uuid.to_ascii_lowercase())
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// The same attachments by **uri**, version included: what to read each by
    /// when a package holds one attachment in two versions. Sorted, without
    /// duplicates.
    pub fn attachment_uris_naming(&self, uuid: &str) -> Vec<String> {
        let Some(id) = self.lay(uuid) else {
            return Vec::new();
        };
        let rows = self
            .relations
            .attached
            .get(&id)
            .map(Vec::as_slice)
            .unwrap_or_default();
        self.row_read(rows.len());
        let mut out: Vec<String> = rows
            .iter()
            .filter(|&&source| self.nodes[source as usize].state == NodeState::Present)
            .map(|&source| self.nodes[source as usize].uri.to_string())
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// Whether an `Attachment` names this object.
    pub fn has_attachment_naming(&self, uuid: &str) -> bool {
        self.lay(uuid).is_some_and(|id| {
            self.relations
                .attached
                .get(&id)
                .is_some_and(|rows| !rows.is_empty())
        })
    }

    /// How many rows the relation tables hold.
    pub fn relation_rows(&self) -> usize {
        self.relations.len()
    }
}
