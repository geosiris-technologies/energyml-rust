//! The NWDF v0.2 tables of the UDAR converter.
//!
//! Four tables, told apart by the **titles of their columns** and nothing else:
//!
//! | role | columns |
//! |---|---|
//! | `processes` | `process_uid`, `activity` |
//! | `links` | `source_uid`, `attribute`, `target`, `target_table` |
//! | `mapping` | `nwdf_uid`, `role`, `epc_uri` |
//! | `references` (optional) | `path`, `reference_uid` |
//!
//! A link is tied to a process by its `source_uid` belonging to the
//! `process_uid` column - never by the `source_table` title - and a target is
//! tied to objects by the mapping table, never by a title.

use std::collections::HashMap;

use super::{VerbatimId, VerbatimProcess, VerbatimRefusal, VerbatimSource, VerbatimTarget};
use crate::source::ArraySource;
use crate::table::{Cell, Rows, Table};

/// The id of the NWDF v0.2 source.
pub const NWDF: VerbatimId = VerbatimId("nwdf-0.2");

/// The NWDF v0.2 tables of the UDAR converter: processes, links, the uid to
/// EPC object mapping, and (optionally) references.
pub struct NwdfTables;

const PROCESSES: &[&str] = &["process_uid", "activity"];
const LINKS: &[&str] = &["source_uid", "attribute", "target", "target_table"];
const MAPPING: &[&str] = &["nwdf_uid", "role", "epc_uri"];
const REFERENCES: &[&str] = &["path", "reference_uid"];

/// How many columns of the table carry this title.
fn count_of(table: &Table<'_>, title: &str) -> usize {
    table
        .columns
        .iter()
        .filter(|column| column.title.as_deref() == Some(title))
        .count()
}

/// Whether the table holds every one of these titles.
fn holds_all(table: &Table<'_>, titles: &[&str]) -> bool {
    titles.iter().all(|title| count_of(table, title) > 0)
}

/// The table that plays a role: exactly one, or a named refusal.
///
/// A role is played by a table that holds every title of it. With none, a
/// single table that holds the role's first title is named as lacking the rest;
/// more than one is a refusal, not a guess.
fn table_of<'t, 'a>(
    tables: &'t [Table<'a>],
    role: &'static str,
    titles: &[&str],
    required: bool,
) -> Result<Option<&'t Table<'a>>, VerbatimRefusal> {
    let playing: Vec<&Table<'a>> = tables.iter().filter(|table| holds_all(table, titles)).collect();
    match playing.as_slice() {
        [one] => Ok(Some(*one)),
        [] if !required => Ok(None),
        [] => {
            let keyed: Vec<&Table<'a>> = tables.iter().filter(|table| count_of(table, titles[0]) > 0).collect();
            if let [only] = keyed.as_slice() {
                let missing = titles
                    .iter()
                    .find(|title| count_of(only, title) == 0)
                    .copied()
                    .unwrap_or(titles[0]);
                return Err(VerbatimRefusal::MissingColumn {
                    table: only.uuid.clone(),
                    column: missing.to_string(),
                });
            }
            Err(VerbatimRefusal::MissingColumn {
                table: role.to_string(),
                column: titles[0].to_string(),
            })
        }
        several => Err(VerbatimRefusal::SeveralTables {
            role,
            tables: several.iter().map(|table| table.uuid.clone()).collect(),
        }),
    }
}

/// A table's cells, read once.
struct Read {
    rows: Rows,
}

impl Read {
    fn of(table: &Table<'_>, arrays: &dyn ArraySource) -> Result<Read, VerbatimRefusal> {
        let rows = table.read_all(arrays).map_err(|cause| VerbatimRefusal::Unreadable {
            table: table.uuid.clone(),
            why: cause.to_string(),
        })?;
        // A column that did not read is a column whose cells would be taken for
        // empty: refused, with the reader's own words.
        if rows.columns.iter().any(|column| column.is_none()) {
            return Err(VerbatimRefusal::Unreadable {
                table: table.uuid.clone(),
                why: rows.notes.join("; "),
            });
        }
        Ok(Read { rows })
    }

    /// The text of one cell: a `NaN` number is an empty cell (`None`), an empty
    /// text is a value (S16).
    fn text(&self, row: usize, column: usize) -> Option<String> {
        match self.rows.cell(row, column)? {
            Cell::Text(text) => Some(text),
            Cell::Number(number) if number.is_nan() => None,
            other => Some(other.to_string()),
        }
    }
}

/// The index of the one column carrying a title.
fn column_of(table: &Table<'_>, title: &str) -> Result<usize, VerbatimRefusal> {
    let found: Vec<usize> = table
        .columns
        .iter()
        .enumerate()
        .filter(|(_, column)| column.title.as_deref() == Some(title))
        .map(|(at, _)| at)
        .collect();
    match found.as_slice() {
        [one] => Ok(*one),
        [] => Err(VerbatimRefusal::MissingColumn {
            table: table.uuid.clone(),
            column: title.to_string(),
        }),
        _ => Err(VerbatimRefusal::AmbiguousColumn {
            table: table.uuid.clone(),
            column: title.to_string(),
        }),
    }
}

impl VerbatimSource for NwdfTables {
    fn id(&self) -> VerbatimId {
        NWDF
    }

    fn recognises(&self, tables: &[Table<'_>]) -> bool {
        [PROCESSES, LINKS, MAPPING]
            .iter()
            .all(|titles| tables.iter().any(|table| holds_all(table, titles)))
    }

    fn read(
        &self,
        tables: &[Table<'_>],
        arrays: &dyn ArraySource,
    ) -> Result<Vec<VerbatimProcess>, VerbatimRefusal> {
        let processes = table_of(tables, "processes", PROCESSES, true)?.expect("required");
        let links = table_of(tables, "links", LINKS, true)?.expect("required");
        let mapping = table_of(tables, "mapping", MAPPING, true)?.expect("required");
        let references = table_of(tables, "references", REFERENCES, false)?;

        let processes_read = Read::of(processes, arrays)?;
        let links_read = Read::of(links, arrays)?;
        let mapping_read = Read::of(mapping, arrays)?;
        let references_read = references.map(|table| Read::of(table, arrays)).transpose()?;

        // The mapping: the uris of a key, and the activity a key stands for.
        let (m_uid, m_role, m_uri) = (
            column_of(mapping, "nwdf_uid")?,
            column_of(mapping, "role")?,
            column_of(mapping, "epc_uri")?,
        );
        let mut uris_of: HashMap<String, Vec<String>> = HashMap::new();
        let mut activity_of: HashMap<String, String> = HashMap::new();
        for row in 0..mapping_read.rows.rows {
            let (Some(uid), Some(uri)) = (mapping_read.text(row, m_uid), mapping_read.text(row, m_uri)) else {
                continue;
            };
            if uri.is_empty() {
                continue;
            }
            if mapping_read.text(row, m_role).as_deref() == Some("eml activity") {
                // Several rows for one process: the first, and `verbatim_edges`
                // is the one that compares.
                activity_of.entry(uid.clone()).or_insert_with(|| uri.clone());
            }
            uris_of.entry(uid).or_default().push(uri);
        }

        // The references: the key a path was given.
        let mut reference_of: HashMap<String, String> = HashMap::new();
        if let (Some(read), Some(table)) = (&references_read, references) {
            let (r_path, r_uid) = (column_of(table, "path")?, column_of(table, "reference_uid")?);
            for row in 0..read.rows.rows {
                if let (Some(path), Some(uid)) = (read.text(row, r_path), read.text(row, r_uid)) {
                    reference_of.entry(path).or_insert(uid);
                }
            }
        }

        // The links, by the process that states them (membership of the
        // `process_uid` column, never the `source_table` title).
        let (l_source, l_attribute, l_target, l_table) = (
            column_of(links, "source_uid")?,
            column_of(links, "attribute")?,
            column_of(links, "target")?,
            column_of(links, "target_table")?,
        );
        let mut links_of: HashMap<String, Vec<(String, String, String)>> = HashMap::new();
        for row in 0..links_read.rows.rows {
            let (Some(source), Some(attribute), Some(target)) = (
                links_read.text(row, l_source),
                links_read.text(row, l_attribute),
                links_read.text(row, l_target),
            ) else {
                continue;
            };
            let target_table = links_read.text(row, l_table).unwrap_or_default();
            links_of.entry(source).or_default().push((attribute, target, target_table));
        }

        // The processes.
        let (p_uid, p_activity) = (column_of(processes, "process_uid")?, column_of(processes, "activity")?);
        let mut result = Vec::new();
        for row in 0..processes_read.rows.rows {
            let Some(key) = processes_read.text(row, p_uid) else {
                continue;
            };
            let activity = processes_read.text(row, p_activity).filter(|text| !text.is_empty());
            let mut fields = Vec::new();
            for (at, column) in processes.columns.iter().enumerate() {
                if at == p_uid || at == p_activity {
                    continue;
                }
                if let Some(text) = processes_read.text(row, at) {
                    fields.push((column.title.clone().unwrap_or_default(), text));
                }
            }
            let mut inputs = Vec::new();
            let mut outputs = Vec::new();
            for (attribute, target, target_table) in links_of.get(&key).map(Vec::as_slice).unwrap_or(&[]) {
                let list = match attribute.as_str() {
                    "inputs" => &mut inputs,
                    "outputs" => &mut outputs,
                    // `dataset_uids` is acquisition context, not processing
                    // provenance: left to the acquisition view.
                    _ => continue,
                };
                list.push(if target_table == "external file" {
                    VerbatimTarget::File {
                        path: target.clone(),
                        reference: reference_of.get(target).cloned(),
                    }
                } else {
                    match uris_of.get(target) {
                        Some(uris) if !uris.is_empty() => VerbatimTarget::Objects {
                            key: target.clone(),
                            uris: uris.clone(),
                        },
                        _ => VerbatimTarget::Unresolved(target.clone()),
                    }
                });
            }
            result.push(VerbatimProcess {
                activity_by_mapping: activity_of.get(&key).cloned(),
                key,
                activity,
                fields,
                inputs,
                outputs,
            });
        }
        Ok(result)
    }
}
