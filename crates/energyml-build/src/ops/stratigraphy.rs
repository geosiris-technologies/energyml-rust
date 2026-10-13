//! `stratigraphy-from-geolts` — a RESQML stratigraphic column from the BRGM
//! chronostratigraphic chart, whole or by branches.
//!
//! The need: import everything from geolts to build the complete
//! stratigraphy, or only sub-parts, for example everything under the
//! 'Cretaceous'. The chart is `energyml_domain::geolts`; this module only
//! turns a selection of it into objects.
//!
//! # The shape, as the RESQML 2.2 reference guide states it
//!
//! ```text
//! Model  “BRGM chronostratigraphy — Crétacé”          the organisation
//!   ↑ InterpretedFeature
//! StratigraphicColumnRankInterpretation × one per level  (§6.45)
//!   StratigraphicUnits → StratigraphicUnitInterpretation, youngest first
//!     ↑ InterpretedFeature
//!   RockVolumeFeature × one per division
//! StratigraphicColumn  Ranks → the rank interpretations  (§6.44)
//! ```
//!
//! - **a rank per level of the chart** — system, series, sub-series, stage —
//!   because RESQML has no link from a unit to the unit that contains it: *“it
//!   is possible to find previous unit containment relationships between
//!   several ranks”* (TRG 2.2 §6.3 p. 166). The containment is carried by the
//!   ranks and the ages, and the register's URI is on every object for whoever
//!   wants the tree;
//! - **a rank is numbered by its level**: `RankInStratigraphicColumn` is the
//!   chart's level less one — the supereonothem 0, the system 3, the series 4,
//!   the stage 6 — whatever else the column holds (see below);
//! - **youngest first** in every rank: `AscendingOrderingCriteria` is `age`,
//!   *“from youngest to oldest period”* (§6.34), and the column's ranks *“must
//!   be ordered by age”* (§6.44 business rule, p. 212). A division the chart
//!   gives no age to is **refused by name** — an order guessed would put it
//!   anywhere; one it gives no **level** to is refused too, having no rank;
//! - **the same uuid in every package**: uuid v5 in the URL namespace over the
//!   division's URI, which anyone can recompute without this tool. The
//!   interpretation adds `#interpretation` to the URI. The organisation and the
//!   ranks depend on the **selection**, so their uuid is derived from it.
//!
//! # What is chosen, and the defaults
//!
//! `units` are the branches — each with everything below it. By default only
//! the chart's **reference** divisions that are still valid, down to the
//! **stage**: the substages are nearly all regional, and a regional synonym
//! beside its reference stage (the Ludien beside the Priabonien) is two units
//! for one interval of time. `regional` and `finest` open both.
//!
//! # A branch comes with the chain above it — `ancestors`, on by default
//!
//! The need: the geolts import shows the whole column very well, but
//! one sometimes wants to work on a sub-part. When a sub-part of the
//! complete group is imported, the parents are not imported, and there
//! seems to be a shift of rank (the Eocene, for example, if it is the one
//! ticked when selecting for the import operation, lands at
//! rank 0).
//!
//! Two defects, and each has its fix:
//!
//! - **the parents were missing.** A column of the Eocene alone began at the
//!   series: nothing said what it is a part of. `ancestors` puts in the **chain**
//!   above each branch chosen — its parent, the parent's parent, up to the root
//!   of the chart — and **none of their other children**: the Eocene brings the
//!   Paleogene, the Cenozoic and the Phanerozoic, never the Paleocene or the
//!   Oligocene, which nobody chose. The parents answer to the same settings as
//!   the branches (`regional`, `finest`), and one they leave out is **passed
//!   over, never a stop**: the Priabonien's parent in the tree is the Upper
//!   Eocene, a *usual* sub-series, and the Eocene above it still comes;
//! - **the numbers were positions.** `RankInStratigraphicColumn` counted the
//!   levels the column happened to hold, so the parents alone would have moved
//!   the Eocene from rank 0 to rank 3 — still not the 4 it has beside the
//!   Precambrian, the chart's only supereonothem, and the stages would still
//!   move by one whenever a branch holds one of the twelve reference
//!   sub-series (both measured on the snapshot of 24/09). So the number is the
//!   **level's**, and a division has the same rank in every column. A level the
//!   column holds nothing of has **no rank at all** — a rank lists at least one
//!   unit (TRG 2.2 §6.45 p. 213, `StratigraphicUnits` 1..*) — and the others
//!   keep their numbers: the gap *is* the absent level, and the plan names it
//!   (`ranks-absent`). A reader that sorts by the number, as
//!   `energyml_domain::stratigraphy` does, is unaffected by a gap.
//!
//! **An added parent is said, and marked where it is true.** A note of the plan
//! (`ancestors`), and on the rank that lists it an `ExtensionNameValue`
//! [`ANCESTOR`] whose value is the division's URI. Not on the division itself:
//! its uuid is the same in every package, chosen or added, and an object saying
//! *not chosen* in one package and nothing in the other would be two objects
//! under one uuid — which a second import into the same package would rewrite
//! back and forth. The rank's uuid derives from the selection, **added parents
//! included**, so it is the one object for which *not chosen* is a stable fact.
//!
//! A parent the chart gives no age is refused on `ancestors`, by name, with the
//! way out — the rule of a chosen division, which is refused on `units`. None
//! on the snapshot of 24/09: its 698 divisions are all dated.
//!
//! `ancestors` off writes the branches alone, **still numbered by their
//! levels**; its uuids are then those of before 24/09, since nothing was added.

use energyml_domain::geolts::{self, Chart, Concept};
use energyml_domain::provenance::Parameter;
use energyml_eml_2_3 as eml;
use energyml_resqml_2_2 as resqml;
use uuid::Uuid;

use crate::declare::{ChartTerm, Choice, Params};
use crate::operation::{
    About, ActivityDraft, Category, Context, Need, NewObject, Note, Operation, Output, Plan, Refusal, Slot,
};

/// The operation.
pub struct StratigraphyFromGeolts;

/// The language the chart's divisions are titled in.
///
/// **Both are the BRGM's own words**: the GeolTs register states every
/// division in French and in English (`skos:prefLabel`, `'Upper Lutetian'@en |
/// 'Lutétien supérieur'@fr` — all 698 concepts of the 24/09 snapshot carry
/// both). The decision: what is taken from the BRGM may be in
/// French, like the names of the units, as it comes from the official stratigraphy,
/// unless an English stratigraphy of the BRGM can be reached — the
/// register is that English stratigraphy. French stays the default, the
/// register's first language; the other is kept as an alias, so a search by
/// either finds the division. The uuids come from the register's URI and do
/// not change with the language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Choice)]
pub enum Language {
    /// The register's French labels.
    Fr,
    /// The register's English labels.
    En,
}

impl Language {
    /// A division's title in this language — the other one when the register
    /// left this one empty, never a translation of ours.
    pub(crate) fn of(self, one: &Concept) -> String {
        match self {
            Language::En if !one.label_en.is_empty() => one.label_en.clone(),
            _ => one.label.clone(),
        }
    }

    /// Its title in the other language, and the alias authority that says which.
    fn other(self, one: &Concept) -> (&'static str, String) {
        match self {
            Language::Fr => ("BRGM label (en)", one.label_en.clone()),
            Language::En => ("BRGM label (fr)", one.label.clone()),
        }
    }
}

/// What it is told.
#[derive(Debug, Params)]
pub struct StratigraphyParams {
    /// the divisions of the BRGM chart to import, each with everything below it
    #[param(least = 1)]
    pub units: Vec<ChartTerm>,
    /// also the chain of divisions above them — not their other children — so each keeps the rank of its level
    ///
    /// On by default since 24/09, and answering to `regional` and `finest` like
    /// the branches do; see the module. Off, the branches are written alone.
    #[param(default = true, optional)]
    pub ancestors: bool,
    /// also the regional, usual, obsolete and informal divisions, and the retired ones
    #[param(default = false, optional)]
    pub regional: bool,
    /// the finest level taken: 4 system, 5 series, 6 sub-series, 7 stage, 8 substage
    #[param(default = 7usize, optional, min = 1.0, max = 10.0)]
    pub finest: usize,
    /// the name of the column; left out, the chart's and the branches'
    pub title: Option<String>,
    /// the language the divisions are titled in — both are the BRGM register's own labels
    #[param(default = Language::Fr, optional)]
    pub language: Language,
}

/// What a column of the chart writes, by role. Reused by
/// `wells-and-properties-from-table`, whose `units` writes one over the
/// divisions its passes name (through [`write_column`]).
///
/// **Two kinds of object share `feature`**: the `Model` (the organisation the
/// ranks interpret) and one `RockVolumeFeature` per division.
pub(crate) fn outputs() -> Vec<Output> {
    vec![
        Output {
            role: "feature",
            types: &["resqml22.Model", "resqml22.RockVolumeFeature"],
            least: 1,
            most: None,
            describe: "the organisation (a Model) and one rock volume feature per division of the chart",
        },
        Output {
            role: "interpretation",
            types: &["resqml22.StratigraphicUnitInterpretation"],
            least: 1,
            most: None,
            describe: "one stratigraphic unit interpretation per division, dated from the chart",
        },
        Output {
            role: "rank",
            types: &["resqml22.StratigraphicColumnRankInterpretation"],
            least: 1,
            most: None,
            describe: "one rank per level of the chart the column holds, its units youngest first",
        },
        Output {
            role: "column",
            types: &["resqml22.StratigraphicColumn"],
            least: 1,
            most: Some(1),
            describe: "the stratigraphic column, listing the ranks",
        },
        Output {
            role: "colours",
            types: &["eml23.GraphicalInformationSet"],
            least: 0,
            most: Some(1),
            describe: "the chart's colours of the divisions, when it states any",
        },
    ]
}

impl Operation for StratigraphyFromGeolts {
    type Params = StratigraphyParams;
    type Inputs = ();

    fn gives(&self) -> Vec<Output> {
        outputs()
    }

    fn about(&self) -> About {
        About {
            name: "stratigraphy-from-geolts",
            describe: "a stratigraphic column from the BRGM chronostratigraphic chart (GeolTs), whole or by branches — takes no input",
            category: Category::Import,
            makes: &[
                "resqml22.Model",
                "resqml22.StratigraphicColumn",
                "resqml22.StratigraphicColumnRankInterpretation",
                "resqml22.StratigraphicUnitInterpretation",
                "resqml22.RockVolumeFeature",
            ],
            recipe: "stratigraphy-from-geolts",
            represents: None,
        }
    }

    /// What the run reports: how many divisions it wrote.
    fn reports(&self) -> Vec<Slot> {
        vec![Slot::optional(
            "unitCount",
            Need::Number {
                uom: None,
                least: None,
                most: None,
            },
            "divisions of the chart written",
        )]
    }

    fn plan(&self, _inputs: (), params: StratigraphyParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
        geolts::with_default(|chart| column(chart, &params, cx))
    }
}

/// The `ExtensionNameValue` a rank carries for each division it lists **only as
/// the parent of a chosen one**; its value is the division's URI in the
/// register. Named after this operation, not `geolts:` like the others: those
/// are the register's facts, this one is a fact of the import.
pub const ANCESTOR: &str = "stratigraphy-from-geolts:ancestor";

/// The divisions a selection takes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selected {
    /// Every division the column holds: the branches, the chart's order kept,
    /// then the parents they bring, nearest first.
    pub taken: Vec<u32>,
    /// The part of `taken` that is there **only** as the parent of a chosen
    /// division — not chosen itself, and said so.
    pub added: Vec<u32>,
}

/// Whether the settings take a division the chart holds — the one filter a
/// branch and a chain of parents share, so a parent is in the column exactly
/// when a larger branch holding it would have put it there.
fn admitted(one: &Concept, params: &StratigraphyParams) -> bool {
    (params.regional || one.is_reference()) && usize::from(one.level) <= params.finest
}

/// The divisions a selection takes: the branches, then — `ancestors` on — the
/// chain above each, **and nothing else of it**.
pub fn selection(chart: &Chart, params: &StratigraphyParams) -> Result<Selected, Refusal> {
    if chart.is_empty() {
        return Err(Refusal::new(
            "refused",
            format!(
                "{} holds no division — this build has no BRGM chart to draw a column from",
                chart.label()
            ),
        ));
    }
    let mut taken: Vec<u32> = Vec::new();
    let mut roots: Vec<u32> = Vec::new();
    let mut unknown: Vec<String> = Vec::new();
    for term in &params.units {
        match chart.resolve(&term.0) {
            Ok(root) => {
                roots.push(root.id);
                for id in chart.descendants(root.id) {
                    let Some(one) = chart.get(id) else { continue };
                    // The branch's own root is kept whatever it is: it was
                    // named, and dropping it would answer a question with
                    // everything but what was asked.
                    let wanted = id == root.id || admitted(one, params);
                    if wanted && !taken.contains(&id) {
                        taken.push(id);
                    }
                }
            }
            Err(why) => unknown.push(why),
        }
    }
    if !unknown.is_empty() {
        return Err(Refusal::on("units", unknown.join("; ")));
    }

    // **The parents once every branch is in**: a division that one branch holds
    // and another sits under was chosen, not added, and must not be marked so.
    let mut added: Vec<u32> = Vec::new();
    if params.ancestors {
        for root in &roots {
            for id in chart.ancestors(*root) {
                if taken.contains(&id) {
                    continue;
                }
                let Some(one) = chart.get(id) else { continue };
                // Passed over, never a stop: its own parent may still be one
                // the settings take.
                if admitted(one, params) {
                    taken.push(id);
                    added.push(id);
                }
            }
        }
    }

    // A parent the column cannot place is refused **where it came in**, and
    // the refusal says it was not asked for: sending somebody to fix the ages
    // of a division they never chose would be the wrong errand.
    let undated: Vec<String> = added
        .iter()
        .filter_map(|id| chart.get(*id))
        .filter(|one| one.begins_ma.is_none() && one.ends_ma.is_none())
        .map(|one| format!("{} ({})", one.label, one.uri()))
        .collect();
    if !undated.is_empty() {
        return Err(Refusal::on(
            "ancestors",
            format!(
                "{} parent(s) of the divisions chosen state no age in the chart, so where they sit in a column \
                 cannot be read: {} — they are there only as parents; `ancestors` off writes the branches alone",
                undated.len(),
                undated.join(", ")
            ),
        ));
    }
    Ok(Selected { taken, added })
}

/// A division the column holds only as the parent of chosen ones.
#[derive(Debug, Clone)]
pub(crate) struct Ancestor {
    /// Its id in the chart.
    pub(crate) id: u32,
    /// The labels of the chosen divisions it is above.
    pub(crate) above: Vec<String>,
}

/// The number of a level's rank: **the level less one**, so a division has the
/// same `RankInStratigraphicColumn` in every column — see the module. `None`
/// for a division the chart gives no level (0), which has no rank to be in.
fn rank_number(level: u8) -> Option<i64> {
    if level == 0 {
        None
    } else {
        Some(i64::from(level) - 1)
    }
}

/// The URL-namespace uuid of a text — computable by anyone, without this tool.
fn url_uuid(text: &str) -> String {
    Uuid::new_v5(&Uuid::NAMESPACE_URL, text.as_bytes()).to_string()
}

/// A reference to an object of the plan.
fn reference(uuid: &str, qualified_type: &str, title: &str) -> eml::DataObjectReference {
    eml::DataObjectReference {
        uuid: uuid.to_string(),
        qualified_type: qualified_type.to_string(),
        title: title.to_string(),
        ..Default::default()
    }
}

/// What the register says of a division beyond its name, for `Citation/Description`:
/// the reference it comes from, then its note. `None` when it says nothing.
fn description(one: &Concept, language: Language) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    let (_, other) = language.other(one);
    if !other.is_empty() && other != language.of(one) {
        parts.push(other);
    }
    if !one.source.is_empty() {
        parts.push(format!("source: {}", one.source));
    }
    if !one.note.is_empty() {
        parts.push(one.note.clone());
    }
    // `Citation/Description` is bounded by the schema (2 000 characters).
    (!parts.is_empty()).then(|| parts.join(" — ").chars().take(2_000).collect())
}

/// The register's facts that RESQML has no field for, as `ExtensionNameValue`
/// — the standard's own place for them, named after the register's predicate.
///
/// `Value` is a `StringMeasure`: simple content, **its text is the value**
/// (common v2.3 `BaseTypes.xsd`), which the JSON of a plan spells `_` and the
/// XML writes as the element's text — `tests/stratigraphy.rs` reads it back
/// from the saved bytes. A fact the register leaves empty is not written: an
/// extension naming nothing says nothing, and this library's own XML reader
/// takes an empty `<Value/>` for an absent one.
fn extensions(one: &Concept) -> Vec<eml::ExtensionNameValue> {
    let mut out = Vec::new();
    let mut add = |name: &str, value: String| {
        if value.trim().is_empty() {
            return;
        }
        out.push(eml::ExtensionNameValue {
            name: name.to_string(),
            value: eml::StringMeasure {
                value,
                ..Default::default()
            },
            ..Default::default()
        });
    };
    add("geolts:level", one.level_name());
    add("geolts:nature", one.nature_name().to_string());
    add("geolts:status", one.status.clone());
    if !one.spatial.is_empty() {
        add("geolts:spatial", one.spatial.clone());
    }
    if one.gssp {
        add("geolts:isGSSP", "true".to_string());
    }
    if one.gssa {
        add("geolts:isGSSA", "true".to_string());
    }
    out
}

/// Where a division comes from, said on each object.
fn aliases(one: &Concept, language: Language) -> Vec<eml::ObjectAlias> {
    let mut out = vec![eml::ObjectAlias {
        authority: "BRGM GeolTs".to_string(),
        identifier: one.uri(),
        description: Some(format!("{} — {}", language.of(one), one.level_name())),
        ..Default::default()
    }];
    for notation in &one.notations {
        out.push(eml::ObjectAlias {
            authority: "BRGM notation".to_string(),
            identifier: notation.chars().take(64).collect(),
            ..Default::default()
        });
    }
    if !one.mnemonic.is_empty() {
        out.push(eml::ObjectAlias {
            authority: "BRGM mnemonic".to_string(),
            identifier: one.mnemonic.clone(),
            ..Default::default()
        });
    }
    // Its other names — the other language's, and the register's synonyms — so
    // a search by any of them finds it. `Identifier` is bounded at 64.
    let other = language.other(one);
    for (authority, name) in std::iter::once((other.0, &other.1))
        .filter(|(_, name)| !name.is_empty())
        .chain(one.alt_labels.iter().map(|name| ("BRGM altLabel", name)))
    {
        out.push(eml::ObjectAlias {
            authority: authority.to_string(),
            identifier: name.chars().take(64).collect(),
            ..Default::default()
        });
    }
    out
}

/// The age span, as eml 2.3 states it: the start is the **old** end.
fn occurred_during(one: &Concept) -> Option<resqml::AbstractTimeInterval> {
    let (old, young) = (one.begins_ma?, one.ends_ma?);
    let at = |ma: f64| eml::GeologicTime {
        date_time: crate::build::AGE_REFERENCE.to_string(),
        age_offset_attribute: Some((ma * 1_000_000.0).round() as i64),
    };
    Some(resqml::AbstractTimeInterval::GeologicTimeBasedTimeInterval(
        resqml::GeologicTimeBasedTimeInterval {
            start: at(old),
            end: at(young),
        },
    ))
}

/// The column of a selection.
fn column(chart: &Chart, params: &StratigraphyParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
    let selected = selection(chart, params)?;
    let chosen: Vec<&Concept> = params
        .units
        .iter()
        .filter_map(|term| chart.resolve(&term.0).ok())
        .collect();
    let roots: Vec<String> = chosen.iter().map(|one| params.language.of(one)).collect();
    let title = params
        .title
        .clone()
        .unwrap_or_else(|| format!("BRGM chronostratigraphy — {}", roots.join(", ")));
    // What each added parent sits above: the chosen divisions whose chain holds it.
    let ancestors: Vec<Ancestor> = selected
        .added
        .iter()
        .map(|id| Ancestor {
            id: *id,
            above: chosen
                .iter()
                .filter(|one| chart.ancestors(one.id).contains(id))
                .map(|one| params.language.of(one))
                .collect(),
        })
        .collect();
    let written = write_column_with(chart, &selected.taken, &ancestors, &title, params.language, cx)?;
    let mut says = vec![Note {
        key: "units",
        detail: format!(
            "{} division(s) of the chart in {} rank(s): {}",
            written.units,
            written.ranks.len(),
            written
                .ranks
                .iter()
                .map(|rank| format!("#{} {}", rank.number, geolts::level_name(rank.level)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }];
    if !ancestors.is_empty() {
        says.push(Note {
            key: "ancestors",
            detail: format!(
                "{} division(s) not chosen are in the column as the parents of those that were, so each keeps the \
                 rank of its level: {} — each marked `{ANCESTOR}` on its rank; `ancestors` off leaves them out",
                ancestors.len(),
                ancestors
                    .iter()
                    .filter_map(|one| chart.get(one.id))
                    .map(|one| format!("{} ({})", one.label, one.level_name()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        });
    }
    // **A gap in the numbers is a level the column holds nothing of**, and it
    // is said: a rank that jumps from #4 to #6 would otherwise read as a rank
    // lost on the way.
    let highest = written.ranks.iter().map(|rank| rank.number).max().unwrap_or(-1);
    let absent: Vec<String> = (0..=highest)
        .filter(|number| !written.ranks.iter().any(|rank| rank.number == *number))
        .map(|number| format!("#{number} ({})", geolts::level_name((number + 1) as u8)))
        .collect();
    if !absent.is_empty() {
        says.push(Note {
            key: "ranks-absent",
            detail: format!(
                "no rank {}: the column holds no division of these levels, and a rank lists at least one unit \
                 (RESQML 2.2 §6.45) — the other ranks keep the numbers of their levels",
                absent.join(", ")
            ),
        });
    }
    says.push(Note {
        key: "source",
        detail: format!(
            "{} — https://data.geoscience.fr/ncl/GeolTs, CC BY 4.0",
            chart.label()
        ),
    });
    Ok(Plan {
        media: Vec::new(),
        blocked: None,
        objects: written.objects,
        datasets: Vec::new(),
        shapes: Vec::new(),
        activity: ActivityDraft {
            recipe: "stratigraphy-from-geolts",
            title: format!("stratigraphy-from-geolts → {}", roots.join(", ")),
            seed: written.key,
            parameters: vec![Parameter::Double {
                title: "unitCount".to_string(),
                value: written.units as f64,
                uom: None,
            }],
        },
        says,
        dropped: Vec::new(),
    })
}

/// A rank the column holds: its level, its object, its units youngest first.
#[derive(Debug, Clone)]
pub(crate) struct RankMade {
    pub(crate) level: u8,
    /// Its `RankInStratigraphicColumn`: the level less one.
    pub(crate) number: i64,
    pub(crate) reference: eml::DataObjectReference,
    /// The chart ids of its units, **in the order the rank lists them** — the
    /// order an index into `StratigraphicUnits` counts in.
    pub(crate) units: Vec<u32>,
}

/// A written column.
pub(crate) struct ColumnMade {
    pub(crate) objects: Vec<NewObject>,
    pub(crate) ranks: Vec<RankMade>,
    pub(crate) units: usize,
    /// The selection, as the uuids of the organisation derive from it.
    pub(crate) key: String,
}

/// The objects of a column over **exactly** these divisions: one rank per
/// level, coarsest first, units youngest first.
///
/// `stratigraphy-from-geolts` calls it with a branch; an import of boreholes
/// with the divisions its passes name. The units' uuids come from their URI
/// alone, so the two columns share the Priabonien rather than holding two —
/// and their ranks are numbered by level alike, so the Priabonien is in rank 6
/// in both.
pub(crate) fn write_column(
    chart: &Chart,
    taken: &[u32],
    title: &str,
    language: Language,
    cx: &Context<'_>,
) -> Result<ColumnMade, Refusal> {
    write_column_with(chart, taken, &[], title, language, cx)
}

/// [`write_column`], with the divisions that are there only as parents of
/// chosen ones — each marked on its rank, and part of the selection's identity.
pub(crate) fn write_column_with(
    chart: &Chart,
    taken: &[u32],
    ancestors: &[Ancestor],
    title: &str,
    language: Language,
    cx: &Context<'_>,
) -> Result<ColumnMade, Refusal> {
    if let crate::operation::Identity::Given(_) = cx.identity {
        return Err(Refusal::new(
            "refused",
            "a column of the BRGM chart names each division from its URI in the register, so the same stage is the same object in every package; it does not take given uuids",
        ));
    }
    let citation = |title: &str| eml::Citation {
        title: title.to_string(),
        originator: cx.when.originator.clone(),
        creation: cx.when.creation.clone(),
        format: energyml_core::CITATION_FORMAT.to_string(),
        ..Default::default()
    };

    // A rank is its level's, so a division with no level has none to be in —
    // refused by name rather than filed under a number it does not have.
    let levelless: Vec<String> = taken
        .iter()
        .filter_map(|id| chart.get(*id))
        .filter(|one| rank_number(one.level).is_none())
        .map(|one| format!("{} ({})", language.of(one), one.uri()))
        .collect();
    if !levelless.is_empty() {
        return Err(Refusal::on(
            "units",
            format!(
                "{} division(s) state no level in the chart, so which rank of a column they belong in cannot be read: {}",
                levelless.len(),
                levelless.join(", ")
            ),
        ));
    }

    // The ranks: one per level present, coarsest first.
    let mut levels: Vec<u8> = taken
        .iter()
        .filter_map(|id| chart.get(*id))
        .map(|one| one.level)
        .collect();
    levels.sort_unstable();
    levels.dedup();

    let mut sorted_ids = taken.to_vec();
    sorted_ids.sort_unstable();
    let mut selection_key = sorted_ids
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    // **Which of them were added is part of the selection.** Two runs can hold
    // the same divisions and differ in what was chosen — a parent with a
    // single child, chosen, or brought by that child — and their ranks say
    // different things. Nothing added, nothing appended: the uuids of a column
    // without parents are those of before 24/09.
    if !ancestors.is_empty() {
        let mut added: Vec<u32> = ancestors.iter().map(|one| one.id).collect();
        added.sort_unstable();
        selection_key.push_str(";added=");
        selection_key.push_str(&added.iter().map(u32::to_string).collect::<Vec<_>>().join(","));
    }

    let mut objects: Vec<NewObject> = Vec::new();
    let model_uuid = url_uuid(&format!("{}?selection={selection_key}#model", geolts::REGISTER));
    let model = resqml::Model {
        uuid: model_uuid.clone(),
        schema_version: "2.2".to_string(),
        citation: citation(title),
        is_well_known: false,
        ..Default::default()
    };
    objects.push(NewObject {
        uuid: model_uuid.clone(),
        qualified_type: "resqml22.Model".to_string(),
        role: "feature".to_string(),
        title: title.to_string(),
        json: energyml_core::json::write_value(&model),
    });
    let model_ref = reference(&model_uuid, "resqml22.Model", title);

    let mut ranks: Vec<RankMade> = Vec::new();
    let mut units_written = 0usize;
    // The chart's colour of each division, and what it decorates.
    let mut painted: Vec<(Vec<energyml_domain::color::Decorated>, [u8; 3], String)> = Vec::new();
    for level in &levels {
        // Checked above: every level here is 1 or more.
        let number = rank_number(*level).unwrap_or_default();
        let ids: Vec<u32> = taken
            .iter()
            .copied()
            .filter(|id| chart.get(*id).is_some_and(|one| one.level == *level))
            .collect();
        let ordered = chart.by_age(&ids).map_err(|why| Refusal::on("units", why))?;
        let mut units: Vec<eml::DataObjectReference> = Vec::new();
        let mut order: Vec<u32> = Vec::new();
        for one in ordered {
            let uri = one.uri();
            let name = language.of(one);
            let described = |mut cited: eml::Citation| {
                cited.description = description(one, language);
                cited
            };
            let feature = resqml::RockVolumeFeature {
                uuid: url_uuid(&uri),
                schema_version: "2.2".to_string(),
                citation: described(citation(&name)),
                aliases: aliases(one, language),
                extension_name_value: extensions(one),
                // `IsWellKnown` is the standard's own list of bodies; a BRGM
                // division is not on it — the same reasoning as `build.rs`.
                is_well_known: false,
                ..Default::default()
            };
            let interpretation = resqml::StratigraphicUnitInterpretation {
                uuid: url_uuid(&format!("{uri}#interpretation")),
                schema_version: "2.2".to_string(),
                citation: described(citation(&name)),
                aliases: aliases(one, language),
                extension_name_value: extensions(one),
                has_occurred_during: occurred_during(one),
                interpreted_feature: reference(&feature.uuid, "resqml22.RockVolumeFeature", &name),
                // The chart is a chronostratigraphy: its divisions are units
                // of time, which is what this role says.
                stratigraphic_role: Some(resqml::StratigraphicRole::Chronostratigraphic),
                ..Default::default()
            };
            units.push(reference(
                &interpretation.uuid,
                "resqml22.StratigraphicUnitInterpretation",
                &name,
            ));
            order.push(one.id);
            if let Some(colour) = one.colour {
                painted.push((
                    vec![
                        energyml_domain::color::Decorated {
                            uuid: interpretation.uuid.clone(),
                            qualified_type: "resqml22.StratigraphicUnitInterpretation".to_string(),
                            title: name.clone(),
                        },
                        energyml_domain::color::Decorated {
                            uuid: feature.uuid.clone(),
                            qualified_type: "resqml22.RockVolumeFeature".to_string(),
                            title: name.clone(),
                        },
                    ],
                    colour,
                    name.clone(),
                ));
            }
            objects.push(NewObject {
                uuid: feature.uuid.clone(),
                qualified_type: "resqml22.RockVolumeFeature".to_string(),
                role: "feature".to_string(),
                title: name.clone(),
                json: energyml_core::json::write_value(&feature),
            });
            objects.push(NewObject {
                uuid: interpretation.uuid.clone(),
                qualified_type: "resqml22.StratigraphicUnitInterpretation".to_string(),
                role: "interpretation".to_string(),
                title: name.clone(),
                json: energyml_core::json::write_value(&interpretation),
            });
            units_written += 1;
        }
        let rank_title = format!("{title} — {}", geolts::level_name(*level));
        let rank_uuid = url_uuid(&format!(
            "{}?selection={selection_key}#rank-{level}",
            geolts::REGISTER
        ));
        // The parents this rank lists without their being chosen, in the order
        // the rank lists them. `Index` numbers the entries of one name, as the
        // schema's own documentation of the field asks.
        let marks: Vec<eml::ExtensionNameValue> = order
            .iter()
            .filter_map(|id| ancestors.iter().find(|one| one.id == *id))
            .filter_map(|added| chart.get(added.id).map(|one| (added, one)))
            .enumerate()
            .map(|(at, (added, one))| eml::ExtensionNameValue {
                name: ANCESTOR.to_string(),
                value: eml::StringMeasure {
                    value: one.uri(),
                    ..Default::default()
                },
                index: Some(at as i64 + 1),
                description: Some(
                    format!(
                        "{} ({}), not chosen: above {}, so each division chosen keeps the rank of its level",
                        language.of(one),
                        one.level_name(),
                        added.above.join(", ")
                    )
                    .chars()
                    .take(2_000)
                    .collect(),
                ),
                ..Default::default()
            })
            .collect();
        let rank = resqml::StratigraphicColumnRankInterpretation {
            uuid: rank_uuid.clone(),
            schema_version: "2.2".to_string(),
            citation: citation(&rank_title),
            extension_name_value: marks,
            interpreted_feature: model_ref.clone(),
            ascending_ordering_criteria: resqml::OrderingCriteria::Age,
            rank_in_stratigraphic_column: number,
            stratigraphic_units: units,
            ..Default::default()
        };
        ranks.push(RankMade {
            level: *level,
            number,
            reference: reference(
                &rank_uuid,
                "resqml22.StratigraphicColumnRankInterpretation",
                &rank_title,
            ),
            units: order,
        });
        objects.push(NewObject {
            uuid: rank_uuid,
            qualified_type: "resqml22.StratigraphicColumnRankInterpretation".to_string(),
            role: "rank".to_string(),
            title: rank_title,
            json: energyml_core::json::write_value(&rank),
        });
    }

    let column_uuid = url_uuid(&format!("{}?selection={selection_key}#column", geolts::REGISTER));
    let column = resqml::StratigraphicColumn {
        uuid: column_uuid.clone(),
        schema_version: "2.2".to_string(),
        citation: citation(title),
        ranks: ranks.iter().map(|rank| rank.reference.clone()).collect(),
        ..Default::default()
    };
    objects.push(NewObject {
        uuid: column_uuid,
        qualified_type: "resqml22.StratigraphicColumn".to_string(),
        role: "column".to_string(),
        title: title.to_string(),
        json: energyml_core::json::write_value(&column),
    });
    // **The chart's own colours**, one `DefaultGraphicalInformation` per
    // division, on its interpretation **and** its feature — the cascade reads
    // the unit first, then the rock it interprets (§4.21), so a view that
    // stops at either finds it. A set of its own, derived from the selection:
    // these colours are a statement of the register, not a choice somebody
    // made in the package, and they must not end up mixed into one.
    if !painted.is_empty() {
        let set_uuid = url_uuid(&format!("{}?selection={selection_key}#colours", geolts::REGISTER));
        let set_title = format!("{title} — BRGM chart colours");
        let set = eml::GraphicalInformationSet {
            uuid: set_uuid.clone(),
            schema_version: "2.3".to_string(),
            citation: citation(&set_title),
            graphical_information: painted
                .iter()
                .map(|(targets, [r, g, b], name)| {
                    energyml_domain::color::constant_colour_entry(
                        targets,
                        energyml_domain::color::Scope::WholeObject,
                        energyml_domain::color::Rgba {
                            r: f64::from(*r) / 255.0,
                            g: f64::from(*g) / 255.0,
                            b: f64::from(*b) / 255.0,
                            a: 1.0,
                        },
                        Some(name),
                    )
                })
                .collect(),
            ..Default::default()
        };
        objects.push(NewObject {
            uuid: set_uuid,
            qualified_type: "eml23.GraphicalInformationSet".to_string(),
            role: "colours".to_string(),
            title: set_title,
            json: energyml_core::json::write_value(&set),
        });
    }
    Ok(ColumnMade {
        objects,
        ranks,
        units: units_written,
        key: selection_key,
    })
}
