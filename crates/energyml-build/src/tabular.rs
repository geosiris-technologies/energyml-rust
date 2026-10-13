//! Reading a tabular file — a format, not a vocabulary.
//!
//! The counterpart of [`crate::geojson`] for tables: it returns rows of
//! cells and **knows nothing** of what they mean. What the columns
//! mean is in [`crate::osdu`]; what is done with them is in
//! `ops::wells_from_table`.
//!
//! # Why a second CSV reader
//!
//! [`crate::mesh`] already reads some, and a second place for the same errors is
//! what this repository reproaches itself for. But they are not the same files: a
//! cloud of coordinates without a mandatory header on one side, a table with named
//! headers on the other. The sharing is therefore done at the **lexer** and not at
//! the vocabulary — the separation that `geojson.rs` (the format) and `sigeol.rs` (the
//! vocabulary) already keep.
//!
//! And the lexer of `mesh.rs` does not fit, which is measured rather than
//! argued. On a real working corpus (120 files sharing one 72-column
//! header), the first data row carries
//!
//! ```text
//! "48° 54' 50.51""N, 2° 24' 15.30""E"
//! ```
//!
//! — a quoted field containing a comma **and** doubled quotes. A correct reader
//! sees 72 fields there, naive splitting sees 73: everything that follows is
//! shifted by one, on every row, without anything signalling it. Over twenty
//! files, 946 rows carry a comma in a field.
//!
//! Hence the two differences from the cloud reader, which are the reason
//! this module exists:
//!
//! - **quotes**, with `""` for a literal quote (RFC 4180);
//! - **the separator is decided once**, on the header, and never row by
//!   row. Choosing it per row means a single row with a comma in a field is
//!   split differently from its neighbours.

use std::collections::BTreeMap;

use crate::mesh::Refused;

/// How the file is read.
///
/// What OSDU's `dataset/File.TabularData.1.0.0` can say, plus the three
/// things it does not say — quotes, the decimal separator and
/// the encoding (checked: zero occurrences in the schema). Those are
/// settings, proposed by [`Dialect::sniff`] and replaceable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dialect {
    /// The field separator — `ColumnSeparator`.
    pub separator: char,
    /// The start of a row to skip — `CommentIndicator`.
    pub comment: Option<char>,
    /// How many guard rows before the data — `NumberOfRowsToSkip`.
    pub skip: usize,
    /// The first row carries the names — `ColumnNamesInFirstRow`.
    pub header: bool,
    /// The decimal separator. **Not in the OSDU schema**, and yet a French
    /// CSV writes `1,5`.
    pub decimal: char,
}

impl Default for Dialect {
    fn default() -> Self {
        Dialect { separator: ',', comment: Some('#'), skip: 0, header: true, decimal: '.' }
    }
}

/// The separators we know how to recognise, in the order they are tried.
const CANDIDATES: [char; 3] = [';', ',', '\t'];

impl Dialect {
    /// What the file looks like.
    ///
    /// **The separator is decided on one row and holds for the whole file.**
    /// That is the difference from the cloud reader, and it is what stops a
    /// row carrying a comma in a field from being split differently from its
    /// neighbours.
    ///
    /// The count is made **outside quotes**: a row whose only quoted field
    /// carries three commas must not suggest a comma-separated file. And the
    /// decimal separator follows from it — a semicolon-separated file writes its
    /// numbers with a comma, which is the French convention and the only
    /// combination that is not ambiguous.
    pub fn sniff(text: &str) -> Dialect {
        let base = Dialect::default();
        let Some(line) = first_line(text, &base) else { return base };
        let counts: Vec<(char, usize)> =
            CANDIDATES.iter().map(|sep| (*sep, outside_quotes(&line, *sep))).collect();
        let best = counts.iter().filter(|(_, n)| *n > 0).max_by_key(|(_, n)| *n);
        match best {
            Some((sep, _)) => Dialect {
                separator: *sep,
                // A semicolon-separated file is almost always French, and
                // French writes `1,5`. Proposed, never imposed: the caller
                // replaces it, and the activity records what was used.
                decimal: if *sep == ';' { ',' } else { '.' },
                ..base
            },
            None => base,
        }
    }

    /// Read a number written in this dialect.
    pub fn number(&self, cell: &str) -> Option<f64> {
        let cell = cell.trim();
        if cell.is_empty() {
            return None;
        }
        if self.decimal == '.' {
            cell.parse().ok()
        } else {
            cell.replace(self.decimal, ".").parse().ok()
        }
    }
}

/// The first row that carries something — neither guard nor comment.
fn first_line(text: &str, dialect: &Dialect) -> Option<String> {
    let mut seen = 0usize;
    for row in text.lines() {
        let row = row.trim_start_matches('\u{feff}');
        if row.trim().is_empty() {
            continue;
        }
        if dialect.comment.is_some_and(|c| row.trim_start().starts_with(c)) {
            continue;
        }
        if seen < dialect.skip {
            seen += 1;
            continue;
        }
        return Some(row.to_string());
    }
    None
}

/// How many times this character appears **outside** a quoted field.
fn outside_quotes(line: &str, sep: char) -> usize {
    let mut quoted = false;
    let mut count = 0usize;
    for c in line.chars() {
        if c == '"' {
            quoted = !quoted;
        } else if c == sep && !quoted {
            count += 1;
        }
    }
    count
}

/// A table read: column names and rows of cells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    /// Where it comes from — a file name.
    pub name: String,
    /// The column names, in the file's order.
    pub headers: Vec<String>,
    /// The rows, each of the length of `headers`.
    pub rows: Vec<Vec<String>>,
    /// How it was read.
    pub dialect: Dialect,
}

impl Table {
    /// The rank of a column by its name, ignoring case and edge
    /// spaces.
    pub fn column(&self, name: &str) -> Option<usize> {
        let wanted = name.trim().to_ascii_lowercase();
        self.headers.iter().position(|h| h.trim().to_ascii_lowercase() == wanted)
    }

    /// How many rows.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// A table with no row — which is not an error here, but a datum
    /// for the caller.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// Read tabular text.
///
/// # What is refused, and why by the line
///
/// A file of fifty thousand rows is corrected by its line, not by
/// "invalid" — the rule `mesh.rs` has already paid for. So every refusal carries
/// **the file's line number**, the one a text editor displays, and
/// not the rank among the rows kept.
pub fn read(name: &str, text: &str, dialect: Dialect) -> Result<Table, Refused> {
    let rows = split(text, dialect)?;
    let mut rows = rows.into_iter();

    let (headers, header_at) = if dialect.header {
        match rows.next() {
            Some((at, cells)) => (cells, at),
            None => {
                return Err(Refused::flat(format!(
                    "{name}: the file carries no row at all, so it cannot carry a header"
                )));
            }
        }
    } else {
        // Without a header, the **position is the key**: columns are named by
        // their rank, and nobody must reorder them.
        let peek: Vec<(usize, Vec<String>)> = rows.collect();
        let width = peek.first().map(|(_, cells)| cells.len()).unwrap_or(0);
        let headers: Vec<String> = (0..width).map(|at| at.to_string()).collect();
        return finish(name, headers, 0, peek.into_iter(), dialect);
    };

    let duplicates: Vec<String> = duplicated(&headers);
    if !duplicates.is_empty() {
        return Err(Refused::flat(format!(
            "{name}: line {header_at} names the same column more than once ({}) — a mapping keys on the column name, \
             so two columns of one name cannot be told apart",
            duplicates.join(", ")
        )));
    }

    finish(name, headers, header_at, rows, dialect)
}

/// The rows, once the header is taken: each must have its width.
fn finish(
    name: &str,
    headers: Vec<String>,
    header_at: usize,
    rows: impl Iterator<Item = (usize, Vec<String>)>,
    dialect: Dialect,
) -> Result<Table, Refused> {
    let width = headers.len();
    let mut kept = Vec::new();
    for (at, cells) in rows {
        if cells.len() != width {
            return Err(Refused::flat(format!(
                "{name}: line {at} has {} field(s) where the header (line {header_at}) names {width} — \
                 a row that does not line up with its header would put every value after the gap under the wrong column",
                cells.len()
            )));
        }
        kept.push(cells);
    }
    Ok(Table { name: name.to_string(), headers, rows: kept, dialect })
}

/// The names carried more than once, in the order they appear.
fn duplicated(headers: &[String]) -> Vec<String> {
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for h in headers {
        *seen.entry(h.trim().to_ascii_lowercase()).or_default() += 1;
    }
    headers
        .iter()
        .filter(|h| seen.get(&h.trim().to_ascii_lowercase()).copied().unwrap_or(0) > 1)
        .map(|h| format!("`{h}`"))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Split the text into rows of cells, quotes included.
///
/// Returns the **file's line number** with each row kept: it is what a
/// refusal must name, and it cannot be recomputed afterwards since rows
/// may have been skipped and a quoted field may contain a line
/// break.
fn split(text: &str, dialect: Dialect) -> Result<Vec<(usize, Vec<String>)>, Refused> {
    let mut out: Vec<(usize, Vec<String>)> = Vec::new();
    let mut cells: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut line = 1usize;
    let mut started = line;
    let mut skipped = 0usize;
    let mut chars = text.trim_start_matches('\u{feff}').chars().peekable();
    let mut fresh = true;

    while let Some(c) = chars.next() {
        if fresh && !quoted {
            fresh = false;
            started = line;
            // A comment, or a guard row: it is consumed whole, and its
            // number counts all the same.
            let is_comment = dialect.comment.is_some_and(|k| k == c);
            let is_guard = skipped < dialect.skip && !is_comment;
            if is_comment || is_guard {
                if is_guard {
                    skipped += 1;
                }
                if c != '\n' {
                    for c in chars.by_ref() {
                        if c == '\n' {
                            break;
                        }
                    }
                }
                line += 1;
                fresh = true;
                continue;
            }
        }
        if quoted {
            if c == '"' {
                // `""` is a literal quote; a single one closes the field.
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cell.push('"');
                } else {
                    quoted = false;
                }
            } else {
                if c == '\n' {
                    line += 1;
                }
                cell.push(c);
            }
            continue;
        }
        match c {
            '"' if cell.trim().is_empty() => {
                cell.clear();
                quoted = true;
            }
            c if c == dialect.separator => {
                cells.push(std::mem::take(&mut cell));
            }
            '\r' => {}
            '\n' => {
                cells.push(std::mem::take(&mut cell));
                if !(cells.len() == 1 && cells[0].trim().is_empty()) {
                    out.push((started, std::mem::take(&mut cells)));
                } else {
                    cells.clear();
                }
                line += 1;
                fresh = true;
            }
            c => cell.push(c),
        }
    }
    if quoted {
        return Err(Refused::flat(format!(
            "the file ends inside a quoted field opened at line {started} — a closing quote is missing, \
             and everything after it was read as one cell"
        )));
    }
    if !cell.is_empty() || !cells.is_empty() {
        cells.push(cell);
        if !(cells.len() == 1 && cells[0].trim().is_empty()) {
            out.push((started, cells));
        }
    }
    Ok(out)
}
