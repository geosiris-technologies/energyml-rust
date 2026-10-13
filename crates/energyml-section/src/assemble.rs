//! From every package's contribution to the catalogue (ADR-0015).
//!
//! Contract: [`assemble`].
//!
//! Nothing is read here: a [`Contribution`] is pure data in the scene's system,
//! and everything this does is arithmetic over it — projecting the heads on each
//! section's [`Corridor`], placing every node of a well on the abscissa, folding
//! the objects of one wellbore into one line, giving every object asked for a
//! row that says what became of it.

use std::collections::HashMap;

use energyml_analysis::profile::{Corridor, Head, Kept, Profile, Reason};

use crate::crosses::{meet, traces};
use crate::inputs::{InputsError, validate};
use crate::model::{
    Catalogue, Contribution, CutShape, EnvelopeItem, FORMAT, Inputs, Item, Mark, ObjectId,
    ObjectRow, Outcome, Plan, PlanWell, RowKind, Section, SectionKey, SectionTrace, Side,
    SurfaceRead, TraceItem, WellItem, WellKind, WellRead,
};

type V = [f64; 2];

/// The angle between two points of a rounded outer turn of a band: 15 degrees,
/// a sagitta of 0.9 % of the width — invisible at the scale of a drawing, and
/// a dozen points for a right angle.
pub const BAND_ARC_STEP: f64 = std::f64::consts::PI / 12.0;

/// The catalogue: wells projected on every section, one line per wellbore,
/// surfaces placed, the plan, every object's fate, the notes.
pub fn assemble(inputs: &Inputs, contributions: &[Contribution]) -> Result<Catalogue, InputsError> {
    validate(inputs)?;
    let (sections, mut notes) = traces(inputs)?;

    // One line per wellbore ------------------------------------------
    let all: Vec<&WellRead> = contributions.iter().flat_map(|c| c.wells.iter()).collect();
    let lines = fold_wells(&all);

    // The corridors, and the heads projected on each ------------------------
    let heads: Vec<Head> =
        lines.iter().map(|l| Head::new(l.well.object.key(), [l.well.head[0], l.well.head[1]])).collect();
    let mut built: Vec<Built> = Vec::with_capacity(sections.len());
    for st in &sections {
        let corridor = Corridor::new(&st.trace, st.left, st.right)
            .map_err(|e| InputsError::Setting(format!("{}: {e}", describe(st.key))))?;
        let band = corridor
            .band(BAND_ARC_STEP)
            .map_err(|e| InputsError::Setting(format!("{}: {e}", describe(st.key))))?;
        let profile = corridor.project(&heads);
        built.push(Built { corridor, band, profile });
    }

    // The sections ------------------------------------------------------------
    // Where the cross sections meet each other, for the marks.
    let mut cross_meets: Vec<(SectionKey, SectionKey, [f64; 2])> = Vec::new();
    for i in 1..sections.len() {
        for j in i + 1..sections.len() {
            if let Some(at) = meet(&sections[i].trace, &sections[j].trace) {
                cross_meets.push((sections[i].key, sections[j].key, at));
            }
        }
    }

    let mut out_sections: Vec<Section> = Vec::with_capacity(sections.len());
    for (st, b) in sections.iter().zip(&built) {
        let mut items: Vec<Item> = Vec::new();
        for surface in contributions.iter().flat_map(|c| c.surfaces.iter()) {
            for cut in surface.cuts.iter().filter(|c| c.section == st.key) {
                items.push(match &cut.shape {
                    CutShape::Trace { spans } => Item::Trace(TraceItem {
                        object: surface.object.clone(),
                        role: surface.role,
                        spans: spans.clone(),
                        time: surface.time,
                        notes: cut.notes.clone(),
                        colour: surface.colour.clone(),
                        age: surface.age,
                        feature: surface.feature.clone(),
                        sources: cut.sources.clone(),
                    }),
                    CutShape::Envelope { top, bottom, fills, faults } => {
                        Item::Envelope(EnvelopeItem {
                            object: surface.object.clone(),
                            top: top.clone(),
                            bottom: bottom.clone(),
                            fills: fills.clone(),
                            faults: faults.clone(),
                            time: surface.time,
                            colour: surface.colour.clone(),
                        })
                    }
                });
            }
        }
        for (line, head) in lines.iter().zip(&heads) {
            let Some(kept) = b.profile.kept.iter().find(|k| k.id == head.id) else { continue };
            items.push(Item::Well(well_item(line, kept)));
        }

        let mut marks: Vec<Mark> = Vec::new();
        let mut at = 0.0;
        marks.push(Mark::Vertex { s: 0.0, index: 0 });
        for (i, p) in st.trace.windows(2).enumerate() {
            at += (p[1][0] - p[0][0]).hypot(p[1][1] - p[0][1]);
            marks.push(Mark::Vertex { s: at, index: i as u32 + 1 });
        }
        match st.key {
            SectionKey::Long => {
                for other in sections.iter().filter(|o| o.key != SectionKey::Long) {
                    if let Some([s, _]) = other.crossing {
                        marks.push(Mark::Crossing { s, other: other.key });
                    }
                }
            }
            SectionKey::Cross { .. } => {
                if let Some([_, own]) = st.crossing {
                    marks.push(Mark::Crossing { s: own, other: SectionKey::Long });
                }
            }
        }
        for &(a, c, [sa, sc]) in &cross_meets {
            if a == st.key {
                marks.push(Mark::Crossing { s: sa, other: c });
            } else if c == st.key {
                marks.push(Mark::Crossing { s: sc, other: a });
            }
        }
        marks.sort_by(|x, y| mark_s(x).total_cmp(&mark_s(y)));

        let z_range = z_range_of(&items);
        out_sections.push(Section {
            key: st.key,
            trace: st.trace.clone(),
            length: b.corridor.length(),
            marks,
            band: b.band.clone(),
            items,
            z_range,
        });
    }

    // Where every well went ----------------------------------------------------
    let drawn_on: Vec<Vec<SectionKey>> = lines
        .iter()
        .zip(&heads)
        .map(|(_, head)| {
            sections
                .iter()
                .zip(&built)
                .filter(|(_, b)| b.profile.kept.iter().any(|k| k.id == head.id))
                .map(|(st, _)| st.key)
                .collect()
        })
        .collect();

    // The plan: local coordinates, first point at the origin ------------
    let origin = inputs.trace[0];
    let local = |p: V| [p[0] - origin[0], p[1] - origin[1]];
    let mut plan = Plan {
        origin,
        system: inputs.system.clone(),
        sections: sections
            .iter()
            .map(|st| (st.key, st.trace.iter().map(|&p| local(p)).collect()))
            .collect(),
        bands: sections
            .iter()
            .zip(&built)
            .map(|(st, b)| (st.key, b.band.iter().map(|&p| local(p)).collect()))
            .collect(),
        wells: Vec::new(),
    };
    let long_profile = &built[0].profile;
    let mut reasons: Vec<Option<String>> = Vec::with_capacity(lines.len());
    for (k, (line, head)) in lines.iter().zip(&heads).enumerate() {
        let feet: Vec<(SectionKey, V)> = sections
            .iter()
            .zip(&built)
            .filter_map(|(st, b)| {
                b.profile.kept.iter().find(|p| p.id == head.id).map(|p| (st.key, local(p.foot)))
            })
            .collect();
        let rejected = if drawn_on[k].is_empty() {
            Some(not_projected(line.well, long_profile, &head.id, inputs, &sections[0]))
        } else {
            None
        };
        reasons.push(rejected.clone());
        plan.wells.push(PlanWell {
            object: line.well.object.clone(),
            head: local([line.well.head[0], line.well.head[1]]),
            feet,
            rejected,
        });
    }

    // One row per object asked for -----------------------------------------------
    let mut objects: Vec<ObjectRow> = Vec::new();
    for contribution in contributions {
        for well in &contribution.wells {
            let key = well.object.key();
            if let Some(k) = lines.iter().position(|l| l.well.object.key() == key) {
                objects.push(ObjectRow {
                    object: well.object.clone(),
                    kind: RowKind::Well,
                    outcome: match &reasons[k] {
                        None => Outcome::Drawn { sections: drawn_on[k].clone() },
                        Some(reason) => Outcome::Refused { reason: reason.clone() },
                    },
                    fillable: false,
                });
            } else if let Some(line) =
                lines.iter().find(|l| l.merged.iter().any(|m| m.object.key() == key))
            {
                objects.push(ObjectRow {
                    object: well.object.clone(),
                    kind: RowKind::Well,
                    outcome: Outcome::Merged { into: line.well.object.clone() },
                    fillable: false,
                });
            }
        }
        for surface in &contribution.surfaces {
            objects.push(surface_row(surface, &sections));
        }
        for refused in &contribution.refused {
            objects.push(ObjectRow {
                object: refused.object.clone(),
                kind: kind_of(&refused.object),
                outcome: Outcome::Refused { reason: refused.reason.clone() },
                fillable: false,
            });
        }
    }

    // What else is worth saying ---------------------------------------------------
    let beyond = long_profile.beyond_an_end();
    if beyond > 0 {
        notes.push(crate::model::Note {
            code: "beyondAnEnd".to_string(),
            sections: vec![SectionKey::Long],
            objects: Vec::new(),
            said: format!(
                "{beyond} well(s) project beyond an end of the long section and were left out"
            ),
        });
    }

    notes.extend(contributions.iter().flat_map(|c| c.notes.iter().cloned()));
    notes.extend(crossing_notes(&out_sections));
    notes.extend(crate::erosion::notes_of(&out_sections));

    let long = out_sections.remove(0);
    Ok(Catalogue {
        format: FORMAT,
        inputs: inputs.clone(),
        long,
        crosses: out_sections,
        plan,
        objects,
        notes,
    })
}

struct Built {
    corridor: Corridor,
    band: Vec<V>,
    profile: Profile,
}

/// One line of the catalogue's wells: the read kept, the others of its wellbore.
struct Line<'a> {
    well: &'a WellRead,
    merged: Vec<&'a WellRead>,
}

/// Folds the objects of one wellbore into one line: the member that
/// carries the most strata intervals is kept, a frame before a trajectory when
/// they carry as many, the first of equals. A well with no wellbore stands
/// alone. Lines keep the order of their first member.
///
/// *The most intervals*, not *the first frame*: a wellbore often has two frames
/// — Lutetia gives each of its boreholes a frame for the geometry and one for
/// the stratigraphy — and keeping the silent one drew wells grey beside a
/// sister that knew their units (03/10, the first export of Lutetia).
fn fold_wells<'a>(all: &[&'a WellRead]) -> Vec<Line<'a>> {
    let mut lines: Vec<Line<'a>> = Vec::new();
    let mut by_wellbore: HashMap<&str, usize> = HashMap::new();
    for &w in all {
        // A well told by its head alone has no strata to rank: it stands alone
        // (a wellbore whose members are all far is not merged).
        let wellbore = if w.head_only { None } else { w.wellbore.as_deref() };
        let Some(wb) = wellbore else {
            lines.push(Line { well: w, merged: Vec::new() });
            continue;
        };
        match by_wellbore.get(wb) {
            None => {
                by_wellbore.insert(wb, lines.len());
                lines.push(Line { well: w, merged: Vec::new() });
            }
            Some(&at) => {
                let line = &mut lines[at];
                if rank(w) > rank(line.well) {
                    let old = std::mem::replace(&mut line.well, w);
                    line.merged.insert(0, old);
                } else {
                    line.merged.push(w);
                }
            }
        }
    }
    lines
}

/// What makes one member of a wellbore a better line than another: having
/// strata at all, then stating them itself, then being a frame (the
/// frame is kept), then how many intervals of rock it has.
///
/// The count comes last since the faces paint a silent member from the frames
/// of its wellbore (`PackageContext::frames_of`): a trajectory, or a frame
/// prolonged below the described units, painted that way carries the stated
/// frame's units on its own nodes — and counted first it won the line from the
/// frame it borrowed them from (03/10). A zero-length interval (a node's own
/// colour, `strata_by_md::Painting::intervals`) is no rock, and not counted.
fn rank(well: &WellRead) -> (bool, bool, bool, usize) {
    (
        !well.intervals.is_empty(),
        !well.borrowed,
        well.kind == WellKind::Frame,
        well.intervals.iter().filter(|one| one.to > one.from).count(),
    )
}

/// How far down a member of a wellbore reaches: the vertical extent of its path.
fn reach(well: &WellRead) -> f64 {
    let (low, high) = well
        .path
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), n| (lo.min(n[2]), hi.max(n[2])));
    if high >= low { high - low } else { 0.0 }
}

/// The member of a line whose path is drawn: the one that reaches deepest.
///
/// The line's strata come from the member that knows the most units ([`rank`]);
/// its **geometry** from the one that goes furthest — Lutetia's stratigraphic
/// frame stops where the described units stop, its geometric frame runs to the
/// bottom of the hole, and drawing the first cut every well short (03/10). The
/// two share the wellbore, so their MD are the wellbore's: an interval is placed
/// on the deeper path by its MD. A member without MD cannot carry the line's
/// intervals, so it lends its path only when the line's own member has none.
fn geometry_of<'a>(line: &Line<'a>) -> &'a WellRead {
    let own = line.well;
    std::iter::once(own)
        .chain(line.merged.iter().copied())
        .filter(|one| std::ptr::eq(*one, own) || one.md.is_some() || own.md.is_none())
        .fold(own, |best, one| if reach(one) > reach(best) { one } else { best })
}

fn well_item(line: &Line<'_>, kept: &Kept) -> WellItem {
    let w = line.well;
    let geometry = geometry_of(line);
    let dir = kept.direction;
    // Every node is placed along the direction the projection kept, from the
    // head's own abscissa — the head and the nodes stay in one rigid plane.
    let path: Vec<V> = geometry
        .path
        .iter()
        .map(|n| {
            let along = (n[0] - geometry.head[0]) * dir[0] + (n[1] - geometry.head[1]) * dir[1];
            [kept.s + along, n[2]]
        })
        .collect();
    WellItem {
        object: w.object.clone(),
        s: kept.s,
        d: kept.d,
        side: match kept.side {
            energyml_analysis::profile::Side::Left => Side::Left,
            energyml_analysis::profile::Side::Right => Side::Right,
        },
        head: [kept.s, w.head[2]],
        path,
        md: geometry.md.clone(),
        intervals: w.intervals.clone(),
        strata: w.strata.clone(),
        merged: line.merged.iter().map(|m| m.object.clone()).collect(),
    }
}

/// Why a well was drawn on no section: the long section's reason.
fn not_projected(
    well: &WellRead,
    long: &Profile,
    id: &str,
    inputs: &Inputs,
    st: &SectionTrace,
) -> String {
    let name = if well.object.title.is_empty() { &well.object.uuid } else { &well.object.title };
    let why = match long.rejected.iter().find(|r| r.id == id).map(|r| r.reason) {
        Some(Reason::TooFarOnItsSide) => format!(
            "its head is close to the long section but outside the corridor of its side \
             (left {}, right {})",
            inputs.left, inputs.right
        ),
        Some(Reason::BeyondAnEnd) => {
            "its head projects beyond an end of the long section".to_string()
        }
        Some(Reason::NoPosition) => "its head has no usable position".to_string(),
        None => format!(
            "its head is farther than {} from the long section",
            st.left.max(st.right)
        ),
    };
    format!("well {name}: {why}, and it falls on no cross section either")
}

fn surface_row(surface: &SurfaceRead, sections: &[SectionTrace]) -> ObjectRow {
    let on: Vec<SectionKey> = sections
        .iter()
        .map(|s| s.key)
        .filter(|k| surface.cuts.iter().any(|c| c.section == *k))
        .collect();
    let grid = surface.cuts.iter().any(|c| matches!(c.shape, CutShape::Envelope { .. }))
        || is_ijk(&surface.object.qualified_type);
    ObjectRow {
        object: surface.object.clone(),
        kind: if grid { RowKind::Grid } else { RowKind::Surface },
        outcome: if on.is_empty() { Outcome::Missed } else { Outcome::Drawn { sections: on } },
        fillable: !grid && surface.role != crate::model::TraceRole::Fault,
    }
}

/// A grid of cells (IJK, or an unstructured grid a section refuses). A
/// `Grid2dRepresentation` is **not** one: it is a surface on a lattice, cut in
/// a line like a triangulated one.
fn is_ijk(qualified_type: &str) -> bool {
    qualified_type.contains("Grid") && !qualified_type.contains("Grid2d")
}

fn kind_of(object: &ObjectId) -> RowKind {
    let t = &object.qualified_type;
    if t.contains("Wellbore") {
        RowKind::Well
    } else if is_ijk(t) {
        RowKind::Grid
    } else {
        RowKind::Surface
    }
}

fn describe(key: SectionKey) -> String {
    match key {
        SectionKey::Long => "the long section".to_string(),
        SectionKey::Cross { index } => format!("cross section {index}"),
    }
}

fn mark_s(m: &Mark) -> f64 {
    match m {
        Mark::Vertex { s, .. } | Mark::Crossing { s, .. } => *s,
    }
}

/// The vertical range of what is drawn.
fn z_range_of(items: &[Item]) -> Option<[f64; 2]> {
    let mut range: Option<[f64; 2]> = None;
    let mut take = |z: f64| {
        if z.is_finite() {
            range = Some(match range {
                None => [z, z],
                Some([lo, hi]) => [lo.min(z), hi.max(z)],
            });
        }
    };
    let spans = |v: &Vec<Vec<V>>, take: &mut dyn FnMut(f64)| {
        v.iter().flatten().for_each(|p| take(p[1]))
    };
    for item in items {
        match item {
            Item::Well(w) => {
                take(w.head[1]);
                w.path.iter().for_each(|p| take(p[1]));
            }
            Item::Trace(t) => spans(&t.spans, &mut take),
            Item::Envelope(e) => {
                spans(&e.top, &mut take);
                spans(&e.bottom, &mut take);
                e.fills.iter().flatten().for_each(|p| take(p[1]));
                e.faults.iter().flatten().for_each(|p| take(p[1]));
            }
        }
    }
    range
}

// ---------------------------------------------------------------------------
// The sections agree where they cross

/// The share of the vertical extent, of the two sections that cross, below
/// which two elevations of one surface at a crossing are the same: 0.5 %.
///
/// Both cuts are exact on the same piecewise-linear surface at the same point
/// of the plane, so a real disagreement is a surface read differently, a grid
/// face cut by two different triangles, or a hole on one side — a difference
/// of the drawing's own order, never of a rounding. 0.5 % of the extent is
/// about a pixel on a 200-pixel-high frame: a gap smaller than that cannot be
/// seen, and saying it would be noise; a larger one is visible in the drawing
/// and deserves its sentence. A share rather than metres: the extent is in the
/// scene's own unit (metres, or seconds of two-way time for a time surface).
pub const CROSSING_TOLERANCE_SHARE: f64 = 0.005;

/// The tolerance when the two sections have no vertical extent worth a share
/// (a flat horizon alone): 1 cm, in the scene's unit — the precision the
/// coordinates of a package are given to.
pub const CROSSING_TOLERANCE_FLOOR: f64 = 0.01;

/// How far outside a span an abscissa may fall and still be read on it, as a
/// share of the section's length: the two sections give the crossing's `s` by
/// two different computations, so a surface that ends exactly there must not
/// be called absent.
const CROSSING_S_SLACK: f64 = 1e-6;

/// Where a surface's cut along one section lies at an abscissa, one list per
/// part: a trace has one (`""`), a grid's envelope two (`top`, `bottom`).
type Elevations = Vec<(&'static str, Vec<f64>)>;

/// The crossing notes: at every crossing of two sections, **a surface drawn on
/// both gives the same elevation on both** — they pass through the same point
/// of the plane. When it does not, it is **said** (`crossingMismatch`), never
/// corrected or hidden; when one section has nothing at the crossing where the
/// other has the surface (a hole, an end), it is said too
/// (`crossingOneSided`).
///
/// A surface seen on only one of the two sections is another fact (the other
/// cut missed it, the object row says so) and says nothing here. **Faults are
/// left out**: a steep surface has no elevation at an abscissa, a hair's
/// breadth of `s` changes it by metres. A grid is compared by its top and its
/// bottom, each by its own rule; where a column is split (a fault step) the
/// elevations at the crossing are several, and the comparison is between the
/// two sets — the worst nearest-pair.
fn crossing_notes(sections: &[Section]) -> Vec<crate::model::Note> {
    let mut notes = Vec::new();
    for a in sections {
        for mark in &a.marks {
            let Mark::Crossing { s: sa, other } = mark else { continue };
            // Each pair once, from its lesser key.
            if a.key >= *other {
                continue;
            }
            let Some(b) = sections.iter().find(|s| s.key == *other) else { continue };
            let Some(sb) = b.marks.iter().find_map(|m| match m {
                Mark::Crossing { s, other } if *other == a.key => Some(*s),
                _ => None,
            }) else {
                continue;
            };
            let extent = match (a.z_range, b.z_range) {
                (Some([al, ah]), Some([bl, bh])) => ah.max(bh) - al.min(bl),
                (Some([l, h]), None) | (None, Some([l, h])) => h - l,
                (None, None) => 0.0,
            };
            let tolerance = (extent * CROSSING_TOLERANCE_SHARE).max(CROSSING_TOLERANCE_FLOOR);
            for item_a in &a.items {
                let Some(object) = surface_of(item_a) else { continue };
                let Some(item_b) = b
                    .items
                    .iter()
                    .find(|i| surface_of(i).is_some_and(|o| o.key() == object.key()))
                else {
                    continue;
                };
                let on_a = elevations(item_a, *sa, a.length);
                let on_b = elevations(item_b, sb, b.length);
                for ((part, za), (_, zb)) in on_a.iter().zip(&on_b) {
                    let name = if part.is_empty() {
                        format!("surface '{}'", label(object))
                    } else {
                        format!("the {part} of grid '{}'", label(object))
                    };
                    let there = format!(
                        "at the crossing of the {} (s = {sa:.2}) and the {} (s = {sb:.2})",
                        describe(a.key),
                        describe(b.key)
                    );
                    let note = |code: &str, said: String| crate::model::Note {
                        code: code.to_string(),
                        sections: vec![a.key, b.key],
                        objects: vec![object.clone()],
                        said,
                    };
                    match (za.is_empty(), zb.is_empty()) {
                        (true, true) => {}
                        (false, true) | (true, false) => {
                            let (has, lacks, z) = if zb.is_empty() {
                                (a.key, b.key, za[0])
                            } else {
                                (b.key, a.key, zb[0])
                            };
                            notes.push(note(
                                "crossingOneSided",
                                format!(
                                    "{name} {there}: the {} gives it at z = {z:.2}, the {} has \
                                     nothing there (a hole or an end of the surface)",
                                    describe(has),
                                    describe(lacks)
                                ),
                            ));
                        }
                        (false, false) => {
                            let (x, y) = worst_pair(za, zb);
                            let gap = (x - y).abs();
                            if gap > tolerance {
                                notes.push(note(
                                    "crossingMismatch",
                                    format!(
                                        "{name} {there}: z = {x:.2} on the {} but {y:.2} on the \
                                         {}, a gap of {gap:.2} (tolerance {tolerance:.2})",
                                        describe(a.key),
                                        describe(b.key)
                                    ),
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
    notes
}

fn surface_of(item: &Item) -> Option<&ObjectId> {
    match item {
        Item::Trace(t) if t.role != crate::model::TraceRole::Fault => Some(&t.object),
        Item::Envelope(e) => Some(&e.object),
        _ => None,
    }
}

fn label(object: &ObjectId) -> &str {
    if object.title.is_empty() { &object.uuid } else { &object.title }
}

/// The elevations of an item at the abscissa `s`, by part.
fn elevations(item: &Item, s: f64, length: f64) -> Elevations {
    let slack = length * CROSSING_S_SLACK;
    match item {
        Item::Trace(t) => vec![("", at(&t.spans, s, slack))],
        Item::Envelope(e) => {
            vec![("top", at(&e.top, s, slack)), ("bottom", at(&e.bottom, s, slack))]
        }
        Item::Well(_) => Vec::new(),
    }
}

/// Every elevation the spans give at `s`, interpolated on the segment that
/// holds it; a vertical segment gives both its ends, a shared vertex once.
fn at(spans: &[Vec<V>], s: f64, slack: f64) -> Vec<f64> {
    let mut out: Vec<f64> = Vec::new();
    let mut push = |z: f64| {
        if z.is_finite() && !out.iter().any(|o| (o - z).abs() < 1e-9) {
            out.push(z);
        }
    };
    for span in spans {
        for w in span.windows(2) {
            let (p, q) = (w[0], w[1]);
            let (lo, hi) = (p[0].min(q[0]), p[0].max(q[0]));
            if s < lo - slack || s > hi + slack {
                continue;
            }
            if hi - lo < 1e-12 {
                push(p[1]);
                push(q[1]);
            } else {
                let t = ((s - p[0]) / (q[0] - p[0])).clamp(0.0, 1.0);
                push(p[1] + t * (q[1] - p[1]));
            }
        }
    }
    out
}

/// The nearest pair of the two sets that is farthest apart: the Hausdorff
/// distance, with the elevations that realise it (first set, second set).
fn worst_pair(a: &[f64], b: &[f64]) -> (f64, f64) {
    let nearest = |x: f64, set: &[f64]| {
        set.iter().copied().min_by(|p, q| (p - x).abs().total_cmp(&(q - x).abs())).unwrap_or(x)
    };
    let mut worst = (a[0], nearest(a[0], b));
    for &x in a {
        let y = nearest(x, b);
        if (x - y).abs() > (worst.0 - worst.1).abs() {
            worst = (x, y);
        }
    }
    for &y in b {
        let x = nearest(y, a);
        if (x - y).abs() > (worst.0 - worst.1).abs() {
            worst = (x, y);
        }
    }
    worst
}
