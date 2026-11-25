//! The colours **along a borehole**, by one door: a property chosen by its title, carried by measured
//! depth onto the nodes of the borehole a section keeps, coloured by the package's cascade.
//!
//! A section keeps a well as a [`WellItem`]: the object drawn (a frame, or a trajectory), its nodes in
//! `(s, z)` and their measured depths. The property a person wants to see along it rarely hangs on that
//! object: it hangs on **another frame of the same well**. [`paint_wells`] does what that takes, and nothing
//! of it is the face's:
//!
//! 1. the well is found among the package's wells ([`well_members_of_index`]) by the objects the item
//!    stands for, and its properties are those that hang on one of its frames;
//! 2. the property is **chosen by its title** - a piece of it, case ignored, the one reading of
//!    [`energyml_storage::resource::title_matches`] that `list`, `show` and the markers share. No match is a
//!    well without the property (it keeps its colour, and is counted); **several properties of different
//!    titles are a refusal that lists them**, never a silent choice; the same title on several frames is one
//!    property in parts;
//! 3. its values are read once ([`energyml_domain::frame_values`]) and **carried by MD onto the item's
//!    nodes** ([`energyml_domain::frame_values::carry`], with the rule that says when two MDs count the same
//!    way): an MD no frame holds, or where the frames disagree, has no colour, a gap and never a neighbour's
//!    value; MDs that cannot be compared refuse the well, by name;
//! 4. each node is coloured by the cascade of the package ([`crate::value_colours`]): for a category the
//!    colour map put on its table, read **by exact key**, a palette for a key the map lacks (and the legend
//!    says it is not the file's); for a measure the file's map, else a palette, over one scale for every well.
//!    [`Blend::Net`] for a category (a code does not run into the next), [`Blend::Smooth`] for a measure;
//! 5. the legend is a ramp for a measure and the list of the categories painted for a category.
//!
//! One property reading per call: the first well painted says whether it is a category or a measure, and a
//! well whose property is the other kind is refused by name. The values of a measure are one scale, linear,
//! from the lowest value painted to the highest, over every well.
//!
//! The faces (the command line today, the desktop next) ask this and apply the answer to their [`View`]
//! ([`WellPaints::apply`]); none of them reads a frame or colours a node by itself.

use std::collections::{BTreeMap, HashMap};

use energyml_core::identity;
use energyml_core::text::plain_number;
use energyml_domain::color::Rgba;
use energyml_domain::frame_values::{self, Carry, FrameValues, Stated, Value, carry};
use energyml_domain::palettes::Reading;
use energyml_domain::property::PropertyKindRef;
use energyml_domain::stats::scale::{Scale, ScaleKind};
use energyml_domain::strata_by_md::md_reference;
use energyml_domain::well_members::{WellMembers, well_members_of_index};
use energyml_domain::wellbore_table::hangs_on;
use energyml_section::model::{
    Blend, CategoryEntry, CategoryLegend, Paint, RampLegend, View, WellColouring, WellItem,
};
use energyml_storage::resource::title_matches;

use crate::colour::{ValueColours, category_colours_noted, value_colours_noted};
use crate::kind::ViewContext;
use crate::refusal::ViewRefusal;

/// The name the refusals of this door are given.
pub const ID: &str = "paint-wells";

/// What is asked.
#[derive(Clone, Copy, Debug)]
pub struct PaintAsk<'a> {
    /// A piece of the property's title, case ignored.
    pub title: &'a str,
    /// A palette, by name, that wins over the file's colours (the user's choice); none: the file's, else the default.
    pub palette: Option<&'a str>,
}

/// What became of one well.
#[derive(Clone, Debug, PartialEq)]
pub enum WellOutcome {
    /// Painted along a property.
    Painted {
        /// The property's title.
        property: String,
        /// The frames its values were read from, by title.
        frames: Vec<String>,
        /// The nodes that were given a colour.
        coloured: usize,
        /// The nodes of the well.
        nodes: usize,
    },
    /// No property of the well has a title that fits: the well keeps its colour.
    WithoutProperty,
    /// The well cannot be painted, and why: it keeps its colour.
    Refused(String),
}

/// One well and what became of it.
#[derive(Clone, Debug, PartialEq)]
pub struct WellResult {
    /// The key of the well's object in the section.
    pub key: String,
    /// Its title.
    pub well: String,
    /// What became of it.
    pub outcome: WellOutcome,
}

/// The legend of what was painted.
#[derive(Clone, Debug, PartialEq)]
pub enum PaintLegend {
    /// A measure: its colour ramp.
    Ramp(RampLegend),
    /// A category: each value painted and its colour.
    Categories(CategoryLegend),
}

/// The answer: what to put in a section view, and what became of each well.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WellPaints {
    /// One paint per well painted, by the key of its object ([`View::paints`]).
    pub paints: Vec<(String, Paint)>,
    /// The legend of the property painted; none when no well was painted.
    pub legend: Option<PaintLegend>,
    /// What became of each well asked, in the order given.
    pub results: Vec<WellResult>,
    /// What was met on the way and is worth saying (where the colours come from, what the carry left out).
    pub notes: Vec<String>,
}

impl WellPaints {
    /// The wells painted.
    pub fn painted(&self) -> usize {
        self.results
            .iter()
            .filter(|one| matches!(one.outcome, WellOutcome::Painted { .. }))
            .count()
    }

    /// The wells that have no property with such a title.
    pub fn without_property(&self) -> usize {
        self.results
            .iter()
            .filter(|one| one.outcome == WellOutcome::WithoutProperty)
            .count()
    }

    /// The wells that were refused, each with its reason.
    pub fn refused(&self) -> Vec<(&str, &str)> {
        self.results
            .iter()
            .filter_map(|one| match &one.outcome {
                WellOutcome::Refused(why) => Some((one.well.as_str(), why.as_str())),
                _ => None,
            })
            .collect()
    }

    /// Several answers - one per package - as one: the paints and the results one after the other, the legend of
    /// the first that has one (a section draws one legend; the others are said).
    pub fn merge(parts: Vec<WellPaints>) -> WellPaints {
        let mut out = WellPaints::default();
        for part in parts {
            out.paints.extend(part.paints);
            out.results.extend(part.results);
            out.notes.extend(part.notes);
            match (&out.legend, part.legend) {
                (None, Some(legend)) => out.legend = Some(legend),
                (Some(_), Some(_)) => out.notes.push(
                    "a section draws one legend of colours: the legend of a second package's property is not drawn"
                        .to_string(),
                ),
                _ => {}
            }
        }
        out
    }

    /// Put the answer in a view: the paints, the wells coloured by what the scene paints
    /// ([`WellColouring::Scene`]) - the wells it paints nothing along keeping their strata
    /// ([`View::strata_for_unpainted`]) -, the legend. A view whose answer painted no well is left as it was - its wells keep
    /// the colouring they had. The limits a view draws over its wells ([`View::limits`]) are not touched: they
    /// are drawn over a painted well as over any other.
    pub fn apply(self, view: &mut View) {
        if self.painted() == 0 {
            return;
        }
        view.paints.extend(self.paints);
        view.well_colouring = WellColouring::Scene;
        // The wells the scene paints nothing along keep their strata, rather than a flat colour.
        view.strata_for_unpainted = true;
        match self.legend {
            Some(PaintLegend::Ramp(legend)) => view.ramp_legend = Some(legend),
            Some(PaintLegend::Categories(legend)) => view.category_legend = Some(legend),
            None => {}
        }
    }
}

/// How the property to read along a well is chosen.
#[derive(Clone, Copy, Debug)]
pub enum Pick<'a> {
    /// A piece of its title, case ignored (what [`paint_wells`] takes).
    Title(&'a str),
    /// These properties, by full uri: one property that hangs on several frames of the well (its parts), or one alone.
    /// What a caller that has chosen the property itself, by its kind for example, gives.
    Uris(&'a [String]),
}

impl Pick<'_> {
    /// Whether the property at `uri`, titled `title`, is picked.
    fn fits(&self, uri: &str, title: &str) -> bool {
        match self {
            Pick::Title(wanted) => title_matches(title, wanted),
            Pick::Uris(uris) => uris.iter().any(|one| one == uri),
        }
    }

    /// What the refusals call the choice.
    fn said(&self) -> String {
        match self {
            Pick::Title(wanted) => (*wanted).to_string(),
            Pick::Uris(uris) => uris.join(", "),
        }
    }
}

/// One property read on one frame of the well: where a part of the values come from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    /// The property's full uri.
    pub property_uri: String,
    /// The full uri of the frame it hangs on.
    pub frame_uri: String,
}

/// What one well gives before it is coloured: the values of a property at its nodes, carried by measured depth from the
/// frames of its wellbore that hold them.
#[derive(Clone, Debug, PartialEq)]
pub struct WellValues {
    /// The key of the well's object in the section.
    pub key: String,
    /// Its title.
    pub well: String,
    /// The first property's full uri.
    pub property_uri: String,
    /// Its title.
    pub property: String,
    /// Whether it is a measure (a continuous property), else a category or a count.
    pub continuous: bool,
    /// Whether it names a table of categories: a category, read by its codes, never by the look of its numbers.
    pub categorical: bool,
    /// Its unit, for a measure.
    pub uom: Option<String>,
    /// The title the property's kind is referred to by: what a legend common to many wells is called by when they agree.
    pub kind_title: Option<String>,
    /// The code and the label of each row of the table, when the property names one.
    pub labels: Vec<(f64, String)>,
    /// The frames the values were read from, by title.
    pub frames: Vec<String>,
    /// Each property read and the frame it hangs on, in the order of the frames above.
    pub parts: Vec<Part>,
    /// The smallest and the largest value the frames state, before the carry (what a scale of its own is taken from).
    pub range: Option<(f64, f64)>,
    /// One per node of the well. `None` is a node no frame speaks for: a gap, never a neighbour's value.
    pub values: Vec<Option<Value>>,
}

/// The wellbore a section's well stands for: the one among `members` that holds the object drawn or one of the objects
/// folded into it. `None` when the package has no such wellbore.
pub fn wellbore_of<'a>(members: &'a [WellMembers], well: &WellItem) -> Option<&'a WellMembers> {
    let stands_for: Vec<&str> = std::iter::once(well.object.uri.as_str())
        .chain(well.merged.iter().map(|folded| folded.uri.as_str()))
        .collect();
    members.iter().find(|one| {
        one.trajectories
            .iter()
            .chain(&one.frames)
            .chain(&one.markers)
            .any(|member| stands_for.contains(&member.uri.as_str()))
    })
}

fn name_of(well: &WellItem) -> String {
    if well.object.title.is_empty() {
        well.object.uri.clone()
    } else {
        well.object.title.clone()
    }
}

/// Paint these wells, **all of one package** (`cx`'s), along the property whose title fits `ask`.
///
/// # Errors
///
/// [`ViewRefusal`] for a request that is wrong whatever the wells (a palette that does not exist or paints
/// another reading), and for a colour door that fails. A well that cannot be painted is **not** an error: it is
/// a [`WellOutcome`], counted and named, and keeps its colour.
pub fn paint_wells(
    cx: &ViewContext<'_>,
    wells: &[&WellItem],
    ask: &PaintAsk<'_>,
) -> Result<WellPaints, ViewRefusal> {
    let title = ask.title.trim();
    if title.is_empty() {
        return Err(ViewRefusal::computing(
            ID,
            "give a piece of the title of the property to paint along the wells",
        ));
    }
    let members = well_members_of_index(cx.index);
    let mut answer = WellPaints::default();
    let mut resolved: Vec<WellValues> = Vec::new();
    // What the carry said of each well: (well, note), grouped below - the same note of sixty wells is one line.
    let mut said: Vec<(String, String)> = Vec::new();
    for well in wells {
        match well_values(cx, &members, well, &Pick::Title(title)) {
            Ok(Some((found, notes))) => {
                said.extend(notes.into_iter().map(|note| (name_of(well), note)));
                resolved.push(found);
            }
            Ok(None) => answer.results.push(WellResult {
                key: well.object.key(),
                well: name_of(well),
                outcome: WellOutcome::WithoutProperty,
            }),
            Err(why) => answer.results.push(WellResult {
                key: well.object.key(),
                well: name_of(well),
                outcome: WellOutcome::Refused(why),
            }),
        }
    }
    let Some(first) = resolved.first() else {
        return Ok(answer);
    };

    // One reading for the whole drawing: the first well says it, the others that read otherwise are refused by name.
    let continuous = first.continuous;
    let (kept, other): (Vec<WellValues>, Vec<WellValues>) =
        resolved.into_iter().partition(|one| one.continuous == continuous);
    for one in other {
        answer.results.push(WellResult {
            key: one.key,
            well: one.well,
            outcome: WellOutcome::Refused(format!(
                "`{}` is {} here and the wells before it painted {}: one drawing paints one reading",
                one.property,
                if one.continuous { "a measure" } else { "a category" },
                if continuous { "a measure" } else { "a category" },
            )),
        });
    }

    let mut painted: Vec<(WellValues, Vec<String>)> = Vec::new();
    if continuous {
        paint_measures(cx, ask, kept, &mut answer, &mut painted)?;
    } else {
        paint_categories(cx, ask, kept, &mut answer, &mut painted)?;
    }
    for (one, nodes) in painted {
        let coloured = one.values.iter().filter(|value| value.is_some()).count();
        answer.results.push(WellResult {
            key: one.key.clone(),
            well: one.well.clone(),
            outcome: WellOutcome::Painted {
                property: one.property.clone(),
                frames: one.frames.clone(),
                coloured,
                nodes: nodes.len(),
            },
        });
        answer.paints.push((
            one.key,
            Paint {
                nodes,
                blend: if continuous { Blend::Smooth } else { Blend::Net },
            },
        ));
    }
    answer.notes.extend(grouped(&said));
    // The results in the order the wells were given, whatever became of each.
    let order: HashMap<String, usize> = wells
        .iter()
        .enumerate()
        .map(|(at, well)| (well.object.key(), at))
        .collect();
    answer.results.sort_by_key(|one| order.get(&one.key).copied());
    Ok(answer)
}

/// The notes of many wells as lines: a note one well says is `well: note`, a note several say is said once with
/// their count.
fn grouped(said: &[(String, String)]) -> Vec<String> {
    let mut by_note: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (well, note) in said {
        by_note.entry(note.as_str()).or_default().push(well.as_str());
    }
    by_note
        .into_iter()
        .map(|(note, wells)| match wells.as_slice() {
            [one] => format!("{one}: {note}"),
            many => format!("{note} ({} wells)", many.len()),
        })
        .collect()
}

/// What the legend of the wells painted is called: the title their property kind goes by when they all agree on one
/// (a hint the references carry, but one the wells of a source share), else the title the properties themselves all
/// bear, else what was asked.
fn legend_title(kept: &[WellValues], ask: &PaintAsk<'_>) -> String {
    match kept.first().and_then(|first| first.kind_title.as_ref()) {
        Some(title) if kept.iter().all(|one| one.kind_title.as_ref() == Some(title)) => title.clone(),
        _ => match kept.first() {
            Some(first) if kept.iter().all(|one| one.property == first.property) => first.property.clone(),
            _ => ask.title.trim().to_string(),
        },
    }
}

/// The colour of a value as the node's text: `#rrggbbaa`; alpha 0 where there is none (a gap, never a colour).
fn node_text(colour: Option<[u8; 4]>) -> String {
    match colour {
        Some(bytes) => Rgba::from_bytes(bytes).to_hex(),
        None => "#00000000".to_string(),
    }
}

fn paint_measures(
    cx: &ViewContext<'_>,
    ask: &PaintAsk<'_>,
    kept: Vec<WellValues>,
    answer: &mut WellPaints,
    painted: &mut Vec<(WellValues, Vec<String>)>,
) -> Result<(), ViewRefusal> {
    let numbers: Vec<f64> = kept
        .iter()
        .flat_map(|one| one.values.iter().flatten())
        .filter_map(Value::as_f64)
        .collect();
    // The scale is the range of the values painted - every node keeps its colour - and not the percentiles an image takes.
    let finite = numbers.iter().copied().filter(|value| value.is_finite());
    let (low, high) = finite.fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), value| {
        (low.min(value), high.max(value))
    });
    let scale = Scale::new(ScaleKind::Linear, low, high, false).map_err(|why| {
        format!("the values of the property have no scale to colour them ({low} to {high}): {why}")
    });
    let scale = match scale {
        Ok(scale) => scale,
        Err(why) => {
            for one in kept {
                answer.results.push(WellResult {
                    key: one.key,
                    well: one.well,
                    outcome: WellOutcome::Refused(why.clone()),
                });
            }
            return Ok(());
        }
    };
    let head = &kept[0];
    // One scale, one set of colours for every well: those of the first property, said once.
    let mut read_notes: Vec<String> = Vec::new();
    let colours = value_colours_noted(
        cx,
        ID,
        Some(&head.property_uri),
        ask.palette,
        (scale.min(), scale.max()),
        false,
        Reading::Continuous,
        &mut read_notes,
    )?;
    answer.notes.push(colours.source.says());
    answer.notes.extend(read_notes);
    answer.legend = Some(PaintLegend::Ramp(energyml_section::ramp::sampled(
        &legend_title(&kept, ask),
        head.uom.as_deref(),
        &scale,
        &|value| colours.sample(value),
    )));
    for one in kept {
        let nodes = one
            .values
            .iter()
            .map(|value| {
                node_text(
                    value
                        .as_ref()
                        .and_then(Value::as_f64)
                        .and_then(|v| colours.sample(v)),
                )
            })
            .collect();
        painted.push((one, nodes));
    }
    Ok(())
}

fn paint_categories(
    cx: &ViewContext<'_>,
    ask: &PaintAsk<'_>,
    kept: Vec<WellValues>,
    answer: &mut WellPaints,
    painted: &mut Vec<(WellValues, Vec<String>)>,
) -> Result<(), ViewRefusal> {
    // The keys the palette that stands in is laid over: every row of the tables the wells name and every code painted,
    // so that a key has the same colour on every well and no two categories share one.
    let mut keys: Vec<f64> = kept
        .iter()
        .flat_map(|one| {
            one.labels
                .iter()
                .map(|(code, _)| *code)
                .chain(one.values.iter().flatten().filter_map(Value::as_f64))
        })
        .collect();
    keys.sort_by(f64::total_cmp);
    keys.dedup();
    // The colours of a category are those of **its** table: one set per property.
    let mut sets: HashMap<String, ValueColours> = HashMap::new();
    let mut sources: BTreeMap<String, usize> = BTreeMap::new();
    // What reading the colours said (a hue read as a fraction of a turn), each note once.
    let mut said: Vec<String> = Vec::new();
    for one in &kept {
        if !sets.contains_key(&one.property_uri) {
            let mut read_notes: Vec<String> = Vec::new();
            let colours = category_colours_noted(
                cx,
                ID,
                Some(&one.property_uri),
                ask.palette,
                &keys,
                &mut read_notes,
            )?;
            *sources.entry(colours.source.says()).or_default() += 1;
            for note in read_notes {
                if !said.contains(&note) {
                    said.push(note);
                }
            }
            sets.insert(one.property_uri.clone(), colours);
        }
    }
    answer.notes.extend(said);
    // Where the colours come from, once for the wells that agree.
    for (says, count) in sources {
        answer.notes.push(if count > 1 {
            format!("{says} ({count} properties)")
        } else {
            says
        });
    }
    // The legend: each code that was painted, ascending, by the label its table gives (else the code), in the colour
    // it was painted in. The same label in the same colour is one line.
    let mut codes: BTreeMap<i64, (String, String)> = BTreeMap::new();
    for one in &kept {
        let colours = &sets[&one.property_uri];
        for code in one.values.iter().flatten().filter_map(Value::as_f64) {
            let (Some(bytes), true) = (colours.sample(code), code.fract() == 0.0) else {
                continue;
            };
            let label = one
                .labels
                .iter()
                .find(|(row, _)| *row == code)
                .map(|(_, label)| label.clone())
                .unwrap_or_else(|| plain_number(code));
            codes
                .entry(code as i64)
                .or_insert_with(|| (label, Rgba::from_bytes(bytes).to_hex_compact()));
        }
    }
    answer.legend = Some(PaintLegend::Categories(CategoryLegend {
        title: legend_title(&kept, ask),
        entries: codes
            .into_values()
            .map(|(label, colour)| CategoryEntry { label, colour })
            .collect(),
    }));
    for one in kept {
        let colours = &sets[&one.property_uri];
        let nodes = one
            .values
            .iter()
            .map(|value| {
                node_text(
                    value
                        .as_ref()
                        .and_then(Value::as_f64)
                        .and_then(|v| colours.sample(v)),
                )
            })
            .collect();
        painted.push((one, nodes));
    }
    Ok(())
}

/// The values of the property `pick` names at the nodes of one well, or why not: `Ok(None)` is a well that has no such
/// property. The notes are what the carry and the reading said. `members` are the package's wellbores
/// (`energyml_domain::well_members::well_members_of_index`), made once for all the wells.
///
/// Several properties of different titles that fit are a refusal that lists them; the same title on several frames is
/// one property in parts; MDs that cannot be compared refuse the well, by name (see the module's rules).
///
/// # Errors
///
/// The reason the well cannot be given values, in words: it keeps its colour.
#[allow(clippy::type_complexity)]
pub fn well_values(
    cx: &ViewContext<'_>,
    members: &[WellMembers],
    well: &WellItem,
    pick: &Pick<'_>,
) -> Result<Option<(WellValues, Vec<String>)>, String> {
    let title = pick.said();
    let title = title.as_str();
    // The well the item stands for: the objects it folds are the same wellbore's.
    let Some(owner) = wellbore_of(members, well) else {
        return Ok(None);
    };

    // The frames of the well that can hold values: those with a `NodeMd`.
    struct Frame {
        uri: String,
        title: String,
        uuid: String,
        mds: Vec<f64>,
        object: Box<dyn energyml_core::registry::DynObject>,
    }
    let mut frames: Vec<Frame> = Vec::new();
    for member in &owner.frames {
        let Ok(object) = cx.source.read_object_at(&member.uri) else {
            continue;
        };
        let Ok(mds) = frame_values::node_mds(object.as_reflect(), cx.source) else {
            continue;
        };
        let uuid = identity::uuid_of_reflect(object.as_reflect())
            .unwrap_or_default()
            .to_string();
        frames.push(Frame {
            uri: member.uri.clone(),
            title: if member.title.is_empty() {
                member.uri.clone()
            } else {
                member.title.clone()
            },
            uuid,
            mds,
            object,
        });
    }

    // The properties of the well whose title fits and that hang on one of those frames.
    struct Hung {
        uri: String,
        title: String,
        frame: usize,
        object: Box<dyn energyml_core::registry::DynObject>,
    }
    let mut hung: Vec<Hung> = Vec::new();
    for member in &owner.properties {
        let Ok(object) = cx.source.read_object_at(&member.uri) else {
            continue;
        };
        let own = if member.title.is_empty() {
            identity::title_of_reflect(object.as_reflect())
                .unwrap_or_default()
                .to_string()
        } else {
            member.title.clone()
        };
        if !pick.fits(&member.uri, &own) {
            continue;
        }
        let Some(frame) = frames
            .iter()
            .position(|frame| hangs_on(object.as_reflect(), &frame.uuid))
        else {
            continue;
        };
        hung.push(Hung {
            uri: member.uri.clone(),
            title: own,
            frame,
            object,
        });
    }
    if hung.is_empty() {
        return Ok(None);
    }

    // One property: the same title, once on each frame. Several titles, or one twice on a frame, is a choice nobody
    // made: refused with the list.
    let mut titles: Vec<String> = hung.iter().map(|one| one.title.trim().to_lowercase()).collect();
    titles.sort();
    titles.dedup();
    if titles.len() > 1 {
        let mut listed: Vec<String> = hung
            .iter()
            .map(|one| format!("`{}` on `{}` ({})", one.title, frames[one.frame].title, one.uri))
            .collect();
        listed.sort();
        return Err(format!(
            "{} properties of this well fit `{title}`: {}; give more of the title",
            titles.len(),
            listed.join("; ")
        ));
    }
    for (at, one) in hung.iter().enumerate() {
        if let Some(other) = hung[..at].iter().find(|before| before.frame == one.frame) {
            return Err(format!(
                "two properties titled `{}` hang on the frame `{}`: {} and {}",
                one.title, frames[one.frame].title, other.uri, one.uri
            ));
        }
    }

    // Their values, each on its own frame.
    let mut read: Vec<(FrameValues, usize)> = Vec::new();
    for one in &hung {
        let frame = &frames[one.frame];
        let values = frame_values::of_given(&frame.uuid, &frame.mds, one.object.as_reflect(), cx.source)
            .map_err(|why| format!("`{}` on `{}`: {why}", one.title, frame.title))?;
        read.push((values, one.frame));
    }
    if read
        .iter()
        .any(|(values, _)| values.continuous != read[0].0.continuous)
    {
        return Err(format!(
            "the properties titled `{}` are a measure on one frame and a category on another",
            hung[0].title
        ));
    }

    // Carried by MD onto the nodes the section kept.
    let Some(mds) = well.md.as_ref().filter(|mds| mds.len() == well.path.len()) else {
        return Err(
            "the section keeps no measured depth for the nodes of this well, so a value has no place on it"
                .to_string(),
        );
    };
    let target = cx
        .source
        .read_object_at(&well.object.uri)
        .map_err(|why| format!("the well's object is not read: {why}"))?;
    let reference = md_reference(target.as_reflect(), cx.source).map_err(|why| why.to_string())?;
    let references: Vec<_> = read
        .iter()
        .map(|(_, at)| {
            md_reference(frames[*at].object.as_reflect(), cx.source).map_err(|why| why.to_string())
        })
        .collect::<Result<_, _>>()?;
    let stated: Vec<Stated<'_>> = read
        .iter()
        .zip(&references)
        .map(|((values, _), reference)| Stated { values, reference })
        .collect();
    let uuid = identity::uuid_of_reflect(target.as_reflect()).unwrap_or_default();
    let carried = carry(uuid, &name_of(well), mds, &reference, &stated).map_err(|why| why.to_string())?;
    let values: Vec<Option<Value>> = carried
        .nodes
        .iter()
        .map(|node| match node {
            Carry::Value(value) => Some(value.clone()),
            Carry::Absent | Carry::Outside | Carry::Disagreement => None,
        })
        .collect();
    if values.iter().all(|value| value.is_none()) {
        // Said by cause, each counted: a node outside every frame that holds the property, a node the frame holds and
        // gives no value (a null), a node two frames name differently. They are not the same fault.
        let nodes = carried.nodes.len();
        let outside = carried.outside();
        let nulls = carried
            .nodes
            .iter()
            .filter(|node| **node == Carry::Absent)
            .count();
        let disputed = carried.disagreements();
        let mut causes = vec![format!(
            "{outside} of {nodes} node(s) lie outside every frame that holds the property"
        )];
        causes.push(format!("{nulls} have no value there (null)"));
        if disputed > 0 {
            causes.push(format!("{disputed} are named differently by two frames"));
        }
        return Err(format!(
            "`{}` gives no value at any node of this well: {}",
            hung[0].title,
            causes.join(", ")
        ));
    }

    let head = &read[0].0;
    let first = &hung[0];
    Ok(Some((
        WellValues {
            key: well.object.key(),
            well: name_of(well),
            property_uri: first.uri.clone(),
            property: first.title.clone(),
            continuous: head.continuous,
            uom: head.uom.clone(),
            kind_title: match &head.kind {
                Some(PropertyKindRef::Object { title, .. }) => {
                    title.clone().filter(|title| !title.trim().is_empty())
                }
                Some(PropertyKindRef::Standard { name }) => Some(name.clone()),
                None => None,
            },
            categorical: head.categories.is_some(),
            labels: head.categories.as_ref().map(|c| c.labels()).unwrap_or_default(),
            frames: read.iter().map(|(_, at)| frames[*at].title.clone()).collect(),
            parts: hung
                .iter()
                .zip(&read)
                .map(|(one, (_, at))| Part {
                    property_uri: one.uri.clone(),
                    frame_uri: frames[*at].uri.clone(),
                })
                .collect(),
            range: read.iter().filter_map(|(values, _)| values.range).fold(
                None,
                |range: Option<(f64, f64)>, one| {
                    Some(range.map_or((one.min, one.max), |(low, high)| {
                        (low.min(one.min), high.max(one.max))
                    }))
                },
            ),
            values,
        },
        carried.notes,
    )))
}
