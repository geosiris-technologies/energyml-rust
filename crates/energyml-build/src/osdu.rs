//! The template: what the columns of a table **mean**.
//!
//! The vocabulary, where [`crate::tabular`] is the format — the same separation
//! as `geojson.rs` and `sigeol.rs`.
//!
//! # Three forms, one content
//!
//! This module reads indifferently:
//!
//! - a **bare list** of `AbstractReferencePropertyType` — simply a list of the
//!   columns of the template, which is what an earlier CSV-to-EnergyML tool
//!   keeps in its `osdu_cols.json`;
//! - a whole **`master-data--ColumnBasedTableTemplate`**, of which `data.Columns`
//!   and `data.KeyColumns` are taken;
//! - a **`dataset--File.TabularData`**, which also carries *how the file is
//!   read* — separator, rows to skip, comment, and above all
//!   `AbsentValue` per column.
//!
//! The three reduce to the same thing: a list of columns, plus possibly a
//! [`Dialect`]. A whole CBTT **brings nothing more** than its list,
//! measured on the schema: `Columns[]` and `KeyColumns[]` are both
//! `AbstractReferencePropertyType:1.2.1`.
//!
//! # The key is the uuid, never the name
//!
//! `PropertyType.PropertyTypeID` is written
//! `{namespace}:reference-data--PropertyType:{uuid}:`, and **that uuid is the
//! Energistics property kind's**. Checked on the ten constants of the earlier
//! tool: ten out of ten resolve in the shipped dictionary, title for
//! title. There are not two vocabularies — OSDU references Energistics's.
//!
//! The title cannot serve as a key: 3 662 kinds for 3 661 titles,
//! `spring stiffness` being carried by two kinds of the **same** quantity
//! class. A key right 3 661 times out of 3 662 is a key that errs without
//! warning. So `PropertyType.Name` is **displayed**, never compared — and
//! when it disagrees with the dictionary, it is a note and not a refusal:
//! a template may be older than the dictionary.

use serde_json::Value;

use crate::mesh::Refused;
use crate::tabular::Dialect;

/// A declared column — an `AbstractReferencePropertyType:1.2.1`.
#[derive(Debug, Clone, PartialEq)]
pub struct Column {
    /// `ColumnName` — the key matching it to the file's header.
    ///
    /// The schema says it is optional and its own description says it is mandatory:
    /// *« For the ColumnBasedTableTemplate the data.Columns[].ColumnName is
    /// **mandatory** although it is defined as optional in the schema. »* A
    /// JSON Schema validator would therefore let an unusable template through;
    /// it is up to us to refuse it.
    pub name: String,
    /// The property kind's uuid, extracted from `PropertyType.PropertyTypeID`.
    ///
    /// `None` when the template does not say what the column means: it is not
    /// an error, it is a column that will be ignored, and counted.
    pub kind: Option<String>,
    /// `PropertyType.Name` — a label, for display and messages.
    pub label: Option<String>,
    /// `ValueType` — `string`, `number`, `integer` or `boolean`.
    pub value_type: ValueType,
    /// `ValueCount` — the size of a tuple, 1 by default.
    pub count: usize,
    /// The unit, extracted from `UnitOfMeasureID`.
    pub uom: Option<String>,
    /// The unit class, extracted from `UnitQuantityID` — a check, not a
    /// value.
    pub quantity: Option<String>,
    /// `FacetIDs`, in the order the schema wants them: `[FacetType, FacetRole]`.
    pub facets: Vec<String>,
    /// What was noticed **about this column**.
    ///
    /// Carried by the column and not only by the template: a remark that does
    /// not name its line forces one to look for it, and a front end that would find
    /// it again by searching for the name **in the sentence** would be rereading our own
    /// messages — which this repository refuses elsewhere (`Refused::missing` exists
    /// for the same reason).
    pub notes: Vec<String>,
    /// `AbsentValue` of `File.TabularData` — the value that means *nothing*.
    ///
    /// **The schema's find**, and it is worth more than the separator: a
    /// borehole file writes its absence with a sentinel — `-999.25` is
    /// the convention for logs — and without knowing it, it is read as a
    /// measure. A depth of −999.25 m in a perfectly valid package.
    pub absent: Option<String>,
}

/// What a cell carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ValueType {
    /// Text.
    #[default]
    Text,
    /// A float.
    Number,
    /// An integer.
    Integer,
    /// A boolean.
    Boolean,
}

impl ValueType {
    /// The schema's word.
    pub fn word(self) -> &'static str {
        match self {
            ValueType::Text => "string",
            ValueType::Number => "number",
            ValueType::Integer => "integer",
            ValueType::Boolean => "boolean",
        }
    }

    fn read(word: &str) -> ValueType {
        match word.trim().to_ascii_lowercase().as_str() {
            "number" => ValueType::Number,
            "integer" => ValueType::Integer,
            "boolean" => ValueType::Boolean,
            _ => ValueType::Text,
        }
    }
}

/// A template read: the columns, and how to read the file when it says so.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Template {
    /// The declared columns, in the template's order.
    pub columns: Vec<Column>,
    /// The columns the template holds as keys — `KeyColumns[]`.
    /// Their names; they are **also** in `columns`.
    pub keys: Vec<String>,
    /// What `File.TabularData` says of the reading, when it is one.
    pub dialect: Option<Dialect>,
    /// What was noticed without refusing — a `Name` that disagrees, an
    /// inconsistent declaration.
    pub notes: Vec<String>,
}

impl Template {
    /// The column bearing this name, case ignored.
    pub fn column(&self, name: &str) -> Option<&Column> {
        let wanted = name.trim().to_ascii_lowercase();
        self.columns
            .iter()
            .find(|c| c.name.trim().to_ascii_lowercase() == wanted)
    }

    /// The column declared for this property kind, if there is only one.
    ///
    /// Returns an error when two columns carry the same kind: it is the
    /// caller who must say which is the role, and guessing would be choosing
    /// a well's abscissa at random.
    pub fn for_kind(&self, uuid: &str) -> Result<Option<&Column>, String> {
        let wanted = uuid.to_ascii_lowercase();
        let found: Vec<&Column> = self
            .columns
            .iter()
            .filter(|c| c.kind.as_deref().map(str::to_ascii_lowercase) == Some(wanted.clone()))
            .collect();
        match found.as_slice() {
            [] => Ok(None),
            [one] => Ok(Some(one)),
            many => Err(format!(
                "{} columns are declared for the same property kind ({}): {}",
                many.len(),
                uuid,
                many.iter()
                    .map(|c| format!("`{}`", c.name))
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    }
}

/// Read a template, in any one of its three forms.
pub fn read(text: &str) -> Result<Template, Refused> {
    let root: Value =
        serde_json::from_str(text).map_err(|why| Refused::flat(format!("this is not JSON: {why}")))?;
    from_json(&root)
}

/// Read a template already deserialised.
pub fn from_json(root: &Value) -> Result<Template, Refused> {
    let (columns_at, keys_at, dialect) = shape(root)?;
    let mut notes = Vec::new();

    let mut columns = Vec::new();
    for (at, entry) in columns_at.iter().enumerate() {
        columns.push(column(entry, at, &mut notes)?);
    }
    // The keys are **also** columns: a template that separates them describes
    // the same cells, and keeping them apart would mean a key would
    // never be matched with its header.
    let mut keys = Vec::new();
    for (at, entry) in keys_at.iter().enumerate() {
        let read = column(entry, at, &mut notes)?;
        keys.push(read.name.clone());
        if !columns.iter().any(|c| c.name.eq_ignore_ascii_case(&read.name)) {
            columns.push(read);
        }
    }

    if columns.is_empty() {
        return Err(Refused::flat(
            "this template declares no column at all — a `ColumnBasedTableTemplate` with an empty `Columns` \
             is legal for OSDU and useless here"
                .to_string(),
        ));
    }

    // Duplicates are refused here rather than at matching: a template that
    // declares `PROF` twice does not say which of the two counts.
    let mut seen: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for c in &columns {
        *seen.entry(c.name.trim().to_ascii_lowercase()).or_default() += 1;
    }
    let twice: Vec<String> = seen
        .iter()
        .filter(|(_, n)| **n > 1)
        .map(|(k, _)| format!("`{k}`"))
        .collect();
    if !twice.is_empty() {
        return Err(Refused::flat(format!(
            "this template declares the same column more than once ({}) — the mapping keys on the column name",
            twice.join(", ")
        )));
    }

    Ok(Template {
        columns,
        keys,
        dialect,
        notes,
    })
}

/// Where the columns are, depending on the form received.
#[allow(clippy::type_complexity)]
fn shape(root: &Value) -> Result<(Vec<Value>, Vec<Value>, Option<Dialect>), Refused> {
    // A bare list.
    if let Some(list) = root.as_array() {
        return Ok((list.clone(), Vec::new(), None));
    }
    // The reduced form this module writes: `{ columns, keys, notes }`.
    if root.get("columns").and_then(Value::as_array).is_some() {
        let columns = array(root, "columns");
        // The keys there are **names**, where OSDU makes them full
        // entries: converted here so that the rest has only one form to
        // read. The de-duplication below finds them in `columns`, so
        // they name without repeating.
        let keys = array(root, "keys")
            .iter()
            .filter_map(Value::as_str)
            .map(|name| serde_json::json!({ "name": name }))
            .collect();
        return Ok((columns, keys, None));
    }
    // An OSDU record: CBTT or TabularData, both store under `data`.
    let data = root.get("data").unwrap_or(root);
    let columns = array(data, "Columns");
    let keys = array(data, "KeyColumns");
    if columns.is_empty() && keys.is_empty() {
        return Err(Refused::flat(
            "this is neither a list of column definitions nor a record carrying `data.Columns`: a \
             `ColumnBasedTableTemplate`, a `File.TabularData`, or the bare list of \
             `AbstractReferencePropertyType` entries are what this reads"
                .to_string(),
        ));
    }
    // `File.TabularData` also says how to read the file. Recognised by what
    // it carries, never by its `kind`: a record reduced to its useful fields
    // stays readable, and that is what a front end without an `acl` produces.
    let dialect = dialect_of(data);
    let columns = with_specifications(columns, data);
    Ok((columns, keys, dialect))
}

/// The named array, or empty.
fn array(node: &Value, name: &str) -> Vec<Value> {
    node.get(name)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// What `File.TabularData` says of the reading.
fn dialect_of(data: &Value) -> Option<Dialect> {
    let has = [
        "ColumnSeparator",
        "ColumnNamesInFirstRow",
        "NumberOfRowsToSkip",
        "CommentIndicator",
    ]
    .iter()
    .any(|name| data.get(*name).is_some());
    if !has {
        return None;
    }
    let base = Dialect::default();
    Some(Dialect {
        separator: data
            .get("ColumnSeparator")
            .and_then(Value::as_str)
            .and_then(|s| s.chars().next())
            .unwrap_or(base.separator),
        comment: match data.get("CommentIndicator").and_then(Value::as_str) {
            Some(s) => s.chars().next(),
            None => base.comment,
        },
        skip: data
            .get("NumberOfRowsToSkip")
            .and_then(Value::as_u64)
            .unwrap_or(base.skip as u64) as usize,
        header: data
            .get("ColumnNamesInFirstRow")
            .and_then(Value::as_bool)
            .unwrap_or(base.header),
        decimal: base.decimal,
    })
}

/// Carry `ColumnSpecifications[].AbsentValue` onto the columns, by name.
fn with_specifications(mut columns: Vec<Value>, data: &Value) -> Vec<Value> {
    let specs = array(data, "ColumnSpecifications");
    if specs.is_empty() {
        return columns;
    }
    for spec in specs {
        let Some(name) = spec.get("ColumnName").and_then(Value::as_str) else {
            continue;
        };
        let Some(absent) = spec.get("AbsentValue") else {
            continue;
        };
        for column in &mut columns {
            if column
                .get("ColumnName")
                .and_then(Value::as_str)
                .is_some_and(|n| n.eq_ignore_ascii_case(name))
            {
                if let Some(map) = column.as_object_mut() {
                    map.insert("AbsentValue".to_string(), absent.clone());
                }
            }
        }
    }
    columns
}

/// An entry of `AbstractReferencePropertyType` — **or the reduced form that this
/// module writes itself**.
///
/// *The reader accepts both, the writer produces only one*: the rule that
/// this repository already applies to selectors and recipes. Here it is
/// necessary and not merely prudent — a front end reads a template through
/// [`read`], shows it, modifies it and **sends it back**; if the way back were not
/// read, the round trip that is required would be impossible.
///
/// That was a defect once: `read_template` returned the reduced form and
/// `parse_input` expected only OSDU, so adjusting its columns in the front end
/// and then planning answered *« this is neither a list of column definitions
/// nor… »* — on the template the front end had itself just written.
fn column(entry: &Value, at: usize, notes: &mut Vec<String>) -> Result<Column, Refused> {
    if entry.get("ColumnName").is_none() && entry.get("name").is_some() {
        return compact_column(entry, at);
    }
    let name = entry
        .get("ColumnName")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            Refused::flat(format!(
                "entry {at} of this template has no `ColumnName` — the schema calls it optional and its own \
                 description calls it mandatory, so nothing but this reader refuses it, and without it the \
                 entry matches no column of any file"
            ))
        })?;

    let kind = entry
        .get("PropertyType")
        .and_then(|pt| pt.get("PropertyTypeID"))
        .and_then(Value::as_str)
        .and_then(uuid_of_osdu_id);
    let label = entry
        .get("PropertyType")
        .and_then(|pt| pt.get("Name"))
        .and_then(Value::as_str)
        .map(str::to_string);

    let value_type = entry
        .get("ValueType")
        .and_then(Value::as_str)
        .map(ValueType::read)
        .unwrap_or_default();
    let count = entry
        .get("ValueCount")
        .and_then(Value::as_u64)
        .unwrap_or(1)
        .max(1) as usize;
    let uom = entry
        .get("UnitOfMeasureID")
        .and_then(Value::as_str)
        .and_then(tail);
    let quantity = entry.get("UnitQuantityID").and_then(Value::as_str).and_then(tail);

    // The schema: *« Only populated if the ValueType is "number" »*. The earlier
    // tool's template carries metres on a `string` column and
    // `dimensionless` on thirteen text columns. No effect on reading,
    // but it is a template another tool will reread: we **say** so.
    let mut mine = Vec::new();
    if value_type == ValueType::Text && uom.is_some() {
        mine.push(format!(
            "`{name}` is declared `string` and carries a unit of measure, which the schema says belongs to `number` only"
        ));
    }
    notes.extend(mine.iter().cloned());

    let facets = entry
        .get("FacetIDs")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    let absent = entry
        .get("AbsentValue")
        .and_then(Value::as_str)
        .map(str::to_string);

    Ok(Column {
        name: name.to_string(),
        kind,
        label,
        value_type,
        count,
        uom,
        quantity,
        facets,
        absent,
        notes: mine,
    })
}

/// The reduced form: the fields as this module returns them.
///
/// It **notes** nothing — a template's remarks are made at its first
/// reading, and making them again at every round trip would repeat them at each keystroke.
fn compact_column(entry: &Value, at: usize) -> Result<Column, Refused> {
    let name = entry
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| Refused::flat(format!("entry {at} of this template has no `name`")))?;
    Ok(Column {
        name: name.to_string(),
        kind: entry
            .get("kind")
            .and_then(Value::as_str)
            .map(|s| s.to_ascii_lowercase()),
        label: entry.get("label").and_then(Value::as_str).map(str::to_string),
        value_type: entry
            .get("valueType")
            .and_then(Value::as_str)
            .map(ValueType::read)
            .unwrap_or_default(),
        count: entry.get("count").and_then(Value::as_u64).unwrap_or(1).max(1) as usize,
        uom: entry.get("uom").and_then(Value::as_str).map(str::to_string),
        quantity: entry.get("quantity").and_then(Value::as_str).map(str::to_string),
        facets: entry
            .get("facets")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        absent: entry.get("absent").and_then(Value::as_str).map(str::to_string),
        notes: entry
            .get("notes")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
    })
}

/// The uuid of an OSDU identifier: `…:reference-data--PropertyType:{uuid}:`.
///
/// Searched **by its shape** and not by its position, because the namespace is
/// free and the version suffix may be missing. **It is not an ETP
/// uri** — an object's uri is read by `EtpUri::parse`, never here — hence this
/// name, which `recipe.rs` no longer shares.
fn uuid_of_osdu_id(id: &str) -> Option<String> {
    id.split(':')
        .find(|part| energyml_core::identifiers::is_uuid(part))
        .map(|found| found.to_ascii_lowercase())
}

/// The last non-empty part of an OSDU identifier — the name of a unit or
/// of a class, which URL percent-encoding may have encoded.
fn tail(id: &str) -> Option<String> {
    let last = id.split(':').rfind(|part| !part.trim().is_empty())?;
    // `plane%20angle` → `plane angle`. A minimal decoding: these identifiers
    // in practice escape only the space, and decoding more would require
    // a dependency for a case that does not occur.
    let decoded = last.replace("%20", " ");
    (!decoded.trim().is_empty()).then(|| decoded.trim().to_string())
}
