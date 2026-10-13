//! `colormap` — a `ContinuousColorMap` that draws flat bands: one colour per
//! band, a band starting at each level.
//!
//! What the old promoter's `colormap` arm wrote, from a [`Source::Ramp`]:
//! the levels ascending and one colour per band. The stops come out in pairs
//! so that a *continuous* map draws flat bands — `energyml_domain::color::banded`
//! decides that, and it is the one place that does. No input from the
//! package: a colour map is a legend, and a legend attaches to a property by
//! the graphical information a later gesture writes.
//!
//! **The levels and their colours may also be said in words** — `levels` and
//! `colours` (`#rrggbb`) — for a caller that has no ramp to hand: a recipe.
//! A use case: testing a painting that follows its colour map, on one of the
//! properties of a DTM built from BRGM boreholes. One or the other: a ramp **and** levels is two
//! answers to one question, and refused by name, as is neither.

use serde_json::json;

use energyml_domain::provenance::Parameter;

use crate::declare::{Inputs, Params, Ramp, Suggestions, Title};
use crate::operation::{
    About, ActivityDraft, Category, Context, Need, NewObject, Operation, Plan, Refusal, Slot,
};
use crate::pieces::{Pieces, Source};

use super::support;

/// The operation.
pub struct Colormap;

/// What `colormap` is told.
#[derive(Debug, Params)]
pub struct ColormapParams {
    /// the colour map's title
    pub title: Title,
    /// the levels, ascending, a band starting at each — when no ramp is given
    pub levels: Vec<f64>,
    /// one colour per level, #rrggbb or #rrggbbaa — when no ramp is given
    pub colours: Vec<String>,
}

/// What `colormap` takes.
#[derive(Debug, Inputs)]
pub struct ColormapInputs {
    /// the levels and their colours — or `levels` and `colours`, said in words
    pub ramp: Option<Ramp>,
}

impl Operation for Colormap {
    type Params = ColormapParams;
    type Inputs = ColormapInputs;

    fn about(&self) -> About {
        About {
            name: "colormap",
            describe: "a colour map of flat bands — one colour per band, a band starting at each level",
            category: Category::Properties,
            makes: &["resqml22.ContinuousColorMap"],
            recipe: "colormap",
            represents: None,
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        if let Some((name, _)) = bag
            .iter()
            .find(|(_, source)| matches!(source, Source::Ramp { .. }))
        {
            slots.set("title", json!(name));
        }
    }

    /// A **ramp** given is judged at the offer, as it always was: one level is
    /// no band, and the colours must be as many as the levels — said before any
    /// work. Bands said in words are judged in `plan`, where the parameters are.
    fn check(&self, inputs: &ColormapInputs) -> Result<(), Refusal> {
        let Some(ramp) = &inputs.ramp else { return Ok(()) };
        bands_refusal(ramp.levels.len(), ramp.colours.len(), "ramp").map_or(Ok(()), Err)
    }

    fn plan(
        &self,
        inputs: ColormapInputs,
        params: ColormapParams,
        cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        let (levels, colours, slot) = match (
            inputs.ramp,
            params.levels.is_empty() && params.colours.is_empty(),
        ) {
            (Some(_), false) => {
                return Err(Refusal::on(
                    "levels",
                    "a ramp and levels are both given: two answers to one question — give one or the other",
                ));
            }
            (Some(ramp), true) => (ramp.levels, ramp.colours, "ramp"),
            (None, true) => {
                return Err(Refusal::on(
                    "ramp",
                    "no ramp, and no `levels` and `colours`: a colour map needs its bands",
                ));
            }
            (None, false) => {
                let mut colours = Vec::with_capacity(params.colours.len());
                for (at, text) in params.colours.iter().enumerate() {
                    let rgba = energyml_domain::color::Rgba::parse(text)
                        .map_err(|why| Refusal::on("colours", format!("colour {at}, `{text}`: {why}")))?;
                    colours.push(energyml_domain::color::Hsv::from_rgba(rgba));
                }
                (params.levels, colours, "levels")
            }
        };
        if let Some(refusal) = bands_refusal(levels.len(), colours.len(), slot) {
            return Err(refusal);
        }
        let (levels, colours) = (levels.as_slice(), colours.as_slice());
        let title = params.title.0;
        let seed = cx.seed(&title);
        let map = write(levels, colours, None, &title, &seed, cx)?;
        Ok(Plan {
            media: Vec::new(),
            blocked: None,
            objects: vec![map],
            datasets: Vec::new(),
            activity: ActivityDraft {
                recipe: "colormap",
                title: format!("colormap → {title}"),
                seed,
                parameters: vec![
                    Parameter::Double {
                        title: "levelCount".to_string(),
                        value: levels.len() as f64,
                        uom: None,
                    },
                    Parameter::Double {
                        title: "low".to_string(),
                        value: levels[0],
                        uom: None,
                    },
                    Parameter::Double {
                        title: "high".to_string(),
                        value: levels[levels.len() - 1],
                        uom: None,
                    },
                ],
            },
            says: Vec::new(),
            dropped: Vec::new(),
            // **No shape**: this operation computes no geometry.
            // An empty list that is an answer, not an oversight.
            shapes: Vec::new(),
        })
    }

    fn reports(&self) -> Vec<Slot> {
        let count = |name: &'static str, about: &'static str| {
            Slot::required(
                name,
                Need::Number {
                    uom: None,
                    least: None,
                    most: None,
                },
                about,
            )
        };
        vec![
            count("levelCount", "levels"),
            count("low", "the lowest level"),
            count("high", "the highest level"),
        ]
    }
}

/// Why these bands are not bands, or `None`: one level is no band, and a band
/// starts at each level, the last one open-ended — as many colours as levels,
/// `banded`'s own contract, named before any work.
fn bands_refusal(levels: usize, colours: usize, slot: &'static str) -> Option<Refusal> {
    if levels < 2 {
        return Some(Refusal::on(
            slot,
            format!("a band needs two levels; {levels} given"),
        ));
    }
    if colours != levels {
        return Some(Refusal::on(
            slot,
            format!(
                "{levels} level(s) and {colours} colour(s): one colour per band, and a band starts at each level"
            ),
        ));
    }
    None
}

/// **The colour map, written** — what the operation does, and what
/// `isocontours` calls with the levels it cut: one `ContinuousColorMap` of flat
/// bands, a band starting at each level. `until` closes the last band there
/// ([`energyml_domain::color::banded_until`]); `None`, it runs upwards with no
/// width of its own, what this operation has always written. `seed` is the
/// caller's.
pub fn write(
    levels: &[f64],
    colours: &[energyml_domain::color::Hsv],
    until: Option<f64>,
    title: &str,
    seed: &str,
    cx: &Context<'_>,
) -> Result<NewObject, Refusal> {
    let stops = match until {
        Some(top) => energyml_domain::color::banded_until(levels, colours, top),
        None => energyml_domain::color::banded(levels, colours),
    }
    .map_err(|e| Refusal::on("ramp", e))?;
    let entries: Vec<serde_json::Value> = stops
        .iter()
        .map(|stop| {
            json!({
                "$type": "resqml22.ContinuousColorMapEntry",
                "Index": stop.index,
                "Hsv": {
                    "$type": "resqml22.HsvColor",
                    "Hue": stop.color.hue,
                    "Saturation": stop.color.saturation,
                    "Value": stop.color.value,
                    "Alpha": stop.color.alpha,
                },
            })
        })
        .collect();
    let mut rank = 0usize;
    let uuid = support::next_uuid(cx.identity, &format!("colormap:{seed}"), &mut rank)?;
    Ok(NewObject {
        uuid: uuid.clone(),
        qualified_type: "resqml22.ContinuousColorMap".to_string(),
        role: "colormap".to_string(),
        title: title.to_string(),
        json: json!({
            "$type": "resqml22.ContinuousColorMap",
            "uuid": uuid,
            "schemaVersion": "2.2",
            "Citation": support::citation(cx.when, title),
            "InterpolationDomain": "rgb",
            "InterpolationMethod": "linear",
            "Entry": entries,
        }),
    })
}
