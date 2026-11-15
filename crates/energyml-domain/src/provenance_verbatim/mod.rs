//! Provenance a package records **outside its activities**, shown as written.
//!
//! Some converters keep where an object came from in tables, not in `Activity`
//! objects: the NWDF tables of the UDAR converter (processes, links, a mapping
//! from their own uids to EPC objects, and the files they read). A
//! [`VerbatimSource`] reads one such convention into [`VerbatimProcess`]es;
//! [`VerbatimSources`] is the registry of them, the caller's first (ADR-0006).
//!
//! The two functions here turn a process into what the graph needs:
//! [`verbatim_edges`] the edges it states, and [`reconcile`] what its activity
//! states against what the tables state - two independent statements of one
//! fact, compared, never merged in silence.
//!
//! A value the tables name and nothing resolves stays [`VerbatimTarget::Unresolved`]:
//! it is **never** resolved by its title (the mapping table says exactly which
//! object a key is, and a title only looks like it).

mod nwdf;

pub use nwdf::{NWDF, NwdfTables};

use crate::activity::{ActivityRecord, ActivityRole, ParameterRole, ParameterValue};
use crate::provenance_graph::{EdgeBasis, GraphIssue, NodeKey, ProvEdge, ProvEdgeKind};
use crate::source::ArraySource;
use crate::table::Table;
use energyml_core::identity;

/// A verbatim source's stable id (`nwdf-0.2`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VerbatimId(pub &'static str);

/// A convention of tables that records provenance outside activities.
pub trait VerbatimSource: Send + Sync {
    /// Its stable id.
    fn id(&self) -> VerbatimId;
    /// Whether these tables follow the convention, from column titles only: no
    /// array is read.
    fn recognises(&self, tables: &[Table<'_>]) -> bool;
    /// The processes the tables state, in the order of the processes table.
    fn read(
        &self,
        tables: &[Table<'_>],
        arrays: &dyn ArraySource,
    ) -> Result<Vec<VerbatimProcess>, VerbatimRefusal>;
}

/// The sources, the caller's first (ADR-0006).
pub struct VerbatimSources {
    sources: Vec<Box<dyn VerbatimSource>>,
    /// How many of `sources`, from the front, a caller added.
    added: usize,
}

impl VerbatimSources {
    /// Built-in: [`NwdfTables`].
    pub fn new() -> Self {
        Self {
            sources: vec![Box::new(NwdfTables)],
            added: 0,
        }
    }

    /// No source at all.
    pub fn empty() -> Self {
        Self {
            sources: Vec::new(),
            added: 0,
        }
    }

    /// A caller's source, tried before the built-ins and after the caller's
    /// earlier ones.
    pub fn add(&mut self, source: Box<dyn VerbatimSource>) {
        self.sources.insert(self.added, source);
        self.added += 1;
    }

    /// The sources that recognise these tables, in registry order.
    pub fn recognised(&self, tables: &[Table<'_>]) -> Vec<&dyn VerbatimSource> {
        self.sources
            .iter()
            .filter(|source| source.recognises(tables))
            .map(|source| source.as_ref())
            .collect()
    }

    /// The ids, in the order the sources are tried.
    pub fn ids(&self) -> Vec<VerbatimId> {
        self.sources.iter().map(|source| source.id()).collect()
    }
}

impl Default for VerbatimSources {
    fn default() -> Self {
        Self::new()
    }
}

/// One process, as the tables state it.
#[derive(Debug, Clone, PartialEq)]
pub struct VerbatimProcess {
    /// Its key (`process_uid`).
    pub key: String,
    /// The activity, from the processes table's `activity` column (full uri as written).
    pub activity: Option<String>,
    /// The activity, from the mapping table's `eml activity` row.
    pub activity_by_mapping: Option<String>,
    /// Every other column, title and text, shown verbatim (an empty text is a value, S16).
    pub fields: Vec<(String, String)>,
    /// What it read.
    pub inputs: Vec<VerbatimTarget>,
    /// What it wrote.
    pub outputs: Vec<VerbatimTarget>,
}

/// What a process read or wrote, as the tables name it.
#[derive(Debug, Clone, PartialEq)]
pub enum VerbatimTarget {
    /// A file: its path as written, and the reference key a references table gives it.
    File {
        /// The path, as written.
        path: String,
        /// The reference key the references table gives this path.
        reference: Option<String>,
    },
    /// A key the mapping resolves: the objects' uris, as the mapping writes them.
    Objects {
        /// The key, as the links table writes it.
        key: String,
        /// The uris the mapping gives it.
        uris: Vec<String>,
    },
    /// A key nothing resolves - never resolved by title (08 section 2.3).
    Unresolved(String),
}

/// Why a verbatim source did not read the tables.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VerbatimRefusal {
    /// A column the convention needs is not there.
    #[error("the table `{table}` lacks the column `{column}`")]
    MissingColumn {
        /// The table (its uuid), or the role when no table could be told.
        table: String,
        /// The column title.
        column: String,
    },
    /// A column title appears twice, so no column is the one.
    #[error("the table `{table}` has two columns titled `{column}`")]
    AmbiguousColumn {
        /// The table (its uuid).
        table: String,
        /// The column title.
        column: String,
    },
    /// Two tables play one role: refused rather than guessing which group each belongs to.
    #[error("several tables play the role `{role}`: {tables:?}; they are not told apart")]
    SeveralTables {
        /// The role.
        role: &'static str,
        /// The uuids of the tables that play it.
        tables: Vec<String>,
    },
    /// A column that carries a needed value could not be read.
    #[error("the table `{table}` could not be read: {why}")]
    Unreadable {
        /// The table (its uuid).
        table: String,
        /// Why, in the reader's words.
        why: String,
    },
}

/// Whether two addresses name the same object: same uuid, and versions that
/// agree - the library's one reading of an address, never a comparison of text.
fn same_object(a: &str, b: &str) -> bool {
    let (uuid_a, version_a) = identity::parse_address(a);
    let (uuid_b, version_b) = identity::parse_address(b);
    uuid_a == uuid_b && identity::versions_agree(version_a.as_deref(), version_b.as_deref())
}

/// The edges a verbatim process states, and what it signals.
///
/// The activity is the process's `activity` column, else the mapping's row; the
/// two present and different is an [`GraphIssue::ActivityLinkDiffers`], and
/// neither is a [`GraphIssue::ProcessWithoutActivity`] with **no edge**.
pub fn verbatim_edges(source: VerbatimId, process: &VerbatimProcess) -> (Vec<ProvEdge>, Vec<GraphIssue>) {
    let mut issues = Vec::new();
    if let (Some(by_column), Some(by_mapping)) = (&process.activity, &process.activity_by_mapping)
        && !same_object(by_column, by_mapping)
    {
        issues.push(GraphIssue::ActivityLinkDiffers {
            source,
            process: process.key.clone(),
            by_column: Some(by_column.clone()),
            by_mapping: Some(by_mapping.clone()),
        });
    }
    let Some(activity) = process.activity.as_ref().or(process.activity_by_mapping.as_ref()) else {
        issues.push(GraphIssue::ProcessWithoutActivity {
            source,
            process: process.key.clone(),
        });
        return (Vec::new(), issues);
    };
    let me = NodeKey::Object(activity.clone());
    let mut edges = Vec::new();
    for (targets, consumed) in [(&process.inputs, true), (&process.outputs, false)] {
        for target in targets {
            let keys: Vec<NodeKey> = match target {
                VerbatimTarget::File { path, .. } => vec![NodeKey::File(path.clone())],
                VerbatimTarget::Objects { uris, .. } => {
                    uris.iter().map(|uri| NodeKey::Object(uri.clone())).collect()
                }
                VerbatimTarget::Unresolved(label) => vec![NodeKey::Unresolved {
                    source,
                    label: label.clone(),
                }],
            };
            for key in keys {
                let (from, to, kind) = if consumed {
                    (key, me.clone(), ProvEdgeKind::Consumed)
                } else {
                    (me.clone(), key, ProvEdgeKind::Produced)
                };
                edges.push(ProvEdge {
                    from,
                    to,
                    kind,
                    basis: EdgeBasis::Verbatim(source),
                });
            }
        }
    }
    (edges, issues)
}

/// What an activity and the tables each say of one process.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Reconciliation {
    /// Stated by both, same direction.
    pub both: Vec<String>,
    /// Only the activity states it.
    pub only_activity: Vec<String>,
    /// Only the tables state it (files included, by path).
    pub only_tables: Vec<String>,
    /// Both state it, in opposite directions.
    pub roles_differ: Vec<String>,
}

/// The two directions of a statement.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Way {
    In,
    Out,
}

fn push_once(list: &mut Vec<String>, text: &str) {
    if !list.iter().any(|held| held == text) {
        list.push(text.to_string());
    }
}

/// An activity's statement of a process against the tables' (triangulation,
/// 08 section 5.4): the objects it reads and writes by its parameters' roles,
/// against the inputs and outputs the tables list. Two objects are the same by
/// uuid and agreeing versions, never by text.
pub fn reconcile(activity: &ActivityRecord, roles: &[ParameterRole], process: &VerbatimProcess) -> Reconciliation {
    // The activity's statements, in parameter order.
    let mut stated: Vec<(&str, Way)> = Vec::new();
    for parameter in &activity.parameters {
        let ParameterValue::Object(reference) = &parameter.value else {
            continue;
        };
        let Some(role) = roles.iter().find(|role| role.position == parameter.position) else {
            continue;
        };
        let ways: &[Way] = match role.role {
            ActivityRole::Input => &[Way::In],
            ActivityRole::Output => &[Way::Out],
            ActivityRole::Both => &[Way::In, Way::Out],
            _ => &[],
        };
        for way in ways {
            stated.push((reference.address.as_str(), *way));
        }
    }

    // The tables' statements: (text, is an object, direction).
    let mut tabled: Vec<(&str, bool, Way)> = Vec::new();
    for (targets, way) in [(&process.inputs, Way::In), (&process.outputs, Way::Out)] {
        for target in targets {
            match target {
                VerbatimTarget::File { path, .. } => tabled.push((path.as_str(), false, way)),
                VerbatimTarget::Objects { uris, .. } => {
                    for uri in uris {
                        tabled.push((uri.as_str(), true, way));
                    }
                }
                VerbatimTarget::Unresolved(label) => tabled.push((label.as_str(), false, way)),
            }
        }
    }
    let table_has = |address: &str, way: Way| {
        tabled
            .iter()
            .any(|(text, object, held)| *object && *held == way && same_object(address, text))
    };
    let activity_has = |text: &str, way: Way| {
        stated
            .iter()
            .any(|(address, held)| *held == way && same_object(address, text))
    };

    let mut result = Reconciliation::default();
    for (address, way) in &stated {
        let opposite = match way {
            Way::In => Way::Out,
            Way::Out => Way::In,
        };
        if table_has(address, *way) {
            push_once(&mut result.both, address);
        } else if table_has(address, opposite) && !activity_has(address, opposite) {
            // The tables state it the other way, and the activity states it one
            // way only: the two disagree.
            push_once(&mut result.roles_differ, address);
        } else {
            push_once(&mut result.only_activity, address);
        }
    }
    for (text, object, way) in &tabled {
        if !*object {
            // A file, or a key nothing resolves: no activity parameter can state it.
            push_once(&mut result.only_tables, text);
            continue;
        }
        let opposite = match way {
            Way::In => Way::Out,
            Way::Out => Way::In,
        };
        if activity_has(text, *way) {
            continue;
        }
        if activity_has(text, opposite) && !table_has(text, opposite) {
            // Already listed in `roles_differ`, by the activity's address.
            continue;
        }
        push_once(&mut result.only_tables, text);
    }
    result
}
