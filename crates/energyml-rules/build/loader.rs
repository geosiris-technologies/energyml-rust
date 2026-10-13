// Reads and judges the rule files. **Included, never compiled as a module**:
// `build.rs` includes it to turn `rules/` into statics, and `tests/` includes
// it to judge fabricated files - so what the build enforces and what the tests
// try to break are the same code, not two readings.
//
// Nothing here reads a file: it takes text. Every guard answers with an
// `Error` that names the file, the rule and what is wrong - a refusal by name.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// The two ids published before the `house/<theme>/<name>` shape existed:
/// reports and tests cite them, so they keep their form.
/// No other id may be flat.
pub const FLAT_IDS: &[&str] = &[
    "house/discrete-value-in-lookup",
    "house/time-series-asset-resolves",
];

/// The closed list of documents a `spec` reference may name. A reference is a
/// quoted string, never read at run time (ADR-0011), so the only
/// thing that can be checked is that it names a document we know.
pub const SPEC_DOCS: &[&str] = &[
    "common TRG 2.3",
    "CTA Overview 2.3",
    "EML 2.3 schema",
    "Identifier Specification 4.0",
    "Packaging Conventions 1.0",
    "PRODML TimeSeriesData.xsd",
    "RESQML TRG 2.0.1",
    "RESQML TRG 2.2",
    "RESQML Usage Guide 2.2",
    "WITSML TRG 2.1",
];

/// The closed list of views a rule may be anchored on.
/// `create.op:<id>` is open after its prefix: operations are a
/// registry of their own.
pub const VIEWS: &[&str] = &[
    "object.colour-stack",
    "object.colour-frame",
    "object.attachments",
    "object.properties",
    "object.arrays",
    "object.stats",
    "check.report",
    "open.diagnostics",
    "scene.well-tube",
    "scene.property-paint",
    "scene.pick-frame",
    "scene.crs-chip",
    "scene.refusals",
    "scene.3d",
    "scene.profile",
    "scene.well-labels",
    "scene.well-style",
    "column.strat",
    "column.log",
    "series.view",
    "map.footprint",
    "map.basemap",
    "graph.decision-path",
    "graph.tree",
    "export.geojson",
    "export.report",
    "translate.report",
    "table.view",
    "package.files",
    "create.chooser",
];

/// The statuses a rule may carry.
pub const STATUSES: &[&str] = &["active", "designed", "superseded"];
/// What the specification does about the question a rule answers.
pub const STANCES: &[&str] = &["silent", "ambiguous", "interpreted", "extends", "restates"];
/// The kinds of reference.
pub const REF_KINDS: &[&str] = &["spec", "design", "code", "guide"];
/// The kinds of anchor: where a rule shows.
pub const ANCHOR_KINDS: &[&str] = &["type", "view", "finding", "operation", "diagnostic"];

/// A file's name and text.
#[derive(Clone, Debug)]
pub struct Source {
    pub name: String,
    pub text: String,
}

/// A refusal, by name.
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    /// The file is not TOML.
    Toml { file: String, reason: String },
    /// A field is missing, of the wrong type, or has a value the registry refuses.
    Field {
        file: String,
        id: String,
        field: String,
        reason: String,
    },
    /// Two rules, or a rule and an alias, share an id.
    DuplicateId { id: String, files: Vec<String> },
    /// A rule's `theme` is not declared in `themes.toml`.
    UnknownTheme { id: String, theme: String },
    /// A translation names a rule that does not exist.
    UnknownTranslation { file: String, id: String },
    /// A theme translation names a theme that does not exist.
    UnknownThemeTranslation { file: String, id: String },
    /// A rule is translated twice into the same language.
    DuplicateTranslation { lang: String, id: String },
    /// `supersedes` names a rule that does not exist.
    SupersedesUnknown { id: String, target: String },
    /// `supersedes` names a rule that is not marked `superseded`.
    SupersedesNotSuperseded { id: String, target: String },
    /// A rule is `superseded` and nobody says what replaces it.
    SupersededByNobody { id: String },
    /// A rule says the specification is silent or ambiguous and says neither
    /// where it looked nor why not.
    StanceWithoutLook { id: String },
    /// Two ids make the same Rust constant.
    ConstantClash { constant: String, ids: Vec<String> },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Toml { file, reason } => write!(f, "{file}: not valid TOML: {reason}"),
            Error::Field {
                file,
                id,
                field,
                reason,
            } => write!(f, "{file}: rule `{id}`, field `{field}`: {reason}"),
            Error::DuplicateId { id, files } => {
                write!(f, "the id `{id}` is used twice ({})", files.join(", "))
            }
            Error::UnknownTheme { id, theme } => write!(
                f,
                "rule `{id}` names the theme `{theme}`, which `themes.toml` does not declare"
            ),
            Error::UnknownTranslation { file, id } => {
                write!(f, "{file}: a translation names `{id}`, which is no rule")
            }
            Error::UnknownThemeTranslation { file, id } => write!(
                f,
                "{file}: a translation names the theme `{id}`, which is not declared"
            ),
            Error::DuplicateTranslation { lang, id } => write!(f, "`{id}` is translated twice into `{lang}`"),
            Error::SupersedesUnknown { id, target } => {
                write!(f, "rule `{id}` supersedes `{target}`, which does not exist")
            }
            Error::SupersedesNotSuperseded { id, target } => write!(
                f,
                "rule `{id}` supersedes `{target}`, which is not marked `superseded`"
            ),
            Error::SupersededByNobody { id } => {
                write!(f, "rule `{id}` is `superseded` but no rule `supersedes` it")
            }
            Error::StanceWithoutLook { id } => write!(
                f,
                "rule `{id}` says the specification is silent or ambiguous but cites no `spec` reference and no note saying where it looked"
            ),
            Error::ConstantClash { constant, ids } => write!(
                f,
                "the ids {} make the same constant `{constant}`",
                ids.join(", ")
            ),
        }
    }
}

impl std::error::Error for Error {}

/// One rule, as written.
#[derive(Clone, Debug)]
pub struct Rule {
    pub file: String,
    pub id: String,
    pub theme: String,
    pub status: String,
    pub stance: String,
    pub level: u8,
    pub since: String,
    pub title: String,
    pub text: String,
    pub why: String,
    pub rulings: Vec<Ruling>,
    pub refs: Vec<Ref>,
    pub at: Vec<Anchor>,
    pub supersedes: Vec<String>,
    pub aliases: Vec<String>,
    pub applies: Vec<String>,
    pub switch: Option<String>,
}

/// Who decided, and when.
#[derive(Clone, Debug)]
pub struct Ruling {
    pub by: String,
    pub on: String,
    pub quote: Option<String>,
}

/// A reference, quoted and never read.
#[derive(Clone, Debug)]
pub struct Ref {
    pub kind: String,
    pub doc: Option<String>,
    pub path: Option<String>,
    pub at: Option<String>,
    pub note: Option<String>,
}

/// Where a rule shows.
#[derive(Clone, Debug)]
pub struct Anchor {
    pub kind: String,
    pub key: String,
}

/// A theme.
#[derive(Clone, Debug)]
pub struct Theme {
    pub id: String,
    pub title: String,
}

/// What a translation may carry: id and the three texts, nothing else - a
/// translation has no right to change a reference, an anchor or a status.
#[derive(Clone, Debug)]
pub struct Translation {
    pub lang: String,
    pub id: String,
    pub title: Option<String>,
    pub text: Option<String>,
    pub why: Option<String>,
}

/// Everything the files say, judged.
#[derive(Debug)]
pub struct Parsed {
    pub themes: Vec<Theme>,
    pub rules: Vec<Rule>,
    pub translations: Vec<Translation>,
    pub theme_translations: Vec<Translation>,
}

fn field(file: &str, id: &str, field: &str, reason: impl Into<String>) -> Error {
    Error::Field {
        file: file.into(),
        id: id.into(),
        field: field.into(),
        reason: reason.into(),
    }
}

fn read(source: &Source) -> Result<toml::Table, Error> {
    source.text.parse::<toml::Table>().map_err(|e| Error::Toml {
        file: source.name.clone(),
        reason: e.to_string().replace('\n', " | "),
    })
}

fn text_of(
    file: &str,
    id: &str,
    t: &toml::Table,
    key: &str,
    required: bool,
) -> Result<Option<String>, Error> {
    match t.get(key) {
        None if required => Err(field(file, id, key, "missing")),
        None => Ok(None),
        Some(toml::Value::String(s)) if !s.trim().is_empty() => Ok(Some(s.trim().to_string())),
        Some(toml::Value::String(_)) => Err(field(file, id, key, "empty")),
        Some(_) => Err(field(file, id, key, "must be a string")),
    }
}

fn texts_of(file: &str, id: &str, t: &toml::Table, key: &str) -> Result<Vec<String>, Error> {
    match t.get(key) {
        None => Ok(Vec::new()),
        Some(toml::Value::Array(items)) => items
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| field(file, id, key, "must be a list of strings"))
            })
            .collect(),
        Some(_) => Err(field(file, id, key, "must be a list of strings")),
    }
}

fn tables_of<'a>(file: &str, id: &str, t: &'a toml::Table, key: &str) -> Result<Vec<&'a toml::Table>, Error> {
    match t.get(key) {
        None => Ok(Vec::new()),
        Some(toml::Value::Array(items)) => items
            .iter()
            .map(|v| {
                v.as_table()
                    .ok_or_else(|| field(file, id, key, "must be a list of tables"))
            })
            .collect(),
        Some(_) => Err(field(file, id, key, "must be a list of tables")),
    }
}

fn one_of(file: &str, id: &str, name: &str, value: &str, allowed: &[&str]) -> Result<(), Error> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(field(
            file,
            id,
            name,
            format!("`{value}` is not one of {}", allowed.join(", ")),
        ))
    }
}

/// `YYYY-MM-DD`, by shape. (A calendar check would pin nothing the shape does not.)
fn date_shape(file: &str, id: &str, name: &str, value: &str) -> Result<(), Error> {
    let b = value.as_bytes();
    let ok = b.len() == 10
        && b.iter().enumerate().all(|(i, c)| {
            if i == 4 || i == 7 {
                *c == b'-'
            } else {
                c.is_ascii_digit()
            }
        });
    if ok {
        Ok(())
    } else {
        Err(field(
            file,
            id,
            name,
            format!("`{value}` is not a YYYY-MM-DD date"),
        ))
    }
}

fn id_shape(file: &str, id: &str, theme: &str) -> Result<(), Error> {
    let tail = id
        .strip_prefix("house/")
        .ok_or_else(|| field(file, id, "id", "must start with `house/`"))?;
    let kebab = |s: &str| {
        !s.is_empty()
            && s.bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
            && !s.starts_with('-')
            && !s.ends_with('-')
    };
    let parts: Vec<&str> = tail.split('/').collect();
    match parts.as_slice() {
        [name] if FLAT_IDS.contains(&id) && kebab(name) => Ok(()),
        [t, name] if kebab(t) && kebab(name) => {
            if *t == theme {
                Ok(())
            } else {
                Err(field(
                    file,
                    id,
                    "theme",
                    format!("`{theme}` does not match the id, which is under `{t}`"),
                ))
            }
        }
        _ => Err(field(
            file,
            id,
            "id",
            "must be `house/<theme>/<name>` in lowercase kebab-case (only the two published flat ids are exempt)",
        )),
    }
}

fn parse_rule(file: &str, t: &toml::Table) -> Result<Rule, Error> {
    let id = text_of(file, "?", t, "id", true)?.unwrap_or_default();
    let get = |key: &str, required: bool| text_of(file, &id, t, key, required);
    let theme = get("theme", true)?.unwrap_or_default();
    id_shape(file, &id, &theme)?;
    let status = get("status", true)?.unwrap_or_default();
    one_of(file, &id, "status", &status, STATUSES)?;
    let stance = get("stance", true)?.unwrap_or_default();
    one_of(file, &id, "stance", &stance, STANCES)?;
    let level = match t.get("level") {
        Some(toml::Value::Integer(1)) => 1,
        Some(toml::Value::Integer(2)) => 2,
        _ => return Err(field(file, &id, "level", "must be 1 or 2")),
    };
    let since = get("since", true)?.unwrap_or_default();
    date_shape(file, &id, "since", &since)?;

    let mut rulings = Vec::new();
    for r in tables_of(file, &id, t, "rulings")? {
        let by = text_of(file, &id, r, "by", true)?.unwrap_or_default();
        let on = text_of(file, &id, r, "on", true)?.unwrap_or_default();
        date_shape(file, &id, "rulings.on", &on)?;
        rulings.push(Ruling {
            by,
            on,
            quote: text_of(file, &id, r, "quote", false)?,
        });
    }

    let mut refs = Vec::new();
    for r in tables_of(file, &id, t, "refs")? {
        let kind = text_of(file, &id, r, "kind", true)?.unwrap_or_default();
        one_of(file, &id, "refs.kind", &kind, REF_KINDS)?;
        let item = Ref {
            doc: text_of(file, &id, r, "doc", false)?,
            path: text_of(file, &id, r, "path", false)?,
            at: text_of(file, &id, r, "at", false)?,
            note: text_of(file, &id, r, "note", false)?,
            kind,
        };
        match item.kind.as_str() {
            "spec" => {
                let doc = item
                    .doc
                    .as_deref()
                    .ok_or_else(|| field(file, &id, "refs.doc", "a `spec` reference names its document"))?;
                one_of(file, &id, "refs.doc", doc, SPEC_DOCS)?;
                let at = item.at.as_deref().ok_or_else(|| {
                    field(
                        file,
                        &id,
                        "refs.at",
                        "a `spec` reference names its place (§ or p.)",
                    )
                })?;
                if !(at.contains('§') || at.contains("p.") || at.contains("l.") || at.contains("element ")) {
                    return Err(field(
                        file,
                        &id,
                        "refs.at",
                        format!("`{at}` names no section (§), page (p.), line (l.) or schema element"),
                    ));
                }
                if item.path.is_some() {
                    return Err(field(
                        file,
                        &id,
                        "refs.path",
                        "a `spec` reference has a `doc`, not a path",
                    ));
                }
            }
            _ => {
                let path = item
                    .path
                    .as_deref()
                    .ok_or_else(|| field(file, &id, "refs.path", "needs a `path`"))?;
                let root_ok = match item.kind.as_str() {
                    // Only what ships with the crate's repository: the public
                    // documentation and the crates themselves.
                    "design" | "guide" => path.starts_with("docs/"),
                    _ => path.starts_with("crates/"),
                };
                if !root_ok || path.contains("..") || path.starts_with('/') || path.contains('\\') {
                    return Err(field(
                        file,
                        &id,
                        "refs.path",
                        format!(
                            "`{path}` is not a repository-relative path under the root a `{}` reference lives in",
                            item.kind
                        ),
                    ));
                }
                if item.doc.is_some() {
                    return Err(field(
                        file,
                        &id,
                        "refs.doc",
                        "only a `spec` reference has a `doc`",
                    ));
                }
            }
        }
        refs.push(item);
    }

    let mut at = Vec::new();
    for a in tables_of(file, &id, t, "at")? {
        let kind = text_of(file, &id, a, "type", true)?.unwrap_or_default();
        one_of(file, &id, "at.type", &kind, ANCHOR_KINDS)?;
        let key = text_of(file, &id, a, "key", true)?.unwrap_or_default();
        if kind == "view" && !(VIEWS.contains(&key.as_str()) || key.starts_with("create.op:")) {
            return Err(field(
                file,
                &id,
                "at.key",
                format!("`{key}` is not a declared view"),
            ));
        }
        at.push(Anchor { kind, key });
    }

    if matches!(stance.as_str(), "silent" | "ambiguous")
        && !refs.iter().any(|r| r.kind == "spec" || r.note.is_some())
    {
        return Err(Error::StanceWithoutLook { id });
    }

    Ok(Rule {
        file: file.into(),
        theme,
        status,
        stance,
        level,
        since,
        title: get("title", true)?.unwrap_or_default(),
        text: get("text", true)?.unwrap_or_default(),
        why: get("why", true)?.unwrap_or_default(),
        rulings,
        refs,
        at,
        supersedes: texts_of(file, &id, t, "supersedes")?,
        aliases: texts_of(file, &id, t, "aliases")?,
        applies: texts_of(file, &id, t, "applies")?,
        switch: get("switch", false)?,
        id,
    })
}

fn translation(lang: &str, file: &str, t: &toml::Table) -> Result<Translation, Error> {
    let id = text_of(file, "?", t, "id", true)?.unwrap_or_default();
    if let Some(extra) = t
        .keys()
        .find(|k| !matches!(k.as_str(), "id" | "title" | "text" | "why"))
    {
        return Err(field(
            file,
            &id,
            extra,
            "a translation carries id, title, text and why - nothing else",
        ));
    }
    Ok(Translation {
        lang: lang.into(),
        title: text_of(file, &id, t, "title", false)?,
        text: text_of(file, &id, t, "text", false)?,
        why: text_of(file, &id, t, "why", false)?,
        id,
    })
}

/// The Rust constant an id becomes: `house/inherit/rank-order` → `INHERIT_RANK_ORDER`.
pub fn constant_of(id: &str) -> String {
    id.trim_start_matches("house/")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect()
}

/// Reads every file and judges the whole: `themes` is `themes.toml`, `rules`
/// the English files in order, `translations` one list per language of
/// `(language, file)`.
pub fn parse(themes: &Source, rules: &[Source], translations: &[(String, Source)]) -> Result<Parsed, Error> {
    let mut parsed = Parsed {
        themes: Vec::new(),
        rules: Vec::new(),
        translations: Vec::new(),
        theme_translations: Vec::new(),
    };

    let theme_root = read(themes)?;
    for t in tables_of(&themes.name, "?", &theme_root, "theme")? {
        let id = text_of(&themes.name, "?", t, "id", true)?.unwrap_or_default();
        let title = text_of(&themes.name, &id, t, "title", true)?.unwrap_or_default();
        parsed.themes.push(Theme { id, title });
    }
    for source in rules {
        let root = read(source)?;
        for t in tables_of(&source.name, "?", &root, "rule")? {
            parsed.rules.push(parse_rule(&source.name, t)?);
        }
    }

    // Identity: ids and aliases never collide, and never make one constant.
    let mut seen: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for r in &parsed.rules {
        seen.entry(&r.id).or_default().push(&r.file);
        for alias in &r.aliases {
            seen.entry(alias).or_default().push(&r.file);
        }
    }
    if let Some((id, files)) = seen.into_iter().find(|(_, files)| files.len() > 1) {
        return Err(Error::DuplicateId {
            id: id.into(),
            files: files.into_iter().map(String::from).collect(),
        });
    }
    let mut constants: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for r in &parsed.rules {
        constants
            .entry(constant_of(&r.id))
            .or_default()
            .push(r.id.clone());
    }
    if let Some((constant, ids)) = constants.into_iter().find(|(_, ids)| ids.len() > 1) {
        return Err(Error::ConstantClash { constant, ids });
    }

    let theme_ids: BTreeSet<&str> = parsed.themes.iter().map(|t| t.id.as_str()).collect();
    if let Some(r) = parsed
        .rules
        .iter()
        .find(|r| !theme_ids.contains(r.theme.as_str()))
    {
        return Err(Error::UnknownTheme {
            id: r.id.clone(),
            theme: r.theme.clone(),
        });
    }

    // History: a replacement names what it replaces; what is replaced says so.
    let by_id: BTreeMap<&str, &Rule> = parsed.rules.iter().map(|r| (r.id.as_str(), r)).collect();
    let mut replaced = BTreeSet::new();
    for r in &parsed.rules {
        for target in &r.supersedes {
            let old = by_id
                .get(target.as_str())
                .ok_or_else(|| Error::SupersedesUnknown {
                    id: r.id.clone(),
                    target: target.clone(),
                })?;
            if old.status != "superseded" {
                return Err(Error::SupersedesNotSuperseded {
                    id: r.id.clone(),
                    target: target.clone(),
                });
            }
            replaced.insert(target.as_str());
        }
    }
    if let Some(r) = parsed
        .rules
        .iter()
        .find(|r| r.status == "superseded" && !replaced.contains(r.id.as_str()))
    {
        return Err(Error::SupersededByNobody { id: r.id.clone() });
    }

    // Translations: facultative per rule, but never about nothing.
    let mut done = BTreeSet::new();
    for (lang, source) in translations {
        let root = read(source)?;
        for t in tables_of(&source.name, "?", &root, "rule")? {
            let one = translation(lang, &source.name, t)?;
            if !by_id.contains_key(one.id.as_str()) {
                return Err(Error::UnknownTranslation {
                    file: source.name.clone(),
                    id: one.id,
                });
            }
            if !done.insert((lang.clone(), one.id.clone())) {
                return Err(Error::DuplicateTranslation {
                    lang: lang.clone(),
                    id: one.id,
                });
            }
            parsed.translations.push(one);
        }
        for t in tables_of(&source.name, "?", &root, "theme")? {
            let one = translation(lang, &source.name, t)?;
            if !theme_ids.contains(one.id.as_str()) {
                return Err(Error::UnknownThemeTranslation {
                    file: source.name.clone(),
                    id: one.id,
                });
            }
            parsed.theme_translations.push(one);
        }
    }
    Ok(parsed)
}
