//! `contacts-from-frames` — the tops and the bases of the units a set of
//! wellbore frames crosses, as point sets, and what the wells' properties hold
//! there.
//!
//! The need: export a point cloud for the top/bottom of a horizon, taking
//! mdMax/mdMin when it is not the highest/lowest MD and there is indeed data
//! before/after; and, for a Top A and a Bottom B, two distinct point clouds even
//! though they will often carry the same points.
//!
//! # What it reads: the units the frames themselves state
//!
//! `IntervalStratigraphicUnits` (TRG 2.2 §5.34) — one unit per **interval**,
//! resolved at the boundary into [`crate::pieces::Known::interval_units`]. So
//! the contact between two units is a **node**: the limit between the last
//! interval of one and the first of the next. Nothing is guessed about *before,
//! after or in between*: the frame's intervals are the file's passes.
//!
//! # What makes a contact
//!
//! - **a top only where something lies above it, a base only where something
//!   lies below** — the top of the first interval of a hole is where the log
//!   starts, not a contact; an interval without a unit (`n.d.`) is no unit, so a
//!   limit with it is no contact either;
//! - **two point sets per unit**, its top and its base, **even when they hold
//!   the same points** as the neighbour's: the base of A and the top of B part
//!   where erosion took something away, and that is what one wants to see;
//! - **a unit that comes back lower in the same hole** (A, B, A) would give two
//!   tops for one hole: refused by name — which of the two is *the* top is a
//!   geologist's call, not this module's.
//!
//! Each point set represents a `HorizonInterpretation` of a `BoundaryFeature`
//! named after the unit and the side, derived from the unit's own uuid: the top
//! of the Priabonien is the same object whichever run extracts it.
//!
//! # At a rank of the chart — `rank`, `unit`
//!
//! A frame names the division each pass was **observed** as, and a horizon is
//! often the limit of a **group** of them: the base of the Lutetian is the base
//! of « Lutétien », « Lutétien moyen » or « Lutétien inférieur » depending on
//! the borehole. Told a `rank` of the BRGM
//! chart, a `unit` of it, or both, each interval is read **against the division
//! of that rank** rather than by its name — inside it, outside it by the chart's
//! ages, or neither — and a contact is only a limit between inside and outside:
//! the limit between the Upper Lutetian and the Lutetian is none. Only the unit
//! of each interval changes; every rule here holds as it stands. How an interval
//! is read is [`super::contacts_rank`], the chart's answer
//! `energyml_domain::geolts_rank`.
//!
//! # What the wells' properties hold at a contact
//!
//! The need: when extracting point sets from wells, the properties attached to
//! the wells should be assigned to the points of the point set. When a point is
//! extracted at, say, md=10, the well's properties are looked up at that md and
//! the value copied, to recreate the continuous and discrete properties on the
//! final point set. It is easy to do: find which index the md has in the list
//! of mds and take the values at the same indices in the well's properties. In
//! the end, properties identical to the wells' are recreated (perhaps all the
//! properties to create must be anticipated up front, in case some wells lack
//! some of them, with null values for some points).
//!
//! The properties arrive through the slot `properties` — left empty, every
//! property of the package — each resolved at the boundary into
//! [`crate::pieces::Known::property`], values included, so the plan stays
//! pure. What is decided here:
//!
//! - **the rank, never the depth.** A contact *is* a node of the frame, and
//!   this module finds it by its rank among the frame's intervals before any
//!   depth is known; a frame's property is positional on the same nodes. So no
//!   MD is looked up and no tolerance is chosen: the index the request wanted to
//!   find is the one the contact was found at, and matching a depth against a
//!   list of depths would be a second, less exact, way of getting it back —
//!   two floats that went through a file and an HDF5 dataset each;
//! - **a node property is read at the node; an interval property on the
//!   unit's side of it.** An interval has no value *at* a node — a node is
//!   where two of them meet. The top of a unit takes the interval that begins
//!   there, the unit's first; its base the one that ends there, the unit's
//!   last. Each point set is a surface **of that unit**, so each takes what was
//!   measured in it — and the base of A and the top of B, the same node, then
//!   carry A's value and B's: what writing two point sets was for;
//! - **one property per (kind, measure, element, facets)**, gathered over every
//!   frame before anything is written — the anticipation the request asks for. The
//!   measure is the title without the wellbore's name (`F1 — PHI` and
//!   `F2 — PHI` are one `PHI`). A point whose wellbore does not carry it is
//!   **null**, and so is one whose wellbore's own value is null there: NaN for
//!   a continuous copy, the standard's null for doubles; `nullValue` on the
//!   array of a discrete one, which RESQML makes state its own. The two are
//!   counted apart, because *this well has no such log* and *the log is empty
//!   here* are two findings;
//! - **the same kind, the same unit, the same table — by reference.** A copy
//!   names the `PropertyKind` and the `CategoryLookup` its sources name, so a
//!   code keeps its meaning: it is read against the very table it was a rank
//!   in;
//! - **a disagreement is named, never settled.** Two units for one measure are
//!   refused rather than converted: converting would choose the unit for
//!   somebody, and `energyml_domain::wellbore_table::merge` calls the same
//!   thing a conflict. So are two tables for one code, a measure continuous in
//!   one well and discrete in another, a code equal to the declared null, and a
//!   wellbore carrying two properties under one key — which of the two is the
//!   well's cannot be guessed, and naming the one in `properties` settles it;
//! - **what is not copied is set aside by name, never read wrong**: a boolean
//!   (a flag has no null, so a point whose well lacks it could only be written
//!   true or false), a comment (text), a points property (positions), a vector
//!   or a tensor (which component?), a time series or several realizations
//!   (which one?), values on another element, another release, values that did
//!   not read, a count that is not the frame's;
//! - **a copy every value of which would be null on a point set is not written
//!   there**, and counted: an array of nulls looks exactly like data.
//!
//! The copies are written **after** every point set, and their identity comes
//! from their content: the point sets keep the uuids they had before this
//! existed, whether it is on or off.
//!
//! # Naming each point after its well — `wellOrigin`
//!
//! The need: when extracting point sets from the wells, a discrete property
//! should assign to each point the name of the original well. That implies, in
//! the 3D view, being able to display a discrete property's value per point as a
//! label, as is done for wellheads — drawing a property as label fields with
//! settings for text size, colour and so on.
//!
//! The second half — the 3D view — is a display concern and none of this
//! crate's; what this operation owes is the property. Every point already
//! names, in [`Origin`], the frame it came from — that is how a copied
//! measurement finds its row. `wellOrigin` writes the same fact as a
//! `DiscreteProperty` on the point set's nodes: one code per point, read
//! against a `ColumnBasedTable` (eml23) that lists every well's name.
//!
//! - **one table, shared by every point set of this run** — a code has to
//!   mean the same well wherever it is read, and a table per point set would
//!   let two of them disagree about what `0` names;
//! - **the kind is `wells-from-table`'s own `wellName` role**
//!   (`348c9a4d-e3e4-4beb-8456-f07a46660de1`), named both by the table's
//!   column and by the property, the same choice for the same reason as
//!   `properties-from-table`'s copies: a consumer resolves this kind exactly
//!   as it resolves the one that role already writes, rather than a second
//!   uuid for the same idea;
//! - **the index is the well's rank of encounter**, never the alphabet — two
//!   runs over the same frames in the same order write the same codes, and a
//!   sorted table would make the codes depend on which wells happen to sort
//!   first;
//! - **on, and the default is the subject** — the same choice `copyProperties`
//!   made above and for the same reason: the request is the subject's own, and
//!   it costs no point set anything. The table and the properties are written
//!   *beside* them, after every one, so a run with it off writes fewer
//!   objects, never different ones.

use std::collections::BTreeMap;

use energyml_domain::provenance::Parameter;
use energyml_domain::values::Values;
use energyml_eml_2_3 as eml;
use energyml_resqml_2_2 as resqml;
use serde_json::{Value, json};

use crate::declare::{ChartTerm, Inputs, ObjectIn, Params, Shape, Suggestions};
use crate::operation::{
    About, ActivityDraft, Category, Context, Dataset, Dropped, Need, NewObject, Note, Numbers, Operation,
    Output, Plan, Refusal, Slot,
};
use crate::pieces::{KnownProperty, Pieces, Ref};

use super::contacts_rank::Reading;
use super::support;
use super::wells_from_table;
use super::written::FrameParams;

/// The operation.
pub struct ContactsFromFrames;

/// What it is told.
#[derive(Debug, Params)]
pub struct ContactsParams {
    /// The frame the points are written in — the frames' own, suggested.
    #[param(flatten)]
    pub frame: FrameParams,
    /// the HDF5 file the points, and the values copied onto them, go into
    #[param(default = String::from("data.h5"), group = "naming")]
    pub data_file: String,
    /// copy onto each point what its wellbore's properties hold there
    ///
    /// **On, and the default is the subject.** What the wells say at a contact
    /// is what the extraction knew, and a point set without it has lost it —
    /// the request asked for it (quoted in the module). It costs no
    /// point set anything: the copies are written *beside* them, and the point
    /// sets' uuids do not depend on it, so a run with it off writes fewer
    /// objects, never different ones.
    ///
    /// Off, nothing is copied and the plan says how many of the frames'
    /// properties were left. They are still read when `properties` is left to
    /// take every one:
    /// the edge fills a slot before any setting is read, and a declaration
    /// cannot make it skip one on a flag.
    #[param(default = true, optional, group = "operation")]
    pub copy_properties: bool,
    /// name each point after the well it came from
    ///
    /// **On, and the default is the subject** — the request itself
    /// (see the module doc), and it costs no point set anything: a
    /// `DiscreteProperty` and a shared `ColumnBasedTable` are written
    /// *beside* the point sets, after every one, so a run with it off writes
    /// fewer objects, never different ones.
    ///
    /// Off, nothing is written and no point names its well.
    #[param(default = true, optional, group = "operation")]
    pub well_origin: bool,
    /// the integer a copied discrete property uses where a wellbore has no value
    ///
    /// A continuous copy has NaN, the standard's null for doubles. An integer
    /// array must **state** its null (`eml23.IntegerExternalArray.NullValue`),
    /// and the sources may each state a different one — so one integer, the
    /// same for every copy, declared rather than borrowed from whichever well
    /// came first. A source's own null becomes this one; a source **code**
    /// equal to it is refused by name, since it would then read as a null.
    /// `-9999`, as `properties-from-table` writes.
    ///
    /// The same integer is declared on `wellOrigin`'s array, even though it
    /// never holds it there — every point has a well, so the field states a
    /// null the array never carries, rather than a second setting for a
    /// value that would go unused.
    #[param(default = -9999i64, optional, group = "operation")]
    pub null_value: i64,
    /// read the frames at this rank of the BRGM chart — its level less one: 3 system, 4 series, 5 sub-series, 6 stage, 7 substage
    ///
    /// Each interval is then read **against the divisions of that rank**
    /// rather than by the unit its frame names, so the limit between two
    /// divisions of one stage is no contact: the base of the Lutetian is where
    /// the Lutetian ends, whether the pass above it is « Lutétien », « Lutétien
    /// moyen » or « Lutétien inférieur ». Alone,
    /// every division of that rank the frames reach — a finer one read as the
    /// coarsest division of that rank above it; with `unit`, that one alone,
    /// which must be of this rank. The number is the `RankInStratigraphicColumn`
    /// `stratigraphy-from-geolts` writes. Left out with `unit`, the unit's own;
    /// left out alone, the units the frames name, as before.
    #[param(min = 0.0, max = 9.0, group = "operation")]
    pub rank: Option<usize>,
    /// only this division of the BRGM chart — its top and its base, whatever finer divisions the frames name
    ///
    /// Its label, its code or its id (`1255`, the Lutetian). An interval is
    /// inside it when the chart places its division under it, outside it when
    /// the chart's ages do not overlap, and neither otherwise — a coarser
    /// division that holds it, a regional one whose time overlaps, no age —
    /// and no contact is read across that. A frame without an interval inside
    /// it gives no point, and is counted.
    #[param(group = "operation")]
    pub unit: Option<ChartTerm>,
    /// with `rank` or `unit`: the language the chart's divisions are titled in — the one the units were written in, so the same setting as `stratigraphy-from-geolts` and `wells-and-properties-from-table`
    #[param(default = super::stratigraphy::Language::Fr, optional)]
    pub language: super::stratigraphy::Language,
}

/// What it takes.
#[derive(Debug, Inputs)]
pub struct ContactsInputs {
    /// the wellbore frames whose intervals name their units
    ///
    /// **Left empty, every frame of the package** — the declaration of
    /// `wells-from-table`. A frame that maps no interval to a unit is counted
    /// and passed over.
    #[input(least = 1, types = "resqml22.WellboreFrameRepresentation", every)]
    pub frames: Vec<Shape>,
    /// the properties of those frames whose values the points take
    ///
    /// **Left empty, every property of the package**, each kept only if it
    /// hangs on one of the frames — the others are counted and passed over.
    /// Named, only those are copied: the way to settle a wellbore that carries
    /// two properties of one measure.
    ///
    /// The five property types of RESQML 2.2 are taken, not only the two that
    /// are copied: a boolean, a comment or a points property on a frame is then
    /// set aside **by name**, where a narrower declaration would leave it out
    /// without a word.
    #[input(
        types = "resqml22.ContinuousProperty, resqml22.DiscreteProperty, resqml22.BooleanProperty, resqml22.CommentProperty, resqml22.PointsProperty",
        every
    )]
    pub properties: Vec<ObjectIn>,
}

/// One side of a unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Side {
    Top,
    Base,
}

impl Side {
    fn word(self) -> &'static str {
        match self {
            Side::Top => "top",
            Side::Base => "base",
        }
    }
}

/// Where a point came from: which frame, which node, and the interval on the
/// unit's side of that node.
#[derive(Debug, Clone, Copy)]
struct Origin {
    /// Its rank among the frames that gave contacts — see [`Used`].
    frame: usize,
    /// The node the point is.
    node: usize,
    /// The unit's own interval at that node: its **first** for a top, its
    /// **last** for a base.
    interval: usize,
}

/// The points of one side of one unit, and where each came from — two lists
/// of one length, in the order of the frames.
struct Found {
    unit: Ref,
    points: Vec<[f64; 3]>,
    origins: Vec<Origin>,
}

impl Found {
    fn of(unit: &Ref) -> Found {
        Found {
            unit: unit.clone(),
            points: Vec::new(),
            origins: Vec::new(),
        }
    }
}

/// The points of each side of each unit, by the unit's uuid.
type Contacts = BTreeMap<(String, Side), Found>;

/// A frame that gave contacts — what a point's [`Origin`] names.
struct Used {
    /// Its uuid, lower-cased: what a property's `SupportingRepresentation`
    /// names.
    uuid: Option<String>,
    /// The wellbore's name — its title without the writer's suffix, what a
    /// property's title starts with.
    well: String,
    /// How many nodes it places.
    nodes: usize,
}

/// A point set written, for the copies that go on it.
struct Written {
    key: (String, Side),
    uuid: String,
    title: String,
}

impl Operation for ContactsFromFrames {
    type Params = ContactsParams;
    type Inputs = ContactsInputs;

    fn gives(&self) -> Vec<Output> {
        let mut out = vec![
            Output {
                role: "feature",
                types: &["resqml22.BoundaryFeature"],
                least: 1,
                most: None,
                describe: "one boundary feature per side (top, base) of each unit, titled after the unit",
            },
            Output {
                role: "interpretation",
                types: &["resqml22.HorizonInterpretation"],
                least: 1,
                most: None,
                describe: "one horizon interpretation per side of each unit, over its boundary feature",
            },
            Output {
                role: "representation",
                types: &["resqml22.PointSetRepresentation"],
                least: 1,
                most: None,
                describe: "the contact points: a point set per side of each unit, one point per wellbore that crosses it",
            },
            Output {
                role: "continuous-property",
                types: &["resqml22.ContinuousProperty"],
                least: 0,
                most: None,
                describe: "a wellbores' continuous property copied onto a point set (`copyProperties`)",
            },
            Output {
                role: "discrete-property",
                types: &["resqml22.DiscreteProperty"],
                least: 0,
                most: None,
                describe: "a wellbores' discrete property copied onto a point set (`copyProperties`), and the property naming each point's well (`wellOrigin`)",
            },
            Output {
                role: "category-lookup",
                types: &["eml23.ColumnBasedTable"],
                least: 0,
                most: Some(1),
                describe: "the table of wells' names the well-origin property's codes are ranks in (`wellOrigin`)",
            },
        ];
        out.extend(super::wells_from_table::frame_outputs(true));
        out
    }

    fn about(&self) -> About {
        About {
            name: "contacts-from-frames",
            describe: "the top and the base of each unit the wellbore frames cross, as two point sets per unit — where their intervals name their units, or read at a rank of the BRGM chart — with what the wells' properties hold there",
            category: Category::Wells,
            makes: &[
                "resqml22.BoundaryFeature",
                "resqml22.HorizonInterpretation",
                "resqml22.PointSetRepresentation",
                "resqml22.ContinuousProperty",
                "resqml22.DiscreteProperty",
            ],
            recipe: "contacts-from-frames",
            represents: None,
        }
    }

    /// The frames' own system: the points are their nodes.
    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        super::written::suggest_written(bag, None, slots);
    }

    fn reports(&self) -> Vec<Slot> {
        support::frame_reports(vec![
            Slot::optional(
                "pointSetCount",
                Need::Number {
                    uom: None,
                    least: None,
                    most: None,
                },
                "point sets written, two per unit at most",
            ),
            Slot::optional(
                "propertyCount",
                Need::Number {
                    uom: None,
                    least: None,
                    most: None,
                },
                "properties copied onto the point sets from the wellbores' own",
            ),
        ])
    }

    fn plan(
        &self,
        inputs: ContactsInputs,
        params: ContactsParams,
        cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        if inputs.frames.is_empty() {
            return Err(Refusal::on(
                "frames",
                "it takes wellbore frames; the input holds none, and the package gives none either",
            ));
        }
        // **How each interval is read**: by the unit its frame names, or — told
        // a rank or a unit of the chart — against the division of that rank.
        // Refused before any frame is read: a rank the chart does not have is
        // the setting's fault, not the frames'.
        let mut reading = Reading::of(params.rank, params.unit.as_ref(), params.language)?;
        let reading_seed = reading.seed();
        let mut rank = 0usize;
        let frame = support::frame_in(&cx.known(), &params.frame, cx.identity, cx.when, &mut rank)?;

        // Every contact, grouped by unit and side, in the order of the frames.
        let gather = tracing::info_span!("contacts.gather", frames = inputs.frames.len()).entered();
        let mut found: Contacts = BTreeMap::new();
        let mut unmapped = 0usize;
        let mut used: Vec<Used> = Vec::new();
        for one in &inputs.frames {
            let Some(known) = one.known.as_ref() else {
                unmapped += 1;
                continue;
            };
            let title = known
                .reference
                .as_ref()
                .map(|r| r.title.clone())
                .unwrap_or_else(|| one.name.clone());
            let Some(units) = known.interval_units.as_ref() else {
                unmapped += 1;
                continue;
            };
            let nodes: Vec<[f64; 3]> = one
                .geometry
                .patches
                .iter()
                .flat_map(|patch| patch.points.iter().copied())
                .collect();
            if nodes.len() != units.len() + 1 {
                return Err(Refusal::on(
                    "frames",
                    format!(
                        "`{title}` has {} node(s) for {} interval(s) — an interval lies between two nodes, so where its contacts are cannot be read",
                        nodes.len(),
                        units.len()
                    ),
                ));
            }
            let from = used.len();
            used.push(Used {
                uuid: known.reference.as_ref().map(|r| r.uuid.to_ascii_lowercase()),
                well: energyml_domain::wellbore_table::stem(&title).to_string(),
                nodes: nodes.len(),
            });
            // One sequence of units per reading of the frame, each as long as
            // its intervals: the frame's own when named; at a rank, one per
            // division read — inside it, its neighbour outside, `None` for
            // neither — so a contact is the same node found the same way.
            for units in reading.sequences(units, &title) {
                // Runs of one unit, and whether one comes back.
                let mut seen: Vec<&str> = Vec::new();
                let mut at = 0usize;
                while at < units.len() {
                    let Some(unit) = &units[at] else {
                        at += 1;
                        continue;
                    };
                    let mut end = at + 1;
                    while end < units.len() && units[end].as_ref().is_some_and(|next| next.uuid == unit.uuid)
                    {
                        end += 1;
                    }
                    // What lies outside the division read is its neighbour: it
                    // makes the division's contacts and has none of its own.
                    if !reading.keeps(unit) {
                        at = end;
                        continue;
                    }
                    if seen.contains(&unit.uuid.as_str()) {
                        return Err(Refusal::on(
                            "frames",
                            format!(
                                "`{title}` crosses `{}` twice, so it would give two tops in one hole — which one is the unit's top is not for this operation to decide",
                                unit.title
                            ),
                        ));
                    }
                    seen.push(&unit.uuid);
                    // A top where a unit lies above, a base where one lies below —
                    // and the unit's own interval at each: the first that begins
                    // at its top, the last that ends at its base.
                    if at > 0 && units[at - 1].is_some() {
                        let top = found
                            .entry((unit.uuid.clone(), Side::Top))
                            .or_insert_with(|| Found::of(unit));
                        top.points.push(nodes[at]);
                        top.origins.push(Origin {
                            frame: from,
                            node: at,
                            interval: at,
                        });
                    }
                    if end < units.len() && units[end].is_some() {
                        let base = found
                            .entry((unit.uuid.clone(), Side::Base))
                            .or_insert_with(|| Found::of(unit));
                        base.points.push(nodes[end]);
                        base.origins.push(Origin {
                            frame: from,
                            node: end,
                            interval: end - 1,
                        });
                    }
                    at = end;
                }
            }
        }
        let frames_used = used.len();
        drop(gather);
        if found.is_empty() {
            // At a rank, the refusal names the division and what the frames
            // gave it; named, it is the one below.
            if let Some(refusal) = reading.nothing(inputs.frames.len(), unmapped) {
                return Err(refusal);
            }
            return Err(Refusal::on(
                "frames",
                format!(
                    "no contact between two units in the {} frame(s) given — {unmapped} of them map no interval to a unit; `wells-and-properties-from-table` writes that mapping with `units`",
                    inputs.frames.len()
                ),
            ));
        }

        let citation = |title: &str| eml::Citation {
            title: title.to_string(),
            originator: cx.when.originator.clone(),
            creation: cx.when.creation.clone(),
            format: energyml_core::CITATION_FORMAT.to_string(),
            ..Default::default()
        };
        let write_sets = tracing::info_span!("contacts.point_sets", sets = found.len()).entered();
        let mut objects = frame.objects;
        let mut datasets = Vec::new();
        let mut shapes = Vec::new();
        let mut written: Vec<Written> = Vec::with_capacity(found.len());
        for ((unit_uuid, side), one) in &found {
            let (unit, points) = (&one.unit, &one.points);
            let title = format!("{} — {}", unit.title, side.word());
            // `reading_seed` is empty when named — the seeds of before — and
            // says the rank otherwise: the base of a stage and the base of the
            // passes named after it are two objects.
            let feature_uuid = support::next_uuid(
                cx.identity,
                &format!("contact-feature:{unit_uuid}:{}{reading_seed}", side.word()),
                &mut rank,
            )?;
            let interpretation_uuid = support::next_uuid(
                cx.identity,
                &format!("contact-interpretation:{unit_uuid}:{}{reading_seed}", side.word()),
                &mut rank,
            )?;
            let feature = resqml::BoundaryFeature {
                uuid: feature_uuid.clone(),
                schema_version: "2.2".to_string(),
                citation: citation(&title),
                is_well_known: false,
                ..Default::default()
            };
            let horizon = resqml::HorizonInterpretation {
                uuid: interpretation_uuid.clone(),
                schema_version: "2.2".to_string(),
                citation: citation(&title),
                domain: Some(resqml::Domain::Depth),
                interpreted_feature: eml::DataObjectReference {
                    uuid: feature_uuid.clone(),
                    qualified_type: "resqml22.BoundaryFeature".to_string(),
                    title: title.clone(),
                    ..Default::default()
                },
                ..Default::default()
            };
            // The points of this side, in the order of the frames — the same
            // content, the same object.
            let digest = points
                .iter()
                .map(|p| format!("{:?},{:?},{:?}", p[0], p[1], p[2]))
                .collect::<Vec<_>>()
                .join(";");
            let set_uuid = support::next_uuid(
                cx.identity,
                &format!(
                    "contact-points:{unit_uuid}:{}{reading_seed}:{digest}",
                    side.word()
                ),
                &mut rank,
            )?;
            let path = format!("/resqml22/{set_uuid}/points");
            let mut set = json!({
                "$type": "resqml22.PointSetRepresentation",
                "uuid": set_uuid,
                "schemaVersion": "2.2",
                "Citation": support::citation(cx.when, &title),
                "NodePatchGeometry": [support::point_geometry(&frame.crs, &params.data_file, &path, points.len())],
            });
            set["RepresentedObject"] = json!({
                "$type": "eml23.DataObjectReference",
                "Uuid": interpretation_uuid,
                "QualifiedType": "resqml22.HorizonInterpretation",
                "Title": title,
            });
            written.push(Written {
                key: (unit_uuid.clone(), *side),
                uuid: set_uuid.clone(),
                title: title.clone(),
            });
            objects.push(NewObject {
                uuid: feature_uuid,
                qualified_type: "resqml22.BoundaryFeature".to_string(),
                role: "feature".to_string(),
                title: title.clone(),
                json: energyml_core::json::write_value(&feature),
            });
            objects.push(NewObject {
                uuid: interpretation_uuid,
                qualified_type: "resqml22.HorizonInterpretation".to_string(),
                role: "interpretation".to_string(),
                title: title.clone(),
                json: energyml_core::json::write_value(&horizon),
            });
            objects.push(NewObject {
                uuid: set_uuid.clone(),
                qualified_type: "resqml22.PointSetRepresentation".to_string(),
                role: "representation".to_string(),
                title: title.clone(),
                json: set,
            });
            let shape = support::made_root(
                &title,
                &set_uuid,
                support::one_patch(
                    "pointset",
                    points.clone(),
                    energyml_geometry::geometry::Topology::Points,
                ),
            );
            datasets.push(Dataset {
                file: params.data_file.clone(),
                path,
                dimensions: vec![points.len() as i64, 3],
                values: Numbers::F64(support::flat_points(&shape.geometry)),
            });
            shapes.push(shape);
        }
        let sets = written.len();
        drop(write_sets);

        // **After every point set**, so that the ranks the point sets draw
        // from a given list of uuids are the ones they drew before copies
        // existed.
        let copy_span =
            tracing::info_span!("contacts.copy_properties", enabled = params.copy_properties).entered();
        let copied = if params.copy_properties {
            copy(
                &inputs.properties,
                &used,
                &found,
                &written,
                &params,
                cx,
                &mut rank,
            )?
        } else {
            Copied::off(&inputs.properties, &used)
        };
        drop(copy_span);
        objects.extend(copied.objects);
        datasets.extend(copied.datasets);

        // **After every point set, and after the copies** — same reason as
        // both: the point sets' uuids do not move when this is toggled.
        let named = if params.well_origin {
            name_wells(&used, &found, &written, &params, cx, &mut rank)?
        } else {
            Named::off()
        };
        objects.extend(named.objects);
        datasets.extend(named.datasets);

        let mut says = vec![Note {
            key: "contacts",
            detail: format!(
                "{sets} point set(s) — a top and a base per unit where a unit lies on the other side — from {frames_used} frame(s)"
            ),
        }];
        if unmapped > 0 {
            says.push(Note {
                key: "frames-unmapped",
                detail: format!("{unmapped} frame(s) map no interval to a unit, and were passed over"),
            });
        }
        // At a rank: what was read, which frames reach the division, and what
        // was read as neither inside nor outside — each counted and named.
        says.extend(reading.says());
        says.extend(copied.says);
        says.extend(named.says);
        // What the frame must say of itself — a `ProjectedCrs` taken as it
        // stands, degrees — and nothing when it is a compound.
        says.extend(frame.says);
        let mut parameters = frame.parameters;
        parameters.push(Parameter::Double {
            title: "pointSetCount".to_string(),
            value: sets as f64,
            uom: None,
        });
        parameters.push(Parameter::Double {
            title: "propertyCount".to_string(),
            value: copied.written as f64,
            uom: None,
        });
        Ok(Plan {
            media: Vec::new(),
            blocked: frame.blocked.clone(),
            objects,
            datasets,
            shapes,
            activity: ActivityDraft {
                recipe: "contacts-from-frames",
                title: format!("contacts-from-frames → {sets} point set(s)"),
                seed: format!("contacts:{frames_used}:{sets}{reading_seed}"),
                parameters,
            },
            says,
            dropped: copied.dropped,
        })
    }
}

// -- the wells' properties ---------------------------------------------------

/// What copying the wells' properties produced.
#[derive(Default)]
struct Copied {
    objects: Vec<NewObject>,
    datasets: Vec<Dataset>,
    says: Vec<Note>,
    dropped: Vec<Dropped>,
    /// Properties written.
    written: usize,
}

impl Copied {
    /// Nothing copied — `copyProperties` is off — and how much that left.
    ///
    /// Said rather than silent: a run that copies nothing and a run over
    /// wells that carry nothing would otherwise read the same.
    fn off(properties: &[ObjectIn], used: &[Used]) -> Copied {
        let left = properties
            .iter()
            .filter(|one| {
                one.known
                    .property
                    .as_ref()
                    .and_then(|facts| frame_of(facts, used))
                    .is_some()
            })
            .count();
        let mut out = Copied::default();
        if left > 0 {
            out.says.push(Note {
                key: "properties-off",
                detail: format!(
                    "{left} propert(ies) hang on these frames and were not copied: `copyProperties` is off"
                ),
            });
        }
        out
    }
}

/// What one copied property is, whichever wellbore carries it: **one property
/// per key** on each point set.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    /// The property kind's uuid, lower-cased.
    kind: String,
    /// The title without the wellbore — see [`measure_of`].
    measure: String,
    /// `nodes` or `intervals`.
    element: String,
    /// The facets, as written — `porosity` and `porosity, max` are two.
    facets: String,
}

/// The values of one frame's property, its nulls already told apart: `None`
/// where the source is null — NaN, or the integer its array declares.
enum Cells {
    Real(Vec<Option<f64>>),
    Code(Vec<Option<i64>>),
}

/// One value of a source.
#[derive(Debug, Clone, Copy)]
enum Number {
    Real(f64),
    Code(i64),
}

impl Cells {
    /// The value at that rank — `None` for a null, or a rank past the end.
    fn at(&self, row: usize) -> Option<Number> {
        match self {
            Cells::Real(values) => values.get(row).copied().flatten().map(Number::Real),
            Cells::Code(values) => values.get(row).copied().flatten().map(Number::Code),
        }
    }
}

/// A property that can be copied: its kind, whether it is discrete, and its
/// values.
struct Judged<'a> {
    kind: &'a Ref,
    discrete: bool,
    cells: Cells,
}

/// Everything gathered under one [`Key`], over every frame — and what must
/// agree for one property to hold all of it.
struct Measure<'a> {
    /// What the copies are titled after.
    name: String,
    kind: &'a Ref,
    discrete: bool,
    uom: Option<&'a str>,
    lookup: Option<&'a Ref>,
    facets: &'a [Value],
    /// On the frames' intervals rather than their nodes.
    intervals: bool,
    /// The values, by the frame they come from ([`Used`] rank), and the title
    /// of the property that held them.
    by_frame: BTreeMap<usize, (&'a str, Cells)>,
    /// Every property gathered here, named — what a refusal lists.
    sources: Vec<String>,
    /// Why this key is not copied, when it is not.
    conflict: Option<String>,
}

impl<'a> Measure<'a> {
    fn new(key: &Key, judged: &Judged<'a>, facts: &'a KnownProperty) -> Measure<'a> {
        Measure {
            name: key.measure.clone(),
            kind: judged.kind,
            discrete: judged.discrete,
            uom: facts.uom.as_deref(),
            lookup: facts.lookup.as_ref(),
            facets: &facts.facets,
            intervals: facts.indexable == "intervals",
            by_frame: BTreeMap::new(),
            sources: Vec::new(),
            conflict: None,
        }
    }

    /// One more wellbore's property under this key — or the reason the key
    /// cannot hold it, which then stands for the whole key: a measure copied
    /// for some wells and refused for others would read as wells without it.
    fn add(
        &mut self,
        at: usize,
        frame: &Used,
        title: &'a str,
        facts: &'a KnownProperty,
        judged: Judged<'a>,
        null: i64,
    ) {
        self.sources.push(format!("`{title}`"));
        if self.conflict.is_some() {
            return;
        }
        let name = self.name.clone();
        let lower = |reference: Option<&Ref>| reference.map(|one| one.uuid.to_ascii_lowercase());
        let table = |reference: Option<&Ref>| {
            reference
                .map(|one| format!("`{}`", one.title))
                .unwrap_or_else(|| "none".to_string())
        };
        self.conflict = if judged.discrete != self.discrete {
            Some(format!(
                "`{name}` is continuous for one wellbore and discrete for another — one name, two measurements"
            ))
        } else if facts.uom.as_deref() != self.uom {
            Some(format!(
                "`{name}` is in {} for one wellbore and in {} for another — a property holds one unit, and converting would choose it for somebody",
                self.uom.unwrap_or("no unit"),
                facts.uom.as_deref().unwrap_or("no unit")
            ))
        } else if lower(self.lookup) != lower(facts.lookup.as_ref()) {
            Some(format!(
                "the codes of `{name}` are ranks in two tables, {} and {} — a property names one table, and translating a code from one to the other is not this operation's to decide",
                table(self.lookup),
                table(facts.lookup.as_ref())
            ))
        } else if let Some((other, _)) = self.by_frame.get(&at) {
            Some(format!(
                "`{}` carries two `{name}`, `{other}` and `{title}` — which one is the wellbore's cannot be guessed; name the one to copy in `properties`",
                frame.well
            ))
        } else if let Cells::Code(codes) = &judged.cells
            && codes.contains(&Some(null))
        {
            Some(format!(
                "`{title}` holds the code {null}, which is also the null this run declares — choose another `nullValue`"
            ))
        } else {
            None
        };
        if self.conflict.is_none() {
            self.by_frame.insert(at, (title, judged.cells));
        }
    }
}

/// Properties not copied, by cause — each one named.
#[derive(Default)]
struct Aside(BTreeMap<String, Vec<String>>);

impl Aside {
    fn add(&mut self, cause: impl Into<String>, what: &str) {
        self.0.entry(cause.into()).or_default().push(format!("`{what}`"));
    }

    fn into_dropped(self) -> Vec<Dropped> {
        self.0
            .into_iter()
            .map(|(cause, names)| Dropped {
                count: names.len(),
                why: format!("{}: not copied — {cause}", listed(&names)),
            })
            .collect()
    }
}

/// A few names and how many more: a list of two hundred is not read.
fn listed(names: &[String]) -> String {
    let shown = names.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
    if names.len() > 3 {
        format!("{shown} and {} more", names.len() - 3)
    } else {
        shown
    }
}

/// The class of a qualified type — `DiscreteProperty` of `resqml22.DiscreteProperty`.
fn class_of(qualified_type: &str) -> &str {
    qualified_type.rsplit('.').next().unwrap_or(qualified_type)
}

/// The rank, among the frames that gave contacts, of the one this property
/// hangs on.
fn frame_of(facts: &KnownProperty, used: &[Used]) -> Option<usize> {
    let uuid = facts.supporting.as_ref()?.uuid.to_ascii_lowercase();
    used.iter()
        .position(|frame| frame.uuid.as_deref() == Some(uuid.as_str()))
}

/// What a property measures, without the wellbore that carries it.
///
/// **The rule of `energyml_domain::wellbore_table`**, where it is private
/// (`heading`): a writer titles a property `« <well> — <measure> »`, and the
/// prefix is removed **only when it is this wellbore's** — cutting at the first
/// separator would make `« porosity — corrected »` a measure named `corrected`.
/// `F1 — PHI` and `F2 — PHI` are then one measure, `PHI`, which is what lets one
/// property hold both wells.
fn measure_of(title: &str, well: &str) -> String {
    if !well.is_empty() {
        for cut in [" — ", " : ", " - "] {
            if let Some(rest) = title.strip_prefix(well).and_then(|rest| rest.strip_prefix(cut)) {
                return rest.to_string();
            }
        }
    }
    title.to_string()
}

/// A frame's property as a column of that frame — or the cause it is not
/// copied. Every cause is a reason **not to read**, never a reading made
/// anyway: a value taken from the wrong component, step or rank looks exactly
/// like a right one.
fn judge<'a>(reference: &Ref, facts: &'a KnownProperty, nodes: usize) -> Result<Judged<'a>, String> {
    if !reference.qualified_type.starts_with("resqml22.") {
        return Err(format!(
            "it is a `{}`, and the copies are RESQML 2.2 properties: another release is not translated here",
            reference.qualified_type
        ));
    }
    let discrete = match class_of(&reference.qualified_type) {
        "ContinuousProperty" => false,
        "DiscreteProperty" => true,
        "BooleanProperty" => {
            return Err("a flag has no null, so a point whose wellbore lacks it could only be written true or false — a value nobody stated".to_string());
        }
        "CommentProperty" => {
            return Err(
                "a comment is text, and this operation copies values, continuous and discrete".to_string(),
            );
        }
        "PointsProperty" => {
            return Err(
                "it holds positions, not values, and a point set's nodes are already its positions"
                    .to_string(),
            );
        }
        other => {
            return Err(format!(
                "a `{other}` is not copied: this operation copies continuous and discrete properties"
            ));
        }
    };
    let values = facts
        .values
        .as_ref()
        .map_err(|why| format!("its values did not read — {why}"))?;
    if facts.timed {
        return Err("it is indexed in time — one value per node and per step, and which step a contact is at is stated nowhere".to_string());
    }
    if facts.realizations {
        return Err(
            "it holds several realizations, and which one a contact takes is stated nowhere".to_string(),
        );
    }
    let per_element: u64 = facts.value_count.iter().product();
    if per_element > 1 {
        return Err(format!(
            "it holds {per_element} values per element — a vector or a tensor — and which component belongs to a point is not for this operation to pick"
        ));
    }
    let rows = match facts.indexable.as_str() {
        "nodes" => nodes,
        "intervals" => nodes.saturating_sub(1),
        other => {
            return Err(format!(
                "its values hang on `{other}`, which is neither a node nor an interval of the frame"
            ));
        }
    };
    let Some(kind) = facts.kind.as_ref() else {
        return Err("it names no property kind by reference, and a RESQML 2.2 property must".to_string());
    };
    // `Uom` is required on a 2.2 continuous property, and an enumeration: a
    // copy written without one would read back as the schema's first unit.
    if !discrete && facts.uom.as_deref().is_none_or(str::is_empty) {
        return Err(
            "it states no unit, and a copy without one would read back in the schema's first unit"
                .to_string(),
        );
    }
    if values.len() != rows {
        return Err(format!(
            "it holds {} value(s) where the frame has {rows} {} — read here, each point would take a neighbour's value",
            values.len(),
            facts.indexable
        ));
    }
    let null = facts.null_value;
    let cells = match (discrete, values) {
        (true, Values::Integer(codes)) => Cells::Code(
            codes
                .iter()
                .map(|code| (Some(*code) != null).then_some(*code))
                .collect(),
        ),
        (true, _) => return Err("it is discrete and its values are not integers".to_string()),
        (false, Values::Floating(reals)) => {
            Cells::Real(reals.iter().map(|one| (!one.is_nan()).then_some(*one)).collect())
        }
        (false, Values::Integer(whole)) => Cells::Real(
            whole
                .iter()
                .map(|one| (Some(*one) != null).then_some(*one as f64))
                .collect(),
        ),
        (false, _) => return Err("it is continuous and its values are not numbers".to_string()),
    };
    Ok(Judged {
        kind,
        discrete,
        cells,
    })
}

/// The wells' properties, copied onto the point sets.
///
/// Two passes, and the order is the subject: **every** property of every
/// frame is gathered under its key first, so that the set of copies is known
/// before one is written — a point set whose wells lack a measure another well
/// carries gets it anyway, null where it is missing.
fn copy(
    properties: &[ObjectIn],
    used: &[Used],
    found: &Contacts,
    written: &[Written],
    params: &ContactsParams,
    cx: &Context<'_>,
    rank: &mut usize,
) -> Result<Copied, Refusal> {
    let mut out = Copied::default();
    let mut aside = Aside::default();
    let mut elsewhere = 0usize;
    let mut measures: BTreeMap<Key, Measure<'_>> = BTreeMap::new();
    for input in properties {
        let Some(reference) = input.known.reference.as_ref() else {
            continue;
        };
        let Some(facts) = input.known.property.as_ref() else {
            // **Not every object in this slot is a property.** The binder
            // hands it every object piece no slot is named after, and the front end
            // sends the CRS picked in the form back as one (`known`): that is
            // an answer, not a property this run failed to copy. A *property*
            // the boundary could not describe is another matter, and named.
            if class_of(&reference.qualified_type).ends_with("Property") {
                aside.add(
                    "it was not described as a property at the boundary",
                    &reference.title,
                );
            }
            continue;
        };
        let Some(at) = frame_of(facts, used) else {
            elsewhere += 1;
            continue;
        };
        let frame = &used[at];
        let judged = match judge(reference, facts, frame.nodes) {
            Ok(judged) => judged,
            Err(cause) => {
                aside.add(cause, &reference.title);
                continue;
            }
        };
        let key = Key {
            kind: judged.kind.uuid.to_ascii_lowercase(),
            measure: measure_of(&reference.title, &frame.well),
            element: facts.indexable.clone(),
            facets: facts
                .facets
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join(";"),
        };
        let measure = measures
            .entry(key)
            .or_insert_with_key(|key| Measure::new(key, &judged, facts));
        measure.add(at, frame, &reference.title, facts, judged, params.null_value);
    }
    for measure in measures.values() {
        if let Some(why) = &measure.conflict {
            out.dropped.push(Dropped {
                count: measure.sources.len(),
                why: format!("{}: not copied — {why}", listed(&measure.sources)),
            });
        }
    }
    out.dropped.extend(aside.into_dropped());

    let (mut absent, mut unstated, mut empty) = (0usize, 0usize, 0usize);
    for set in written {
        let Some(one) = found.get(&set.key) else { continue };
        for (key, measure) in measures.iter().filter(|(_, measure)| measure.conflict.is_none()) {
            // One cell per point, in the order of the points.
            let (mut lacking, mut null) = (0usize, 0usize);
            let cells: Vec<Option<Number>> = one
                .origins
                .iter()
                .map(|origin| {
                    let Some((_, cells)) = measure.by_frame.get(&origin.frame) else {
                        lacking += 1;
                        return None;
                    };
                    let row = if measure.intervals {
                        origin.interval
                    } else {
                        origin.node
                    };
                    let cell = cells.at(row);
                    if cell.is_none() {
                        null += 1;
                    }
                    cell
                })
                .collect();
            if cells.iter().all(Option::is_none) {
                empty += 1;
                continue;
            }
            absent += lacking;
            unstated += null;
            let (object, dataset) = write_one(set, key, measure, &cells, params, cx, rank)?;
            out.objects.push(object);
            out.datasets.push(dataset);
            out.written += 1;
        }
    }

    let kept = measures
        .values()
        .filter(|measure| measure.conflict.is_none())
        .count();
    out.says.push(Note {
        key: "properties",
        detail: format!(
            "{} propert(ies) copied onto the point sets — {kept} measure(s) of the wellbores, one property per kind, name and element",
            out.written
        ),
    });
    if out.written > 0 {
        // **Said, always**: an array of nulls and an array of values have the
        // same shape in the file, and only this count tells them apart.
        out.says.push(Note {
            key: "properties-null",
            detail: format!(
                "{} value(s) written as null: {absent} where the wellbore does not carry the property, {unstated} where its own value is null",
                absent + unstated
            ),
        });
    }
    if empty > 0 {
        out.says.push(Note {
            key: "properties-empty",
            detail: format!(
                "{empty} propert(ies) left off a point set where every value would have been null"
            ),
        });
    }
    let refused: usize = out.dropped.iter().map(|one| one.count).sum();
    if refused > 0 {
        out.says.push(Note {
            key: "properties-aside",
            detail: format!("{refused} propert(ies) of these frames not copied, each named with its reason"),
        });
    }
    if elsewhere > 0 {
        out.says.push(Note {
            key: "properties-elsewhere",
            detail: format!(
                "{elsewhere} propert(ies) hang on no frame this run extracted from, and were passed over"
            ),
        });
    }
    Ok(out)
}

/// One copy on one point set: the object and its array.
///
/// **The shape is the one `properties-from-table` writes** —
/// `ValuesForPatch` and not `Values`, `ValueCountPerIndexableElement` a list,
/// the integer null on the array — taken from that writer rather than from
/// memory: written from memory the first time there, it gave 1 680 objects
/// `energyml check` refused, and a plan cannot say so.
///
/// Its uuid comes from its content: the point set, the key and the
/// values — the same run writes the same object, a changed value a second one.
fn write_one(
    set: &Written,
    key: &Key,
    measure: &Measure<'_>,
    cells: &[Option<Number>],
    params: &ContactsParams,
    cx: &Context<'_>,
    rank: &mut usize,
) -> Result<(NewObject, Dataset), Refusal> {
    let title = format!("{} — {}", set.title, measure.name);
    let (values, digest) = if measure.discrete {
        let codes: Vec<i64> = cells
            .iter()
            .map(|cell| match cell {
                Some(Number::Code(code)) => *code,
                _ => params.null_value,
            })
            .collect();
        let digest = codes.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
        (Numbers::I64(codes), digest)
    } else {
        let reals: Vec<f64> = cells
            .iter()
            .map(|cell| match cell {
                Some(Number::Real(value)) => *value,
                _ => f64::NAN,
            })
            .collect();
        // `{:?}` rereads as the same double, so two arrays that differ in the
        // last bit are two contents; a NaN is an absence and is written as one.
        let digest = reals
            .iter()
            .map(|value| {
                if value.is_nan() {
                    "·".to_string()
                } else {
                    format!("{value:?}")
                }
            })
            .collect::<Vec<_>>()
            .join(",");
        (Numbers::F64(reals), digest)
    };
    let uuid = support::next_uuid(
        cx.identity,
        &format!(
            "contact-property:{}:{}|{}|{}|{}:{digest}",
            set.uuid, key.kind, key.measure, key.element, key.facets
        ),
        rank,
    )?;
    let count = values.len();
    let path = format!("/resqml22/{uuid}/values");
    let array = support::external(&params.data_file, &path, &[count as i64]);
    let (qualified_type, role, patch) = if measure.discrete {
        (
            "resqml22.DiscreteProperty",
            "discrete-property",
            json!({
                "$type": "eml23.IntegerExternalArray",
                "ArrayIntegerType": "arrayOfInt64LE",
                "NullValue": params.null_value,
                "CountPerValue": 1,
                "Values": array,
            }),
        )
    } else {
        (
            "resqml22.ContinuousProperty",
            "continuous-property",
            json!({
                "$type": "eml23.FloatingPointExternalArray",
                "ArrayFloatingPointType": "arrayOfDouble64LE",
                "CountPerValue": 1,
                "Values": array,
            }),
        )
    };
    let mut object = json!({
        "$type": qualified_type,
        "uuid": uuid,
        "schemaVersion": "2.2",
        "Citation": support::citation(cx.when, &title),
        // On the point set, a point **is** a node, whatever the source hung on:
        // each has taken the one value of its side.
        "IndexableElement": "nodes",
        "ValueCountPerIndexableElement": [1],
        // The kind the sources name — a reference, never a copy.
        "PropertyKind": measure.kind.dor(),
        "SupportingRepresentation": {
            "$type": "eml23.DataObjectReference",
            "Uuid": set.uuid,
            "QualifiedType": "resqml22.PointSetRepresentation",
            "Title": set.title,
        },
        "ValuesForPatch": [patch],
    });
    if let Some(uom) = measure.uom.filter(|_| !measure.discrete) {
        object["Uom"] = json!(uom);
    }
    // **The same table**, so a code keeps its meaning: it is read against the
    // very rows it was a rank in.
    if let Some(lookup) = measure.lookup.filter(|_| measure.discrete) {
        object["CategoryLookup"] = lookup.dor();
    }
    if !measure.facets.is_empty() {
        object["Facet"] = Value::Array(measure.facets.to_vec());
    }
    Ok((
        NewObject {
            uuid,
            qualified_type: qualified_type.to_string(),
            role: role.to_string(),
            title,
            json: object,
        },
        Dataset {
            file: params.data_file.clone(),
            path,
            dimensions: vec![count as i64],
            values,
        },
    ))
}

// -- naming each point after its well ---------------------------------------

/// The distinct wells seen so far, and the code each was given.
///
/// **The order of encounter, never the alphabet**: a code is the well's rank
/// among the frames that gave contacts, so two runs over the same frames in
/// the same order write the same table — the rule `properties-from-table`'s
/// own `Categories` follows for a text column, applied to a name this module
/// already knows rather than one read from a file.
#[derive(Default)]
struct WellNames {
    order: Vec<String>,
}

impl WellNames {
    /// This well's code — assigned at the first frame that names it.
    fn code(&mut self, well: &str) -> i64 {
        match self.order.iter().position(|one| one == well) {
            Some(at) => at as i64,
            None => {
                self.order.push(well.to_string());
                (self.order.len() - 1) as i64
            }
        }
    }
}

/// What naming each point after its well produced.
#[derive(Default)]
struct Named {
    objects: Vec<NewObject>,
    datasets: Vec<Dataset>,
    says: Vec<Note>,
}

impl Named {
    /// `wellOrigin` is off.
    fn off() -> Named {
        Named {
            says: vec![Note {
                key: "well-origin-off",
                detail: "`wellOrigin` is off: no point set names the well each of its points came from"
                    .to_string(),
            }],
            ..Default::default()
        }
    }
}

/// Name every point of every point set after the well it came from — see
/// "Naming each point after its well" in the module doc.
///
/// **One table, shared by every point set of this run**, written once beside
/// the first property that needs it: a code has to mean the same well
/// wherever it is read, and a table per point set would let two of them
/// disagree about what `0` names.
fn name_wells(
    used: &[Used],
    found: &Contacts,
    written: &[Written],
    params: &ContactsParams,
    cx: &Context<'_>,
    rank: &mut usize,
) -> Result<Named, Refusal> {
    // One code per frame, in the order `used` already keeps — the same order
    // `Origin::frame` indexes, so a point's code is `well_codes[origin.frame]`
    // with no lookup of its own.
    let mut names = WellNames::default();
    let well_codes: Vec<i64> = used.iter().map(|frame| names.code(&frame.well)).collect();
    if names.order.is_empty() {
        // `frames` was checked non-empty before this runs, so this is not
        // reached in practice; kept so a future caller cannot read a missing
        // table as one with zero rows.
        return Ok(Named::default());
    }

    // **The kind `wells-from-table`'s own `wellName` role already names**
    // (see the module doc): a consumer resolves this the way it resolves
    // that role, rather than a second uuid for the same idea.
    let well = wells_from_table::role("wellName");
    let kind = json!({
        "$type": "eml23.DataObjectReference",
        "Uuid": well.uuid,
        "QualifiedType": "eml23.PropertyKind",
        "Title": well.title,
    });
    let title = well.title.to_string();

    // **The table's identity comes from what it lists**, the same
    // separator `properties-from-table` joins a table's rows with, chosen
    // there because it cannot appear in a value read from a file.
    let joined = names.order.join("\u{1f}");
    let table_uuid = support::next_uuid(cx.identity, &format!("contacts-well-table:{joined}"), rank)?;
    let table = json!({
        "$type": "eml23.ColumnBasedTable",
        "uuid": table_uuid,
        "schemaVersion": "2.3",
        "Citation": support::citation(cx.when, &title),
        // **A single column, in the order of encounter** — the shape
        // `properties-from-table::lookup_table` writes for a text column: no
        // key column, because the value carried by each property already
        // *is* the row's position, not a key looked up against one.
        "Column": [{
            "Title": title,
            "ValueCountPerRow": 1,
            "PropertyKind": kind,
            "Values": {
                "$type": "eml23.StringXmlArray",
                "CountPerValue": 1,
                "Values": names.order.clone(),
            },
        }],
    });
    let table_reference = json!({
        "$type": "eml23.DataObjectReference",
        "Uuid": table_uuid,
        "QualifiedType": "eml23.ColumnBasedTable",
        "Title": title,
    });
    let mut objects = vec![NewObject {
        uuid: table_uuid.clone(),
        qualified_type: "eml23.ColumnBasedTable".to_string(),
        role: "category-lookup".to_string(),
        title: title.clone(),
        json: table,
    }];
    let mut datasets = Vec::new();
    let mut properties = 0usize;

    for set in written {
        let Some(one) = found.get(&set.key) else { continue };
        // Every origin names a frame, and every frame has a code: unlike a
        // copied measurement, this is never null.
        let codes: Vec<i64> = one
            .origins
            .iter()
            .map(|origin| well_codes[origin.frame])
            .collect();
        let digest = codes.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
        let uuid = support::next_uuid(
            cx.identity,
            &format!("contacts-well-origin:{}:{digest}", set.uuid),
            rank,
        )?;
        let count = codes.len();
        let path = format!("/resqml22/{uuid}/values");
        let array = support::external(&params.data_file, &path, &[count as i64]);
        let property_title = format!("{} — well", set.title);
        let object = json!({
            "$type": "resqml22.DiscreteProperty",
            "uuid": uuid,
            "schemaVersion": "2.2",
            "Citation": support::citation(cx.when, &property_title),
            "IndexableElement": "nodes",
            "ValueCountPerIndexableElement": [1],
            "PropertyKind": kind,
            "SupportingRepresentation": {
                "$type": "eml23.DataObjectReference",
                "Uuid": set.uuid,
                "QualifiedType": "resqml22.PointSetRepresentation",
                "Title": set.title,
            },
            "CategoryLookup": table_reference,
            "ValuesForPatch": [{
                "$type": "eml23.IntegerExternalArray",
                "ArrayIntegerType": "arrayOfInt64LE",
                // Declared, as every discrete array here must, though it never
                // occurs: every point's origin is a frame this run read.
                "NullValue": params.null_value,
                "CountPerValue": 1,
                "Values": array,
            }],
        });
        objects.push(NewObject {
            uuid,
            qualified_type: "resqml22.DiscreteProperty".to_string(),
            role: "discrete-property".to_string(),
            title: property_title,
            json: object,
        });
        datasets.push(Dataset {
            file: params.data_file.clone(),
            path,
            dimensions: vec![count as i64],
            values: Numbers::I64(codes),
        });
        properties += 1;
    }

    Ok(Named {
        objects,
        datasets,
        says: vec![Note {
            key: "well-origin",
            detail: format!(
                "{properties} point set(s) named after the well each point came from, against a table of {} well(s)",
                names.order.len()
            ),
        }],
    })
}
