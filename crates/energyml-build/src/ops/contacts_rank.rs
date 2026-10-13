//! Contacts **at a rank** of the BRGM chart — how `contacts-from-frames` reads
//! its frames when it is told `rank`, `unit`, or both.
//!
//! The synthetic reference model could not get two of its horizons out of the frames:
//! `contacts-from-frames` returns one cloud per **named** unit, not per unit of
//! a rank of the column, and gathering the clouds by title would mix internal
//! limits of the Lutetian. A frame names the division each pass was
//! **observed** as, and a horizon is often the limit of a **group** of them:
//! the base of the Lutetian is the base of « Lutétien », « Lutétien moyen » or
//! « Lutétien inférieur » depending on the borehole, and the limit between
//! « Lutétien supérieur » and « Lutétien » (NTCE, 20.4 m) is no limit of the
//! stage.
//!
//! # Only the unit of each interval changes
//!
//! A reading turns the units a frame names into **sequences of the same
//! length**, and `contacts-from-frames` finds its contacts in them exactly as
//! it finds them in the frame's own: a contact is a node, found by its rank
//! before any depth is read; the top takes the unit's first interval, the base
//! its last; a unit that comes back lower in one hole is refused by name; the
//! copies of the wells' properties come after every point set. Nothing of that
//! is written twice.
//!
//! # How an interval is read
//!
//! - **its unit, to a division of the chart, by uuid**: the uuid a column of the
//!   chart writes for the division's interpretation — v5 in the URL namespace
//!   over `<URI>#interpretation` (`ops/stratigraphy.rs`, module doc), the one
//!   `wells-and-properties-from-table` gives the units of its frames. A unit
//!   that is no division of the chart is read as **no unit**, counted and named
//!   (`rank-foreign`), never matched by its title — a title is not an identity;
//! - **against a division of the rank**, by `energyml_domain::geolts_rank`:
//!   *inside* it — the chart places its division under it —, *outside* it — the
//!   chart's ages do not overlap —, or *neither*: a coarser division that holds
//!   it (the Middle Eocene for the Lutetian), a regional one whose time overlaps
//!   (the Ludien beside the Priabonien), no age. *Neither* is read as no unit,
//!   so **no contact is read across it**, and it is counted (`rank-unknown`);
//! - **which divisions**: `unit` names one. A `rank` alone reads every division
//!   of that rank the frames reach, a finer one being read as **the coarsest
//!   division of that rank above it** — the Cuisien, a regional stage under the
//!   Ypresian, is the Ypresian. One sequence per division read.
//!
//! What lies outside the division read is its **neighbour**: it makes the
//! division's top and base contacts, and gets none of its own — outside the
//! Lutetian is not a unit, and the Quaternary above and the Ypresian below are
//! two different things it cannot stand for.
//!
//! # Refused before any frame is read
//!
//! A chart this build does not have, a `unit` it does not know, a `rank` it has
//! no division of, a `unit` that is not of the `rank` given: each is the
//! setting's fault and refused on it, by name.
//!
//! # Identity, and a title that says the rank
//!
//! A unit read at a rank is titled `<label> (<level>)` — `Lutétien (stage)` —
//! and its seeds end with `:rank-<n>` ([`Reading::seed`]): the base of the stage
//! and the base of the passes named « Lutétien » are two findings, and two
//! objects, even where their points coincide. Named, the seeds are those of
//! before. `unit` alone and `rank` alone read the Lutetian alike, and write the
//! same objects for it.

use super::stratigraphy::Language;
use std::collections::{BTreeMap, HashMap};

use energyml_domain::geolts::{self, Chart, Concept};
use energyml_domain::geolts_rank::{self, Relation};
use uuid::Uuid;

use crate::declare::ChartTerm;
use crate::operation::{Note, Refusal};
use crate::pieces::Ref;

/// The type of a unit a frame names, and of a division read in its place.
const UNIT: &str = "resqml22.StratigraphicUnitInterpretation";

/// What an interval outside the division read stands as in a sequence: its
/// neighbour. Never written, never referenced — see [`Reading::keeps`].
const OUTSIDE: &str = "outside:";

/// How the frames' intervals are read.
pub enum Reading {
    /// Unit by unit, as the frames name them — neither `rank` nor `unit`.
    Named,
    /// Against divisions of one rank of the BRGM chart.
    AtRank(Box<AtRank>),
}

impl Reading {
    /// The reading the settings ask for, against the chart in use
    /// (`energyml_domain::geolts`) — or the refusal, on the setting at fault.
    pub fn of(rank: Option<usize>, unit: Option<&ChartTerm>, language: Language) -> Result<Reading, Refusal> {
        if rank.is_none() && unit.is_none() {
            return Ok(Reading::Named);
        }
        geolts::with_default(|chart| AtRank::of(chart, rank, unit, language)).map(|at| Reading::AtRank(Box::new(at)))
    }

    /// The sequences of units one frame is read as, **each as long as
    /// `named`**: the frame's own when named; at a rank, one per division read.
    pub fn sequences(&mut self, named: &[Option<Ref>], frame: &str) -> Vec<Vec<Option<Ref>>> {
        match self {
            Reading::Named => vec![named.to_vec()],
            Reading::AtRank(at) => at.sequences(named, frame),
        }
    }

    /// Whether a unit of a sequence has its top and base written, and is
    /// refused when it comes back: every unit when named; at a rank, the
    /// division read, never its neighbour.
    pub fn keeps(&self, unit: &Ref) -> bool {
        match self {
            Reading::Named => true,
            Reading::AtRank(_) => !unit.uuid.starts_with(OUTSIDE),
        }
    }

    /// What the seeds of a contact's feature, interpretation and point set —
    /// and of the run's activity — end with: nothing when named, so the uuids
    /// are those of before; `:rank-<n>` at a rank.
    pub fn seed(&self) -> String {
        match self {
            Reading::Named => String::new(),
            Reading::AtRank(at) => format!(":rank-{}", at.rank),
        }
    }

    /// What the plan says of the reading — nothing when named.
    pub fn says(&self) -> Vec<Note> {
        match self {
            Reading::Named => Vec::new(),
            Reading::AtRank(at) => at.says(),
        }
    }

    /// The refusal when no contact was found — `None` when named, whose
    /// refusal is the operation's own.
    pub fn nothing(&self, frames: usize, unmapped: usize) -> Option<Refusal> {
        match self {
            Reading::Named => None,
            Reading::AtRank(at) => Some(at.nothing(frames, unmapped)),
        }
    }
}

/// A reading at one rank of the chart — see the module.
pub struct AtRank {
    /// The chart in use when the plan began, for the whole plan.
    chart: Chart,
    /// `RankInStratigraphicColumn`: the chart's level less one.
    rank: usize,
    /// The chart's level.
    level: u8,
    /// The one division `unit` names, when it names one.
    unit: Option<u32>,
    /// The chart's divisions, by the lower-cased uuid of their interpretation.
    by_uuid: HashMap<String, u32>,
    /// What the frames read so far gave.
    tally: Tally,
    /// The language a division read is titled in — the one the chart's units
    /// were written in (`stratigraphy-from-geolts`, `wells-and-properties-from-
    /// table`): a reference whose cached title is not its target's would be
    /// one more place for them to disagree, and the contacts are named after it.
    language: Language,
}

/// What reading the frames gave, for the plan to say.
#[derive(Default)]
struct Tally {
    /// Frames read.
    frames: usize,
    /// By division read: the frames with an interval inside it.
    reached: BTreeMap<u32, usize>,
    /// The frames with no interval inside any division read — the one `unit`
    /// names, or, a rank alone, any of that rank.
    absent: Vec<String>,
    /// Intervals neither inside nor outside a division read, by the division
    /// and the reason, then by the label of the interval's own division.
    unknown: BTreeMap<(u32, Relation), BTreeMap<String, usize>>,
    /// Intervals whose unit is no division of the chart, by the unit's title.
    foreign: BTreeMap<String, usize>,
    /// Intervals under two divisions of the rank, neither holding the other —
    /// a rank alone, which will not choose — by label.
    ambiguous: BTreeMap<String, usize>,
}

impl AtRank {
    /// A reading of `chart` at `rank` — of `unit` alone when one is named, at
    /// its own rank when `rank` is left out.
    pub fn of(chart: &Chart, rank: Option<usize>, unit: Option<&ChartTerm>, language: Language) -> Result<AtRank, Refusal> {
        let slot = if unit.is_some() { "unit" } else { "rank" };
        if chart.is_empty() {
            return Err(Refusal::on(
                slot,
                format!("{} holds no division — this build has no BRGM chart to read the frames against", chart.label()),
            ));
        }
        let asked: Option<&Concept> =
            unit.map(|term| chart.resolve(&term.0).map_err(|why| Refusal::on("unit", why))).transpose()?;
        if let Some(one) = asked
            && one.level == 0
        {
            return Err(Refusal::on(
                "unit",
                format!("`{}` ({}) states no level in the chart, so it is at no rank of a column", one.label, one.uri()),
            ));
        }
        let level = match (rank, asked) {
            (Some(rank), _) => {
                let level = rank
                    .checked_add(1)
                    .and_then(|level| u8::try_from(level).ok())
                    .filter(|level| geolts_rank::levels(chart).contains(level));
                let Some(level) = level else {
                    return Err(Refusal::on(
                        "rank",
                        format!(
                            "{} has no division at rank {rank} — a rank is the chart's level less one, and its ranks are {}",
                            chart.label(),
                            ranks_of(chart)
                        ),
                    ));
                };
                if let Some(one) = asked
                    && one.level != level
                {
                    return Err(Refusal::on(
                        "unit",
                        format!(
                            "`{}` is a {}, at rank {} — not a division of rank {rank}; left out, `rank` is the unit's own",
                            one.label,
                            one.level_name(),
                            usize::from(one.level) - 1
                        ),
                    ));
                }
                level
            }
            (None, Some(one)) => one.level,
            (None, None) => {
                return Err(Refusal::on("rank", "a reading at a rank of the chart needs `rank`, `unit`, or both"));
            }
        };
        let by_uuid = chart.concepts().iter().map(|one| (interpretation_uuid(one), one.id)).collect();
        Ok(AtRank {
            chart: chart.clone(),
            rank: usize::from(level) - 1,
            level,
            unit: asked.map(|one| one.id),
            by_uuid,
            tally: Tally::default(),
            language,
        })
    }

    /// The unit a division read stands as in a sequence: the uuid of its
    /// interpretation — the one a column of the chart writes — and a title
    /// that says the level.
    fn unit_ref(&self, id: u32) -> Ref {
        match self.chart.get(id) {
            Some(one) => Ref::new(interpretation_uuid(one), UNIT, title_of(one, self.language)),
            None => Ref::new(format!("geolts:{id}"), UNIT, format!("GeolTs {id}")),
        }
    }

    /// The label of a division, for a note.
    fn label(&self, id: u32) -> String {
        self.chart.get(id).map(|one| one.label.clone()).unwrap_or_else(|| format!("GeolTs {id}"))
    }

    /// The title of a division read, for a note.
    fn title(&self, id: u32) -> String {
        self.chart.get(id).map(|one| title_of(one, self.language)).unwrap_or_else(|| format!("GeolTs {id}"))
    }

    /// One frame's sequences — see [`Reading::sequences`].
    fn sequences(&mut self, named: &[Option<Ref>], frame: &str) -> Vec<Vec<Option<Ref>>> {
        self.tally.frames += 1;
        // Each interval's division, when its unit is one of the chart.
        let mut divisions: Vec<Option<u32>> = Vec::with_capacity(named.len());
        for one in named {
            let Some(unit) = one else {
                divisions.push(None);
                continue;
            };
            let found = self.by_uuid.get(&unit.uuid.to_ascii_lowercase()).copied();
            if found.is_none() {
                *self.tally.foreign.entry(unit.title.clone()).or_default() += 1;
            }
            divisions.push(found);
        }
        // The divisions this frame is read against.
        let read: Vec<u32> = match self.unit {
            Some(unit) => vec![unit],
            None => {
                let mut out: Vec<u32> = Vec::new();
                for division in divisions.iter().flatten() {
                    match geolts_rank::at_level(&self.chart, *division, self.level).as_slice() {
                        // Coarser than the rank, or a branch the register numbers
                        // past it: no division of the rank to read it as.
                        [] => {}
                        [single] => {
                            if !out.contains(single) {
                                out.push(*single);
                            }
                        }
                        _ => {
                            let label = self.label(*division);
                            *self.tally.ambiguous.entry(label).or_default() += 1;
                        }
                    }
                }
                out
            }
        };
        let mut out: Vec<Vec<Option<Ref>>> = Vec::with_capacity(read.len());
        for unit in read {
            let inside = self.unit_ref(unit);
            let outside = Ref::new(format!("{OUTSIDE}{}", inside.uuid), "", format!("outside {}", inside.title));
            let mut reached = false;
            let mut sequence: Vec<Option<Ref>> = Vec::with_capacity(divisions.len());
            for division in &divisions {
                let Some(division) = division else {
                    sequence.push(None);
                    continue;
                };
                match geolts_rank::relation(&self.chart, *division, unit) {
                    Relation::Inside => {
                        reached = true;
                        sequence.push(Some(inside.clone()));
                    }
                    Relation::Outside => sequence.push(Some(outside.clone())),
                    other => {
                        let label = self.label(*division);
                        *self.tally.unknown.entry((unit, other)).or_default().entry(label).or_default() += 1;
                        sequence.push(None);
                    }
                }
            }
            if reached {
                *self.tally.reached.entry(unit).or_default() += 1;
                out.push(sequence);
            }
        }
        if out.is_empty() {
            self.tally.absent.push(frame.to_string());
        }
        out
    }

    /// Who was read: the unit asked, or every division of the rank.
    fn whom(&self) -> String {
        match self.unit {
            Some(unit) => format!("`{}`", self.title(unit)),
            None => format!("any division of rank {} ({})", self.rank, geolts::level_name(self.level)),
        }
    }

    /// See [`Reading::says`].
    fn says(&self) -> Vec<Note> {
        let tally = &self.tally;
        let mut out = vec![Note {
            key: "rank",
            detail: format!(
                "read at rank {} of the BRGM chart ({}), {} — each interval inside a division, outside it by the chart's ages, or neither, and a contact only between inside and outside; {}",
                self.rank,
                geolts::level_name(self.level),
                match self.unit {
                    Some(unit) => format!("`{}` alone", self.title(unit)),
                    None => "every division of that rank the frames reach".to_string(),
                },
                self.chart.label()
            ),
        }];
        if !tally.reached.is_empty() {
            out.push(Note {
                key: "rank-units",
                detail: format!(
                    "of {} frame(s) read: {}",
                    tally.frames,
                    tally
                        .reached
                        .iter()
                        .map(|(unit, frames)| format!("`{}` in {frames}", self.title(*unit)))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            });
        }
        if !tally.absent.is_empty() {
            out.push(Note {
                key: "rank-absent",
                detail: format!(
                    "{} frame(s) have no interval inside {}, and give no point: {}",
                    tally.absent.len(),
                    self.whom(),
                    few(&tally.absent)
                ),
            });
        }
        if !tally.unknown.is_empty() {
            let total: usize = tally.unknown.values().flat_map(|labels| labels.values()).sum();
            let parts: Vec<String> = tally
                .unknown
                .iter()
                .map(|((unit, relation), labels)| {
                    format!(
                        "against `{}`, {} — {}",
                        self.title(*unit),
                        labels.iter().map(|(label, count)| format!("{label} × {count}")).collect::<Vec<_>>().join(", "),
                        relation.reason()
                    )
                })
                .collect();
            out.push(Note {
                key: "rank-unknown",
                detail: format!(
                    "{total} interval(s) read as neither inside nor outside, so no contact is read across them: {}",
                    parts.join("; ")
                ),
            });
        }
        if !tally.foreign.is_empty() {
            out.push(Note {
                key: "rank-foreign",
                detail: format!(
                    "{} interval(s) name a unit that is no division of the BRGM chart, and are read as no unit: {}",
                    tally.foreign.values().sum::<usize>(),
                    counted(&tally.foreign)
                ),
            });
        }
        if !tally.ambiguous.is_empty() {
            out.push(Note {
                key: "rank-ambiguous",
                detail: format!(
                    "{} interval(s) lie under two divisions of rank {} that do not hold each other, and read as neither: {}",
                    tally.ambiguous.values().sum::<usize>(),
                    self.rank,
                    counted(&tally.ambiguous)
                ),
            });
        }
        out
    }

    /// See [`Reading::nothing`].
    fn nothing(&self, frames: usize, unmapped: usize) -> Refusal {
        let tally = &self.tally;
        let unknown: usize = tally.unknown.values().flat_map(|labels| labels.values()).sum();
        Refusal::on(
            if self.unit.is_some() { "unit" } else { "rank" },
            format!(
                "no top and no base of {} in the {frames} frame(s) given: {} have an interval inside it, {} have none, {unmapped} map no interval to a unit — a contact is a limit between an interval inside and one outside, and {unknown} interval(s) were read as neither",
                self.whom(),
                tally.frames - tally.absent.len(),
                tally.absent.len()
            ),
        )
    }
}

/// The uuid a column of the chart gives a division's interpretation: v5 in
/// the URL namespace over its URI and `#interpretation` — `ops/stratigraphy.rs`,
/// *« which anyone can recompute without this tool »*. The tests read it off
/// a column that operation writes, never off this function.
fn interpretation_uuid(one: &Concept) -> String {
    Uuid::new_v5(&Uuid::NAMESPACE_URL, format!("{}#interpretation", one.uri()).as_bytes()).to_string()
}

/// A division read, titled with its level — `Lutétien (stage)`, or
/// `Lutetian (stage)` in English: the chart's own label in either language
/// (`Language::of`), never a translation of ours.
fn title_of(one: &Concept, language: Language) -> String {
    format!("{} ({})", language.of(one), one.level_name())
}

/// The ranks a chart has, for a refusal — `3 system, 4 series, …`.
fn ranks_of(chart: &Chart) -> String {
    geolts_rank::levels(chart)
        .iter()
        .map(|level| format!("{} {}", level - 1, geolts::level_name(*level)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A few names and how many more — a list of two hundred is not read (the
/// rule `contacts-from-frames` lists its set-aside properties by).
fn few(names: &[String]) -> String {
    let shown = names.iter().take(5).map(|one| format!("`{one}`")).collect::<Vec<_>>().join(", ");
    if names.len() > 5 { format!("{shown} and {} more", names.len() - 5) } else { shown }
}

/// Names and their counts — `` `Remblais` × 3 ``.
fn counted(names: &BTreeMap<String, usize>) -> String {
    let all: Vec<String> = names.iter().map(|(name, count)| format!("{name} × {count}")).collect();
    few(&all)
}
