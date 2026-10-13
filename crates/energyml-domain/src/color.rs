//! Colour: the HSV the schema stores, the maps that turn values into it, and
//! the rule that decides which of them applies to what.
//!
//! # Three facts that shape everything here
//!
//! **RESQML 2.0.1 has no colour at all.** An exhaustive search for
//! `color|colour|graphical|legend|hsv|rgb` over the whole 2.0.1 schema returns
//! one hit, and it is the word *stratigraphically*. So ALWYN and Volve cannot
//! carry colour, and any colour for a 2.0.1 model is outside the standard.
//! That is a design constraint, not a gap.
//!
//! **The container is EML, the contents are RESQML.** A
//! `GraphicalInformationSet` is an eml 2.1/2.2/2.3 object whose only
//! instantiable members are RESQML 2.2 types, injected by `xsi:type`. The eml
//! annotation says it outright: *"Currently, only RESQML v2.2 specifies
//! detailed graphical information."*
//!
//! **There is no RGB type.** Every colour in the standard is an `HsvColor`,
//! and the schema puts **no bounds** on its four numbers — no
//! `minInclusive`, no `maxInclusive`, just a link to Wikipedia. The corpus is
//! consistent with hue in 0–360 and the rest in 0–1, so that is what [`Hsv`]
//! validates, and a colour outside it is refused by name rather than silently
//! producing a different one.
//!
//! # What decides which colour applies — the cascade
//!
//! Nothing in the three technical reference guides defines precedence or
//! inheritance for graphical information: searched for `inherit`, `propagat`,
//! `override`, `precedence`, `priorit`, `fallback`, `default color`, found
//! nothing. So the rule is this repository's, written down in ADR-0009:
//!
//! 1. a `ColorInformation` whose target **is the property** being drawn;
//! 2. one targeting **its property kind**, then the kind's **parents**, in
//!    order;
//! 3. the representation's own `DefaultGraphicalInformation`, for the element
//!    being drawn, then for the whole object;
//! 4. the same on its **interpretation**;
//! 5. the same on its **feature**;
//! 6. nothing — and then it is `None`, never an invented colour.
//!
//! Two guarantees come with it. The result always says **which level** it came
//! from, so an application can refuse an inherited colour rather than have the
//! choice made for it; and steps 4–5 can be switched off entirely
//! ([`Cascade::inherit`]).
//!
//! # `ValueVectorIndex` is an index into the data
//!
//! It selects the component of a vector-valued property, the same index space
//! `LabelPerComponent` labels — **not** an entry of the colour map. The
//! decisive argument is in the schema: `ContourLineSetInformation` carries
//! `ValueVectorIndex` and has no `ColorMap` at all. Reading it as a palette
//! index turns a 5 000-node gradient into one flat colour, which is exactly
//! what `energyml-utils` does today.

use std::cell::RefCell;
use std::collections::HashMap;

use energyml_core::any_xml::AnyXml;
use energyml_core::identity;
use energyml_core::value::{FieldValue, Reflect, Scalar};

use crate::error::{Error, Result};
use crate::inherit::uuid_of_identity;
use crate::scan;
use crate::source::ObjectSource;

// ---------------------------------------------------------------------------
// The colour itself
// ---------------------------------------------------------------------------

/// A colour as the standard stores it.
///
/// Bounds are checked on the way in ([`Hsv::read`]), because the schema
/// declares none and a hue of 500 would otherwise come out as a plausible
/// wrong colour.
#[derive(Clone, PartialEq, Debug)]
pub struct Hsv {
    /// 0–360 degrees.
    pub hue: f64,
    /// 0–1.
    pub saturation: f64,
    /// 0–1.
    pub value: f64,
    /// 0 fully transparent, 1 fully opaque — the one bound the schema does
    /// state, in prose.
    pub alpha: f64,
    /// `Title`, the name of the colour, when the producer gave one.
    pub title: Option<String>,
}

/// A colour ready to be drawn, each channel in 0–1.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rgba {
    /// Red, 0–1.
    pub r: f64,
    /// Green, 0–1.
    pub g: f64,
    /// Blue, 0–1.
    pub b: f64,
    /// Alpha, 0–1.
    pub a: f64,
}

impl Rgba {
    /// The four channels as bytes, for a format that wants them that way.
    pub fn to_bytes(self) -> [u8; 4] {
        let byte = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        [byte(self.r), byte(self.g), byte(self.b), byte(self.a)]
    }

    /// `#rrggbbaa`.
    pub fn to_hex(self) -> String {
        let [r, g, b, a] = self.to_bytes();
        format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
    }

    /// `#rrggbb` or `#rrggbbaa` — six digits are opaque — the inverse of
    /// [`Rgba::to_hex`].
    ///
    /// Everything else is refused, with the text it was given: a colour that
    /// silently becomes another is the failure a web front end's colour module
    /// was written about. Stricter than `u8::from_str_radix` alone, which takes
    /// a sign — `#+f+f+f` read as a dark grey in the front end's bridge until
    /// 24/09/2026, whose own doc promised *nothing else is accepted* — and the
    /// `#` is required. The one reader of a colour's text: the front end's bridge
    /// and the `graphical-information` operation call it.
    pub fn parse(text: &str) -> std::result::Result<Rgba, String> {
        let text = text.trim();
        let refused = || {
            format!(
                "`{text}` is not a colour written `#rrggbb` or `#rrggbbaa` — red, green, blue and, with eight digits, alpha, two hexadecimal digits each"
            )
        };
        let Some(digits) = text.strip_prefix('#') else { return Err(refused()) };
        if !matches!(digits.len(), 6 | 8) || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(refused());
        }
        let channel = |at: usize| -> std::result::Result<f64, String> {
            u8::from_str_radix(&digits[at..at + 2], 16).map(|value| f64::from(value) / 255.0).map_err(|_| refused())
        };
        Ok(Rgba {
            r: channel(0)?,
            g: channel(2)?,
            b: channel(4)?,
            a: if digits.len() == 8 { channel(6)? } else { 1.0 },
        })
    }

    /// Linear interpolation between two colours, channel by channel.
    pub fn lerp(self, other: Rgba, t: f64) -> Rgba {
        let mix = |a: f64, b: f64| a + (b - a) * t;
        Rgba {
            r: mix(self.r, other.r),
            g: mix(self.g, other.g),
            b: mix(self.b, other.b),
            a: mix(self.a, other.a),
        }
    }
}

impl Hsv {
    /// Convert from RGB.
    ///
    /// The direction a **caller** needs, and it is not decoration. An
    /// application thinks in `#rrggbb`: a palette offered in a menu, a colour
    /// picked in a dialog, a ramp taken from a chart library. The standard
    /// stores HSV and nothing else, so every one of those has to come through
    /// here — to be sampled like a map the file carries, and to be *written*
    /// as a `DiscreteColorMap` or a `ContinuousColorMap` the day a package is
    /// given the colours somebody chose.
    ///
    /// Grey has no hue; `0` is written rather than left undefined, which is
    /// what makes the trip back exact:
    ///
    /// ```
    /// # use energyml_domain::color::{Hsv, Rgba};
    /// let red = Rgba { r: 1.0, g: 0.0, b: 0.0, a: 1.0 };
    /// assert_eq!(Hsv::from_rgba(red).to_rgba().to_hex(), red.to_hex());
    /// ```
    pub fn from_rgba(rgba: Rgba) -> Hsv {
        let (r, g, b) = (
            rgba.r.clamp(0.0, 1.0),
            rgba.g.clamp(0.0, 1.0),
            rgba.b.clamp(0.0, 1.0),
        );
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let chroma = max - min;
        let hue = if chroma <= f64::EPSILON {
            0.0
        } else if max == r {
            60.0 * (((g - b) / chroma) % 6.0)
        } else if max == g {
            60.0 * ((b - r) / chroma + 2.0)
        } else {
            60.0 * ((r - g) / chroma + 4.0)
        };
        Hsv {
            hue: hue.rem_euclid(360.0),
            saturation: if max <= f64::EPSILON { 0.0 } else { chroma / max },
            value: max,
            alpha: rgba.a.clamp(0.0, 1.0),
            title: None,
        }
    }

    /// Convert to RGB.
    pub fn to_rgba(&self) -> Rgba {
        let h = self.hue.rem_euclid(360.0) / 60.0;
        let s = self.saturation.clamp(0.0, 1.0);
        let v = self.value.clamp(0.0, 1.0);
        let c = v * s;
        let x = c * (1.0 - ((h % 2.0) - 1.0).abs());
        let m = v - c;
        let (r, g, b) = match h as u32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        Rgba {
            r: r + m,
            g: g + m,
            b: b + m,
            a: self.alpha.clamp(0.0, 1.0),
        }
    }

    /// Interpolate two colours **in HSV**, hue included, linearly.
    ///
    /// Linearly and not by the shortest arc: the schema says
    /// `InterpolationDomain = hsv` and nothing more, and taking the short way
    /// round the circle would make green→red pass through yellow in one build
    /// and through magenta in another. A producer who wants the short arc
    /// writes the stop in between.
    pub fn lerp(&self, other: &Hsv, t: f64) -> Hsv {
        let mix = |a: f64, b: f64| a + (b - a) * t;
        Hsv {
            hue: mix(self.hue, other.hue),
            saturation: mix(self.saturation, other.saturation),
            value: mix(self.value, other.value),
            alpha: mix(self.alpha, other.alpha),
            title: None,
        }
    }

    /// Read an `HsvColor` object, bounds checked.
    pub fn read(object: &dyn Reflect, at: &str) -> Result<Hsv> {
        let number = |field: &str| -> Result<f64> {
            scalar_f64(object, field).ok_or_else(|| {
                Error::malformed(
                    at,
                    format!("`{field}` is absent from an `HsvColor`, where the schema makes it mandatory"),
                )
            })
        };
        Hsv::checked(
            number("Hue")?,
            number("Saturation")?,
            number("Value")?,
            number("Alpha")?,
            text(object, "Title"),
            at,
        )
    }

    /// The four numbers, checked against the bounds the schema does not state.
    ///
    /// Refuses rather than wraps: a hue of 500 wrapped to 140 is a *different
    /// colour*, drawn without a word. There is no facet anywhere on
    /// `HsvColor`, so no validator will ever catch this — which is precisely
    /// why it is caught here.
    pub fn checked(
        hue: f64,
        saturation: f64,
        value: f64,
        alpha: f64,
        title: Option<String>,
        at: &str,
    ) -> Result<Hsv> {
        let check = |name: &str, v: f64, low: f64, high: f64| -> Result<()> {
            if v.is_nan() || v < low || v > high {
                return Err(Error::malformed(
                    at,
                    format!(
                        "`{name}` is {v}, outside {low}–{high}. The XSD declares no bounds on \
                         `HsvColor`, so this cannot be caught by validation; it is refused here \
                         rather than drawn as a different colour"
                    ),
                ));
            }
            Ok(())
        };
        check("Hue", hue, 0.0, 360.0)?;
        check("Saturation", saturation, 0.0, 1.0)?;
        check("Value", value, 0.0, 1.0)?;
        check("Alpha", alpha, 0.0, 1.0)?;
        Ok(Hsv {
            hue,
            saturation,
            value,
            alpha,
            title,
        })
    }
}

// ---------------------------------------------------------------------------
// Colour maps
// ---------------------------------------------------------------------------

/// How a continuous map interpolates between two stops.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Domain {
    /// Interpolate the H, S and V numbers.
    Hsv,
    /// Convert both stops to RGB and interpolate there.
    ///
    /// Not the same picture: green→red interpolated in HSV passes through
    /// yellow, in RGB through olive grey. The reference package has a map
    /// whose stops are HSV and whose domain is `rgb`, so this is not academic.
    Rgb,
}

/// Linear or logarithmic spacing between stops.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Interpolation {
    /// Even spacing.
    Linear,
    /// Logarithmic spacing.
    Logarithmic,
}

/// One stop of a map.
#[derive(Clone, PartialEq, Debug)]
pub struct Stop {
    /// `Index` on a continuous entry (a double), `index` on a discrete one (a
    /// long). The capital letter really does differ between the two — it is in
    /// the schema, not a typo of the writer.
    pub index: f64,
    /// The colour at that index.
    pub color: Hsv,
}

/// A `ContinuousColorMap` or a `DiscreteColorMap`.
#[derive(Clone, PartialEq, Debug)]
pub struct ColorMap {
    /// Uuid of the map object.
    pub uuid: String,
    /// `Citation.Title`.
    pub title: Option<String>,
    /// Continuous maps interpolate; discrete ones do not.
    pub continuous: Option<(Domain, Interpolation)>,
    /// Stops, sorted by index.
    pub stops: Vec<Stop>,
    /// Colour for a null value.
    pub null_color: Option<Hsv>,
    /// Colour for a value above the map's own maximum.
    pub above_max: Option<Hsv>,
    /// Colour for a value below the map's own minimum.
    pub below_min: Option<Hsv>,
}

impl ColorMap {
    /// Read a colour map object.
    pub fn read(object: &dyn Reflect) -> Result<ColorMap> {
        let name = object.type_info().name;
        let uuid = identity::uuid_of_reflect(object)
            .ok_or_else(|| Error::malformed(name, "a colour map with no uuid"))?
            .to_string();

        let continuous = if name.contains("Continuous") {
            let domain = match text(object, "InterpolationDomain").as_deref() {
                Some("rgb") => Domain::Rgb,
                Some("hsv") | None => Domain::Hsv,
                Some(other) => {
                    return Err(Error::malformed(
                        &uuid,
                        format!("`{other}` is not an interpolation domain the schema defines (hsv, rgb)"),
                    ));
                }
            };
            let method = match text(object, "InterpolationMethod").as_deref() {
                Some("logarithmic") => Interpolation::Logarithmic,
                Some("linear") | None => Interpolation::Linear,
                Some(other) => {
                    return Err(Error::malformed(
                        &uuid,
                        format!("`{other}` is not an interpolation method the schema defines (linear, logarithmic)"),
                    ));
                }
            };
            Some((domain, method))
        } else if name.contains("Discrete") {
            None
        } else {
            return Err(Error::unsupported(
                format!("`{name}` as a colour map"),
                "the schema has exactly two: `ContinuousColorMap` and `DiscreteColorMap`".to_string(),
            ));
        };

        let mut stops = Vec::new();
        for (at, entry) in list(object, "Entry").into_iter().enumerate() {
            let Some(entry) = entry.as_object() else { continue };
            // `Index` on a continuous entry, `index` on a discrete one. Both
            // are tried on both, because a producer that follows the other
            // spelling is still saying the same thing.
            let index = scalar_f64(entry, "Index")
                .or_else(|| scalar_f64(entry, "index"))
                .ok_or_else(|| {
                    Error::malformed(&uuid, format!("entry {at} has no index, so it colours nothing"))
                })?;
            let color = entry
                .field("Hsv")
                .and_then(|f| f.as_object())
                .ok_or_else(|| Error::malformed(&uuid, format!("entry {at} carries no colour")))?;
            stops.push(Stop {
                index,
                color: Hsv::read(color, &format!("{uuid} entry {at}"))?,
            });
        }
        if stops.is_empty() {
            return Err(Error::malformed(
                &uuid,
                "a colour map with no entry at all; the schema requires at least one (two for a \
                 continuous map)",
            ));
        }
        stops.sort_by(|a, b| a.index.partial_cmp(&b.index).unwrap_or(std::cmp::Ordering::Equal));

        let optional_color = |field: &str| -> Result<Option<Hsv>> {
            match object.field(field).and_then(|f| f.as_object()) {
                Some(color) => Ok(Some(Hsv::read(color, &format!("{uuid}.{field}"))?)),
                None => Ok(None),
            }
        };

        Ok(ColorMap {
            title: object
                .field("Citation")
                .and_then(|f| f.as_object())
                .and_then(|citation| text(citation, "Title")),
            continuous,
            stops,
            null_color: optional_color("NullColor")?,
            above_max: optional_color("AboveMaxColor")?,
            below_min: optional_color("BelowMinColor")?,
            uuid,
        })
    }

    /// The lowest and highest index of the stops.
    pub fn range(&self) -> (f64, f64) {
        (
            self.stops.first().map(|s| s.index).unwrap_or(0.0),
            self.stops.last().map(|s| s.index).unwrap_or(0.0),
        )
    }

    /// The colour for one value.
    ///
    /// `None` where the map genuinely has no answer — a null value and no
    /// `NullColor` — because an invented colour is indistinguishable from a
    /// real one.
    ///
    /// The order, which is the only reading of §4.21 and §4.5 of the TRG that
    /// does not contradict itself:
    ///
    /// 1. a null value takes `NullColor`;
    /// 2. `ColorInformation.MinMax`, when given, is the range of the
    ///    **property**: values outside it *"continue to have the extreme
    ///    colour from this range"*, so they are clamped — and that clamping is
    ///    what makes `AboveMaxColor`/`BelowMinColor` unreachable, which is why
    ///    they only apply when `MinMax` is absent;
    /// 3. the value is placed in the map's own index range;
    /// 4. a discrete map takes **the nearest lower entry that exists** — the
    ///    schema's own example: stops at 10, 20, 30 and a value of 16.5 give
    ///    the colour of 10.
    pub fn sample(&self, value: Option<f64>, scaling: &Scaling) -> Option<Rgba> {
        let Some(value) = value.filter(|v| !v.is_nan()) else {
            return self.null_color.as_ref().map(Hsv::to_rgba);
        };

        let (map_low, map_high) = self.range();
        let mut position = value;

        if let Some((min, max)) = scaling.min_max {
            // Clamp to the property's range, then rescale onto the map's.
            let clamped = value.clamp(min.min(max), min.max(max));
            position = if (max - min).abs() < f64::EPSILON {
                map_low
            } else {
                map_low + (clamped - min) / (max - min) * (map_high - map_low)
            };
        } else {
            if value < map_low && let Some(color) = &self.below_min {
                return Some(color.to_rgba());
            }
            if value > map_high && let Some(color) = &self.above_max {
                return Some(color.to_rgba());
            }
        }

        if scaling.reverse {
            position = map_low + map_high - position;
        }

        match self.continuous {
            None => Some(self.discrete_at(position)),
            Some((domain, method)) => Some(self.continuous_at(position, domain, method, scaling)),
        }
    }

    /// The schema's business rule: the nearest **existing** lower entry.
    fn discrete_at(&self, position: f64) -> Rgba {
        let mut chosen = &self.stops[0];
        for stop in &self.stops {
            if stop.index <= position {
                chosen = stop;
            } else {
                break;
            }
        }
        // Below every stop: the first one, unless the map says otherwise.
        if position < self.stops[0].index {
            if let Some(color) = &self.below_min {
                return color.to_rgba();
            }
        }
        chosen.color.to_rgba()
    }

    fn continuous_at(
        &self,
        position: f64,
        domain: Domain,
        method: Interpolation,
        scaling: &Scaling,
    ) -> Rgba {
        let first = &self.stops[0];
        let last = &self.stops[self.stops.len() - 1];
        if position <= first.index {
            return first.color.to_rgba();
        }
        if position >= last.index {
            return last.color.to_rgba();
        }
        let window = self
            .stops
            .windows(2)
            .find(|pair| position >= pair[0].index && position <= pair[1].index)
            .unwrap_or(&self.stops[..2]);
        let (low, high) = (&window[0], &window[1]);
        let span = high.index - low.index;
        let mut t = if span.abs() < f64::EPSILON {
            0.0
        } else {
            (position - low.index) / span
        };

        // Logarithmic spacing, from the map; `UseLogarithmicMapping`, from the
        // colour information. Both mean the same thing here, and either one
        // turns it on.
        if matches!(method, Interpolation::Logarithmic) || scaling.logarithmic {
            let (a, b) = (low.index.max(f64::MIN_POSITIVE), high.index.max(f64::MIN_POSITIVE));
            let p = position.max(f64::MIN_POSITIVE);
            if b > a {
                t = ((p / a).ln() / (b / a).ln()).clamp(0.0, 1.0);
            }
        }

        match domain {
            Domain::Hsv => low.color.lerp(&high.color, t).to_rgba(),
            Domain::Rgb => low.color.to_rgba().lerp(high.color.to_rgba(), t),
        }
    }
}

/// How far below a level the closing stop of its band is placed.
///
/// A `ContinuousColorMap` interpolates between its stops, so a band of one
/// flat colour is written as **two** stops of that colour: one at the level
/// where the band starts, one just short of the level where it ends. The jump
/// then happens across this gap instead of across the whole band.
///
/// A millionth of the band, and the two bounds that pick that number:
///
/// - **big enough to survive being written down.** One ULP would be the
///   sharpest edge arithmetic allows, and the wrong choice: a producer or
///   reader that prints fifteen significant digits collapses the pair onto one
///   index, and a colour map with two stops at the same place is not a map any
///   more. A millionth of the band survives any sane serialisation;
/// - **small enough to be invisible.** Over a band of 25 it puts the closing
///   stop at 24.999975 — under a thousandth of a pixel on any legend anyone
///   will draw.
const BAND_EDGE: f64 = 1e-6;

/// Stops that paint **flat bands** between the given levels.
///
/// What an iso-contour map looks like: everything between two levels takes one
/// colour, and the colour changes abruptly at each level rather than sliding
/// from one to the next.
///
/// `levels` are the band boundaries, ascending, and `colours` gives the colour
/// of the band *starting* at each level — so the two have the same length, and
/// the last colour runs from the last level upwards.
///
/// # Why this is a shape and not a kind of map
///
/// RESQML has a `DiscreteColorMap`, which does not interpolate and would give
/// hard edges for free — but it is indexed by a **discrete** value, so using
/// it means replacing the measurements with band numbers. The decision was to
/// keep the `ContinuousProperty` carrying its real values, and
/// this is what that costs: a continuous map whose stops come in pairs. A
/// reader that knows nothing of this sees a perfectly ordinary map with twice
/// as many stops, and draws the same picture.
///
/// Refuses rather than guesses when the two lists disagree, when a level is
/// not a finite number, or when the levels do not ascend — a map whose stops
/// are out of order paints bands nobody asked for.
pub fn banded(levels: &[f64], colours: &[Hsv]) -> Result<Vec<Stop>> {
    if levels.len() != colours.len() {
        return Err(crate::Error::unsupported(
            "banding a colour map",
            format!(
                "{} level(s) and {} colour(s) — one colour per band, and a band starts at each level",
                levels.len(),
                colours.len()
            ),
        ));
    }
    if levels.is_empty() {
        return Err(crate::Error::unsupported(
            "banding a colour map",
            "no level at all, so there is no band to paint".to_string(),
        ));
    }
    if let Some(bad) = levels.iter().find(|one| !one.is_finite()) {
        return Err(crate::Error::unsupported(
            "banding a colour map",
            format!("the level {bad} is not a finite number"),
        ));
    }
    if let Some(at) = (1..levels.len()).find(|at| levels[*at] <= levels[at - 1]) {
        return Err(crate::Error::unsupported(
            "banding a colour map",
            format!(
                "the levels do not ascend: {} then {} at position {at}",
                levels[at - 1],
                levels[at]
            ),
        ));
    }

    let mut stops = Vec::with_capacity(levels.len() * 2 - 1);
    for (at, (level, colour)) in levels.iter().zip(colours).enumerate() {
        stops.push(Stop {
            index: *level,
            color: colour.clone(),
        });
        // Every band but the last one is closed just short of the next level.
        // The last has no next level: it runs upwards, and `above_max` is what
        // says what happens past the map.
        if let Some(next) = levels.get(at + 1) {
            stops.push(Stop {
                index: next - (next - level) * BAND_EDGE,
                color: colour.clone(),
            });
        }
    }
    Ok(stops)
}

/// [`banded`], with the **last band closed** at `top`: one more stop, the last
/// colour, at `top` — when `top` is above the last level; `banded` itself
/// otherwise.
///
/// Reported: in the object edit view, the coloured band did not show the last
/// colour. The last band of [`banded`] is a single stop: it
/// runs upwards by the map's clamping and has **no width** on the map itself,
/// so a legend drawn from the first index to the last never shows it — while
/// every value above the last level is painted with it. A caller that knows
/// where its values end (`isocontours`, from the range it cut) closes it there.
pub fn banded_until(levels: &[f64], colours: &[Hsv], top: f64) -> Result<Vec<Stop>> {
    let mut stops = banded(levels, colours)?;
    let last = levels[levels.len() - 1];
    if top.is_finite() && top > last {
        stops.push(Stop { index: top, color: colours[colours.len() - 1].clone() });
    }
    Ok(stops)
}

/// How a value is placed on a map — everything `ColorInformation` says about
/// the mapping, without the map itself.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Scaling {
    /// `ColorInformation.MinMax` — the range of the **property**.
    pub min_max: Option<(f64, f64)>,
    /// `UseLogarithmicMapping`.
    pub logarithmic: bool,
    /// `UseReverseMapping`.
    pub reverse: bool,
}

// ---------------------------------------------------------------------------
// The graphical information set
// ---------------------------------------------------------------------------

/// Which elements of an object a piece of graphical information covers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(missing_docs)]
pub enum Scope {
    WholeObject,
    Nodes,
    Edges,
    Faces,
    Volumes,
}

impl Scope {
    /// The five targets, the whole object first.
    pub const ALL: [Scope; 5] = [Scope::WholeObject, Scope::Nodes, Scope::Edges, Scope::Faces, Scope::Volumes];

    /// Position in [`Scope::ALL`].
    pub fn index(self) -> usize {
        match self {
            Scope::WholeObject => 0,
            Scope::Nodes => 1,
            Scope::Edges => 2,
            Scope::Faces => 3,
            Scope::Volumes => 4,
        }
    }

    /// A short word for JSON and logs: `whole`, `nodes`, `edges`, `faces`, `volumes`.
    pub fn word(self) -> &'static str {
        match self {
            Scope::WholeObject => "whole",
            Scope::Nodes => "nodes",
            Scope::Edges => "edges",
            Scope::Faces => "faces",
            Scope::Volumes => "volumes",
        }
    }

    /// The concrete class an entry uses to say *this element of the object*.
    ///
    /// The inverse of [`Scope::from_type_name`], and beside it on purpose: the
    /// reader is the writer's specification, and two lists of five names in two
    /// files is how they come to disagree.
    pub fn type_name(self) -> &'static str {
        match self {
            Scope::WholeObject => "GraphicalInformationForWholeObject",
            Scope::Nodes => "GraphicalInformationForNodes",
            Scope::Edges => "GraphicalInformationForEdges",
            Scope::Faces => "GraphicalInformationForFaces",
            Scope::Volumes => "GraphicalInformationForVolumes",
        }
    }

    fn from_type_name(name: &str) -> Option<Scope> {
        let (_, tail) = name.split_once("GraphicalInformationFor")?;
        Some(match tail {
            "WholeObject" => Scope::WholeObject,
            "Nodes" => Scope::Nodes,
            "Edges" => Scope::Edges,
            "Faces" => Scope::Faces,
            "Volumes" => Scope::Volumes,
            _ => return None,
        })
    }
}

impl std::fmt::Display for Scope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Scope::WholeObject => "the whole object",
            Scope::Nodes => "nodes",
            Scope::Edges => "edges",
            Scope::Faces => "faces",
            Scope::Volumes => "volumes",
        })
    }
}

/// One `IndexableElementInfo` of a `DefaultGraphicalInformation`.
#[derive(Clone, PartialEq, Debug)]
pub struct ElementInfo {
    /// Its rank in `IndexableElementInfo` — **the file's**, not this list's.
    ///
    /// An information this build does not know is skipped with a note, so the
    /// n-th element read is not always the n-th of the file. An editor that
    /// writes back into the entry (the colour set's own view, 24/09) addresses
    /// `IndexableElementInfo[at]`, and a rank counted here would write into a
    /// neighbour.
    pub at: usize,
    /// Which elements this covers.
    pub scope: Scope,
    /// `IsVisible`, which the schema makes mandatory.
    pub visible: bool,
    /// A colour for every element at once.
    pub constant_color: Option<Hsv>,
    /// An alpha for every element at once.
    pub constant_alpha: Option<f64>,
    /// `true`: the constant alpha replaces the mapped one. `false` or absent:
    /// the two are multiplied.
    pub overwrite_color_alpha: bool,
    /// `ActiveColorInformationIndex` — a **positional** index into the set.
    pub active_color: Option<usize>,
}

/// What one `GraphicalInformation` says.
#[derive(Clone, PartialEq, Debug)]
pub enum Information {
    /// `DefaultGraphicalInformation`.
    Default {
        /// `3d`, `base map`, `section`, `well correlation`.
        viewer_kind: Option<String>,
        /// `ViewerId`.
        viewer_id: Option<String>,
        /// One per indexable element the producer decorated.
        elements: Vec<ElementInfo>,
    },
    /// `ColorInformation`.
    Color {
        /// How values are placed on the map.
        scaling: Scaling,
        /// Uuid of the `ColorMap`.
        map: Option<String>,
        /// Which component of a vector-valued property to colour by.
        ///
        /// An index into the **data**, never into the map. See the module
        /// documentation.
        value_vector_index: Option<u64>,
    },
    /// `AlphaInformation`, `SizeInformation`, `AnnotationInformation`,
    /// `ContourLineSetInformation` — kept rather than dropped, because
    /// `Active*InformationIndex` is a **position** in this list and dropping
    /// an entry would silently shift every index after it.
    Other {
        /// The concrete type name, so a caller can say what it skipped.
        type_name: String,
    },
}

/// One entry of the set, with its targets.
#[derive(Clone, PartialEq, Debug)]
pub struct Entry {
    /// Position in the set, which is what `Active*InformationIndex` means.
    pub position: usize,
    /// Uuids of the objects this decorates. One in eml 2.1/2.2, one or more in
    /// eml 2.3. **For a face that shows one**: the walks go by
    /// [`Entry::target_uris`].
    pub targets: Vec<String>,
    /// The **address** of each target, parallel to `targets`: the uri of the
    /// reference ([`identity::address_of_parts`] — its type and, when it names
    /// one, its version), or the uuid when the reference names no type. What an
    /// entry decorates is an object **at a version**: two versions of one
    /// representation are two objects, and an entry that names one does not
    /// paint the other. Empty for an entry built by hand from uuids — it then
    /// names every version of each ([`Entry::addresses`]).
    pub target_uris: Vec<String>,
    /// The uri of the colour map a `ColorInformation` names, **version
    /// included** — what the map is read by, so that a package holding the map
    /// twice answers the one the entry names. `None` for an entry that names no
    /// map, was read from raw XML, or was built by hand: the map's uuid in
    /// [`Information::Color`] then stands in.
    pub map_uri: Option<String>,
    /// What it says.
    pub information: Information,
}

impl Entry {
    /// What the entry names, by address: [`Entry::target_uris`] when they say
    /// one per target, else the uuids — which name every version.
    pub fn addresses(&self) -> &[String] {
        if self.target_uris.len() == self.targets.len() { &self.target_uris } else { &self.targets }
    }
}

/// A `GraphicalInformationSet`, read.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct GraphicalSet {
    /// Entries, in document order — the order is part of the meaning.
    pub entries: Vec<Entry>,
    /// What could not be read, in words.
    pub notes: Vec<String>,
}

impl GraphicalSet {
    /// Several sets of one package read as one, in package order — **the one
    /// reading of several sets**, which every caller goes through.
    ///
    /// The standard says nothing of how two sets combine
    /// (ADR-0009, checked against the three TRGs). The rule
    /// settled for the rest of the cascade — *the most
    /// specific wins; at equal specificity, the order of the document* (§7.3)
    /// — extends to it without a new decision: the entries are laid end to
    /// end, so the cascade still tries the most specific level first **across
    /// every set**, and at one level the **last** entry written answers, as it
    /// does within one set ([`GraphicalSet::targeting`]) — a later set in the
    /// package is a later statement. Positions are renumbered so they stay one
    /// wiring.
    ///
    /// Until 28/09 the front end read only the **first** set, the export too, and
    /// the command line all of them — three readings of one package. It showed
    /// on Lutetia the day a second set appeared: the thirteen units of the
    /// strata table got their own set, listed before the chart's, and the
    /// chart's other thirty-four units lost their colour in the front end.
    pub fn merged(sets: impl IntoIterator<Item = GraphicalSet>) -> GraphicalSet {
        let mut merged = GraphicalSet::default();
        for read in sets {
            let offset = merged.entries.len();
            for mut entry in read.entries {
                entry.position += offset;
                merged.entries.push(entry);
            }
            merged.notes.extend(read.notes);
        }
        merged
    }

    /// Read a `GraphicalInformationSet` object.
    ///
    /// An entry that cannot be read is kept as [`Information::Other`] with a
    /// note, never dropped: positions are the wiring.
    ///
    /// # Why an entry arrives as raw XML
    ///
    /// `AbstractGraphicalInformation` lives in eml, and **every one of its
    /// concrete subtypes lives in RESQML 2.2**. RESQML depends on eml, so the
    /// generated eml crate cannot know them: the generator keeps what it
    /// cannot type as [`AnyXml`], and every entry of every real set arrives
    /// that way. Reading it means reading the subtree — which is what
    /// `read_raw_entry` does, on the same narrow scanner the property kind
    /// dictionary uses.
    ///
    /// Both paths are kept. The day a version generates those subtypes into a
    /// crate this one can see, the typed path takes over with nothing else to
    /// change.
    pub fn read(object: &dyn Reflect) -> GraphicalSet {
        let mut set = GraphicalSet::default();
        for (position, item) in list(object, "GraphicalInformation").into_iter().enumerate() {
            // The usual case today: the entry is a subtree nothing could type.
            if let Some(raw) = item.as_object().and_then(|o| o.downcast_ref::<AnyXml>()) {
                let entry = read_raw_entry(raw, position, &mut set.notes);
                set.entries.push(entry);
                continue;
            }
            let Some(entry) = item.as_object() else {
                set.notes
                    .push(format!("entry {position} is not an object and was kept as opaque"));
                set.entries.push(Entry {
                    position,
                    targets: Vec::new(),
                    target_uris: Vec::new(),
                    map_uri: None,
                    information: Information::Other {
                        type_name: "<unreadable>".to_string(),
                    },
                });
                continue;
            };
            let type_name = entry.type_info().name.to_string();
            let (targets, target_uris): (Vec<String>, Vec<String>) = list(entry, "TargetObject")
                .into_iter()
                .filter_map(|target| {
                    let target = target.as_object()?;
                    Some((uuid_of_reference(target)?, identity::address_of_reflect(target)?))
                })
                .unzip();
            if targets.is_empty() {
                set.notes.push(format!(
                    "entry {position} (`{type_name}`) names no target object, so it decorates nothing"
                ));
            }

            let map_uri = entry
                .field("ColorMap")
                .and_then(|f| f.as_object())
                .and_then(identity::uri_of_reflect);
            let information = if type_name.contains("DefaultGraphicalInformation") {
                let mut elements = Vec::new();
                for (at, info) in list(entry, "IndexableElementInfo").into_iter().enumerate() {
                    let Some(info) = info.as_object() else { continue };
                    let inner = info.type_info().name;
                    let Some(scope) = Scope::from_type_name(inner) else {
                        set.notes.push(format!(
                            "entry {position}: `{inner}` is not an indexable-element information \
                             this build knows, so its colour was not read"
                        ));
                        continue;
                    };
                    let constant_color = match info.field("ConstantColor").and_then(|f| f.as_object()) {
                        Some(color) => match Hsv::read(color, &format!("entry {position}")) {
                            Ok(color) => Some(color),
                            Err(e) => {
                                set.notes.push(format!("entry {position}: {e}"));
                                None
                            }
                        },
                        None => None,
                    };
                    elements.push(ElementInfo {
                        at,
                        scope,
                        visible: !matches!(
                            info.field("IsVisible").and_then(|f| f.as_scalar()),
                            Some(Scalar::Bool(false))
                        ),
                        constant_color,
                        constant_alpha: scalar_f64(info, "ConstantAlpha"),
                        overwrite_color_alpha: matches!(
                            info.field("OverwriteColorAlpha").and_then(|f| f.as_scalar()),
                            Some(Scalar::Bool(true))
                        ),
                        active_color: scalar_u64(info, "ActiveColorInformationIndex").map(|v| v as usize),
                    });
                }
                Information::Default {
                    viewer_kind: text(entry, "ViewerKind"),
                    viewer_id: text(entry, "ViewerId"),
                    elements,
                }
            } else if type_name.contains("ColorInformation") {
                let min_max = entry.field("MinMax").and_then(|f| f.as_object()).and_then(|range| {
                    Some((scalar_f64(range, "Minimum")?, scalar_f64(range, "Maximum")?))
                });
                Information::Color {
                    scaling: Scaling {
                        min_max,
                        logarithmic: matches!(
                            entry.field("UseLogarithmicMapping").and_then(|f| f.as_scalar()),
                            Some(Scalar::Bool(true))
                        ),
                        reverse: matches!(
                            entry.field("UseReverseMapping").and_then(|f| f.as_scalar()),
                            Some(Scalar::Bool(true))
                        ),
                    },
                    map: entry
                        .field("ColorMap")
                        .and_then(|f| f.as_object())
                        .and_then(uuid_of_reference),
                    value_vector_index: scalar_u64(entry, "ValueVectorIndex"),
                }
            } else {
                Information::Other { type_name }
            };

            set.entries.push(Entry {
                position,
                targets,
                target_uris,
                map_uri,
                information,
            });
        }
        set
    }

    /// Entries decorating one object, latest first.
    ///
    /// Latest first because at equal specificity the last entry written wins:
    /// the list is ordered, and a producer appending an entry is saying
    /// something newer than what is already there.
    ///
    /// `address` is the uri of the object (its identity): an entry that names
    /// another version of it does not decorate it. A bare uuid, the library's
    /// entry adapter, names no version and takes the entries of every one.
    pub fn targeting(&self, address: &str) -> Vec<&Entry> {
        let (wanted, version) = identity::parse_address(address);
        let mut found: Vec<&Entry> = self
            .entries
            .iter()
            .filter(|entry| {
                entry.addresses().iter().any(|named| {
                    let (uuid, named_version) = identity::parse_address(named);
                    uuid == wanted && identity::versions_agree(version.as_deref(), named_version.as_deref())
                })
            })
            .collect();
        found.reverse();
        found
    }

    /// The entry at a positional index, as `Active*InformationIndex` means it.
    pub fn at(&self, position: usize) -> Option<&Entry> {
        self.entries.get(position)
    }
}

// ---------------------------------------------------------------------------
// The cascade
// ---------------------------------------------------------------------------

/// Where a colour was found.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    /// A `ColorInformation` on the property itself.
    Property,
    /// One on the property's kind, or on an ancestor of that kind.
    PropertyKind,
    /// The representation's own default information.
    Representation,
    /// Its interpretation's.
    Interpretation,
    /// Its feature's.
    Feature,
    /// A collection that holds the object, or one of its parents — only on an
    /// answer of [`crate::color_inherit`], never of [`Cascade::resolve`].
    Collection,
    /// The model that references the object or one of its parents — same.
    Model,
}

impl std::fmt::Display for Level {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Level::Property => "the property",
            Level::PropertyKind => "the property kind",
            Level::Representation => "the representation",
            Level::Interpretation => "the interpretation",
            Level::Feature => "the feature",
            Level::Collection => "a collection",
            Level::Model => "the model",
        })
    }
}

/// What to paint with.
///
/// `Mapped` is boxed: a map carries every one of its stops, and a decision is
/// usually a constant colour, so the small case should not pay for the large
/// one.
#[derive(Clone, PartialEq, Debug)]
pub enum Paint {
    /// One colour for every element.
    Constant(Rgba),
    /// A map, to be sampled per value.
    Mapped {
        /// The map itself, read.
        map: Box<ColorMap>,
        /// How to place a value on it.
        scaling: Scaling,
        /// Which component of a vector-valued property to colour by.
        value_vector_index: Option<u64>,
    },
}

/// A colour decision, with where it came from.
#[derive(Clone, PartialEq, Debug)]
pub struct Decision {
    /// What to paint with.
    pub paint: Paint,
    /// Which level answered.
    pub level: Level,
    /// Uuid of the object that carried the information — for a face that shows
    /// one; the walk goes by [`Decision::found_on_uri`].
    pub found_on: String,
    /// Uri (the full identity) of the object that carried it, or the address
    /// the caller gave for an object the package does not hold.
    pub found_on_uri: String,
    /// Position of the winning entry in the set.
    pub entry: usize,
    /// Whether that object says it is invisible.
    pub visible: bool,
    /// Everything that was skipped, and why.
    pub notes: Vec<String>,
}

/// What one object carries as `DefaultGraphicalInformation`, **per target**.
///
/// A rung of the inheritance walk is read once, whatever targets are asked of
/// it: the colour for the nodes, the edges, the faces, the volumes and the
/// whole object are all here, and a target the object does not give is `None`.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Carried {
    slots: [Option<Decision>; 5],
    /// Entries of this object that were for another viewer, in words.
    pub skipped: Vec<String>,
}

impl Carried {
    /// What the object says for exactly this target — **not** a fallback.
    pub fn exact(&self, scope: Scope) -> Option<&Decision> {
        self.slots[scope.index()].as_ref()
    }

    /// Whether it carries anything at all.
    pub fn any(&self) -> bool {
        self.slots.iter().any(Option::is_some)
    }
}

/// One link of the cascade, and whether it answers.
///
/// What [`Cascade::chain`] is for: showing a person the whole ladder instead of
/// only the rung that won, so a fallback can be **chosen** rather than inferred.
#[derive(Clone, PartialEq, Debug)]
pub struct Step {
    /// Which level this link is.
    pub level: Level,
    /// Uuid of the object at this link — for a face that shows one; the walk
    /// goes by [`Step::uri`].
    ///
    /// No title: this module reads colours, and resolving a title means opening
    /// the object. The caller already has the package's index and can do it for
    /// nothing.
    pub uuid: String,
    /// Uri (the full identity) of the object at this link, or the address the
    /// caller gave for one the package does not hold. Two versions of one
    /// representation are two rungs.
    pub uri: String,
    /// What it carries for this request, or `None` when it carries nothing.
    pub carries: Option<Decision>,
}

/// What is being drawn.
#[derive(Clone, Debug)]
pub struct Request<'a> {
    /// The **address** of the representation: its **uri**, the full identity —
    /// the version it declares included, so that two versions of one
    /// representation are two requests with two colours. A bare uuid is the
    /// library's entry adapter: it names no version, and decorates the entries
    /// of every one.
    ///
    /// Both spellings are read since 05/10/2026 (entry targets are addresses
    /// too, [`Entry::target_uris`]). The confusion between the two cost a defect
    /// found on 04/09: the wasm layer passed a uri here while entry targets were
    /// uuids, and they never matched — a colour set on the representation itself
    /// was never found, while one inherited from its interpretation or its
    /// feature was, because the walk up ([`parent_of`]) yielded uuids. A package
    /// that duplicates its colours at all three levels, as `80wells` does, hid
    /// that completely.
    pub representation: &'a str,
    /// The address (uri) of the property being coloured by, when there is one.
    pub property: Option<&'a str>,
    /// The addresses of the property's kind and its ancestors, nearest first
    /// (uuids today: the kind chain is resolved by uuid).
    ///
    /// Comes straight from [`crate::property_kind::Resolution::chain`]; the
    /// cascade walks it in order, so a colour on `length` applies to every
    /// kind derived from it.
    pub kind_chain: &'a [String],
    /// Which elements are being drawn.
    pub element: Scope,
}

/// The cascade, and the two switches on it.
#[derive(Clone, Debug)]
pub struct Cascade<'a> {
    /// The set to read from.
    pub set: &'a GraphicalSet,
    /// Only entries for this viewer kind, plus those that name none.
    ///
    /// `3d` by default. An entry meant for a section view must not paint a 3D
    /// scene, and `energyml-utils` ignores the field entirely.
    pub viewer_kind: Option<String>,
    /// Whether to walk up to the interpretation and the feature.
    ///
    /// The default is `true`. Switch it off to get only what the
    /// object itself carries.
    pub inherit: bool,
    /// The set's entries by target, built the first time an object is asked
    /// about — see [`Cascade::targeting`].
    targets: RefCell<Option<TargetMap>>,
}

/// Which entries of a set target each object: lowercase uuid → positions, in
/// document order.
///
/// Built **once per cascade**. [`GraphicalSet::targeting`] scans every entry and
/// lowercases every target of each, for **each object asked about** — and a walk
/// asks about every object it passes: a package whose set holds thousands of
/// entries paid thousands of string allocations per rung, and a walk has a
/// dozen. The map is keyed by the set it was built from (its address and its
/// length), so a cascade whose `set` is replaced rebuilds it.
#[derive(Clone, Debug)]
struct TargetMap {
    set: *const GraphicalSet,
    entries: usize,
    /// Lowercase uuid → the positions of the entries naming it, each with the
    /// version it names — the version is **part of what is targeted**, so it is
    /// compared when an object (a uri) asks, not dropped by the key.
    by_target: HashMap<String, Vec<(usize, Option<String>)>>,
}

impl<'a> Cascade<'a> {
    /// A cascade over a set, for a 3D viewer, with inheritance on.
    pub fn new(set: &'a GraphicalSet) -> Self {
        Self {
            set,
            viewer_kind: Some("3d".to_string()),
            inherit: true,
            targets: RefCell::new(None),
        }
    }

    /// The entries of the set that target this object, **latest first** — what
    /// [`GraphicalSet::targeting`] answers, from a map instead of a scan.
    fn targeting(&self, address: &str) -> Vec<&'a Entry> {
        let set: &'a GraphicalSet = self.set;
        let mut held = self.targets.borrow_mut();
        let fresh = matches!(&*held, Some(map) if std::ptr::eq(map.set, set) && map.entries == set.entries.len());
        if !fresh {
            let mut by_target: HashMap<String, Vec<(usize, Option<String>)>> = HashMap::new();
            for (at, entry) in set.entries.iter().enumerate() {
                let mut named: Vec<(String, Option<String>)> =
                    entry.addresses().iter().map(|address| identity::parse_address(address)).collect();
                named.sort_unstable();
                named.dedup();
                for (uuid, version) in named {
                    by_target.entry(uuid).or_default().push((at, version));
                }
            }
            *held = Some(TargetMap { set, entries: set.entries.len(), by_target });
        }
        let map = held.as_ref().expect("just built");
        let (uuid, version) = identity::parse_address(address);
        match map.by_target.get(&uuid) {
            Some(rows) => {
                let mut positions: Vec<usize> = rows
                    .iter()
                    .filter(|(_, named)| identity::versions_agree(version.as_deref(), named.as_deref()))
                    .map(|&(at, _)| at)
                    .collect();
                // One entry may name two versions of the object: once.
                positions.sort_unstable();
                positions.dedup();
                positions.into_iter().rev().map(|at| &set.entries[at]).collect()
            }
            None => Vec::new(),
        }
    }

    /// Every link of the cascade for one request, in the order it is consulted.
    ///
    /// [`resolve`] answers *which* colour applies; this answers *what the
    /// choice was*, which is a different question and the one a person asks
    /// when the colour is not the one they wanted. The need: an
    /// interface to choose the colour of an object, either its own colour or a
    /// fallback colour with the list of parent objects that have an applicable
    /// colour.
    ///
    /// **Links that carry nothing are in the list.** They are the answer to
    /// "why not that one" — a parent that exists and says nothing is not the
    /// same as a parent that does not exist, and dropping it would leave a
    /// person looking for a rung that was never shown.
    ///
    /// The order is `resolve`'s own, so **the winner is the first link that
    /// carries something**. That is a property, not a coincidence, and a test
    /// holds the two functions to it — they walk separately (`resolve` stops
    /// early, and stopping early is why it does not open every parent of every
    /// object in a scene), so nothing but that test keeps them from drifting.
    ///
    /// [`resolve`]: Cascade::resolve
    pub fn chain(&self, request: &Request<'_>, objects: &dyn ObjectSource) -> Result<Vec<Step>> {
        let mut out = Vec::new();
        let mut notes = Vec::new();

        if let Some(property) = request.property {
            let carries = self.color_information_on(property, Level::Property, objects, &mut notes)?;
            out.push(Step {
                level: Level::Property,
                uuid: uuid_of_identity(property),
                uri: property.to_string(),
                carries,
            });
        }
        for kind in request.kind_chain {
            let carries = self.color_information_on(kind, Level::PropertyKind, objects, &mut notes)?;
            out.push(Step { level: Level::PropertyKind, uuid: uuid_of_identity(kind), uri: kind.clone(), carries });
        }

        let mut level = Level::Representation;
        // The identity of the rung: the caller's address for the first one (a
        // uri, or a bare uuid at the entry), and for every rung above it the
        // address its reference names — read by the version that reference
        // names.
        let mut current = request.representation.to_string();
        let mut guard = 0;
        loop {
            let carries = self.default_information_on(&current, level, request.element, &mut notes);
            out.push(Step { level, uuid: uuid_of_identity(&current), uri: current.clone(), carries });
            if !self.inherit {
                break;
            }
            let Some((next, reached)) = parent_of(&current, objects) else {
                break;
            };
            level = reached.unwrap_or(match level {
                Level::Representation => Level::Interpretation,
                _ => Level::Feature,
            });
            current = next;
            guard += 1;
            if guard > 8 {
                break;
            }
        }
        Ok(out)
    }

    /// Resolve one request.
    ///
    /// `None` means no level answered — and that is the answer, not a failure:
    /// the standard defines no default colour, so inventing one would be a
    /// picture that says something the file does not.
    pub fn resolve(&self, request: &Request<'_>, objects: &dyn ObjectSource) -> Result<Option<Decision>> {
        let mut notes = Vec::new();

        // 1 — the property itself.
        if let Some(property) = request.property
            && let Some(decision) = self.color_information_on(property, Level::Property, objects, &mut notes)?
        {
            return Ok(Some(decision));
        }

        // 2 — its kind, then the kind's ancestors. The chain is already
        // ordered nearest-first, so "most specific wins" is just its order.
        for (depth, kind) in request.kind_chain.iter().enumerate() {
            if let Some(mut decision) =
                self.color_information_on(kind, Level::PropertyKind, objects, &mut notes)?
            {
                if depth > 0 {
                    decision.notes.push(format!(
                        "no colour on the property kind itself; this one comes from its ancestor \
                         {depth} step(s) up ({kind})"
                    ));
                }
                return Ok(Some(decision));
            }
        }

        // 3, 4, 5 — the object, then up the chain of what it represents.
        let mut level = Level::Representation;
        let mut current = request.representation.to_string();
        let mut guard = 0;
        loop {
            if let Some(decision) = self.default_information_on(&current, level, request.element, &mut notes) {
                return Ok(Some(decision));
            }
            if !self.inherit {
                notes.push(
                    "inheritance is switched off, so the interpretation and the feature were not \
                     looked at"
                        .to_string(),
                );
                break;
            }
            let Some((next, reached)) = parent_of(&current, objects) else {
                break;
            };
            // The type the reference declares, when it declares one. Falling
            // back on the step number is what this used to do always, and it
            // is wrong exactly where 2.2 allows a representation to name a
            // technical feature directly.
            level = reached.unwrap_or(match level {
                Level::Representation => Level::Interpretation,
                _ => Level::Feature,
            });
            current = next;
            guard += 1;
            if guard > 8 {
                notes.push("the representation → interpretation → feature chain does not end".to_string());
                break;
            }
        }

        Ok(None)
    }

    /// A `ColorInformation` targeting one object.
    pub(crate) fn color_information_on(
        &self,
        uuid: &str,
        level: Level,
        objects: &dyn ObjectSource,
        notes: &mut Vec<String>,
    ) -> Result<Option<Decision>> {
        for entry in self.targeting(uuid) {
            let Information::Color {
                scaling,
                map,
                value_vector_index,
            } = &entry.information
            else {
                continue;
            };
            let Some(map_uuid) = map else {
                notes.push(format!(
                    "entry {} decorates {uuid} but names no colour map, so it says nothing about \
                     colour; looking further",
                    entry.position
                ));
                continue;
            };
            // By the uri the entry names, so the version it names is the one
            // read; the uuid only when the entry carries no uri.
            let found = match &entry.map_uri {
                Some(uri) => objects.object_at(uri),
                None => objects.object(map_uuid),
            };
            let Some(object) = found else {
                notes.push(format!(
                    "entry {} points at colour map {map_uuid}, which is not in this package; \
                     looking further",
                    entry.position
                ));
                continue;
            };
            let map = ColorMap::read(object.as_reflect())?;
            return Ok(Some(Decision {
                paint: Paint::Mapped {
                    map: Box::new(map),
                    scaling: *scaling,
                    value_vector_index: *value_vector_index,
                },
                level,
                found_on: uuid_of_identity(uuid),
                found_on_uri: uuid.to_string(),
                entry: entry.position,
                visible: true,
                notes: std::mem::take(notes),
            }));
        }
        Ok(None)
    }

    /// **Everything** the `DefaultGraphicalInformation` of one object says,
    /// each of the five targets once: what the walk reads at a rung, so that
    /// the rung is read a single time whatever the number of targets asked.
    ///
    /// Per target, the entries are tried latest first (as
    /// [`GraphicalSet::targeting`] orders them) and the first that gives the
    /// target a constant colour answers — the same rule
    /// [`Cascade::default_information_on`] applies to the one target it is
    /// asked for. Entries meant for another viewer are skipped, and said, in
    /// [`Carried::skipped`] and in the notes of the decisions found after them.
    pub(crate) fn default_information_all(&self, uuid: &str, level: Level) -> Carried {
        let mut out = Carried::default();
        let mut skips: Vec<String> = Vec::new();
        for entry in self.targeting(uuid) {
            let Information::Default {
                viewer_kind,
                elements,
                ..
            } = &entry.information
            else {
                continue;
            };
            if let (Some(wanted), Some(theirs)) = (&self.viewer_kind, viewer_kind)
                && !theirs.eq_ignore_ascii_case(wanted)
            {
                skips.push(format!(
                    "entry {} on {uuid} is for the `{theirs}` viewer, not `{wanted}`; skipped",
                    entry.position
                ));
                continue;
            }
            for scope in Scope::ALL {
                if out.slots[scope.index()].is_some() {
                    continue;
                }
                let Some(info) = elements.iter().find(|info| info.scope == scope) else {
                    continue;
                };
                let Some(color) = &info.constant_color else {
                    continue;
                };
                let mut rgba = color.to_rgba();
                if let Some(alpha) = info.constant_alpha {
                    rgba.a = if info.overwrite_color_alpha {
                        alpha
                    } else {
                        rgba.a * alpha
                    };
                }
                out.slots[scope.index()] = Some(Decision {
                    paint: Paint::Constant(rgba),
                    level,
                    found_on: uuid_of_identity(uuid),
                found_on_uri: uuid.to_string(),
                    entry: entry.position,
                    visible: info.visible,
                    notes: skips.clone(),
                });
            }
        }
        out.skipped = skips;
        out
    }

    /// The entry and the element that give one object a constant colour for
    /// `element` — **the one choice**: entries latest first, another viewer's
    /// skipped (and said in `notes`), the element asked for first and the whole
    /// object second. [`Cascade::default_information_on`] decides by it, and
    /// [`Cascade::constant_written`] answers with it, so *the colour shown* and
    /// *the value edited* cannot be two different entries.
    fn constant_on(&self, uuid: &str, element: Scope, notes: &mut Vec<String>) -> Option<(&'a Entry, &'a ElementInfo)> {
        for entry in self.targeting(uuid) {
            let Information::Default {
                viewer_kind,
                elements,
                ..
            } = &entry.information
            else {
                continue;
            };
            // An entry meant for another viewer must not paint this one.
            if let (Some(wanted), Some(theirs)) = (&self.viewer_kind, viewer_kind)
                && !theirs.eq_ignore_ascii_case(wanted)
            {
                notes.push(format!(
                    "entry {} on {uuid} is for the `{theirs}` viewer, not `{wanted}`; skipped",
                    entry.position
                ));
                continue;
            }
            for scope in [element, Scope::WholeObject] {
                let Some(info) = elements.iter().find(|info| info.scope == scope) else {
                    continue;
                };
                if info.constant_color.is_some() {
                    return Some((entry, info));
                }
            }
        }
        None
    }

    /// A `DefaultGraphicalInformation` on one object: the element asked for
    /// first, the whole object second.
    pub(crate) fn default_information_on(
        &self,
        uuid: &str,
        level: Level,
        element: Scope,
        notes: &mut Vec<String>,
    ) -> Option<Decision> {
        let (entry, info) = self.constant_on(uuid, element, notes)?;
        let color = info.constant_color.as_ref()?;
        let mut rgba = color.to_rgba();
        if let Some(alpha) = info.constant_alpha {
            // The schema's rule, stated in the annotation of
            // `OverwriteColorAlpha`: replace, or multiply.
            rgba.a = if info.overwrite_color_alpha {
                alpha
            } else {
                rgba.a * alpha
            };
        }
        if info.scope != element {
            notes.push(format!(
                "nothing for {element} on {uuid}; used what it says for the whole object"
            ));
        }
        Some(Decision {
            paint: Paint::Constant(rgba),
            level,
            found_on: uuid_of_identity(uuid),
                found_on_uri: uuid.to_string(),
            entry: entry.position,
            visible: info.visible,
            notes: std::mem::take(notes),
        })
    }

    /// **Every** statement of the constant colour one object carries at
    /// `scope`: each entry naming it — in this set, latest first, another
    /// viewer's skipped as the decision skips them — whose element for that
    /// scope states a `ConstantColor`. The first is the one the cascade takes
    /// ([`Cascade::constant_written`]); the others say the same thing about the
    /// same object somewhere else — another set of the package, typically: the
    /// GeolTs chart writes its units' colours in a set of its own, and a
    /// package's own set may colour them too.
    ///
    /// Changing the colour **where it is written** changes all of them:
    /// changing only the one in force would leave the file stating two colours
    /// for one object, and a reader that asks every set (the strata of
    /// wellbores once did) would then find no colour at all — a black legend.
    pub fn constant_statements(&self, uuid: &str, scope: Scope) -> Vec<Written> {
        let mut out = Vec::new();
        for entry in self.targeting(uuid) {
            let Information::Default {
                viewer_kind,
                elements,
                ..
            } = &entry.information
            else {
                continue;
            };
            if let (Some(wanted), Some(theirs)) = (&self.viewer_kind, viewer_kind)
                && !theirs.eq_ignore_ascii_case(wanted)
            {
                continue;
            }
            if let Some(info) = elements.iter().find(|info| info.scope == scope)
                && info.constant_color.is_some()
            {
                out.push(Written {
                    position: entry.position,
                    at: info.at,
                    scope: info.scope,
                    targets: entry.targets.clone(),
                });
            }
        }
        out
    }

    /// **Where** the constant colour one object carries for `element` is
    /// written: the entry (its position in this set), the element (its rank in
    /// `IndexableElementInfo`, the file's) and every object the entry names —
    /// the objects a change of that value changes too. `None` when the object
    /// carries no constant colour of its own (it may still inherit one: ask its
    /// carrier). Decided by the same choice as the colour shown
    /// ([`Cascade::constant_on`]).
    pub fn constant_written(&self, uuid: &str, element: Scope) -> Option<Written> {
        let (entry, info) = self.constant_on(uuid, element, &mut Vec::new())?;
        Some(Written {
            position: entry.position,
            at: info.at,
            scope: info.scope,
            targets: entry.targets.clone(),
        })
    }
}

// ---------------------------------------------------------------------------
// The raw-XML path
// ---------------------------------------------------------------------------

/// Read one `GraphicalInformation` that arrived as an untyped subtree.
///
/// The shape is fixed and shallow — a target reference, a viewer kind, a few
/// numbers and one `HsvColor` per indexable element — so it is read with the
/// crate's narrow scanner rather than by pulling in an XML parser. What it
/// cannot make sense of becomes [`Information::Other`] **with a note**, and
/// keeps its position, because `Active*InformationIndex` counts these.
fn read_raw_entry(raw: &AnyXml, position: usize, notes: &mut Vec<String>) -> Entry {
    let content = raw.content.as_str();
    // `xsi:type` is what says which subtype this is; it sits on the carrying
    // element, so it comes from the attributes rather than the content.
    let declared = raw
        .attributes
        .iter()
        .find(|(name, _)| name == "xsi:type" || name == "type" || name.ends_with(":type"))
        .map(|(_, value)| value.as_str())
        .unwrap_or("");
    // `resqml:ColorInformation` → `ColorInformation`.
    let type_name = declared.rsplit(':').next().unwrap_or(declared).to_string();

    // Each target as its uuid and as its address: the type it declares and the
    // version it names, when it names them (eml 2.3 `ObjectVersion`, eml 2.0
    // `VersionString`).
    let (targets, target_uris): (Vec<String>, Vec<String>) = scan::sections_all(content, "TargetObject")
        .into_iter()
        .filter_map(|target| {
            let uuid = scan::element_text(target, "Uuid")?;
            let declared =
                scan::element_text(target, "QualifiedType").or_else(|| scan::element_text(target, "ContentType"));
            let version =
                scan::element_text(target, "ObjectVersion").or_else(|| scan::element_text(target, "VersionString"));
            let address = identity::address_of_parts(declared.as_deref(), &uuid, version.as_deref());
            Some((uuid, address))
        })
        .unzip();
    if targets.is_empty() {
        notes.push(format!(
            "entry {position} (`{type_name}`) names no target object, so it decorates nothing"
        ));
    }

    let information = if type_name.contains("DefaultGraphicalInformation") {
        let mut elements = Vec::new();
        for (at, info) in scan::sections_all(content, "IndexableElementInfo").into_iter().enumerate() {
            let inner = scan::attribute_local(info, "type").unwrap_or_default();
            let inner_name = inner.rsplit(':').next().unwrap_or(&inner).to_string();
            let Some(scope) = Scope::from_type_name(&inner_name) else {
                notes.push(format!(
                    "entry {position}: `{inner_name}` is not an indexable-element information this \
                     build knows, so its colour was not read"
                ));
                continue;
            };
            let constant_color = match scan::section(info, "ConstantColor") {
                Some(color) => match read_raw_color(color, &format!("entry {position}")) {
                    Ok(color) => Some(color),
                    Err(e) => {
                        notes.push(format!("entry {position}: {e}"));
                        None
                    }
                },
                None => None,
            };
            elements.push(ElementInfo {
                at,
                scope,
                visible: scan::element_text(info, "IsVisible")
                    .map(|text| !text.trim().eq_ignore_ascii_case("false"))
                    .unwrap_or(true),
                constant_color,
                constant_alpha: raw_f64(info, "ConstantAlpha"),
                overwrite_color_alpha: scan::element_text(info, "OverwriteColorAlpha")
                    .is_some_and(|text| text.trim().eq_ignore_ascii_case("true")),
                active_color: raw_f64(info, "ActiveColorInformationIndex").map(|v| v as usize),
            });
        }
        Information::Default {
            viewer_kind: scan::element_text(content, "ViewerKind"),
            viewer_id: scan::element_text(content, "ViewerId"),
            elements,
        }
    } else if type_name.contains("ColorInformation") {
        let min_max = scan::section(content, "MinMax").and_then(|range| {
            Some((raw_f64(range, "Minimum")?, raw_f64(range, "Maximum")?))
        });
        Information::Color {
            scaling: Scaling {
                min_max,
                logarithmic: scan::element_text(content, "UseLogarithmicMapping")
                    .is_some_and(|text| text.trim().eq_ignore_ascii_case("true")),
                reverse: scan::element_text(content, "UseReverseMapping")
                    .is_some_and(|text| text.trim().eq_ignore_ascii_case("true")),
            },
            map: scan::section(content, "ColorMap").and_then(|map| scan::element_text(map, "Uuid")),
            value_vector_index: raw_f64(content, "ValueVectorIndex").map(|v| v as u64),
        }
    } else {
        if type_name.is_empty() {
            notes.push(format!(
                "entry {position} carries no `xsi:type`, so nothing says which kind of graphical \
                 information it is; it is kept as opaque and still counts for the positional \
                 indices"
            ));
        }
        Information::Other {
            type_name: if type_name.is_empty() {
                "<untyped>".to_string()
            } else {
                type_name
            },
        }
    };

    Entry {
        position,
        targets,
        target_uris,
        map_uri: None,
        information,
    }
}

/// An `HsvColor` written as raw XML, with the same bounds check as the typed
/// path.
fn read_raw_color(section: &str, at: &str) -> Result<Hsv> {
    let number = |field: &str| -> Result<f64> {
        raw_f64(section, field).ok_or_else(|| {
            Error::malformed(
                at,
                format!("`{field}` is absent from an `HsvColor`, where the schema makes it mandatory"),
            )
        })
    };
    Hsv::checked(
        number("Hue")?,
        number("Saturation")?,
        number("Value")?,
        number("Alpha")?,
        scan::element_text(section, "Title").filter(|t| !t.is_empty()),
        at,
    )
}

fn raw_f64(section: &str, field: &str) -> Option<f64> {
    scan::element_text(section, field)?.trim().parse().ok()
}

/// What an object represents, one step up: a representation names its
/// interpretation, an interpretation names its feature.
///
/// **Only upward, and only by these three fields.** The rule: colour
/// propagation follows the FIRP hierarchy only, never an interpretation that
/// references a representation. An object that points *down* — a set naming the
/// interpretation of each of its parts, say — is not a step of this chain, and
/// following it would give one representation as many parents as it has pieces.
///
/// The level comes back with the uuid because **the step number does not say
/// what was reached**. RESQML 2.2's own business rule on `RepresentedObject`:
/// *"The data object represented by the representation is either an
/// interpretation or a technical feature."* One step up from a representation
/// is therefore sometimes a feature, and calling it "the interpretation"
/// misnames the source of a colour a user is looking at.
///
/// Read by **address**: the uri the rung was reached by (the version its
/// reference names), or the caller's own address for the first rung — a uri, or
/// a bare uuid at the entry. Returns the next rung's address: the uri its
/// reference names (the uuid when the reference names no type).
fn parent_of(address: &str, objects: &dyn ObjectSource) -> Option<(String, Option<Level>)> {
    let object = objects.object_at(address)?;
    let reflect = object.as_reflect();
    // `RepresentedObject` in RESQML 2.2, `RepresentedInterpretation` in 2.0.1,
    // `InterpretedFeature` on an interpretation. One of the three answers.
    for field in ["RepresentedObject", "RepresentedInterpretation", "InterpretedFeature"] {
        if let Some(target) = reflect.field(field).and_then(|f| f.as_object())
            && let Some(next) = identity::address_of_reflect(target)
        {
            // `InterpretedFeature` names what it reaches; the other two carry
            // it in the reference's own type, which costs no second lookup.
            let level = match field {
                "InterpretedFeature" => Some(Level::Feature),
                _ => level_of_reference(target),
            };
            return Some((next, level));
        }
    }
    None
}

/// Which level a reference points at, read from the type it declares.
///
/// `None` when the reference carries no usable type — an answer, not a guess:
/// the caller then says the step is unnamed rather than naming it wrong.
fn level_of_reference(reference: &dyn Reflect) -> Option<Level> {
    let declared = text(reference, "QualifiedType").or_else(|| text(reference, "ContentType"))?;
    let parsed = identity::ObjectId::from_qualified_type(&declared, "")
        .or_else(|_| identity::ObjectId::from_content_type(&declared, ""))
        .ok()?;
    let key = parsed.type_key();
    if key.ends_with("Interpretation") {
        Some(Level::Interpretation)
    } else if key.ends_with("Feature") {
        Some(Level::Feature)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Reflection helpers
// ---------------------------------------------------------------------------

fn list<'a>(object: &'a dyn Reflect, field: &str) -> Vec<FieldValue<'a>> {
    match object.field(field) {
        Some(FieldValue::List(items)) => items,
        Some(FieldValue::Null) | None => Vec::new(),
        Some(other) => vec![other],
    }
}

fn text(object: &dyn Reflect, field: &str) -> Option<String> {
    match object.field(field)?.as_scalar()? {
        Scalar::Str(s) if !s.is_empty() => Some(s.to_string()),
        Scalar::Str(_) => None,
        other => Some(other.to_string()),
    }
}

fn scalar_f64(object: &dyn Reflect, field: &str) -> Option<f64> {
    object.field(field)?.as_scalar()?.as_f64()
}

fn scalar_u64(object: &dyn Reflect, field: &str) -> Option<u64> {
    match object.field(field)?.as_scalar()? {
        Scalar::UInt(v) => Some(v),
        Scalar::Int(v) if v >= 0 => Some(v as u64),
        _ => None,
    }
}

/// The uuid a reference names — the identity layer's reading, so every
/// spelling of the field is one answer.
fn uuid_of_reference(object: &dyn Reflect) -> Option<String> {
    energyml_core::identity::uuid_of_reflect(object).filter(|uuid| !uuid.is_empty()).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two forms, and nothing else: a sign, a missing `#`, three digits
    /// or a stray character are refused by name — never read as some colour.
    #[test]
    fn a_colour_reads_back_from_its_hex_and_nothing_else_reads() {
        let red = Rgba::parse("#ff0000").expect("six digits");
        assert_eq!(red.to_bytes(), [255, 0, 0, 255]);
        let clear = Rgba::parse("#00ff0080").expect("eight digits");
        assert_eq!(clear.to_bytes(), [0, 255, 0, 128]);
        assert_eq!(Rgba::parse(&clear.to_hex()).map(Rgba::to_bytes), Ok(clear.to_bytes()));
        for wrong in ["ff0000", "#+f+f+f", "#fff", "#ff00zz", "#ff00000", ""] {
            let why = Rgba::parse(wrong).expect_err(wrong);
            assert!(why.contains(&format!("`{wrong}`")), "{why}");
        }
    }

    /// The rank an element carries is **the file's**, even past one this build
    /// skips — what an editor writing the colour back into
    /// `IndexableElementInfo[at]` needs (24/09). Counted in the list read, it
    /// would point at the skipped neighbour.
    #[test]
    fn an_element_keeps_its_rank_in_the_file_past_one_that_is_skipped() {
        let raw = AnyXml {
            attributes: vec![("xsi:type".to_string(), "resqml:DefaultGraphicalInformation".to_string())],
            content: "<eml:TargetObject><eml:Uuid>00000000-0000-0000-0000-000000000001</eml:Uuid>\
                      <eml:QualifiedType>resqml22.BoundaryFeature</eml:QualifiedType><eml:Title>f</eml:Title></eml:TargetObject>\
                      <resqml:ViewerKind>3d</resqml:ViewerKind>\
                      <resqml:IndexableElementInfo xsi:type=\"resqml:GraphicalInformationForSomethingNew\">\
                        <resqml:IsVisible>true</resqml:IsVisible>\
                      </resqml:IndexableElementInfo>\
                      <resqml:IndexableElementInfo xsi:type=\"resqml:GraphicalInformationForWholeObject\">\
                        <resqml:IsVisible>true</resqml:IsVisible>\
                        <resqml:ConstantColor><resqml:Alpha>1</resqml:Alpha><resqml:Hue>120</resqml:Hue>\
                        <resqml:Saturation>1</resqml:Saturation><resqml:Value>1</resqml:Value></resqml:ConstantColor>\
                      </resqml:IndexableElementInfo>"
                .to_string(),
            namespaces: vec![
                (EML.0.to_string(), EML.1.to_string()),
                (RESQML.0.to_string(), RESQML.1.to_string()),
                (XSI.0.to_string(), XSI.1.to_string()),
            ],
        };
        let mut notes = Vec::new();
        let entry = read_raw_entry(&raw, 0, &mut notes);
        let Information::Default { elements, .. } = entry.information else {
            panic!("a DefaultGraphicalInformation reads as one: {:?}", entry.information);
        };
        assert_eq!(elements.len(), 1, "the unknown one is skipped: {notes:?}");
        assert_eq!(elements[0].scope, Scope::WholeObject);
        assert_eq!(elements[0].at, 1, "second in the file, first in the list read");
        assert!(notes.iter().any(|n| n.contains("SomethingNew")), "{notes:?}");
    }

    fn hsv(hue: f64) -> Hsv {
        Hsv {
            hue,
            saturation: 1.0,
            value: 1.0,
            alpha: 1.0,
            title: None,
        }
    }

    fn map(continuous: Option<(Domain, Interpolation)>, stops: Vec<(f64, f64)>) -> ColorMap {
        ColorMap {
            uuid: "map".into(),
            title: None,
            continuous,
            stops: stops
                .into_iter()
                .map(|(index, hue)| Stop {
                    index,
                    color: hsv(hue),
                })
                .collect(),
            null_color: None,
            above_max: None,
            below_min: None,
        }
    }

    #[test]
    fn the_primaries_convert_the_way_everyone_expects() {
        assert_eq!(hsv(0.0).to_rgba().to_hex(), "#ff0000ff");
        assert_eq!(hsv(120.0).to_rgba().to_hex(), "#00ff00ff");
        assert_eq!(hsv(240.0).to_rgba().to_hex(), "#0000ffff");
        // The orange of the corpus, H=24.08 S=0.84 V=0.66.
        let orange = Hsv {
            hue: 24.08,
            saturation: 0.84,
            value: 0.66,
            alpha: 1.0,
            title: None,
        };
        let rgba = orange.to_rgba();
        assert!(rgba.r > rgba.g && rgba.g > rgba.b, "{rgba:?}");
    }

    #[test]
    fn the_interpolation_domain_changes_the_picture() {
        // Green → red. In HSV the midpoint passes through yellow; in RGB it is
        // a dull olive. The reference package really does store HSV stops with
        // an `rgb` domain, so getting this wrong is not academic.
        let stops = vec![(0.0, 120.0), (1.0, 0.0)];
        let in_hsv = map(Some((Domain::Hsv, Interpolation::Linear)), stops.clone());
        let in_rgb = map(Some((Domain::Rgb, Interpolation::Linear)), stops);
        let scaling = Scaling::default();

        let yellow = in_hsv.sample(Some(0.5), &scaling).unwrap();
        let olive = in_rgb.sample(Some(0.5), &scaling).unwrap();
        assert!(yellow.r > 0.9 && yellow.g > 0.9, "HSV midpoint is yellow: {yellow:?}");
        assert!(olive.r < 0.6 && olive.g < 0.6, "RGB midpoint is dull: {olive:?}");
        assert_ne!(yellow.to_hex(), olive.to_hex());
    }

    #[test]
    fn a_discrete_map_takes_the_nearest_lower_entry_that_exists() {
        // The schema's own example: stops at 10, 20, 30 and a value of 16.5
        // gives the colour of 10 — not of 16.
        let map = map(None, vec![(10.0, 0.0), (20.0, 120.0), (30.0, 240.0)]);
        let scaling = Scaling::default();
        assert_eq!(map.sample(Some(16.5), &scaling).unwrap().to_hex(), "#ff0000ff");
        assert_eq!(map.sample(Some(20.0), &scaling).unwrap().to_hex(), "#00ff00ff");
        assert_eq!(map.sample(Some(999.0), &scaling).unwrap().to_hex(), "#0000ffff");
    }

    #[test]
    fn a_discrete_map_says_out_of_range_only_when_told_how() {
        // The two edge colours are what lets a caller keep an index no table
        // names from borrowing a neighbouring category: below the first stop
        // the fallback is the *first entry*, above the last it is the nearest
        // lower — both plausible, both silent. A category palette supplied by
        // an application's palette sets both edges
        // transparent for exactly this reason; this test is what it stands on.
        let mut m = map(None, vec![(10.0, 0.0), (20.0, 120.0)]);
        let scaling = Scaling::default();
        assert_eq!(
            m.sample(Some(5.0), &scaling).unwrap().to_hex(),
            "#ff0000ff",
            "no edges declared: below every stop, the first entry answers"
        );
        m.below_min = Some(hsv(240.0));
        m.above_max = Some(hsv(60.0));
        assert_eq!(m.sample(Some(5.0), &scaling).unwrap().to_hex(), "#0000ffff");
        assert_eq!(m.sample(Some(25.0), &scaling).unwrap().to_hex(), "#ffff00ff");
        // On a stop, the entry itself — the edges only speak outside the range.
        assert_eq!(m.sample(Some(20.0), &scaling).unwrap().to_hex(), "#00ff00ff");
    }

    #[test]
    fn min_max_clamps_and_rescales_onto_the_map() {
        // `MinMax` is the range of the *property*: a value outside it keeps
        // the extreme colour of that range.
        let map = map(Some((Domain::Rgb, Interpolation::Linear)), vec![(0.0, 120.0), (1.0, 0.0)]);
        let scaling = Scaling {
            min_max: Some((100.0, 200.0)),
            ..Default::default()
        };
        assert_eq!(map.sample(Some(50.0), &scaling).unwrap().to_hex(), "#00ff00ff");
        assert_eq!(map.sample(Some(500.0), &scaling).unwrap().to_hex(), "#ff0000ff");
        let middle = map.sample(Some(150.0), &scaling).unwrap();
        assert!((middle.r - 0.5).abs() < 0.01, "{middle:?}");
    }

    #[test]
    fn above_and_below_only_apply_when_min_max_is_absent() {
        // With `MinMax` given, the clamp makes them unreachable — which is the
        // only reading of the two TRG sections that does not contradict
        // itself.
        let mut m = map(Some((Domain::Rgb, Interpolation::Linear)), vec![(0.0, 120.0), (1.0, 0.0)]);
        m.above_max = Some(hsv(240.0));
        assert_eq!(
            m.sample(Some(5.0), &Scaling::default()).unwrap().to_hex(),
            "#0000ffff",
            "no MinMax: the above-max colour applies"
        );
        let clamped = Scaling {
            min_max: Some((0.0, 1.0)),
            ..Default::default()
        };
        assert_eq!(
            m.sample(Some(5.0), &clamped).unwrap().to_hex(),
            "#ff0000ff",
            "MinMax given: clamped to the extreme colour of the range"
        );
    }

    #[test]
    fn a_null_value_has_no_colour_unless_the_map_gives_one() {
        let mut m = map(None, vec![(0.0, 0.0)]);
        assert_eq!(m.sample(None, &Scaling::default()), None, "nothing is invented");
        m.null_color = Some(hsv(240.0));
        assert_eq!(m.sample(None, &Scaling::default()).unwrap().to_hex(), "#0000ffff");
    }

    #[test]
    fn reversing_swaps_the_ends() {
        let m = map(Some((Domain::Rgb, Interpolation::Linear)), vec![(0.0, 120.0), (1.0, 0.0)]);
        let forward = m.sample(Some(0.0), &Scaling::default()).unwrap();
        let reversed = m
            .sample(
                Some(0.0),
                &Scaling {
                    reverse: true,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(forward.to_hex(), "#00ff00ff");
        assert_eq!(reversed.to_hex(), "#ff0000ff");
    }
}

// ---------------------------------------------------------------------------
// Writing a colour back into the package

/// The object a colour entry decorates.
///
/// Three fields and not a uri: a `TargetObject` is a `DataObjectReference`,
/// and a reference names the object by uuid **and** by type. Writing only the
/// uuid would make an entry that no reader can resolve without the package,
/// which is the thing a reference exists to avoid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decorated {
    /// The object's uuid.
    pub uuid: String,
    /// Its qualified type, as a reference spells it (`resqml22.BoundaryFeature`).
    pub qualified_type: String,
    /// Its title, for a reader that has the entry and not the object.
    pub title: String,
}

/// The two namespaces a colour entry is written under.
const EML: (&str, &str) = ("eml", "http://www.energistics.org/energyml/data/commonv2");
const RESQML: (&str, &str) = ("resqml", "http://www.energistics.org/energyml/data/resqmlv2");
const XSI: (&str, &str) = ("xsi", "http://www.w3.org/2001/XMLSchema-instance");

/// A `DefaultGraphicalInformation` entry giving one object one colour.
///
/// # Why this is XML text and not an object
///
/// `GraphicalInformationSet` is an EML 2.3 object and its entries are RESQML
/// 2.2 types — the slot is declared as a foreign subtree, and the reader keeps
/// it as [`AnyXml`] for that reason. So writing an entry means writing the
/// subtree, and the shape below is the one [`GraphicalSet::read`] parses: the
/// reader is the specification of the writer, and the pair is held by a test
/// that writes an entry and reads it back.
///
/// The colour is HSV because RESQML says so; `Rgba` comes in because that is
/// what a colour picker gives, and [`Hsv::from_rgba`] is the one conversion.
/// `Title` carries the name a human chose, or the hexadecimal when nobody did:
/// the field is free text and an empty one tells a later reader nothing.
///
/// # Several targets, and one scope
///
/// `AbstractGraphicalInformation.TargetObject` is `maxOccurs="unbounded"`, so
/// **one entry may decorate a whole set of objects** — which is what colouring
/// thirty surfaces at once produces, and it is one statement rather than thirty.
/// An empty `targets` gives an entry that decorates nothing; the reader already
/// reports that state, so it is written rather than refused here, and it is the
/// caller that has a reason to have no targets or not.
///
/// The `scope` picks the concrete class of the `IndexableElementInfo`: colouring
/// the faces of an object is not the same statement as colouring the object, and
/// the two live side by side in one set (§13.3).
pub fn constant_colour_entry(
    targets: &[Decorated],
    scope: Scope,
    colour: Rgba,
    name: Option<&str>,
) -> AnyXml {
    let hsv = Hsv::from_rgba(colour);
    let title = match name {
        Some(name) if !name.trim().is_empty() => name.trim().to_string(),
        _ => format!(
            "#{:02x}{:02x}{:02x}",
            (colour.r.clamp(0.0, 1.0) * 255.0).round() as u8,
            (colour.g.clamp(0.0, 1.0) * 255.0).round() as u8,
            (colour.b.clamp(0.0, 1.0) * 255.0).round() as u8
        ),
    };
    // The targets first and in the order given: the schema puts `TargetObject`
    // before the rest of the sequence, and a document that reorders it is one no
    // validator accepts.
    let mut content = targets_xml(targets);
    content.push_str(&format!(
        "<{r}:ViewerKind>3d</{r}:ViewerKind>\
         <{r}:IndexableElementInfo xsi:type=\"{r}:{element}\">\
           <{r}:IsVisible>true</{r}:IsVisible>\
           <{r}:ConstantColor>\
             <{r}:Alpha>{alpha}</{r}:Alpha>\
             <{r}:Hue>{hue}</{r}:Hue>\
             <{r}:Saturation>{saturation}</{r}:Saturation>\
             <{r}:Title>{title}</{r}:Title>\
             <{r}:Value>{value}</{r}:Value>\
           </{r}:ConstantColor>\
         </{r}:IndexableElementInfo>",
        r = RESQML.0,
        element = scope.type_name(),
        alpha = number(hsv.alpha),
        hue = number(hsv.hue),
        saturation = number(hsv.saturation),
        value = number(hsv.value),
        title = escape(&title),
    ));
    AnyXml {
        attributes: vec![("xsi:type".to_string(), format!("{}:DefaultGraphicalInformation", RESQML.0))],
        content,
        namespaces: vec![
            (EML.0.to_string(), EML.1.to_string()),
            (RESQML.0.to_string(), RESQML.1.to_string()),
            (XSI.0.to_string(), XSI.1.to_string()),
        ],
    }
}

/// The `TargetObject` elements of an entry, in the order given.
///
/// Shared by the two writers, because it is the same sequence in the same
/// namespace: `AbstractGraphicalInformation` declares it and both concrete
/// classes inherit it. Two copies would drift the day a release adds a field.
fn targets_xml(targets: &[Decorated]) -> String {
    let mut out = String::new();
    for target in targets {
        out.push_str(&format!(
            "<{eml}:TargetObject>\
               <{eml}:Uuid>{uuid}</{eml}:Uuid>\
               <{eml}:QualifiedType>{qt}</{eml}:QualifiedType>\
               <{eml}:Title>{title}</{eml}:Title>\
             </{eml}:TargetObject>",
            eml = EML.0,
            uuid = escape(&target.uuid),
            qt = escape(&target.qualified_type),
            title = escape(&target.title),
        ));
    }
    out
}

/// A `ColorInformation` entry: colour these objects **by a colour map**.
///
/// # The order of the elements is the schema's, not a preference
///
/// `TargetObject`, `MinMax`, `UseLogarithmicMapping`, `UseReverseMapping`,
/// `ValueVectorIndex`, `ColorMap` — an `xsd:sequence`, so a document that puts
/// them in another order is one no validator accepts. The two booleans are
/// **required** and written even when false; the three optionals are omitted
/// when there is nothing to say, which is not the same as writing a default.
///
/// # What this does not do
///
/// It does not create the map. §13.2 point 6: *a colormap must already be in
/// the set or referenced by it — the frame chooses one, it does not make one*.
/// So `map` is a reference to an object that exists, and this writer has no
/// opinion about where it came from.
pub fn colour_map_entry(
    targets: &[Decorated],
    map: &Decorated,
    scaling: &Scaling,
    value_vector_index: Option<u64>,
) -> AnyXml {
    let mut content = targets_xml(targets);
    if let Some((low, high)) = scaling.min_max {
        content.push_str(&format!(
            "<{r}:MinMax>\
               <{r}:Minimum>{min}</{r}:Minimum>\
               <{r}:Maximum>{max}</{r}:Maximum>\
             </{r}:MinMax>",
            r = RESQML.0,
            min = number(low),
            max = number(high),
        ));
    }
    content.push_str(&format!(
        "<{r}:UseLogarithmicMapping>{log}</{r}:UseLogarithmicMapping>\
         <{r}:UseReverseMapping>{rev}</{r}:UseReverseMapping>",
        r = RESQML.0,
        log = scaling.logarithmic,
        rev = scaling.reverse,
    ));
    if let Some(index) = value_vector_index {
        content.push_str(&format!(
            "<{r}:ValueVectorIndex>{index}</{r}:ValueVectorIndex>",
            r = RESQML.0
        ));
    }
    content.push_str(&format!(
        "<{r}:ColorMap>\
           <{eml}:Uuid>{uuid}</{eml}:Uuid>\
           <{eml}:QualifiedType>{qt}</{eml}:QualifiedType>\
           <{eml}:Title>{title}</{eml}:Title>\
         </{r}:ColorMap>",
        r = RESQML.0,
        eml = EML.0,
        uuid = escape(&map.uuid),
        qt = escape(&map.qualified_type),
        title = escape(&map.title),
    ));

    AnyXml {
        attributes: vec![("xsi:type".to_string(), format!("{}:ColorInformation", RESQML.0))],
        content,
        namespaces: vec![
            (EML.0.to_string(), EML.1.to_string()),
            (RESQML.0.to_string(), RESQML.1.to_string()),
            (XSI.0.to_string(), XSI.1.to_string()),
        ],
    }
}

/// A double as the schema wants it: always with a decimal point, so that `1`
/// does not arrive where `1.0` is expected.
fn number(value: f64) -> String {
    if value.is_finite() && value == value.trunc() && value.abs() < 1e15 {
        format!("{value:.1}")
    } else {
        format!("{value}")
    }
}

/// The five characters XML text cannot carry as they are.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// Where a constant colour is stated — [`Cascade::constant_written`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Written {
    /// The entry's position in the set read — in a merged set, the merged
    /// position ([`locate_merged`] gives it back in its own set).
    pub position: usize,
    /// The element's rank in `IndexableElementInfo`, the file's.
    pub at: usize,
    /// Which elements that element covers.
    pub scope: Scope,
    /// Every object the entry decorates: changing the value changes them all.
    pub targets: Vec<String>,
}

/// A position of [`GraphicalSet::merged`] given back as (which set, which
/// position in it) — the inverse of the renumbering `merged` does, and nothing
/// else: the sets in the order they were merged.
pub fn locate_merged(sets: &[GraphicalSet], position: usize) -> Option<(usize, usize)> {
    let mut offset = 0;
    for (at, set) in sets.iter().enumerate() {
        if position < offset + set.entries.len() {
            return Some((at, position - offset));
        }
        offset += set.entries.len();
    }
    None
}

/// Change the colour of **one element** of one entry, in place: the value where
/// it is stated, and nothing else.
///
/// The rule: either modify the existing referenced value, or assign the object
/// its own if it uses one through a path — this is the first
/// half. Where [`set_constant_colour`] rewrites an entry from what this build
/// read of it, and so refuses an entry that names several objects (rebuilding
/// it could drop what the reader does not model), this edits the one
/// `ConstantColor` through the typed subtree and writes the entry back: every
/// target, every other element, every field the reader skips stays as it was.
/// An entry that names several objects is therefore recoloured for all of them
/// — which is what *changing the value at its source* means; the caller says
/// so before ([`Written::targets`]).
///
/// The entry is a subtree typed on the way in (`AnyXml::typed`, the road the
/// generic editor takes); one this build cannot type is refused by name rather
/// than rewritten. An element with no `ConstantColor` is refused too: there is
/// no value to change, and inventing one is the other half's business.
pub fn recolour_element(
    set: &mut dyn Reflect,
    position: usize,
    at: usize,
    colour: Rgba,
    registry: &energyml_core::registry::Registry,
) -> Result<()> {
    use energyml_core::path;
    use energyml_core::value::OwnedValue;

    let entry_path = format!("GraphicalInformation[{position}]");
    let colour_path = format!("IndexableElementInfo[{at}].ConstantColor");
    let hsv = Hsv::from_rgba(colour);
    let write = |target: &mut dyn Reflect, base: &str| -> Result<()> {
        let at_field = |field: &str| if base.is_empty() { format!("{colour_path}.{field}") } else { format!("{base}.{colour_path}.{field}") };
        let present = path::get(target, &if base.is_empty() { colour_path.clone() } else { format!("{base}.{colour_path}") })
            .ok()
            .and_then(|value| value.as_object().map(|_| ()))
            .is_some();
        if !present {
            return Err(Error::unsupported(
                "recolouring an element",
                format!("{entry_path}, element {at}, states no constant colour to change"),
            ));
        }
        for (field, value) in [("Hue", hsv.hue), ("Saturation", hsv.saturation), ("Value", hsv.value), ("Alpha", hsv.alpha)] {
            path::set(target, &at_field(field), OwnedValue::Double(value))
                .map_err(|why| Error::malformed(&entry_path, format!("writing `{field}`: {why}")))?;
        }
        Ok(())
    };

    let object = path::object_mut(set, &entry_path)
        .map_err(|why| Error::malformed(&entry_path, format!("no such entry: {why}")))?;
    // The usual case: the entry is a subtree nothing could type when it was read.
    if let Some(any) = object.downcast_mut::<AnyXml>() {
        let read = any.typed(registry).ok_or_else(|| {
            Error::unsupported("recolouring an element", format!("{entry_path} declares no type this build knows"))
        })?;
        let (mut typed, _) = read.map_err(|why| Error::malformed(&entry_path, format!("reading it: {why}")))?;
        write(typed.as_reflect_mut(), "")?;
        *any = any
            .rebuilt(typed.as_ref())
            .map_err(|why| Error::malformed(&entry_path, format!("writing it back: {why}")))?;
        return Ok(());
    }
    write(set, &entry_path)
}

/// What [`set_constant_colour`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wrote {
    /// The object had no whole-object colour in this set; one was appended.
    Added,
    /// It had one, at this position, and it was replaced.
    Replaced(usize),
}

/// Give a set of objects one colour in a `GraphicalInformationSet`, in place.
///
/// **Replaces rather than adds** when the set already colours these objects at
/// this scope: two entries naming one target with one role is a case the
/// standard leaves undecided (ADR-0009, point 3), and writing a second one would be *this* tool creating the
/// ambiguity it complains about elsewhere. An entry that colours the same
/// object's nodes when the faces are being coloured is left alone — it says
/// something else, which is what `scope` is for.
///
/// The entries this does not understand are carried through untouched. They
/// are foreign subtrees; rebuilding the list is the only way to change one, so
/// every other one has to survive the rebuild, and a set with an entry we
/// cannot parse must not lose it because somebody recoloured a horizon.
///
/// # What it refuses, and the colour it used to lose
///
/// An entry may decorate **several** objects, and until 04/09 recolouring one of
/// them replaced that entry with a single-target one: the other objects lost
/// their colour, in silence, from a gesture that named only one of them. So an
/// entry that names a target being recoloured **and** targets that are not is a
/// named refusal ([`Error::Unsupported`]) rather than a rewrite.
///
/// Splitting it — keeping the others in place and adding one entry for the
/// recoloured object — is the operation somebody will eventually want, and it is
/// deliberately not done here: rebuilding the remainder means writing it back
/// from what this build could parse, and anything in that subtree that the
/// reader does not model would be dropped on the way. Refusing loses nothing and
/// says which entry stands in the way; that is the honest half of the answer,
/// and the other half is `move an entry between sets` (§13.3).
pub fn set_constant_colour(
    set: &mut dyn Reflect,
    targets: &[Decorated],
    scope: Scope,
    colour: Rgba,
    name: Option<&str>,
) -> Result<Wrote> {
    put(set, targets, &format!("{scope}"), |entry| {
        matches!(&entry.information, Information::Default { elements, .. }
            if elements.iter().any(|element| element.scope == scope))
    }, || constant_colour_entry(targets, scope, colour, name))
}

/// Colour these objects **by a colour map**, in place.
///
/// The sibling of [`set_constant_colour`], and the same rules: it replaces the
/// `ColorInformation` that already colours these objects rather than adding a
/// second one, it carries every entry it does not understand through untouched,
/// and it **refuses** an entry that also names objects nobody asked about
/// instead of taking their colour away.
///
/// What it does *not* share is the notion of a scope: a colour map paints a
/// property's values, and the schema gives `ColorInformation` no indexable
/// element to aim at. So *the same statement* here means *a colour map on these
/// objects*, full stop.
///
/// The map is a reference to a `ColorMap` that **already exists** — §13.2 point
/// 6. Creating one is a different operation, and one this writer would have no
/// values to fill.
pub fn set_colour_map(
    set: &mut dyn Reflect,
    targets: &[Decorated],
    map: &Decorated,
    scaling: &Scaling,
    value_vector_index: Option<u64>,
) -> Result<Wrote> {
    put(
        set,
        targets,
        "a colour map",
        |entry| matches!(&entry.information, Information::Color { .. }),
        || colour_map_entry(targets, map, scaling, value_vector_index),
    )
}

/// Put one entry in a set, replacing the one that says the same thing.
///
/// The road both writers take, written once. `says` is what *the same thing*
/// means for the caller — a constant colour at a scope, a colour map — and
/// `about` names it in a refusal, where a person reads it.
fn put(
    set: &mut dyn Reflect,
    targets: &[Decorated],
    about: &str,
    says: impl Fn(&Entry) -> bool,
    make: impl Fn() -> AnyXml,
) -> Result<Wrote> {
    let wanted: Vec<String> = targets.iter().map(|target| target.uuid.to_lowercase()).collect();
    let read = GraphicalSet::read(&*set);
    let touching: Vec<&Entry> = read
        .entries
        .iter()
        .filter(|entry| {
            entry.targets.iter().any(|uuid| wanted.contains(&uuid.to_lowercase())) && says(entry)
        })
        .collect();

    // More than one entry to replace, or one that decorates objects nobody
    // asked about: either way this write cannot be expressed as *replace that
    // entry*, and doing it anyway is how the colours of the other objects went
    // missing.
    if let Some(entry) = touching.iter().find(|entry| {
        entry.targets.iter().any(|uuid| !wanted.contains(&uuid.to_lowercase()))
    }) {
        return Err(Error::unsupported(
            "recolouring one object of a shared colour entry",
            format!(
                "entry {} colours {} object(s) with {about}, and only {} of them are being \
                 recoloured; replacing it would take the colour away from the others",
                entry.position,
                entry.targets.len(),
                entry.targets.iter().filter(|uuid| wanted.contains(&uuid.to_lowercase())).count(),
            ),
        ));
    }
    if touching.len() > 1 {
        return Err(Error::unsupported(
            "recolouring objects coloured by several entries",
            format!(
                "{} entries colour these objects with {about}; the set says two things about \
                 them already, and this would say a third",
                touching.len()
            ),
        ));
    }
    let position = touching.first().map(|entry| entry.position);

    let mut items: Vec<energyml_core::value::OwnedValue> = Vec::new();
    for (index, item) in list(&*set, "GraphicalInformation").into_iter().enumerate() {
        let kept = item
            .as_object()
            .and_then(|object| object.downcast_ref::<AnyXml>())
            .cloned()
            .ok_or_else(|| Error::Malformed {
                at: format!("GraphicalInformation[{index}]"),
                reason: "not a subtree this build can carry, so the set cannot be rewritten"
                    .to_string(),
            })?;
        let value = if Some(index) == position { make() } else { kept };
        items.push(energyml_core::value::OwnedValue::Object(Box::new(value)));
    }
    if position.is_none() {
        items.push(energyml_core::value::OwnedValue::Object(Box::new(make())));
    }
    set.set_field("GraphicalInformation", energyml_core::value::OwnedValue::List(items))
        .map_err(|e| Error::Malformed { at: "GraphicalInformation".to_string(), reason: e.to_string() })?;
    Ok(match position {
        Some(index) => Wrote::Replaced(index),
        None => Wrote::Added,
    })
}

/// The colour of an object the package says nothing about: a hue derived from
/// a seed (its uuid), at a fixed saturation and lightness — **a front ender's
/// `colourFor`**, which gives every uncoloured
/// object of its 3D view its colour.
///
/// Two implementations of one fact, kept together the way the repository keeps
/// such pairs (a second reader is confronted with the first): the same table
/// of seeds and answers is asserted on both sides (`tests/derived_colour.rs`
/// here, a front end's colour test). It exists in Rust so a drawing
/// made without a scene — the section view's SVG from the CLI — gives a surface
/// the colour the 3D view gives it, rather than one colour for every surface.
///
/// FNV-1a over the seed's UTF-16 code units (JavaScript's `charCodeAt`), the
/// hue the absolute value of the signed 32-bit hash modulo 360, then HSL to
/// `#rrggbb` at saturation 0.55 and lightness 0.6.
pub fn derived_colour(seed: &str) -> String {
    // JavaScript's numbers, exactly: the offset basis is the *unsigned*
    // 2166136261 until the first `^=` turns the hash into a signed 32-bit
    // integer — so an empty seed hashes to the unsigned value, every other seed
    // to a signed one. The shared table holds the empty seed for that reason.
    let mut hash: i64 = 0x811c9dc5;
    for unit in seed.encode_utf16() {
        let mixed = (hash as u32 as i32) ^ i32::from(unit);
        hash = i64::from(mixed.wrapping_mul(0x01000193));
    }
    let hue = (hash.abs() % 360) as f64;
    let (saturation, lightness) = (0.55_f64, 0.6_f64);
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let second = chroma * (1.0 - ((hue / 60.0) % 2.0 - 1.0).abs());
    let shift = lightness - chroma / 2.0;
    let (r, g, b) = match ((hue / 60.0).floor() as i64) % 6 {
        0 => (chroma, second, 0.0),
        1 => (second, chroma, 0.0),
        2 => (0.0, chroma, second),
        3 => (0.0, second, chroma),
        4 => (second, 0.0, chroma),
        _ => (chroma, 0.0, second),
    };
    let byte = |value: f64| ((value + shift) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", byte(r), byte(g), byte(b))
}
