//! `wells-from-table` — boreholes from a table and its OSDU template.
//!
//! The sibling of [`super::sigeol_boreholes`], which reads the same kind of object
//! from another format: one operation per vocabulary, both going through
//! [`super::well::write`] and [`super::well::datum_in`]. Here the vocabulary
//! is a **template**, which is the difference: we are not the ones who know
//! what the columns mean, it is the file we are given.
//!
//!
//! # Roles are read by the uuid of the property kind
//!
//! `PropertyType.PropertyTypeID` carries the uuid of the Energistics kind, and it is the
//! key — never the name. Measured: the shipped dictionary holds 3,662 kinds for
//! 3,661 titles, `spring stiffness` being carried by two kinds of the same
//! quantity class. A key right 3,661 times out of 3,662 is wrong without
//! warning.
//!
//! The roles form a **closed list**, [`ROLES`], and everything else is a
//! column that is ignored, and counted. So there are no magic names: there
//! is a table of uuids.
//!
//! # Two forms of table, and the criterion is declared
//!
//! | | what a row is | what it carries |
//! |---|---|---|
//! | **heads table** | a borehole | name, head, ground, codes — the constants |
//! | **indexed table** | a measurement at an MD | the MD, and the properties |
//!
//! A table **with no MD column** is a heads table. The criterion falls out of the
//! template, it is not guessed — and it is the pair that every borehole dataset has
//! already, a file of heads and a file of measurements.
//!
//! This module writes the boreholes; the properties along the MD are
//! the business of a second operation, as the operation's design splits it.
//!
//! # What is refused, and what is only counted
//!
//! A required role that no column fills is a refusal that names **the
//! role**: it is the template that must be corrected, not the file. A constant
//! that varies where it should not is a refusal that names the borehole and its
//! two values. A column that the template does not describe, on the other hand, is
//! ignored and counted — a business file always carries some.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::declare::{Epsg, Inputs, ObjectIn, Params, Suggestions, TableIn};
use crate::operation::{
    About, ActivityDraft, Category, Context, Dropped, Identity, NewObject, Note, Operation, Output, Plan, Refusal,
};
use crate::osdu::Column;
use crate::pieces::Pieces;
use crate::tabular::Table;

use super::support;
use super::well::{Datum, DatumKind, TrajectoryShape, Wellbore};
use super::written::FrameParams;

/// The uuid of a property kind, and what it is worth to us.
pub struct Role {
    /// The uuid of the kind in the Energistics dictionary.
    pub uuid: &'static str,
    /// The title it carries, for messages — **never for matching**.
    pub title: &'static str,
    /// The word by which a refusal names it.
    pub role: &'static str,
}

/// The structural roles, by uuid.
///
/// Checked against the shipped dictionary by `tests/wells_from_table.rs`: a
/// uuid that was not in it would make this array a wish list.
pub const ROLES: &[Role] = &[
    Role { uuid: "348c9a4d-e3e4-4beb-8456-f07a46660de1", title: "well name", role: "wellName" },
    Role { uuid: "620cec71-5ce3-4ea5-9120-833756b427d7", title: "x coordinate", role: "x" },
    Role { uuid: "cfcfdb04-02c1-4262-b66b-459b0aab3e8e", title: "y coordinate", role: "y" },
    Role { uuid: "8508cef4-0ade-454a-9ea4-e00d5905e677", title: "elevation", role: "ground" },
    Role { uuid: "c48c65d4-9680-4d10-903c-a1b2e30f66b5", title: "measured depth", role: "measuredDepth" },
    // **The top of a pass**. Declared,
    // a row is an interval `[top, MD]` and not a measurement at an MD:
    // the frame receives one node per limit and the properties are laid on its
    // intervals. This is what makes the tops and bases of units exact —
    // the file says where a pass begins, and keeping only the end forced
    // guessing.
    Role { uuid: "cb1166d9-3874-4267-abea-1cd97e4bf448", title: "top depth", role: "topDepth" },
    Role { uuid: "d48ef4e7-f82e-4e91-bda2-685146223a85", title: "projection code", role: "sourceEpsg" },
];

/// The role carrying this word.
pub(crate) fn role(word: &str) -> &'static Role {
    ROLES.iter().find(|r| r.role == word).expect("a role named in this module")
}

/// The operation.
pub struct WellsFromTable;

/// What it is told.
#[derive(Debug, Params)]
pub struct TableParams {
    /// the EPSG code the file's coordinates are in — left out, a column must state it
    ///
    /// A constant is read in a column **or** imposed here, and this setting
    /// wins. When given, the activity records that the caller said so.
    pub source_epsg: Option<Epsg>,
    /// reproject into this EPSG system, in metres — a borehole cannot be written in degrees
    pub project: Option<Epsg>,
    /// the EPSG code of the vertical system, when known; left out, the package says unknown
    pub vertical: Option<Epsg>,
    /// what the measured depths are counted from — the ground, unless the file says otherwise
    #[param(default = DatumKind::GroundLevel)]
    pub datum_kind: DatumKind,
    /// the external file the coordinates go into
    #[param(default = String::from("data.h5"))]
    pub data_file: String,
    /// how far apart two wellheads may be and still be the same hole, in metres
    ///
    /// **A distance, not a float equality**: a package written by another
    /// producer is not right to the bit. A centimetre is below
    /// what any survey claims to measure.
    #[param(default = 0.01)]
    pub head_tolerance: f64,
    /// add the new depths to a recognised well's frame instead of writing a second one
    ///
    /// **Off, and the default is the point.** Adding an MD in the middle
    /// of a frame renumbers its nodes, so **every property already laid
    /// on it then describes the wrong depths** — a valid package,
    /// curves that draw, and everything shifted. Writing a second trajectory/frame
    /// pair is what protects what is already there; it hangs on the
    /// same interpretation, so nothing is lost.
    ///
    /// When on, the plan **says** how many nodes were inserted and that the
    /// properties of this frame are to be rewritten: keeping quiet would silently do
    /// exactly what this paragraph describes.
    #[param(default = false)]
    pub enrich: bool,
}

/// What it takes.
#[derive(Debug, Inputs)]
pub struct TableInputs {
    /// the table, with what its columns mean
    #[input(least = 1)]
    pub table: Vec<TableIn>,
    /// the wells already in the package, to recognise rather than duplicate
    ///
    /// **A borehole is the same when its title and its head agree**
    /// (a decision taken for identity matching). The title alone is the producer's key
    /// and can collide — two campaigns, two `SP1`; the head
    /// alone says nothing about a deviated hole that crosses another.
    ///
    /// **An object entry, not a shape**: nothing here looks at the geometry
    /// of a frame, and a shape slot would not carry the filter by type
    /// — the front end applies it only to object slots, so that declaring it
    /// would be a declaration that does nothing.
    ///
    /// What a given frame contributes is already resolved at the edge: `wellhead`
    /// returns `[x, y, ground]` — read from the datum, never from the geometry —,
    /// `node_mds` the depths, `interpretation` what a second trajectory
    /// must attach to, `trajectory` and `datum` what an enrichment
    /// rewrites. `plan` stays pure: it does not read the package, it receives what
    /// it is given of it.
    ///
    /// **Left empty, these are all the frames of the target package** (a decision:
    /// all the boreholes of the current stage should be used by default to enrich
    /// them) — it is
    /// `every`, filled by `read::every` for the front end as for the CLI.
    /// A frame that is that of no borehole of the file gets in the way of nothing: it
    /// is matched to nobody.
    ///
    /// A package without a frame, or a caller who builds their bag themselves, gets
    /// the earlier behaviour: the seed keeps the MDs, so a different set
    /// writes a second object — the only safe answer when one cannot see what
    /// is there, since replacing would lose the depths one does not know
    /// about.
    #[input(types = "resqml22.WellboreFrameRepresentation, resqml20.obj_WellboreFrameRepresentation", every)]
    pub wells: Vec<ObjectIn>,
}

/// A borehole **already in the package**, as the `wells` slot gave it.
///
/// Nothing is read here: everything comes from `Known`, resolved at the edge.
struct Held<'a> {
    /// Its frame — what is rewritten when enriching.
    frame: &'a crate::pieces::Ref,
    /// What a second trajectory attaches to. `None` when the frame does not name
    /// its interpretation, and it is then **a new one**: one cannot
    /// attach a second representation to what nobody names.
    interpretation: Option<&'a crate::pieces::Ref>,
    /// Its trajectory — what is rewritten with the frame when enriching.
    trajectory: Option<&'a crate::pieces::Ref>,
    /// Its datum: the point from which its MDs are counted. One more trajectory
    /// must name **that one**, otherwise *counted from what* no longer has a single answer.
    datum: Option<&'a crate::pieces::Ref>,
    /// `[x, y, ground]`, read from the datum.
    head: [f64; 3],
    /// The depths of its nodes.
    mds: &'a [f64],
    /// The frame of its geometry — what a **rewritten** trajectory must not
    /// leave silently.
    crs: Option<&'a crate::pieces::Ref>,
}

/// The boreholes given, findable by title **and** by head.
struct Standing<'a> {
    held: Vec<Held<'a>>,
    /// The frames that state neither their head nor their depths, by title.
    mute: Vec<&'a str>,
}

impl<'a> Standing<'a> {
    /// What the slot carries, or a refusal that names what is wrong.
    ///
    /// **What is not a frame is refused by its type**, and named. One day,
    /// the scene's *choose* list gave the trajectories with the
    /// frames — 240 pieces for 120 boreholes — and the refusal said *"120 of the
    /// 240 given state no wellhead or no node depths"*: true, and mute on the
    /// one thing to correct.
    ///
    /// **A mute frame is a fault only if a borehole of the file bears
    /// its name** — see [`Standing::find`]. Keeping quiet about it would then pass it off
    /// as a borehole absent from the package, and a duplicate would be written in the belief of
    /// creating. But since `wells` takes all the frames of the package by default,
    /// refusing one that no row names would block an import
    /// over a borehole of another producer, which has nothing to do with it.
    fn of(wells: &'a [ObjectIn]) -> Result<Standing<'a>, Refusal> {
        let frame = crate::operation::Takes::object(&["resqml22.WellboreFrameRepresentation"]);
        let mut held = Vec::new();
        let mut mute = Vec::new();
        let mut strangers = Vec::new();
        for well in wells {
            let known = &well.known;
            if let Some(other) = known.reference.as_ref().filter(|one| !frame.accepts(&one.qualified_type)) {
                strangers.push(format!("`{}` is `{}` ({})", well.name, other.qualified_type, other.title));
                continue;
            }
            match (known.reference.as_ref(), known.wellhead, known.node_mds.as_deref()) {
                (Some(frame), Some(head), Some(mds)) if !mds.is_empty() => held.push(Held {
                    frame,
                    interpretation: known.interpretation.as_ref(),
                    trajectory: known.trajectory.as_ref(),
                    datum: known.datum.as_ref(),
                    head,
                    mds,
                    crs: known.crs.as_ref(),
                }),
                (Some(frame), _, _) => mute.push(frame.title.as_str()),
                // Not even a reference: nothing to match, nor to name
                // other than by the piece.
                (None, _, _) => mute.push(well.name.as_str()),
            }
        }
        if !strangers.is_empty() {
            return Err(Refusal::on(
                "wells",
                format!(
                    "{} of the {} given are no wellbore frame, and a well is recognised by its frame: {}. Give the `WellboreFrameRepresentation`s — or nothing, and every frame of the package is taken",
                    strangers.len(),
                    wells.len(),
                    strangers.join(", ")
                ),
            ));
        }
        Ok(Standing { held, mute })
    }

    /// The borehole of the package that **is** this one: title and head agreeing.
    ///
    /// The title first, exact then by prefix — a writer titles its frame
    /// `"<borehole> — frame"` and the file only carries `<borehole>` — with the
    /// rule that **a name followed by an alphanumeric character is another
    /// name**, otherwise `…-SP1` would be a candidate for `…-SP102`.
    ///
    /// Then the head, within `tolerance` metres. Two remaining candidates are
    /// **refused** and not settled on the first: putting a second trajectory on the
    /// wrong borehole would be seen nowhere.
    fn find(&self, well: &str, head: [f64; 2], tolerance: f64) -> Result<Option<&Held<'a>>, Refusal> {
        let near = |one: &Held<'_>| {
            let (dx, dy) = (one.head[0] - head[0], one.head[1] - head[1]);
            (dx * dx + dy * dy).sqrt() <= tolerance
        };
        let titled = |title: &str| -> bool {
            title == well
                || title
                    .strip_prefix(well)
                    .is_some_and(|rest| !rest.chars().next().is_some_and(char::is_alphanumeric))
        };

        let mut found: Vec<&Held<'a>> = self.held.iter().filter(|one| one.frame.title == well && near(one)).collect();
        if found.is_empty() {
            found = self.held.iter().filter(|one| titled(&one.frame.title) && near(one)).collect();
        }
        match found.len() {
            // **Nothing readable bears this name — but a mute frame may
            // bear it**, and that is then the one thing that cannot
            // be kept quiet: it would be writing a second borehole beside it.
            0 => match self.mute.iter().find(|title| titled(title)) {
                Some(title) => Err(Refusal::on(
                    "wells",
                    format!(
                        "`{well}` may be `{title}` of the package, which states no wellhead or no node depths, so whether it continues it cannot be decided. A `WellboreFrameRepresentation` states the depths in `NodeMd` and the head through its trajectory's datum; a build with no array backend cannot read the first"
                    ),
                )),
                None => Ok(None),
            },
            1 => Ok(Some(found[0])),
            _ => Err(Refusal::on(
                "wells",
                format!(
                    "`{well}` matches {} of the given wells by title and wellhead ({}), so which one it continues cannot be decided: give the one it is",
                    found.len(),
                    found.iter().map(|one| one.frame.title.as_str()).collect::<Vec<_>>().join(", ")
                ),
            )),
        }
    }
}

/// Notes of the same key, in one sentence.
///
/// When identical, they are counted (`120 × …`); when different — each names its
/// borehole —, the first are said and the rest counted. A list of a hundred and
/// twenty titles is not news, it is a wall.
fn gathered(details: &[String]) -> String {
    const SHOWN: usize = 3;
    match details {
        [one] => one.clone(),
        [first, rest @ ..] if rest.iter().all(|one| one == first) => format!("{} × {first}", details.len()),
        _ => {
            let named = details.iter().take(SHOWN).cloned().collect::<Vec<_>>().join("; ");
            match details.len().saturating_sub(SHOWN) {
                0 => format!("{} wellbores: {named}", details.len()),
                more => format!("{} wellbores: {named}; and {more} more", details.len()),
            }
        }
    }
}

/// The union of two sets of depths, sorted, without duplicates within `tolerance`.
///
/// **A union and not a concatenation**: an MD already there must not make a
/// second node at the same depth, which would give two values for the
/// same point and an array that no reader can tell apart.
fn union(held: &[f64], fresh: &[f64], tolerance: f64) -> Vec<f64> {
    let mut all: Vec<f64> = held.iter().chain(fresh).copied().collect();
    all.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut out: Vec<f64> = Vec::with_capacity(all.len());
    for one in all {
        if out.last().is_none_or(|last| (one - last).abs() > tolerance) {
            out.push(one);
        }
    }
    out
}

/// What the import counted.
#[derive(Default)]
struct Tally {
    wells: usize,
    nodes: usize,
    rows: usize,
    ignored_columns: usize,
    /// The boreholes whose top is not measured, and by how much.
    unmeasured: Vec<(String, f64)>,
}

/// The rows set aside, grouped by reason.
#[derive(Default)]
pub(crate) struct SetAside(pub(crate) BTreeMap<String, usize>);

impl SetAside {
    pub(crate) fn add(&mut self, why: impl Into<String>) {
        *self.0.entry(why.into()).or_default() += 1;
    }

    pub(crate) fn into_dropped(self) -> Vec<Dropped> {
        self.0.into_iter().map(|(why, count)| Dropped { count, why }).collect()
    }
}

/// A borehole being read: its rows, in file order.
pub(crate) struct Hole {
    pub(crate) title: String,
    pub(crate) rows: Vec<usize>,
}

/// The objects of **the frame a run creates** — `crs::Built::new_objects`, by
/// the roles it writes them under (`crs::FRAME_ROLE` is the one a geometry
/// names). `degrees` is whether the run may write the frame in degrees, which
/// is a `Geographic2dCrs` under the same role: a well cannot (its vertical
/// extent has no reader in degrees), a point set can.
///
/// Every one of them is `0..1` — a named `crs` writes none, and a run has one
/// frame. Shared by the operations that take `sourceEpsg`/`project`/`vertical`.
pub(crate) fn frame_outputs(degrees: bool) -> Vec<Output> {
    vec![
        Output {
            role: crate::crs::FRAME_ROLE,
            types: if degrees {
                &["eml23.LocalEngineeringCompoundCrs", "eml23.Geographic2dCrs"]
            } else {
                &["eml23.LocalEngineeringCompoundCrs"]
            },
            least: 0,
            most: Some(1),
            describe: "the coordinate system the geometry names — the compound over the projected plane and the vertical system, or the geographic system when kept in degrees; none when the run names one of the package",
        },
        Output {
            role: "projected-crs",
            types: &["eml23.ProjectedCrs"],
            least: 0,
            most: Some(1),
            describe: "the projected system the compound is built on, written when the run creates its frame",
        },
        Output {
            role: "vertical-crs",
            types: &["eml23.VerticalCrs"],
            least: 0,
            most: Some(1),
            describe: "the vertical system of the frame, written when the run creates it",
        },
        Output {
            role: "local-2d-crs",
            types: &["eml23.LocalEngineering2dCrs"],
            least: 0,
            most: Some(1),
            describe: "the local plane of the compound, written when the run creates its frame",
        },
    ]
}

/// What a run of `wells-from-table` writes, by role — the wells' own objects
/// and the frame's. Reused by `wells-and-properties-from-table`, which runs it
/// first.
pub(crate) fn outputs() -> Vec<Output> {
    let mut out = vec![
        Output {
            role: "feature",
            types: &["resqml22.WellboreFeature"],
            least: 0,
            most: None,
            describe: "one wellbore feature per hole of the file that the package does not hold already",
        },
        Output {
            role: "interpretation",
            types: &["resqml22.WellboreInterpretation"],
            least: 0,
            most: None,
            describe: "one wellbore interpretation per new hole",
        },
        Output {
            role: "datum",
            types: &["eml23.ReferencePointInALocalEngineeringCompoundCrs"],
            least: 0,
            most: None,
            describe: "the reference point each hole's depths are counted from, with its ground elevation — the collar",
        },
        Output {
            role: "trajectory",
            types: &["resqml22.WellboreTrajectoryRepresentation"],
            least: 0,
            most: None,
            describe: "one trajectory per hole the file gives depths for; none for a table of heads",
        },
        Output {
            role: "frame",
            types: &["resqml22.WellboreFrameRepresentation"],
            least: 0,
            most: None,
            describe: "one frame per hole the file gives depths for — its nodes are the measured depths",
        },
    ];
    out.extend(frame_outputs(false));
    out
}

impl Operation for WellsFromTable {
    type Params = TableParams;
    type Inputs = TableInputs;

    fn gives(&self) -> Vec<Output> {
        outputs()
    }

    fn about(&self) -> About {
        About {
            name: "wells-from-table",
            describe: "wellbores from a tabular file and an OSDU column template: a feature, an interpretation, a trajectory and a frame per hole",
            category: Category::Import,
            // The datum sits in the compound the run creates (decision (b) of
            // 24/09, `well::datum_in`); a continued well keeps its own.
            makes: &[
                "eml23.ReferencePointInALocalEngineeringCompoundCrs",
                "resqml22.WellboreFeature",
                "resqml22.WellboreInterpretation",
                "resqml22.WellboreTrajectoryRepresentation",
                "resqml22.WellboreFrameRepresentation",
            ],
            recipe: "wells-from-table",
            represents: None,
        }
    }

    fn also(&self) -> &'static [Category] {
        &[Category::Wells]
    }

    /// Propose a metric system, taken from the coordinates of the file.
    ///
    /// The asymmetry that `sigeol-boreholes` paid for on 19/09: an operation that
    /// fills no slot lets a package in degrees be written, which `check`
    /// validates and which nothing draws. A suggestion, never a default — a
    /// reprojection moves every coordinate.
    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        let Some((_, _, table, template)) = bag.tables().next() else { return };
        let (Ok(Some(x)), Ok(Some(y))) = (
            template.for_kind(role("x").uuid),
            template.for_kind(role("y").uuid),
        ) else {
            return;
        };
        let (Some(ix), Some(iy)) = (table.column(&x.name), table.column(&y.name)) else { return };
        let first = table.rows.iter().find_map(|row| {
            let lon = table.dialect.number(row.get(ix)?)?;
            let lat = table.dialect.number(row.get(iy)?)?;
            Some((lon, lat))
        });
        // The projection code declared by the file, when a column
        // carries it: it is data, not a guess.
        if let Ok(Some(code)) = template.for_kind(role("sourceEpsg").uuid) {
            if let Some(found) = table
                .column(&code.name)
                .and_then(|at| table.rows.iter().find_map(|row| row.get(at)))
                .and_then(|cell| cell.trim().parse::<u32>().ok())
            {
                slots.set("sourceEpsg", json!(found));
            }
        }
        // The target, for its part, is deduced from the numbers and not from a country — a UTM zone
        // is a function of the longitude. It only makes sense if the file
        // is in degrees, which only a source code of 4326 says.
        if let Some((lon, lat)) = first {
            if (-180.0..=180.0).contains(&lon) && (-90.0..=90.0).contains(&lat) {
                if let Some(found) = crate::crs::utm_epsg_of(lon, lat) {
                    slots.set("project", json!(found));
                }
            }
        }
    }

    /// The **content**, not the kind: a table links, and only a template that
    /// names the necessary roles makes it a file of boreholes.
    fn check(&self, inputs: &TableInputs) -> Result<(), Refusal> {
        for table in &inputs.table {
            missing_roles(&table.template, true)?;
        }
        Ok(())
    }

    fn plan(&self, inputs: TableInputs, params: TableParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
        self.plan_ending(inputs, params, cx, &|_| None).map(|(plan, _)| plan)
    }
}

impl WellsFromTable {
    /// The plan, **and the frame on which each borehole ends** with its
    /// depths — what `wells-and-properties-from-table` passes to
    /// `properties_from_table::lay`. The frame of an unchanged borehole is that
    /// of the package; that of a written borehole is the new one, which the plan carries.
    ///
    /// `more` returns, for a borehole, **what its frame will carry beyond its
    /// depths** — the correspondence of its passes with units
    /// (`wells-and-properties-from-table`). This text enters the seed of
    /// the trajectory and of the frame: a frame that says something else is another
    /// object, never the same one rewritten. And a recognised borehole whose
    /// depths are already there is **not** left as it is when there is
    /// something more to say — the frame of the package does not carry it.
    pub(crate) fn plan_ending(
        &self,
        inputs: TableInputs,
        params: TableParams,
        cx: &Context<'_>,
        more: &dyn Fn(&str) -> Option<String>,
    ) -> Result<(Plan, Vec<Ending>), Refusal> {
        if let Identity::Given(_) = cx.identity {
            return Err(Refusal::new(
                "refused",
                "`wells-from-table` names its objects from the file's own well names, so a second run of the \
                 same file writes the same package; it does not take given uuids",
            ));
        }
        let identity = cx.identity;
        let when = cx.when;
        let mut rank = 0usize;

        let mut tally = Tally::default();
        let mut ends: Vec<Ending> = Vec::new();
        let mut aside = SetAside::default();
        let mut says: Vec<Note> = Vec::new();

        // **The source code: a column, or the setting — and the setting
        // wins.** Read before the frame, since it is what decides it.
        let mut source_epsg = params.source_epsg;
        for table in &inputs.table {
            let found = constant_epsg(&table.table, &table.template)?;
            match (source_epsg, found) {
                (None, Some(code)) => source_epsg = Some(Epsg(code as i64)),
                (Some(given), Some(code)) if given.0 != code as i64 => says.push(Note {
                    key: "epsg-overridden",
                    detail: format!(
                        "the file's `{}` column says EPSG:{code} and the run was told EPSG:{}; the run wins",
                        role("sourceEpsg").title,
                        given.0
                    ),
                }),
                _ => {}
            }
        }
        let Some(source_epsg) = source_epsg else {
            return Err(Refusal::on(
                "sourceEpsg",
                format!(
                    "nothing states the system the coordinates are in: declare a `{}` column in the template, \
                     or set `sourceEpsg` for the whole file",
                    role("sourceEpsg").title
                ),
            ));
        };

        // **A table that is already projected need not restate its code.** `frame_in` reads
        // an absent `project` as *keep the degrees*, which is right for a
        // GeoJSON — RFC 7946 fixes it at 4326 — and wrong here: the coordinates
        // of a CSV are in the system the file declares, and it is
        // often already metric (EPSG:3949 in the BRGM corpus). Requiring
        // `project` would force retyping the code that has just been read.
        //
        // So: without a target, the target **is** the source — unless the source is
        // geographic, in which case nothing is decided here and the refusal further down
        // does its work. A code that this build does not know is treated as
        // geographic: *I do not know* must lead to the refusal that asks, not
        // to a silent projection.
        let project = params.project.or_else(|| {
            (crate::crs::is_geographic(source_epsg.0) == Some(false)).then_some(source_epsg)
        });
        let mut frame = support::frame_in(
            &[],
            &FrameParams {
                crs: None,
                source_epsg: Some(source_epsg),
                project,
                vertical: params.vertical,
            },
            identity,
            when,
            &mut rank,
        )?;
        let (target, from) = frame.place.clone().ok_or_else(|| {
            Refusal::new("sourceEpsg", "the frame was not created here: `wells-from-table` places the file's own coordinates")
        })?;
        // A borehole **is** a vertical extent: writing it in a system in
        // degrees makes it between a hundred and ten thousand times too long, in a package
        // that `check` validates. The full argument is in `sigeol_boreholes`.
        if let crate::crs::Target::Geographic(code) = &target {
            return Err(Refusal::new(
                "project",
                format!(
                    "a borehole is a vertical extent, and EPSG:{code} measures its horizontal axes in degrees \
                     while its depths are linear: every hole would be written between a hundred and ten thousand \
                     times longer than the model is wide — name a projected system in `project` (EPSG:2154 for mainland France)"
                ),
            ));
        }
        let vertical_up = Some(true);
        let file = params.data_file.clone();

        let mut objects = std::mem::take(&mut frame.objects);
        let mut datasets = Vec::new();
        let parameters = std::mem::take(&mut frame.parameters);
        // The notes per borehole, **all kept**: `unchanged`, `merged`,
        // `ground-moved` each name their borehole, and keeping only the
        // first would write *"120 × `SP1` is already in the package"* —
        // one borehole, a hundred and twenty times.
        let mut from_writer: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();

        // **What the package already holds**, read once: `plan` does not read the
        // package, it receives what it is given of it.
        let standing = {
            let _span = tracing::info_span!("wells.standing", wells = inputs.wells.len()).entered();
            Standing::of(&inputs.wells)?
        };

        let build = tracing::info_span!("wells.build", tables = inputs.table.len(), wells = tracing::field::Empty).entered();
        for input in &inputs.table {
            let table = &input.table;
            let template = &input.template;
            tally.rows += table.len();
            tally.ignored_columns += table
                .headers
                .iter()
                .filter(|h| template.column(h).is_none_or(|c| c.kind.is_none()))
                .count();

            let at = Columns::of(table, template)?;
            for hole in group(table, at.name, &mut aside) {
                match one_hole(
                    &hole,
                    table,
                    &at,
                    &target,
                    from,
                    &frame,
                    &params,
                    &file,
                    vertical_up,
                    &standing,
                    more(&hole.title),
                    identity,
                    when,
                    &mut rank,
                ) {
                    Ok(made) => {
                        ends.extend(made.frame.map(|(frame, mds)| (hole.title.clone(), frame, mds)));
                        tally.wells += 1;
                        tally.nodes += made.nodes;
                        if let Some(depth) = made.unmeasured {
                            tally.unmeasured.push((hole.title.clone(), depth));
                        }
                        objects.extend(made.objects);
                        datasets.extend(made.datasets);
                        for note in made.says {
                            from_writer.entry(note.key).or_default().push(note.detail);
                        }
                    }
                    Err(why) => aside.add(why),
                }
            }
        }

        build.record("wells", tally.wells);
        drop(build);
        if tally.wells == 0 {
            let why = aside
                .0
                .iter()
                .map(|(why, count)| format!("{count} × {why}"))
                .collect::<Vec<_>>()
                .join("; ");
            return Err(Refusal::on(
                "table",
                format!("no wellbore could be written from this table: {why}"),
            ));
        }

        says.push(Note { key: "wells", detail: format!("{} wellbore(s) from {} row(s)", tally.wells, tally.rows) });
        says.push(Note { key: "nodes", detail: format!("{} frame node(s)", tally.nodes) });
        if !tally.unmeasured.is_empty() {
            // The longest first, and named: a count alone would say
            // *how many* without saying *which*, and it is the borehole that will
            // be looked at, not the number.
            let mut worst = tally.unmeasured.clone();
            worst.sort_by(|a, b| b.1.total_cmp(&a.1));
            let named = worst
                .iter()
                .take(3)
                .map(|(title, depth)| format!("{title} ({depth} m)"))
                .collect::<Vec<_>>()
                .join(", ");
            let rest = worst.len().saturating_sub(3);
            says.push(Note {
                key: "unmeasured-top",
                detail: format!(
                    "{} wellbore(s) state no measurement at the ground: their trajectory starts at their first stated depth, not at the datum; the deepest starts are {named}{}",
                    worst.len(),
                    if rest > 0 { format!(", and {rest} more") } else { String::new() }
                ),
            });
        }
        if tally.ignored_columns > 0 {
            says.push(Note {
                key: "columns-ignored",
                detail: format!(
                    "{} column(s) of the file are not named by the template, and were left out",
                    tally.ignored_columns
                ),
            });
        }
        for input in &inputs.table {
            for note in &input.template.notes {
                says.push(Note { key: "template", detail: note.clone() });
            }
        }
        for (key, details) in from_writer {
            says.push(Note { key, detail: gathered(&details) });
        }

        let source = inputs.table.first().map(|t| t.file.clone()).unwrap_or_default();
        let plan = Plan {
            media: Vec::new(),
            blocked: frame.blocked,
            objects,
            datasets,
            // **No shape**, for the reason `sigeol-boreholes` gives:
            // an import returns thousands of objects, and what is written is a
            // parametric line whose control points are the
            // trajectory only if it is linear.
            shapes: Vec::new(),
            activity: ActivityDraft {
                recipe: "wells-from-table",
                title: format!("wells-from-table → {source}"),
                seed: source,
                parameters,
            },
            says,
            dropped: aside.into_dropped(),
        };
        Ok((plan, ends))
    }
}

/// Where a borehole ends: its name, its frame, and the depths of its nodes.
pub(crate) type Ending = (String, crate::pieces::Ref, Vec<f64>);

/// The ranks of the role columns in the table.
pub(crate) struct Columns {
    pub(crate) name: usize,
    x: usize,
    y: usize,
    ground: usize,
    pub(crate) md: Option<usize>,
    /// The top of each pass, when the template declares it.
    pub(crate) top: Option<usize>,
}

impl Columns {
    /// Match the template and the header.
    ///
    /// **Both lists are named in the refusal**, because a declared
    /// column that the file does not have and a column of the file that the template
    /// ignores are two different pieces of news.
    pub(crate) fn of(table: &Table, template: &crate::osdu::Template) -> Result<Columns, Refusal> {
        let at = |word: &str| -> Result<Option<usize>, Refusal> {
            let role = role(word);
            let column = template
                .for_kind(role.uuid)
                .map_err(|why| Refusal::on(word, why))?;
            let Some(column) = column else { return Ok(None) };
            match table.column(&column.name) {
                Some(found) => Ok(Some(found)),
                None => Err(Refusal::on(
                    word,
                    format!(
                        "the template declares `{}` for the {} of a hole, and the file's header does not carry it \
                         (it names: {})",
                        column.name,
                        role.title,
                        table.headers.join(", ")
                    ),
                )),
            }
        };
        let need = |word: &str, found: Option<usize>| -> Result<usize, Refusal> {
            found.ok_or_else(|| {
                Refusal::on(
                    word,
                    format!(
                        "no column is declared for the {} of a hole — add one to the template, keyed on the \
                         property kind `{}`",
                        role(word).title,
                        role(word).uuid
                    ),
                )
            })
        };
        Ok(Columns {
            name: need("wellName", at("wellName")?)?,
            x: need("x", at("x")?)?,
            y: need("y", at("y")?)?,
            ground: need("ground", at("ground")?)?,
            md: at("measuredDepth")?,
            top: at("topDepth")?,
        })
    }
}

/// What a template does not say and which would be needed.
fn missing_roles(template: &crate::osdu::Template, _deep: bool) -> Result<(), Refusal> {
    for word in ["wellName", "x", "y", "ground"] {
        let role = role(word);
        let found = template.for_kind(role.uuid).map_err(|why| Refusal::on(word, why))?;
        if found.is_none() {
            return Err(Refusal::on(
                word,
                format!(
                    "this template declares no column for the {} of a hole (property kind `{}`), so it does not \
                     describe a file of boreholes",
                    role.title, role.uuid
                ),
            ));
        }
    }
    Ok(())
}

/// Group the rows by borehole, in the order in which the names appear.
/// Group the rows by borehole name.
///
/// **The column, and not the whole `Columns`**: an enrichment table has
/// neither head nor ground, so requiring the complete structure would refuse a
/// perfectly readable file. What this grouping asks for is a name, and nothing
/// else.
pub(crate) fn group(table: &Table, name_at: usize, aside: &mut SetAside) -> Vec<Hole> {
    let mut order: Vec<String> = Vec::new();
    let mut holes: BTreeMap<String, Hole> = BTreeMap::new();
    for (rank, row) in table.rows.iter().enumerate() {
        let title = row.get(name_at).map(|s| s.trim()).unwrap_or_default();
        if title.is_empty() {
            // An empty name groups nothing: two rows without a name are not the
            // same borehole, and joining them would invent one.
            aside.add("a row with no well name: nothing can name its objects");
            continue;
        }
        let key = title.to_string();
        if !holes.contains_key(&key) {
            order.push(key.clone());
            holes.insert(key.clone(), Hole { title: key.clone(), rows: Vec::new() });
        }
        holes.get_mut(&key).expect("just inserted").rows.push(rank);
    }
    order.into_iter().filter_map(|k| holes.remove(&k)).collect()
}

/// What a borehole produced.
struct Made {
    objects: Vec<NewObject>,
    datasets: Vec<crate::operation::Dataset>,
    says: Vec<Note>,
    nodes: usize,
    /// The depth of the first measurement, when the ground zero was added.
    unmeasured: Option<f64>,
    /// **The frame on which this borehole ends**, and its depths:
    /// that of the package when nothing changes, the new one otherwise. It is what an
    /// operation that lays properties right after must aim at — `None` for
    /// a heads table, which writes no frame.
    frame: Option<(crate::pieces::Ref, Vec<f64>)>,
}

/// Write a borehole.
#[allow(clippy::too_many_arguments)]
fn one_hole(
    hole: &Hole,
    table: &Table,
    at: &Columns,
    target: &crate::crs::Target,
    from: i64,
    frame: &support::Frame,
    params: &TableParams,
    file: &str,
    vertical_up: Option<bool>,
    standing: &Standing<'_>,
    more: Option<String>,
    identity: &Identity,
    when: &crate::operation::When,
    rank: &mut usize,
) -> Result<Made, String> {
    let title = hole.title.clone();

    // **The constants, and their verified constancy.** A head that changes from
    // one row to the next for the same borehole is not an average to be taken: it is
    // a file that says two things, and both values are named.
    let head_x = one_number(table, &hole.rows, at.x, "x coordinate", &title)?;
    let head_y = one_number(table, &hole.rows, at.y, "y coordinate", &title)?;
    let ground_cell = one_number(table, &hole.rows, at.ground, "elevation", &title)?;

    // The depths: sorted, de-duplicated or refused. **None** when the
    // template names no MD column — a heads table says where a
    // borehole is collared and nothing of its path, and making up a node at
    // MD 0 for it would write a trajectory that the file does not state.
    let mds = match (at.md, at.top) {
        (None, None) => Vec::new(),
        (Some(column), None) => depths(table, &hole.rows, column, &title)?,
        // **Passes: one node per limit.** Tops and bases together, sorted,
        // without duplicates — a pass contiguous with the next shares its node
        // with it, and that is the limit between the two.
        (Some(base), Some(top)) => {
            let passes = intervals(table, &hole.rows, top, base, &title)?;
            limits(&passes)
        }
        (None, Some(_)) => {
            return Err(format!(
                "`{title}`: the template declares a `{}` column and no `{}` column — a pass needs where it ends as well as where it starts",
                role("topDepth").title,
                role("measuredDepth").title
            ));
        }
    };
    // **What nobody measured is said all the same.** The borehole no longer starts
    // at the ground, so nothing is *drawn* across what is not
    // sounded — but the gap between the datum and the first measurement remains a fact
    // of the file, and three boreholes of the BRGM corpus carry it at 10.5, 31.5 and
    // 36.25 m. Keeping quiet about it would make a borehole that starts thirty metres under the
    // ground an ordinary borehole.
    let unmeasured = mds.first().copied().filter(|first| *first > 0.0);

    let placed = crate::crs::place(
        &[crate::geojson::Position { lon: head_x, lat: head_y, z: Some(ground_cell) }],
        target,
        from,
    )?;
    let [x, y, ground] = placed[0];

    // **The borehole of the package that is this one**: title and head, the rule
    // of identity matching. Nothing is read here — `standing` carries what the slot
    // `wells` gave, resolved at the edge.
    let matched = standing.find(&title, [x, y], params.head_tolerance).map_err(|why| why.to_string())?;

    // **The ground is a value, not an identity.** Two files that place the
    // same head at two altitudes do not describe two boreholes: they
    // disagree on a measurement, and every node depends on it. It is therefore a
    // conflict — a second trajectory, never a silent correction.
    let ground_moved = matched.is_some_and(|held| (held.head[2] - ground).abs() > params.head_tolerance);
    // What is **continued**: the recognised borehole, unless the ground has moved.
    let continuing = matched.filter(|_| !ground_moved);

    // **The union of depths**, when enriching. Otherwise those of the file.
    let held_mds = continuing.map(|held| held.mds).unwrap_or(&[]);
    let all = union(held_mds, &mds, params.head_tolerance);
    let mds = if params.enrich && continuing.is_some() { all.clone() } else { mds };

    // **Nothing to do is an answer.** A recognised borehole all of whose
    // depths are already there needs no object: writing a second one,
    // identical, would make the package grow without adding anything to it. This is the
    // rule that if the values are identical there is nothing to do.
    if let Some(held) = continuing
        && all.len() == held.mds.len()
        && more.is_none()
    {
        return Ok(Made {
            objects: Vec::new(),
            datasets: Vec::new(),
            says: vec![Note {
                key: "unchanged",
                detail: format!("`{title}` is already in the package with these depths"),
            }],
            nodes: 0,
            unmeasured: None,
            frame: Some((held.frame.clone(), held.mds.to_vec())),
        });
    }

    // **What is rewritten**, and that is only possible if the package gave us
    // both objects: rewriting the frame while letting the trajectory derive
    // would make a frame that samples a path that nothing carries.
    //
    // The trajectory is **always** there when the borehole is in `standing`:
    // `Standing::of` requires a head, and `read::wellhead_of` only finds it by
    // going through `Trajectory`. The `None` is therefore the branch that the invariant
    // closes, kept rather than an `unwrap` — a panic here would cost a whole
    // run for a malformed frame.
    let keeping = continuing
        .filter(|_| params.enrich)
        .and_then(|held| held.trajectory.map(|trajectory| (trajectory.uuid.as_str(), held.frame.uuid.as_str())));

    let line: Vec<[f64; 3]> = mds.iter().map(|md| [x, y, ground - md]).collect();
    // **Identity comes from what the object is, not from the file**.
    // A measurement that made it necessary: the
    // same CSV under another name took three boreholes to six, each
    // doubled under two identities — and nothing said so.
    //
    // Three seeds, because three things differ:
    //
    // - **the feature and the interpretation** are the borehole, and nothing else:
    //   two exports of the same producer speak of the same borehole, so a
    //   second trajectory hooks onto the same interpretation. This is
    //   the idiom RESQML provides, and what is wanted;
    // - **the datum** is the borehole **and its position**: a corrected head is
    //   another reference point, not the same one silently corrected;
    // - **the trajectory and the frame** are the borehole **and the depths
    //   they carry**: same MDs, same object and nothing to do; different
    //   MDs, a second object beside it.
    // The seed of the borehole **is the matching rule**: a borehole is its
    // title and its head. Two campaigns that both name two distant holes `SP1`
    // write two boreholes; the same hole redelivered writes one.
    // To the millimetre (`support::seed_mm`): a head reprojected, a ground read
    // on a DTM, carry the platform's last bit.
    let mm = super::support::seed_mm;
    let key = format!("{title}|{},{}", mm(x), mm(y));
    let placed_at = format!("{title}|{},{},{}", mm(x), mm(y), mm(ground));
    // The trajectory carries **the head, the ground and the depths**: a corrected ground
    // moves every node, so it is another path even with equal MDs.
    let along =
        format!("{placed_at}|{}", mds.iter().map(|md| format!("{:?}", mm(*md))).collect::<Vec<_>>().join(","));
    // What the frame carries beyond that, in its identity: see `plan_ending`.
    let along = match &more {
        Some(extra) => format!("{along}|{extra}"),
        None => along,
    };
    let cite = |name: &str| -> Value {
        json!({
            "$type": "eml23.Citation",
            "Title": name,
            "Originator": when.originator,
            "Format": energyml_core::CITATION_FORMAT,
            "Creation": when.creation,
        })
    };

    let mut objects = Vec::new();

    // **A recognised borehole keeps its feature and its interpretation**, and none
    // is written. Rewriting one would repoint it at a
    // derived feature that the package does not know; and it is precisely to that
    // interpretation that a second trajectory must attach, as wanted:
    // a v2 hooked onto the same interpretation.
    let interpretation = if let Some(existing) = matched.and_then(|held| held.interpretation) {
        existing.dor()
    } else {
        let feature_uuid = support::next_uuid(identity, &format!("well-feature:{key}"), rank)
            .map_err(|why| why.to_string())?;
        objects.push(NewObject {
            uuid: feature_uuid.clone(),
            qualified_type: "resqml22.WellboreFeature".to_string(),
            role: "feature".to_string(),
            title: title.clone(),
            json: json!({
                "$type": "resqml22.WellboreFeature",
                "uuid": feature_uuid,
                "schemaVersion": "2.2",
                "Citation": cite(&title),
            }),
        });

        let interpretation_uuid = support::next_uuid(identity, &format!("well-interpretation:{key}"), rank)
            .map_err(|why| why.to_string())?;
        let interpretation_title = format!("{title} — interpretation");
        objects.push(NewObject {
            uuid: interpretation_uuid.clone(),
            qualified_type: "resqml22.WellboreInterpretation".to_string(),
            role: "interpretation".to_string(),
            title: interpretation_title.clone(),
            json: json!({
                "$type": "resqml22.WellboreInterpretation",
                "uuid": interpretation_uuid,
                "schemaVersion": "2.2",
                "Citation": cite(&interpretation_title),
                "Domain": "depth",
                "InterpretedFeature": {
                    "$type": "eml23.DataObjectReference",
                    "Uuid": feature_uuid,
                    "QualifiedType": "resqml22.WellboreFeature",
                    "Title": title,
                },
            }),
        });
        json!({
            "$type": "eml23.DataObjectReference",
            "Uuid": interpretation_uuid,
            "QualifiedType": "resqml22.WellboreInterpretation",
            "Title": interpretation_title,
        })
    };

    // **The datum of a continued borehole is its own**, not a derived copy: the
    // trajectory already there names it, and two reference points at the same
    // place under two uuids would make a package where *counted from what*
    // would not have a single answer. When the ground has moved, on the other hand, it is
    // another point — hence `continuing` and not `matched`.
    let kept_datum = continuing.and_then(|held| held.datum).map(|one| one.dor());
    let (datum, datum_objects) = super::well::datum_in(
        &Datum {
            named: kept_datum.as_ref(),
            head: [x, y],
            ground,
            kind: params.datum_kind,
            crs: &frame.crs,
            vertical_crs: frame.vertical.as_ref(),
            title: &title,
            seed: &placed_at,
            citation: &cite,
        },
        identity,
        rank,
    )
    .map_err(|why| why.to_string())?;
    objects.extend(datum_objects);

    // **A heads table stops here**: the borehole, its interpretation and its
    // datum are written, and there is neither trajectory nor frame. It is the pair
    // that every borehole dataset has — a file of heads, a file of measurements —
    // and what the first lacks is precisely the path.
    if mds.is_empty() {
        return Ok(Made { objects, datasets: Vec::new(), says: Vec::new(), nodes: 0, unmeasured: None, frame: None });
    }

    // **A rewritten trajectory does not change frame silently** (an audit
    // finding on the harvest and the CRS). Enriching rewrites the object of the
    // package under its uuid; writing it under the frame of this run made
    // a BRGM borehole change frame — its own compound against ours — without
    // any line saying so.
    //
    // **Keeping it is not possible here**, and that is what decides the refusal:
    // the numbers of this run are placed — projected, like `Known::wellhead`
    // which `read::wellhead_of` places — and writing them under the frame of the borehole
    // would need its `from_projected`, which this operation, being pure, does not receive.
    // Under an identity compound they would land right; under a
    // rotated or shifted one, elsewhere, and nothing says which of the two it is. So:
    // the same frame as this run, or the borehole set aside **by name**, with the
    // way that remains — `enrich` off writes a second trajectory beside it.
    if let (Some(_), Some(held)) = (keeping, continuing) {
        let ours = frame.crs["Uuid"].as_str().unwrap_or_default();
        match held.crs {
            Some(own) if own.uuid.eq_ignore_ascii_case(ours) => {}
            Some(own) => {
                return Err(format!(
                    "`{title}`: `enrich` rewrites the trajectory already in the package, which is written in `{}` and not in this run's frame `{}` — its depths would be placed in this run's frame and written under one whose origin and azimuth this operation does not receive, changing its frame without a word. Leave `enrich` off to write a second trajectory beside it",
                    own.title,
                    frame.crs["Title"].as_str().unwrap_or_default()
                ));
            }
            None => {
                return Err(format!(
                    "`{title}`: `enrich` rewrites the trajectory already in the package, and the package does not say which frame it is written in — rewritten in this run's frame, it could change frame without a word. Leave `enrich` off to write a second trajectory beside it"
                ));
            }
        }
    }

    let written = super::well::write(
        &Wellbore {
            title: &title,
            seed: &along,
            line: &line,
            // The depths of the file, not depths rebuilt from
            // the line derived from them — the detour is not exact.
            mds: Some(&mds),
            ground,
            uom: "m",
            shape: TrajectoryShape::Linear,
            tangents: None,
            spacing: None,
            datum: &datum,
            interpretation: &interpretation,
            file,
            vertical_up,
            crs: &frame.crs,
            citation: &cite,
            // **Rewrite the objects of the recognised borehole**, when enriching:
            // they are what the properties already laid designate. Without
            // `enrich`, derivation is allowed and the second pair is added beside.
            keep: keeping,
        },
        identity,
        rank,
    )
    .map_err(|why| why.to_string())?;

    let nodes = written.nodes;
    // The frame that this borehole now carries — read from what has just been
    // written, never rederived: its uuid is the one `well::write` chose.
    let ends = written
        .objects
        .iter()
        .find(|one| one.qualified_type == "resqml22.WellboreFrameRepresentation")
        .map(|one| (crate::pieces::Ref::new(one.uuid.clone(), one.qualified_type.clone(), one.title.clone()), mds.clone()));
    objects.extend(written.objects);
    let mut says = written.says;

    // **What a recognition changed is said**, otherwise the option would silently do
    // exactly what the earlier warning cautions against.
    if let Some(held) = continuing {
        let added = all.len().saturating_sub(held.mds.len());
        if keeping.is_some() {
            says.push(Note {
                key: "merged",
                detail: format!(
                    "`{title}`: {added} depth(s) added to the frame already in the package. Its nodes are renumbered, \
                     so every property laid on it describes the wrong depths until it is written again"
                ),
            });
        } else {
            says.push(Note {
                key: "continued",
                detail: format!(
                    "`{title}`: a second trajectory and frame beside the {} node(s) already there, on the same \
                     interpretation — the properties already laid on the first one keep their meaning",
                    held.mds.len()
                ),
            });
        }
    } else if ground_moved {
        says.push(Note {
            key: "ground-moved",
            detail: format!(
                "`{title}`: the ground here differs from the one the package states, so this is a second reference \
                     point and a second trajectory, not a correction of the first"
            ),
        });
    }

    Ok(Made { objects, datasets: written.datasets, says, nodes, unmeasured, frame: ends })
}

/// A value that must be the same on all the rows of a borehole.
///
/// An empty cell is an **absence**, not a contradiction: real files
/// do not always repeat the head. Only two *different* values
/// refusent.
fn one_number(
    table: &Table,
    rows: &[usize],
    column: usize,
    what: &str,
    title: &str,
) -> Result<f64, String> {
    let mut found: Option<(f64, usize)> = None;
    for rank in rows {
        let Some(cell) = table.rows[*rank].get(column) else { continue };
        if cell.trim().is_empty() {
            continue;
        }
        let Some(value) = table.dialect.number(cell) else {
            return Err(format!(
                "`{title}`: line {} has `{}` where the {what} is wanted, which is not a number",
                rank + 1,
                cell.trim()
            ));
        };
        match found {
            None => found = Some((value, *rank)),
            Some((seen, at)) if (seen - value).abs() > f64::EPSILON => {
                return Err(format!(
                    "`{title}`: the {what} is stated twice and differently — {seen} on line {} and {value} on line {}; \
                     a wellhead has one position, and averaging would be this importer deciding",
                    at + 1,
                    rank + 1
                ));
            }
            _ => {}
        }
    }
    found.map(|(v, _)| v).ok_or_else(|| format!("`{title}`: no row states its {what}"))
}

/// The depths of a borehole: sorted, and without duplicates.
///
/// **Rows are sorted, never a column** — here the MDs alone suffice
/// since nothing else depends on them yet, but the order returned is that of the
/// values and it is increasing, which a frame requires.
///
/// A duplicate means one of three things that the file does not tell apart —
/// two contradictory measurements, the same row twice, or two properties
/// filled in on two complementary rows — and two of them lose
/// a measurement. So it refuses, naming the two rows.
fn depths(table: &Table, rows: &[usize], column: usize, title: &str) -> Result<Vec<f64>, String> {
    let mut found: Vec<(f64, usize)> = Vec::new();
    for rank in rows {
        let Some(cell) = table.rows[*rank].get(column) else { continue };
        if cell.trim().is_empty() {
            continue;
        }
        let Some(value) = table.dialect.number(cell) else {
            return Err(format!(
                "`{title}`: line {} has `{}` where a measured depth is wanted, which is not a number",
                rank + 1,
                cell.trim()
            ));
        };
        found.push((value, *rank));
    }
    if found.is_empty() {
        return Err(format!("`{title}`: no row states a measured depth"));
    }
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    for pair in found.windows(2) {
        if (pair[0].0 - pair[1].0).abs() <= f64::EPSILON {
            return Err(format!(
                "`{title}`: two rows state the measured depth {} (lines {} and {}) — a frame's nodes are distinct, \
                 and which of the two holds cannot be guessed",
                pair[0].0,
                pair[0].1 + 1,
                pair[1].1 + 1
            ));
        }
    }
    // **The ground zero is not added**, and that is a decision taken on the
    // comparison with `csv-to-energyml`: the MD=0 should not be added. It corresponds
    // to the DEM, and there is no data at those MDs. Adding it would risk
    // shifting all the property arrays. On this precise point, the translation
    // of the other project is right.
    //
    // Three things hold it, and the first is the strongest:
    //
    // - **a frame node is an array rank.** A property written along
    //   the MD carries one value per node; a node that the file
    //   does not state forces each property to carry an absent value at
    //   rank 0, and forgetting it once shifts the whole column. The other chain
    //   carries the same setting (`FRAME_INCLUDE_TOP_NODE`), off, with the same
    //   reason written in its code;
    // - **the ground is not lost for all that**: it is the datum, written as
    //   `ReferencePointInACrs` and named by the `MdInterval`. It is from there that
    //   the DEM cloud of a batch of boreholes will be drawn, and not from frame nodes;
    // - **a one-point line is refused by `well::write`**, by its
    //   name. Without the zero, a borehole with a single measurement has no trajectory
    //   any more — and that is good news, where a phantom node made one.
    //
    // The rule of 18/09 (*the zero of the MDs is the ground*) remains that of
    // `borehole::Borehole::mds`, whose format states intervals from
    // the ground: two vocabularies, two answers, and this one is said here.
    Ok(found.into_iter().map(|(v, _)| v).collect())
}

/// The passes of a borehole: `(top, base, line)`, sorted, **without
/// overlap**.
///
/// A pass whose top is not above its base is refused,
/// naming its line — a zero or negative thickness is not a measurement to
/// correct here. Two passes that overlap too, naming both
/// lines: on the common interval, the file says two things, and keeping
/// one would be deciding which. **A gap between two passes is allowed**: the
/// file says nothing of that interval, and a property will carry a null there.
pub(crate) fn intervals(
    table: &Table,
    rows: &[usize],
    top: usize,
    base: usize,
    title: &str,
) -> Result<Vec<(f64, f64, usize)>, String> {
    let number = |rank: usize, column: usize, what: &str| -> Result<Option<f64>, String> {
        let Some(cell) = table.rows[rank].get(column) else { return Ok(None) };
        if cell.trim().is_empty() {
            return Ok(None);
        }
        table.dialect.number(cell).map(Some).ok_or_else(|| {
            format!("`{title}`: line {} has `{}` where the {what} is wanted, which is not a number", rank + 1, cell.trim())
        })
    };
    let mut found: Vec<(f64, f64, usize)> = Vec::new();
    for rank in rows {
        let (from, to) = (number(*rank, top, "top depth")?, number(*rank, base, "measured depth")?);
        let (Some(from), Some(to)) = (from, to) else {
            // A half-stated pass cannot be placed: neither its start nor its end
            // can be guessed from its neighbours.
            if from.is_some() || to.is_some() {
                return Err(format!(
                    "`{title}`: line {} states one end of its pass and not the other, so where it lies cannot be read",
                    rank + 1
                ));
            }
            continue;
        };
        if from >= to {
            return Err(format!(
                "`{title}`: line {} runs from {from} down to {to} — a pass's top lies above its base",
                rank + 1
            ));
        }
        found.push((from, to, *rank));
    }
    if found.is_empty() {
        return Err(format!("`{title}`: no row states a pass"));
    }
    found.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    for pair in found.windows(2) {
        // One millionth of a metre: two contiguous passes written by two
        // computation paths do not land to the bit on the same limit.
        if pair[1].0 < pair[0].1 - 1e-6 {
            return Err(format!(
                "`{title}`: lines {} and {} overlap — {}–{} and {}–{} — and on the common interval the file says two things",
                pair[0].2 + 1,
                pair[1].2 + 1,
                pair[0].0,
                pair[0].1,
                pair[1].0,
                pair[1].1
            ));
        }
    }
    Ok(found)
}

/// The limits of passes: the nodes of the frame, sorted, without duplicates.
pub(crate) fn limits(passes: &[(f64, f64, usize)]) -> Vec<f64> {
    let mut all: Vec<f64> = passes.iter().flat_map(|(top, base, _)| [*top, *base]).collect();
    all.sort_by(|a, b| a.total_cmp(b));
    let mut out: Vec<f64> = Vec::with_capacity(all.len());
    for one in all {
        if out.last().is_none_or(|last| (one - last).abs() > 1e-6) {
            out.push(one);
        }
    }
    out
}

/// The projection code that the file declares, if it declares one.
///
/// Constant **for the whole file**: two boreholes of one file in two
/// different systems have no common frame, and placing them together
/// would be placing them wrong.
fn constant_epsg(table: &Table, template: &crate::osdu::Template) -> Result<Option<u32>, Refusal> {
    let role = role("sourceEpsg");
    let Some(column) = template.for_kind(role.uuid).map_err(|why| Refusal::on("sourceEpsg", why))? else {
        return Ok(None);
    };
    let Some(at) = table.column(&column.name) else { return Ok(None) };
    let mut found: Option<(u32, usize)> = None;
    for (rank, row) in table.rows.iter().enumerate() {
        let Some(cell) = row.get(at) else { continue };
        let cell = cell.trim();
        if cell.is_empty() {
            continue;
        }
        let Ok(code) = cell.parse::<u32>() else {
            return Err(Refusal::on(
                "sourceEpsg",
                format!("line {} has `{cell}` in the projection code column, which is not an EPSG code", rank + 1),
            ));
        };
        match found {
            None => found = Some((code, rank)),
            Some((seen, at)) if seen != code => {
                return Err(Refusal::on(
                    "sourceEpsg",
                    format!(
                        "the file states two projection codes — EPSG:{seen} on line {} and EPSG:{code} on line {}; \
                         holes of one file share one frame, and placing them together in either would place some of them wrong",
                        at + 1,
                        rank + 1
                    ),
                ));
            }
            _ => {}
        }
    }
    Ok(found.map(|(code, _)| code))
}

/// The rank of the column that a role occupies in **this** file.
///
/// `None` when the template does not declare this role; a named refusal when it
/// declares it and the file does not have the column — the two pieces of news are
/// different, and only the second can be fixed by editing the CSV.
pub(crate) fn column_at(
    table: &Table,
    template: &crate::osdu::Template,
    word: &str,
) -> Result<Option<usize>, Refusal> {
    let role = role(word);
    let Some(column) = template.for_kind(role.uuid).map_err(|why| Refusal::on(word, why))? else {
        return Ok(None);
    };
    match table.column(&column.name) {
        Some(found) => Ok(Some(found)),
        None => Err(Refusal::on(
            word,
            format!(
                "the template declares `{}` for the {} of a hole, and the file's header does not carry it (it names: {})",
                column.name,
                role.role,
                table.headers.join(", ")
            ),
        )),
    }
}

/// The column of a role, for the tests and the front end.
pub fn column_for<'a>(template: &'a crate::osdu::Template, word: &str) -> Option<&'a Column> {
    template.for_kind(role(word).uuid).ok().flatten()
}
