//! The **BRGM chronostratigraphic chart** — the register `GeolTs` published at
//! `https://data.geoscience.fr/ncl/GeolTs`, and the ages of its bounds
//! (`GeolTsBound`).
//!
//! The need: generate a model from the stratigraphy officially published by the
//! BRGM, all of it or only a branch (everything that descends from the
//! Cretaceous, say), chosen from the chart drawn as a tree.
//!
//! # Why the register and not a table of our own
//!
//! Each concept has a **perennial URI** (the Priabonien is `GeolTs/1263`), its
//! place in the hierarchy (`skos:broader`), its level (stage, series…), the
//! chart's colour and the ages of its two bounds. A stable uuid is derived from
//! the URI by the writer — the same Priabonien in every package, with no table
//! of correspondences to keep. And a code found in a borehole file is looked up
//! here: measured on a partner borehole corpus, the five `STRA` codes are the
//! register's `skos:notation` — once the label disambiguates, since `e7` alone
//! names four concepts.
//!
//! # A snapshot, compacted at build time
//!
//! `data/geolts/GeolTs.csv` and `GeolTsBound.csv` (licence and attribution in
//! `data/geolts/LICENSE.md`), fetched on 24/09/2026 (licence
//! CC BY 4.0, BRGM), are compacted by `build.rs` like the property kind
//! dictionary: the same scanner reads a fresh export at run time
//! ([`Chart::from_csv`]), so the two cannot disagree. `build.rs` fails the build
//! when either file is absent or unreadable — never an empty chart.
//!
//! # What is decided here, and said
//!
//! - **the parent of a concept** is the `broader` of the **deepest level above
//!   its own**: the Priabonien names both the Upper Eocene (sub-series) and the
//!   Eocene (series), and a tree has one parent. The others are kept;
//! - **youngest first**, the order RESQML's `age` criterion states (*"from
//!   youngest to oldest period"*, TRG 2.2 §6.34): by the young end, then the
//!   old end. A concept without ages is not placed by a guess — see
//!   [`Chart::by_age`].

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

#[allow(dead_code)]
mod scan {
    include!("geolts_csv.rs");
}
use scan::RawConcept;

/// Where the register lives: a concept's URI is this followed by its id.
pub const REGISTER: &str = "https://data.geoscience.fr/ncl/GeolTs/";

/// One concept of the chart.
#[derive(Debug, Clone, PartialEq)]
pub struct Concept {
    /// The number ending its URI.
    pub id: u32,
    /// The French label, the register's own.
    pub label: String,
    /// The English label, or empty.
    pub label_en: String,
    /// The map codes (`e7`); none is unique by itself.
    pub notations: Vec<String>,
    /// The BRGM mnemonic (`PRIA`), or empty.
    pub mnemonic: String,
    /// 1 supereonothem, 2 eonothem, 3 erathem, 4 system, 5 series, 6
    /// sub-series, 7 stage, 8 substage; 9 and 10 are finer divisions the
    /// register numbers without naming.
    pub level: u8,
    /// 1 reference, 2 regional, 3 usual, 4 obsolete, 5 informal.
    pub nature: u8,
    /// Every `broader` the register states.
    pub broader: Vec<u32>,
    /// The chart's colour.
    pub colour: Option<[u8; 3]>,
    /// The old end, in Ma.
    pub begins_ma: Option<f64>,
    /// The young end, in Ma.
    pub ends_ma: Option<f64>,
    /// `valid`, `retired`, `submitted`.
    pub status: String,
    /// Where it applies — `Monde`, `Europe`, `Bassin de Paris`…
    pub spatial: String,
    /// The reference it is taken from — `Gradstein et al. (2012)` —, or empty.
    pub source: String,
    /// The register's note on it, or empty.
    pub note: String,
    /// Its other names.
    pub alt_labels: Vec<String>,
    /// Whether its base is defined by a GSSP — a point in a reference section.
    pub gssp: bool,
    /// Whether its base is defined by a GSSA — an age.
    pub gssa: bool,
}

impl Concept {
    /// Its URI in the register.
    pub fn uri(&self) -> String {
        format!("{REGISTER}{}", self.id)
    }

    /// The name of its level, for a message or a rank title.
    pub fn level_name(&self) -> String {
        level_name(self.level)
    }

    /// The name of its nature, for a message or an extension value.
    pub fn nature_name(&self) -> &'static str {
        match self.nature {
            1 => "reference",
            2 => "regional",
            3 => "usual",
            4 => "obsolete",
            5 => "informal",
            _ => "unknown",
        }
    }

    /// Whether it is the chart's reference division — not a regional,
    /// usual, obsolete or informal one — and still valid.
    pub fn is_reference(&self) -> bool {
        self.nature == 1 && self.status == "valid"
    }
}

/// The name of a level.
pub fn level_name(level: u8) -> String {
    match level {
        1 => "supereonothem".to_string(),
        2 => "eonothem".to_string(),
        3 => "erathem".to_string(),
        4 => "system".to_string(),
        5 => "series".to_string(),
        6 => "sub-series".to_string(),
        7 => "stage".to_string(),
        8 => "substage".to_string(),
        other => format!("level {other}"),
    }
}

impl From<RawConcept> for Concept {
    fn from(raw: RawConcept) -> Concept {
        Concept {
            id: raw.id,
            label: raw.label_fr,
            label_en: raw.label_en,
            notations: raw.notations,
            mnemonic: raw.mnemonic,
            level: raw.level,
            nature: raw.nature,
            broader: raw.parents,
            colour: raw.colour,
            begins_ma: raw.begins_ma,
            ends_ma: raw.ends_ma,
            status: raw.status,
            spatial: raw.spatial,
            source: raw.source,
            note: raw.note,
            alt_labels: raw.alt_labels,
            gssp: raw.gssp,
            gssa: raw.gssa,
        }
    }
}

/// The chart.
#[derive(Debug, Clone, Default)]
pub struct Chart {
    concepts: Vec<Concept>,
    index: HashMap<u32, usize>,
    /// What this chart is, for a message: *the built-in snapshot of 24/09*.
    label: String,
}

impl Chart {
    /// A chart from concepts.
    fn of(concepts: Vec<Concept>, label: impl Into<String>) -> Chart {
        let index = concepts.iter().enumerate().map(|(at, one)| (one.id, at)).collect();
        Chart { concepts, index, label: label.into() }
    }

    /// A chart read from a GeolTs export and, when given, its bounds.
    ///
    /// Without the bounds there are no ages, so no order: [`Chart::by_age`]
    /// refuses rather than place a unit by its id.
    pub fn from_csv(geolts: &str, bounds: Option<&str>) -> Result<Chart, String> {
        let ages = match bounds {
            Some(text) => scan::bounds(text)?,
            None => Vec::new(),
        };
        let concepts = scan::concepts(geolts, &ages)?.into_iter().map(Concept::from).collect();
        Ok(Chart::of(concepts, "a GeolTs export given at run time"))
    }

    /// What this chart is.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// How many concepts.
    pub fn len(&self) -> usize {
        self.concepts.len()
    }

    /// Whether it holds nothing — a chart handed to `Chart::of` with nothing in it.
    pub fn is_empty(&self) -> bool {
        self.concepts.is_empty()
    }

    /// Every concept, in the register's order.
    pub fn concepts(&self) -> &[Concept] {
        &self.concepts
    }

    /// A concept by its id.
    pub fn get(&self, id: u32) -> Option<&Concept> {
        self.index.get(&id).map(|at| &self.concepts[*at])
    }

    /// Its parent in the tree: the `broader` of the deepest level above its
    /// own. See the module.
    pub fn parent(&self, id: u32) -> Option<&Concept> {
        let one = self.get(id)?;
        let parents: Vec<&Concept> = one.broader.iter().filter_map(|p| self.get(*p)).collect();
        parents
            .iter()
            .filter(|p| p.level < one.level || one.level == 0)
            .max_by_key(|p| p.level)
            .or_else(|| parents.first())
            .copied()
    }

    /// Its children in the tree, youngest first.
    pub fn children(&self, id: u32) -> Vec<&Concept> {
        let mut out: Vec<&Concept> =
            self.concepts.iter().filter(|one| self.parent(one.id).is_some_and(|p| p.id == id)).collect();
        out.sort_by(|a, b| young_first(a, b));
        out
    }

    /// The concepts that have no parent in the chart.
    pub fn roots(&self) -> Vec<&Concept> {
        let mut out: Vec<&Concept> = self.concepts.iter().filter(|one| self.parent(one.id).is_none()).collect();
        out.sort_by(|a, b| young_first(a, b));
        out
    }

    /// A concept and **everything below it**, the concept first.
    pub fn descendants(&self, id: u32) -> Vec<u32> {
        let mut out = vec![id];
        let mut at = 0;
        while at < out.len() {
            let current = out[at];
            for child in self.children(current) {
                if !out.contains(&child.id) {
                    out.push(child.id);
                }
            }
            at += 1;
        }
        out
    }

    /// A concept's ancestors, nearest first.
    pub fn ancestors(&self, id: u32) -> Vec<u32> {
        let mut out = Vec::new();
        let mut current = self.parent(id);
        while let Some(one) = current {
            if out.contains(&one.id) {
                break;
            }
            out.push(one.id);
            current = self.parent(one.id);
        }
        out
    }

    /// Concepts **youngest first**, or a refusal naming those without ages.
    ///
    /// The order is what a `StratigraphicColumnRankInterpretation` lists, and
    /// an order guessed for a unit whose ages the register does not state would
    /// put it anywhere — with nothing in the column to say so.
    pub fn by_age(&self, ids: &[u32]) -> Result<Vec<&Concept>, String> {
        let mut found: Vec<&Concept> = Vec::new();
        let mut undated: Vec<String> = Vec::new();
        for id in ids {
            let Some(one) = self.get(*id) else { continue };
            if one.begins_ma.is_none() && one.ends_ma.is_none() {
                undated.push(format!("{} ({})", one.label, one.uri()));
            }
            found.push(one);
        }
        if !undated.is_empty() {
            return Err(format!(
                "{} concept(s) state no age in the chart, so where they sit in a column cannot be read: {}",
                undated.len(),
                undated.join(", ")
            ));
        }
        found.sort_by(|a, b| young_first(a, b));
        Ok(found)
    }

    /// The concept a **code and its label** name — how a borehole file writes
    /// a stratigraphy (`STRA` = `e7`, `Nom_STRA` = `Priabonien`).
    ///
    /// The code alone is ambiguous by construction (`e7` names four concepts),
    /// so the label decides; compared without case, in French or English.
    /// `Ok(None)` is *the chart knows no such pair* — a local unit, or `n.d.`;
    /// several matches are a refusal naming them.
    pub fn find(&self, code: &str, label: &str) -> Result<Option<&Concept>, String> {
        let code = code.trim();
        let label = label.trim().to_lowercase();
        let found: Vec<&Concept> = self
            .concepts
            .iter()
            .filter(|one| one.notations.iter().any(|n| n.eq_ignore_ascii_case(code)))
            .filter(|one| one.label.to_lowercase() == label || one.label_en.to_lowercase() == label)
            .collect();
        match found.as_slice() {
            [] => Ok(None),
            [one] => Ok(Some(one)),
            many => Err(format!(
                "`{code}` `{label}` names {} concepts of the chart — {}",
                many.len(),
                many.iter().map(|one| one.uri()).collect::<Vec<_>>().join(", ")
            )),
        }
    }

    /// A concept named by what a person types: its id, its URI, or a label
    /// or a notation **when only one concept has it**.
    pub fn resolve(&self, text: &str) -> Result<&Concept, String> {
        let text = text.trim();
        let id = text.trim_start_matches(REGISTER).trim_start_matches("GeolTs/");
        if let Ok(id) = id.parse::<u32>() {
            return self.get(id).ok_or_else(|| format!("the chart has no concept {id}"));
        }
        let lower = text.to_lowercase();
        let by_label: Vec<&Concept> = self
            .concepts
            .iter()
            .filter(|one| one.label.to_lowercase() == lower || one.label_en.to_lowercase() == lower)
            .collect();
        let found = if by_label.is_empty() {
            self.concepts.iter().filter(|one| one.notations.iter().any(|n| n.eq_ignore_ascii_case(text))).collect()
        } else {
            by_label
        };
        match found.as_slice() {
            [] => Err(format!("no concept of the chart is called `{text}` — give its label, its code or its id")),
            [one] => Ok(one),
            many => Err(format!(
                "`{text}` names {} concepts — {}; give the id",
                many.len(),
                many.iter().map(|one| format!("{} {} ({})", one.id, one.label, one.level_name())).collect::<Vec<_>>().join(", ")
            )),
        }
    }
}

/// Youngest first: by the young end, then the old end.
fn young_first(a: &Concept, b: &Concept) -> std::cmp::Ordering {
    let key = |one: &Concept| (one.ends_ma.unwrap_or(f64::MAX), one.begins_ma.unwrap_or(f64::MAX));
    let (ka, kb) = (key(a), key(b));
    ka.0.total_cmp(&kb.0).then(ka.1.total_cmp(&kb.1)).then(a.id.cmp(&b.id))
}

/// The chart compacted into this build (never empty: `build.rs` refuses).
const BUILT_IN: &str = include_str!(concat!(env!("OUT_DIR"), "/geolts.tsv"));

static DEFAULT: OnceLock<RwLock<Chart>> = OnceLock::new();

fn slot() -> &'static RwLock<Chart> {
    DEFAULT.get_or_init(|| {
        let chart = Chart::of(
            scan::from_compact(BUILT_IN).into_iter().map(Concept::from).collect(),
            "the built-in BRGM chart (GeolTs, snapshot of 24/09/2026)",
        );
        RwLock::new(chart)
    })
}

/// Do something with the chart in use.
pub fn with_default<T>(f: impl FnOnce(&Chart) -> T) -> T {
    let guard = slot().read().expect("the chart lock is never held across a panic");
    f(&guard)
}

/// Replace the chart in use — a fresh export, or a test's own extract.
pub fn set_default(chart: Chart) {
    let mut guard = slot().write().expect("the chart lock is never held across a panic");
    *guard = chart;
}
