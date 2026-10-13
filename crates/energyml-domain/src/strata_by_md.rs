//! A wellbore's stratigraphy, carried by measured depth onto its **other**
//! representations: the frames that state no unit, and the trajectories.
//!
//! Reported: the colours of the stratigraphy were hard to see on boreholes. A
//! borehole of Lutetia is drawn four times — two frames, two
//! trajectories — and only the frame that carries `IntervalStratigraphicUnits`
//! was painted; the three others, drawn over the same place in a flat colour,
//! left the strata showing in slivers.
//!
//! # The rule
//!
//! [`well_strata::of_frame`] reads what a frame **states**: its intervals,
//! between two MDs, and the unit of each. Another representation of the same
//! wellbore that knows the MD of each of its nodes is coloured by looking each
//! MD up in those intervals. Nothing is interpolated, nothing is averaged: a
//! node takes the unit of the interval it falls in, or nothing.
//!
//! # Which representations are "the same wellbore"
//!
//! The ones that represent the **same object** — `RepresentedObject` (2.2),
//! `RepresentedInterpretation` (2.0.1), compared by uuid. In practice a
//! `WellboreInterpretation`, and it is that rather than its `WellboreFeature`
//! on purpose: RESQML TRG 2.2 §12.14 (p. 311) says a wellbore interpretation
//! *"is relative to one particular well trajectory"*, and that *"One wellbore
//! feature may have multiple wellbore interpretations"* — a planned one and a
//! drilled one (`IsDrilled`). An MD along a planned path is not a place along
//! the drilled hole, so walking up to the feature would paint a plan with the
//! rock the drill met. A representation of a WITSML `Wellbore` (which the TRG
//! lets a frame and a trajectory represent, §12.13 p. 310, §12.17 p. 314) is
//! joined the same way, by the uuid it names.
//!
//! # When two MDs can be compared
//!
//! Energistics common TRG 2.3 §6.48 (`MeasuredDepth`, p. 134): *"A measured
//! depth coordinate in a wellbore. Positive moving from the reference datum
//! toward the bottomhole. All coordinates with the same datum (and same UOM)
//! can be considered to be in the same coordinate reference system (CRS) and
//! are thus directly comparable."* So an MD is read against its datum and its
//! unit, and a frame's are its trajectory's — RESQML TRG 2.2 §12.13 (p. 310),
//! `NodeMd`: *"BUSINESS RULE: MD values and UOM must be consistent with the
//! trajectory representation."* [`MdReference`] is those three things read
//! off the trajectory's `MdInterval` (2.2) or its flat `MdDatum`/`MdUom`
//! (2.0.1), and [`comparable`] is the rule:
//!
//! - **one trajectory** — the representations count from the very same object:
//!   comparable, nothing to check;
//! - otherwise **the same datum object**, by uuid. Two datums at the same place
//!   are two datums: nothing here moves an MD from one to the other;
//! - **the unit**: equal, or converted — a length in feet is a length in
//!   metres, and that is arithmetic, not a guess. A unit this build cannot
//!   read is refused;
//! - **the MD domain** (`MdDomain`, TRG 2.2 §12.8 p. 304): *driller* depths
//!   are *"The original depths recorded while drilling"*, *logger* depths
//!   *"in general considered to be more accurate than driller's depth"* — two
//!   measurements of one hole, which disagree by the stretch of a cable. Two
//!   domains, or one stated and one not, are not compared.
//!
//! A frame that fails the rule is **set aside by name** — never shifted. When
//! none is left, the representation is refused ([`Refusal::Incomparable`]).
//!
//! # A node exactly on a limit
//!
//! It takes the interval **it opens** — the one below it, as MDs increase —
//! and the deepest limit takes the last interval. That is the rule
//! [`Strata::node_colours`] gives the stating frame's own nodes, so looking the
//! frame's own MDs up here gives back exactly its own colours: the intervals
//! are `[top, base)`, the last one `[top, base]`.
//!
//! # What is not coloured, and said
//!
//! - an MD outside every stated interval — above the first, below the last, or
//!   in a gap between two frames — is **uncoloured** and counted
//!   ([`Carried::outside`]), never given its neighbour's colour;
//! - an interval that names no single unit (a null index, or several at once)
//!   leaves its nodes uncoloured, as it leaves the frame's own;
//! - two frames of the wellbore naming **different** units at one MD is a
//!   disagreement: the node is uncoloured, counted
//!   ([`Carried::disagreements`]) and named in the notes, and neither frame is
//!   chosen. A frame naming nothing at an MD where another names a unit does not
//!   disagree — a null index says *not known here*, not *no unit here*.
//!
//! # Which frames speak
//!
//! Those of the wellbore that carry `IntervalStratigraphicUnits` — told on the
//! document alone ([`well_strata::states_units`]), so a frame that states
//! nothing is never read and an array of it that cannot be read stops nothing.
//! A frame that does state units and cannot be read **is** a refusal: its
//! defect is not hidden under a picture painted by the others.
//!
//! # Scope
//!
//! The frames' own `IntervalStratigraphicUnits` only (the `units` source). A
//! category property is a property **of a frame**: carrying it would be
//! carrying a property's values onto a representation that does not hold it,
//! which is a different decision and not made here.

use std::collections::HashMap;

use energyml_core::identity;
use energyml_core::registry::DynObject;
use energyml_core::value::{FieldValue, Reflect};

use crate::color::Rgba;
use crate::source::ArraySource;
use crate::units;
use crate::values::{self, Values};
use crate::well_strata::{self, Drawn, Known, Refusal, Source, Strata, Unit, Wanted};

/// What a representation represents — 2.2, then 2.0.1.
const REPRESENTED: &[&str] = &["RepresentedObject", "RepresentedInterpretation"];
/// Where a 2.2 trajectory groups its MD bounds; 2.0.1 puts them on the object.
const MD_INTERVAL: &str = "MdInterval";
/// The datum an MD counts from — 2.2, then 2.0.1.
const MD_DATUM: &[&str] = &["Datum", "MdDatum"];
/// The unit of the MDs — 2.2, then 2.0.1.
const MD_UOM: &[&str] = &["Uom", "MdUom"];
/// The MD bounds — 2.2, then 2.0.1.
const MD_MIN: &[&str] = &["MdMin", "StartMd"];
const MD_MAX: &[&str] = &["MdMax", "FinishMd"];
/// `driller` or `logger` (TRG 2.2 §12.8).
const MD_DOMAIN: &str = "MdDomain";
const FRAME: &str = "WellboreFrameRepresentation";
const TRAJECTORY: &str = "WellboreTrajectoryRepresentation";

// ---------------------------------------------------------------------------
// What an MD is measured against
// ---------------------------------------------------------------------------

/// How a representation's measured depths are counted: the trajectory that
/// says so, its datum, its unit and its MD domain — each `None` when the file
/// does not say.
#[derive(Clone, Debug, PartialEq)]
pub struct MdReference {
    /// The trajectory whose `MdInterval` states them, lowercased — the
    /// representation itself for a trajectory, its `Trajectory` for a frame.
    pub trajectory: String,
    /// The datum object's uuid, lowercased.
    pub datum: Option<String>,
    /// The unit, as the file spells it.
    pub uom: Option<String>,
    /// `driller` or `logger`, as the file spells it.
    pub domain: Option<String>,
}

/// Whether MDs counted against `target` can be looked up in intervals
/// counted against `stated` — and if so, the factor that takes a `target` MD
/// into `stated`'s unit. `Err` is the reason, in a sentence.
///
/// The module's *When two MDs can be compared*: one trajectory, or one datum,
/// one unit up to a conversion, and one MD domain.
pub fn comparable(target: &MdReference, stated: &MdReference) -> Result<f64, String> {
    if target.trajectory == stated.trajectory {
        return Ok(1.0);
    }
    let (Some(ours), Some(theirs)) = (&target.datum, &stated.datum) else {
        return Err(format!(
            "the datum of {} is not stated, so its MDs cannot be compared with those of {} \
             (common TRG 2.3 §6.48: only MDs with the same datum are directly comparable)",
            if target.datum.is_none() {
                &target.trajectory
            } else {
                &stated.trajectory
            },
            if target.datum.is_none() {
                &stated.trajectory
            } else {
                &target.trajectory
            },
        ));
    };
    if ours != theirs {
        return Err(format!(
            "its MDs count from datum {ours} and the frame's from datum {theirs} — two datums, and an MD \
             is never moved from one to the other (common TRG 2.3 §6.48)"
        ));
    }
    if target.domain != stated.domain {
        return Err(format!(
            "its MDs are {} depths and the frame's are {} depths (MdDomain, RESQML TRG 2.2 §12.8) — two \
             measurements of the hole, not compared",
            target.domain.as_deref().unwrap_or("unstated"),
            stated.domain.as_deref().unwrap_or("unstated")
        ));
    }
    match (&target.uom, &stated.uom) {
        (Some(ours), Some(theirs)) if ours == theirs => Ok(1.0),
        (Some(ours), Some(theirs)) => match (units::metres(1.0, ours), units::metres(1.0, theirs)) {
            (Ok(ours_m), Ok(theirs_m)) if theirs_m != 0.0 => Ok(ours_m / theirs_m),
            _ => Err(format!(
                "its MDs are in `{ours}` and the frame's in `{theirs}`, and this build cannot convert one into the other"
            )),
        },
        _ => Err(format!(
            "the unit of the MDs of {} is not stated, so they cannot be compared with MDs in another unit",
            if target.uom.is_none() {
                &target.trajectory
            } else {
                &stated.trajectory
            }
        )),
    }
}

/// How a frame's or a trajectory's MDs are counted.
///
/// A frame's are its trajectory's (TRG 2.2 §12.13, `NodeMd`), so the
/// trajectory is opened; a trajectory the package does not hold is an
/// absence, named.
pub fn md_reference(representation: &dyn Reflect, source: &dyn ArraySource) -> Result<MdReference, Refusal> {
    let title = title_of(representation);
    let key = representation.type_info().type_key;
    let held;
    let trajectory: &dyn Reflect = match key {
        TRAJECTORY => representation,
        FRAME => {
            let Some(FieldValue::Object(named)) = representation.field("Trajectory") else {
                return Err(Refusal::Defect {
                    what: format!("`{title}`"),
                    why: "names no trajectory, so where its MDs count from is unknown".to_string(),
                });
            };
            let Some(uuid) = identity::uuid_of_reflect(named).filter(|uuid| !uuid.is_empty()) else {
                return Err(Refusal::Defect {
                    what: format!("`{title}`"),
                    why: "names no trajectory, so where its MDs count from is unknown".to_string(),
                });
            };
            // By the uri the frame names: two versions of a trajectory have two
            // datums and two MD intervals.
            let Some(object) = source.object_of(named) else {
                return Err(Refusal::Absent {
                    what: format!("`{title}`"),
                    why: format!(
                        "names the trajectory {uuid}, which the package does not hold — where its MDs count \
                         from is unknown"
                    ),
                });
            };
            held = object;
            held.as_reflect()
        }
        other => {
            return Err(Refusal::Unsupported {
                what: format!("`{other}` `{title}`"),
                why: "is neither a wellbore frame nor a wellbore trajectory, so it has no measured depth"
                    .to_string(),
            });
        }
    };
    let interval = match trajectory.field(MD_INTERVAL) {
        Some(FieldValue::Object(interval)) => interval,
        _ => trajectory,
    };
    Ok(MdReference {
        trajectory: identity::uuid_of_reflect(trajectory)
            .unwrap_or_default()
            .to_ascii_lowercase(),
        datum: MD_DATUM
            .iter()
            .find_map(|name| reference_of(interval, name))
            .map(|uuid| uuid.to_ascii_lowercase()),
        uom: MD_UOM.iter().find_map(|name| string_of(interval, name)),
        domain: string_of(trajectory, MD_DOMAIN),
    })
}

/// The MD of each node a representation is drawn with, in the order it is
/// drawn — the order `energyml_geometry` gives its points:
///
/// - a frame: its `NodeMd`;
/// - a trajectory with a `ParametricLineGeometry`: its `ControlPointParameters`
///   — the line is drawn at its knots, and a trajectory's parameter **is** its
///   MD (the contract `energyml_geometry::well::path_of` states). A line with
///   no parameters has knots 0, 1, 2 …, which are no MD: refused;
/// - a trajectory with no `Geometry`: its `MdMin` and `MdMax`, the two points
///   it is drawn with, down from its datum.
pub fn node_mds(representation: &dyn Reflect, source: &dyn ArraySource) -> Result<Vec<f64>, Refusal> {
    let title = title_of(representation);
    match representation.type_info().type_key {
        FRAME => {
            let Some(FieldValue::Object(array)) = representation.field("NodeMd") else {
                return Err(Refusal::Defect {
                    what: format!("`{title}`"),
                    why: "states no NodeMd".to_string(),
                });
            };
            doubles(array, source, &format!("`{title}`.NodeMd"))
        }
        TRAJECTORY => {
            let Some(FieldValue::Object(geometry)) = representation.field("Geometry") else {
                let interval = match representation.field(MD_INTERVAL) {
                    Some(FieldValue::Object(interval)) => interval,
                    _ => representation,
                };
                let bound = |names: &[&str]| names.iter().find_map(|name| number_of(interval, name));
                return match (bound(MD_MIN), bound(MD_MAX)) {
                    (Some(min), Some(max)) => Ok(vec![min, max]),
                    _ => Err(Refusal::Absent {
                        what: format!("`{title}`"),
                        why: "has neither a geometry nor MD bounds, so none of its points has an MD"
                            .to_string(),
                    }),
                };
            };
            let kind = geometry.type_info().name;
            if !kind.ends_with("ParametricLineGeometry") || kind.contains("FromRepresentation") {
                return Err(Refusal::Unsupported {
                    what: format!("`{title}`"),
                    why: format!(
                        "is drawn from a `{kind}`, whose points this reader cannot give an MD to — a parametric \
                         line's own knots are what it knows"
                    ),
                });
            }
            let Some(FieldValue::Object(parameters)) = geometry.field("ControlPointParameters") else {
                return Err(Refusal::Absent {
                    what: format!("`{title}`"),
                    why: "states no ControlPointParameters: its knots are then 0, 1, 2 …, and no point along it \
                          has an MD"
                        .to_string(),
                });
            };
            doubles(parameters, source, &format!("`{title}`.ControlPointParameters"))
        }
        other => Err(Refusal::Unsupported {
            what: format!("`{other}` `{title}`"),
            why: "is neither a wellbore frame nor a wellbore trajectory, so it has no measured depth"
                .to_string(),
        }),
    }
}

/// The object a representation represents, lowercased — what makes two
/// representations *the same wellbore*.
pub fn represented(representation: &dyn Reflect) -> Option<String> {
    REPRESENTED
        .iter()
        .find_map(|name| reference_of(representation, name))
        .map(|uuid| uuid.to_ascii_lowercase())
}

// ---------------------------------------------------------------------------
// The lookup — pure
// ---------------------------------------------------------------------------

/// A frame's stratigraphy, with how its MDs are counted.
pub struct Stated<'a> {
    /// What the frame states.
    pub strata: &'a Strata,
    /// Against what.
    pub reference: &'a MdReference,
}

/// What one node of a carried painting is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Node {
    /// A unit — its position in [`Carried::strata`]'s `units`.
    Unit(usize),
    /// No stated interval holds this MD (or the MD is not a number).
    Outside,
    /// The intervals holding it name no single unit — a null index, or
    /// several units at once.
    Unnamed,
    /// Two frames name different units here.
    Disagreement,
}

/// A representation coloured by the strata its wellbore's frames state.
#[derive(Clone, Debug, PartialEq)]
pub struct Carried {
    /// The representation's uuid, lowercased.
    pub representation: String,
    /// Its title.
    pub title: String,
    /// The units, merged over the frames used — every unit of the first frame
    /// in its order, then those the others add — and **no section**: this
    /// representation has nodes, not the frames' intervals. What a legend
    /// reads, and what [`Carried::node_colours_with`] draws from, so the two
    /// cannot differ.
    pub strata: Strata,
    /// One per node, in the order the representation is drawn.
    pub nodes: Vec<Node>,
    /// The intervals the frames state, **in this representation's MD** — each
    /// frame's sections that name a unit, `(top, base, unit)` with the unit's
    /// position in `strata.units`. What [`Painting::intervals`] draws: the
    /// nodes only sample the rock, the frames state it.
    pub spans: Vec<(f64, f64, usize)>,
    /// The frames the units were read from, lowercased.
    pub from: Vec<String>,
    /// What was set aside or could not be decided.
    pub notes: Vec<String>,
}

impl Carried {
    /// Nodes no stated interval holds.
    pub fn outside(&self) -> usize {
        self.nodes.iter().filter(|node| **node == Node::Outside).count()
    }

    /// Nodes where two frames name two units.
    pub fn disagreements(&self) -> usize {
        self.nodes
            .iter()
            .filter(|node| **node == Node::Disagreement)
            .count()
    }

    /// Nodes given a unit.
    pub fn named(&self) -> usize {
        self.nodes
            .iter()
            .filter(|node| matches!(node, Node::Unit(_)))
            .count()
    }

    /// One RGBA per node — [`Strata::node_colours_with`]'s contract: a unit is
    /// drawn with [`Strata::drawn`] over the merged units, and every node that
    /// is not a unit — outside, unnamed, disputed — takes `missing`.
    pub fn node_colours_with(&self, missing: Rgba, fallback: &[Rgba]) -> Vec<u8> {
        let drawn = self.strata.drawn(fallback);
        self.nodes
            .iter()
            .flat_map(|node| {
                match node {
                    Node::Unit(unit) => drawn.get(*unit).and_then(|drawn| drawn.colour()),
                    _ => None,
                }
                .unwrap_or(missing)
                .to_bytes()
            })
            .collect()
    }
}

/// The identity of a unit across two frames: its uri (two versions of a unit
/// are two units), else its uuid, else its title.
fn identity_of(unit: &Unit) -> String {
    match (&unit.uri, &unit.uuid) {
        (Some(uri), _) => uri.to_ascii_lowercase(),
        (None, Some(uuid)) => uuid.to_ascii_lowercase(),
        (None, None) => format!("title:{}", unit.title),
    }
}

/// What one frame says at an MD, in its own unit.
enum Says {
    /// No interval holds it.
    Nothing,
    /// An interval holds it and names no single unit.
    Unnamed,
    /// An interval holds it and names this unit of the frame.
    Unit(usize),
}

/// The interval of `strata` holding `md` — `[top, base)`, the last one
/// `[top, base]`: the rule [`Strata::node_colours`] gives a frame's own
/// nodes, so a frame's own MDs find the colours it paints itself with.
fn says(strata: &Strata, md: f64) -> Says {
    let last = strata.sections.len().saturating_sub(1);
    let found = strata.sections.iter().enumerate().find(|(at, section)| {
        section.top <= md && (md < section.base || (*at == last && md == section.base))
    });
    match found {
        None => Says::Nothing,
        Some((_, section)) => match section.unit {
            Some(unit) => Says::Unit(unit),
            None => Says::Unnamed,
        },
    }
}

/// A frame's intervals, checked to run down: an MD lookup needs `top <= base`
/// in every interval, which is what the frames of every file this was written
/// against state — [`well_strata`] does not re-sort them, and neither does this.
fn runs_down(strata: &Strata) -> Result<(), String> {
    // A `NaN` limit orders against nothing, and is refused with the rest.
    let backwards = |section: &well_strata::Section| {
        section
            .top
            .partial_cmp(&section.base)
            .is_none_or(|order| order == std::cmp::Ordering::Greater)
    };
    match strata.sections.iter().position(backwards) {
        None => Ok(()),
        Some(at) => Err(format!(
            "interval {at} of `{}` runs from MD {} up to MD {}; an MD is looked up in intervals that run down",
            strata.title, strata.sections[at].top, strata.sections[at].base
        )),
    }
}

/// Colour the nodes at `mds` — counted against `target` — by what the frames
/// in `stated` say.
///
/// Frames whose MDs cannot be compared ([`comparable`]) or whose intervals do
/// not run down are set aside **by name** in the notes. None left: refused,
/// [`Refusal::Incomparable`], each reason given.
pub fn carry(
    representation: &str,
    title: &str,
    mds: &[f64],
    target: &MdReference,
    stated: &[Stated<'_>],
) -> Result<Carried, Refusal> {
    let mut notes = Vec::new();
    let mut set_aside = Vec::new();
    // (the frame, its factor from the target's unit)
    let mut used: Vec<(&Strata, f64)> = Vec::new();
    for one in stated {
        match comparable(target, one.reference).and_then(|factor| runs_down(one.strata).map(|()| factor)) {
            Ok(factor) => used.push((one.strata, factor)),
            Err(why) => set_aside.push(format!("`{}`: {why}", one.strata.title)),
        }
    }
    if used.is_empty() {
        return Err(Refusal::Incomparable {
            what: format!("`{title}`"),
            why: format!(
                "the frames of its wellbore that state its stratigraphic units cannot be read against its MDs — {}",
                set_aside.join("; ")
            ),
        });
    }
    for why in &set_aside {
        notes.push(format!("set aside, {why}"));
    }

    // The units, merged: each frame's own position → the merged one.
    let mut units: Vec<Unit> = Vec::new();
    let mut position_of: HashMap<String, usize> = HashMap::new();
    let mut maps: Vec<Vec<usize>> = Vec::with_capacity(used.len());
    let mut ranks: Vec<String> = Vec::new();
    for (strata, _) in &used {
        if let Source::IntervalUnits { ranks: theirs } = &strata.source {
            for rank in theirs {
                if !ranks.contains(rank) {
                    ranks.push(rank.clone());
                }
            }
        }
        let map = strata
            .units
            .iter()
            .map(|unit| {
                *position_of.entry(identity_of(unit)).or_insert_with(|| {
                    let mut merged = unit.clone();
                    merged.code = units.len() as f64;
                    units.push(merged);
                    units.len() - 1
                })
            })
            .collect();
        maps.push(map);
    }

    let mut disputed: Vec<String> = Vec::new();
    let nodes: Vec<Node> = mds
        .iter()
        .map(|md| {
            let mut held = false;
            // (merged unit, the frame that names it)
            let mut named: Vec<(usize, usize)> = Vec::new();
            for (which, (strata, factor)) in used.iter().enumerate() {
                match says(strata, md * factor) {
                    Says::Nothing => {}
                    Says::Unnamed => held = true,
                    Says::Unit(unit) => {
                        held = true;
                        let merged = maps[which][unit];
                        if !named.iter().any(|(already, _)| *already == merged) {
                            named.push((merged, which));
                        }
                    }
                }
            }
            match named.as_slice() {
                [] if held => Node::Unnamed,
                [] => Node::Outside,
                [(unit, _)] => Node::Unit(*unit),
                many => {
                    if disputed.len() < 5 {
                        disputed.push(format!(
                            "at MD {md}, {}",
                            many.iter()
                                .map(|(unit, which)| format!(
                                    "`{}` says {}",
                                    used[*which].0.title, units[*unit].title
                                ))
                                .collect::<Vec<_>>()
                                .join(" and ")
                        ));
                    }
                    Node::Disagreement
                }
            }
        })
        .collect();

    // The stated rock, in the target's MD: a frame MD is the target's times
    // the factor (`says` is asked `md * factor`).
    let mut spans: Vec<(f64, f64, usize)> = Vec::new();
    for (which, (strata, factor)) in used.iter().enumerate() {
        for section in &strata.sections {
            if let Some(unit) = section.unit {
                spans.push((section.top / factor, section.base / factor, maps[which][unit]));
            }
        }
    }

    let carried_title = title.to_string();
    let mut carried = Carried {
        representation: representation.to_ascii_lowercase(),
        title: carried_title.clone(),
        strata: Strata {
            frame: representation.to_ascii_lowercase(),
            title: carried_title,
            source: Source::IntervalUnits { ranks },
            units,
            sections: Vec::new(),
            notes: Vec::new(),
        },
        nodes,
        spans,
        from: used.iter().map(|(strata, _)| strata.frame.clone()).collect(),
        notes,
    };
    let disagreements = carried.disagreements();
    if disagreements > 0 {
        carried.notes.push(format!(
            "{disagreements} node(s) where two frames of the wellbore name two different units — left \
             uncoloured, neither frame chosen: {}{}",
            disputed.join("; "),
            if disagreements > disputed.len() {
                "; …"
            } else {
                ""
            }
        ));
    }
    let outside = carried.outside();
    if outside > 0 {
        carried.notes.push(format!(
            "{outside} of {} node(s) lie outside every interval the frames state — left uncoloured, never given \
             a neighbour's unit",
            carried.nodes.len()
        ));
    }
    Ok(carried)
}

// ---------------------------------------------------------------------------
// The whole question
// ---------------------------------------------------------------------------

/// A representation of a wellbore, painted by its stratigraphic units.
#[derive(Clone, Debug, PartialEq)]
pub enum Painting {
    /// A frame that states them itself — [`well_strata::of_frame`].
    Own(Strata),
    /// Another representation of the wellbore, looked up by MD.
    Carried(Carried),
}

impl Painting {
    /// The units a legend lists and the nodes are drawn from.
    pub fn strata(&self) -> &Strata {
        match self {
            Painting::Own(strata) => strata,
            Painting::Carried(carried) => &carried.strata,
        }
    }

    /// One RGBA per node — [`Strata::node_colours_with`] or
    /// [`Carried::node_colours_with`].
    pub fn node_colours_with(&self, missing: Rgba, fallback: &[Rgba]) -> Vec<u8> {
        match self {
            Painting::Own(strata) => strata.node_colours_with(missing, fallback),
            Painting::Carried(carried) => carried.node_colours_with(missing, fallback),
        }
    }
}

/// One stratigraphic interval of a well, along its measured depth — what a
/// section view draws as a coloured stretch of the well, where a 3D view
/// colours nodes ([`Painting::node_colours_with`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Interval {
    /// Where it starts, in the MD of the painted representation.
    pub from: f64,
    /// Where it ends.
    pub to: f64,
    /// The colour [`Strata::drawn`] resolves for its unit: the file's, else
    /// the caller's `fallback` palette. `None` is *nothing gives one* — never
    /// a transparent colour, the caller draws its own.
    pub colour: Option<Rgba>,
    /// The unit's title.
    pub unit: String,
    /// **The unit's uri** — the identity the reader of the interval finds the
    /// unit by (its legend row, its colour set, its object) instead of
    /// guessing from the title and the colour. `None` for a unit that is only a
    /// row of a table, which no object stands behind.
    pub unit_uri: Option<String>,
}

impl Painting {
    /// The painting as intervals along the MD, by **the same rule** as
    /// [`Painting::node_colours_with`]: *a node opens the interval it starts*.
    ///
    /// - a frame stating its own units ([`Painting::Own`]): its sections, `[top,
    ///   base)`, the deepest limit belonging to the last one (the frame's last
    ///   node takes the last section's colour);
    /// - a representation carried by MD ([`Painting::Carried`]): node *i* opens
    ///   `[md i, md i+1)` with the unit it was given. The deepest node closes
    ///   nothing, so a unit it was given is a zero-length interval
    ///   `[md, md]` — present so that looking the node's own MD up finds its
    ///   colour, drawn as nothing.
    ///
    /// A carried painting draws **the intervals its frames state**
    /// ([`Carried::spans`]), cut to the MDs its nodes span — never a node's unit
    /// stretched to the next node. Stretched, a node at the base of the last
    /// stated interval carried that unit down to the next node: on Lutetia,
    /// the Sparnacian of BSS000NUGK ran 10 m below where its frame ends it
    /// (03/10). Where two frames state two units, the stretch is a gap, as the
    /// nodes there are.
    ///
    /// A stretch with no unit — outside every stated interval, unnamed,
    /// disputed between two frames — is **a gap**, not an interval: the same
    /// nodes that take the caller's `missing` colour in
    /// [`Painting::node_colours_with`]. Neighbouring intervals of one unit are
    /// joined. `mds` is the MD of each node, in the order of the painting's
    /// nodes — [`node_mds`] — and is only read for a carried painting, which it
    /// must match node for node: a count that differs is refused, since an MD
    /// given to the wrong node is a stretch of rock in the wrong place. A
    /// carried unit lasts to the next node, as the nodes are all the painting
    /// knows.
    pub fn intervals(&self, mds: &[f64], fallback: &[Rgba]) -> Result<Vec<Interval>, String> {
        if let Painting::Carried(carried) = self {
            if carried.nodes.len() != mds.len() {
                return Err(format!(
                    "`{}` is painted at {} node(s) but {} measured depth(s) were given, so no interval can be                      placed without a guess",
                    carried.title,
                    carried.nodes.len(),
                    mds.len()
                ));
            }
        }
        let strata = self.strata();
        let drawn = strata.drawn(fallback);
        // (from, to, the unit's place in `strata.units`)
        let mut runs: Vec<(f64, f64, usize)> = Vec::new();
        let mut push = |from: f64, to: f64, unit: usize| match runs.last_mut() {
            Some(last) if from < to && last.1 == from && last.2 == unit && last.0 < last.1 => last.1 = to,
            _ => runs.push((from, to, unit)),
        };
        match self {
            Painting::Own(strata) => {
                for section in &strata.sections {
                    if let Some(unit) = section.unit {
                        push(section.top, section.base, unit);
                    }
                }
            }
            Painting::Carried(carried) => {
                let finite = mds.iter().copied().filter(|md| md.is_finite());
                let (low, high) = finite.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), md| {
                    (lo.min(md), hi.max(md))
                });
                // Every limit a frame states inside the nodes' reach, and the
                // reach itself: between two of them, one unit or none.
                let mut cuts: Vec<f64> = vec![low, high];
                for &(top, base, _) in &carried.spans {
                    cuts.extend([top, base].into_iter().filter(|md| *md > low && *md < high));
                }
                cuts.sort_by(f64::total_cmp);
                cuts.dedup();
                for pair in cuts.windows(2) {
                    let middle = (pair[0] + pair[1]) / 2.0;
                    let mut held = carried
                        .spans
                        .iter()
                        .filter(|(top, base, _)| *top <= middle && middle < *base)
                        .map(|(_, _, unit)| *unit);
                    if let Some(unit) = held.next()
                        && held.all(|other| other == unit)
                    {
                        push(pair[0], pair[1], unit);
                    }
                }
                // A node no interval holds but which has a unit — the deepest
                // node, one on the closed base of a frame's last interval —
                // is a zero-length interval, so that its own MD finds its
                // colour, drawn as nothing.
                let mut points: Vec<(f64, usize)> = Vec::new();
                for (node, md) in carried.nodes.iter().zip(mds) {
                    let Node::Unit(unit) = node else { continue };
                    if !runs.iter().any(|(from, to, _)| *from <= *md && *md < *to) {
                        points.push((*md, *unit));
                    }
                }
                for (md, unit) in points {
                    runs.push((md, md, unit));
                }
                runs.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
            }
        }
        Ok(runs
            .into_iter()
            .filter_map(|(from, to, unit)| {
                let stated = strata.units.get(unit)?;
                Some(Interval {
                    from,
                    to,
                    colour: drawn.get(unit).and_then(Drawn::colour),
                    unit: stated.title.clone(),
                    unit_uri: stated.uri.clone(),
                })
            })
            .collect())
    }
}

/// Paint `target` by stratigraphic unit: its own `IntervalStratigraphicUnits`
/// if it is a frame that states them, else the units its wellbore's other
/// frames state, looked up at its MDs.
///
/// `frames_of` gives the wellbore frames that **may** represent the wellbore
/// named by its uuid (lowercase) — a caller that indexes the package hands
/// back that wellbore's frames, one that does not hands back all of them: the
/// ones that represent another object, and `target` itself, are skipped here
/// either way, so a wider answer is never a wrong one. It is **asked only when
/// it is needed** — not for a frame that states its own units, which is the
/// whole of a package written by the SIGEOL import. The list was built before
/// the call until 01/10, and a front end painting 2 697 such frames parsed the
/// package's 2 697 frames for each one of them (1,15 s a well, debug, on
/// `forages-unites-surfaces.epc`). `rank` keeps one rank, as in
/// [`Wanted::IntervalUnits`].
///
/// Refused by name: a frame of the wellbore whose units cannot be read (the
/// refusal is that frame's, a defect is not hidden behind a picture), no frame
/// stating any ([`Refusal::Absent`]), none whose MDs compare
/// ([`Refusal::Incomparable`]).
pub fn of_representation(
    target: &dyn Reflect,
    frames_of: &dyn Fn(&str) -> Vec<Box<dyn DynObject>>,
    rank: Option<&str>,
    source: &dyn ArraySource,
    known: &Known<'_>,
) -> Result<Painting, Refusal> {
    let wanted = Wanted::IntervalUnits { rank };
    let key = target.type_info().type_key;
    // A frame that states units says for itself; anything that is neither a
    // frame nor a trajectory gets `of_frame`'s own refusal, in its own words.
    // A frame that states none is not read here: its `NodeMd` is read below,
    // once there is something to look it up in.
    if key != TRAJECTORY && (key != FRAME || well_strata::states_units(target)) {
        match well_strata::of_frame(target, &wanted, source, known) {
            Ok(strata) => return Ok(Painting::Own(strata)),
            Err(refusal) if refusal.is_absence() && key == FRAME => {}
            Err(refusal) => return Err(refusal),
        }
    }
    let title = title_of(target);
    let uuid = identity::uuid_of_reflect(target)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let Some(wellbore) = represented(target) else {
        return Err(Refusal::Absent {
            what: format!("`{title}`"),
            why: "states no stratigraphic unit itself, and names no represented object — so no other \
                  representation of its wellbore can be recognised"
                .to_string(),
        });
    };

    let mut read: Vec<(Strata, MdReference)> = Vec::new();
    let frames = frames_of(&wellbore);
    for frame in &frames {
        let frame = frame.as_reflect();
        let theirs = identity::uuid_of_reflect(frame)
            .unwrap_or_default()
            .to_ascii_lowercase();
        // Told apart on the document alone: a frame that states no unit is
        // not asked, so an array of it that cannot be read stops nothing.
        if theirs == uuid
            || represented(frame).as_deref() != Some(wellbore.as_str())
            || !well_strata::states_units(frame)
        {
            continue;
        }
        match well_strata::of_frame(frame, &wanted, source, known) {
            Ok(strata) => read.push((strata, md_reference(frame, source)?)),
            Err(refusal) if refusal.is_absence() => {}
            Err(refusal) => return Err(refusal),
        }
    }
    if read.is_empty() {
        return Err(Refusal::Absent {
            what: format!("`{title}`"),
            why: format!(
                "states no stratigraphic unit itself, and no frame of the same wellbore ({wellbore}) does{}",
                rank.map(|rank| format!(" through rank {rank}"))
                    .unwrap_or_default()
            ),
        });
    }

    let reference = md_reference(target, source)?;
    let mds = node_mds(target, source)?;
    let stated: Vec<Stated<'_>> = read
        .iter()
        .map(|(strata, reference)| Stated { strata, reference })
        .collect();
    carry(&uuid, &title, &mds, &reference, &stated).map(Painting::Carried)
}

// ---------------------------------------------------------------------------
// Reading helpers
// ---------------------------------------------------------------------------

fn title_of(object: &dyn Reflect) -> String {
    identity::title_of_reflect(object)
        .filter(|title| !title.is_empty())
        .unwrap_or(object.type_info().type_key)
        .to_string()
}

fn reference_of(object: &dyn Reflect, field: &str) -> Option<String> {
    let FieldValue::Object(reference) = object.field(field)? else {
        return None;
    };
    identity::uuid_of_reflect(reference)
        .filter(|uuid| !uuid.is_empty())
        .map(str::to_string)
}

fn string_of(object: &dyn Reflect, field: &str) -> Option<String> {
    match object.field(field)? {
        FieldValue::Scalar(scalar) => scalar
            .as_str()
            .map(str::to_string)
            .filter(|text| !text.trim().is_empty()),
        _ => None,
    }
}

fn number_of(object: &dyn Reflect, field: &str) -> Option<f64> {
    match object.field(field)? {
        FieldValue::Scalar(scalar) => scalar.as_f64(),
        _ => None,
    }
}

fn doubles(array: &dyn Reflect, source: &dyn ArraySource, at: &str) -> Result<Vec<f64>, Refusal> {
    match values::read(array, source, at)? {
        Values::Floating(values) => Ok(values),
        Values::Integer(values) => Ok(values.into_iter().map(|value| value as f64).collect()),
        _ => Err(Refusal::Defect {
            what: at.to_string(),
            why: "holds no numbers".to_string(),
        }),
    }
}
