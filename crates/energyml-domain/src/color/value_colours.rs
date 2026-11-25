//! The one door from a **value to a colour**.
//!
//! The colour a value takes was decided in four places that each chose between the same three questions: *does the file
//! give a colour, a map or a constant?* *is the value a category (read by its exact key) or a measure (placed on a
//! scale)?* *what stands in when the file says nothing, and where does the extent of the scale come from?* The views, the
//! units of a wellbore, the legend of a scene and the 3D of the studio each answered them, and the answers drifted: a
//! category with no entry in the map took the colour of its lower neighbour in one of them and none in another.
//! [`ValueColours`] answers them once.
//!
//! # The rule
//!
//! - **A constant colour** colours every value, a missing one too.
//! - **A discrete map of the file read as a category** ([`Reading::Categorical`], the property names a table):
//!   [`ColorMap::sample_key`], the entry that has the key and no other - RESQML TRG 2.2 section 4.11 states the "next lowest"
//!   rule for a *continuous* property read through a discrete map, not for a code. A key the map has no entry for
//!   takes the colour of the palette that stands in, laid **over the keys** ([`Palette::over_categories`]: one colour
//!   per key, in turn), and [`ColourSource::FileAndPalette`] says it is not the file's; with no palette it has no colour.
//! - **Any other map of the file** is a measure: [`ColorMap::sample`], on the scale the file gives
//!   (`ColorInformation.MinMax`, RESQML TRG 2.2 section 4.5; absent, the map's own range of indices stands for the
//!   range of the values, ADR 0025). The extent a caller asks for is **never** applied to a file's map.
//! - **No colour in the file**: a palette stands in, [`ColourSource::Palette`]. A category palette is laid over the keys;
//!   a continuous one over the extent the caller gives, which it needs because its indices (0 to 1) say nothing of
//!   the values.
//! - **A missing value** takes the map's `NullColor`, or nothing: no colour is invented.
//!
//! The scale a caller has merged itself (the request's fields over the file's, field by field) goes in as `scaling`
//! and replaces the file's.

use crate::color::{ColorMap, Hsv, Paint, Rgba, Scaling};
use crate::palettes::{Palette, Reading};

/// Where the colours of a picture come from: what its legend says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ColourSource {
    /// The map the file gives the object.
    File,
    /// A palette, by name: not from the file.
    Palette(String),
    /// The file's map, read **by key** (a category is the entry that has its key, never the nearest lower one), and
    /// this palette for a key the map has no entry for: not from the file.
    FileAndPalette(String),
}

impl ColourSource {
    /// The legend's words.
    pub fn says(&self) -> String {
        match self {
            ColourSource::File => "colours: the file's map".to_string(),
            ColourSource::Palette(name) => format!("colours: palette {name} (not from the file)"),
            ColourSource::FileAndPalette(name) => format!(
                "colours: the file's map by key; palette {name} for a key it has no entry for (not from the file)"
            ),
        }
    }
}

/// Why no colours could be made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ColourRefusal {
    /// The file gives no colour and no palette stands in.
    NoColours,
    /// A continuous palette was asked with no extent: its indices say nothing of the values.
    NeedsExtent {
        /// The palette.
        palette: &'static str,
    },
    /// A category palette was asked with no key to lay it over.
    NoCategories {
        /// The palette.
        palette: &'static str,
    },
}

impl std::fmt::Display for ColourRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ColourRefusal::NoColours => f.write_str("no palette for this reading"),
            ColourRefusal::NeedsExtent { palette } => write!(
                f,
                "the palette {palette} needs the range of the values to lay its colours on"
            ),
            ColourRefusal::NoCategories { .. } => f.write_str("there is no category to colour"),
        }
    }
}

impl std::error::Error for ColourRefusal {}

/// What paints a key the file's map has no entry for.
#[derive(Clone, Debug)]
enum Fallback {
    /// A palette laid over the keys: the entry that has the key.
    Keys(Box<ColorMap>),
    /// A ramp over an extent, for a caller that gave no keys.
    Ramp { map: Box<ColorMap>, scaling: Scaling },
}

#[derive(Clone, Debug)]
enum Painting {
    Constant(Rgba),
    Mapped {
        map: Box<ColorMap>,
        scaling: Scaling,
    },
    /// A category: the entry of the map that has its key, else what `fallback` paints, else nothing.
    ByKey {
        map: Box<ColorMap>,
        fallback: Option<Fallback>,
    },
}

/// The colours of one picture's values.
#[derive(Clone, Debug)]
pub struct ValueColours {
    painting: Painting,
    /// Where they come from.
    pub source: ColourSource,
}

impl ValueColours {
    /// The colours a picture's values take, by the rule of the module.
    ///
    /// - `file`: what the cascade of the package decided for the object, `None` when it decided nothing, or when the
    ///   caller prefers a palette (a palette asked for by name wins over the file: pass `None`);
    /// - `reading`: [`Reading::Categorical`] for a property that names a table (the codes), [`Reading::Continuous`] for a
    ///   measure;
    /// - `stand_in`: the palette that paints what the file does not;
    /// - `extent`: the range of the values, **for a continuous palette only** - a file's map keeps its own scale;
    /// - `keys`: the keys a category palette is laid over (ascending or not, repeated or not);
    /// - `scaling`: a scale the caller has merged itself, which replaces the file's, and which a palette takes with
    ///   `extent` filling a missing `min_max`.
    ///
    /// # Errors
    ///
    /// [`ColourRefusal`] when nothing can paint: no file colour and no palette, a continuous palette with no extent, a
    /// category palette with no key.
    pub fn new(
        file: Option<&Paint>,
        reading: Reading,
        stand_in: Option<&Palette>,
        extent: Option<(f64, f64)>,
        keys: &[f64],
        scaling: Option<Scaling>,
    ) -> Result<ValueColours, ColourRefusal> {
        match file {
            Some(Paint::Constant(colour)) => Ok(ValueColours {
                painting: Painting::Constant(*colour),
                source: ColourSource::File,
            }),
            Some(Paint::Mapped {
                map, scaling: filed, ..
            }) => {
                let scaling = scaling.unwrap_or(*filed);
                if reading == Reading::Categorical && map.continuous.is_none() {
                    let fallback = stand_in.and_then(|palette| {
                        let laid = match palette.over_categories(keys) {
                            Some(over) => Fallback::Keys(Box::new(over)),
                            None => Fallback::Ramp {
                                map: Box::new(palette.ramp()),
                                scaling: Scaling {
                                    min_max: Some(extent?),
                                    ..Scaling::default()
                                },
                            },
                        };
                        Some((laid, palette.name.to_string()))
                    });
                    let (fallback, source) = match fallback {
                        Some((laid, name)) => (Some(laid), ColourSource::FileAndPalette(name)),
                        None => (None, ColourSource::File),
                    };
                    Ok(ValueColours {
                        painting: Painting::ByKey {
                            map: map.clone(),
                            fallback,
                        },
                        source,
                    })
                } else {
                    Ok(ValueColours {
                        painting: Painting::Mapped {
                            map: map.clone(),
                            scaling,
                        },
                        source: ColourSource::File,
                    })
                }
            }
            None => {
                let palette = stand_in.ok_or(ColourRefusal::NoColours)?;
                let source = ColourSource::Palette(palette.name.to_string());
                let ramp = |scaling: Option<Scaling>| -> Result<Painting, ColourRefusal> {
                    let mut scaling = scaling.unwrap_or_default();
                    if scaling.min_max.is_none() {
                        scaling.min_max = Some(extent.ok_or(ColourRefusal::NeedsExtent {
                            palette: palette.name,
                        })?);
                    }
                    Ok(Painting::Mapped {
                        map: Box::new(palette.ramp()),
                        scaling,
                    })
                };
                let painting = match reading {
                    // Laid over the keys when there are some; with none, the ramp a caller that knows no key has
                    // always had.
                    Reading::Categorical => match palette.over_categories(keys) {
                        Some(map) => Painting::ByKey {
                            map: Box::new(map),
                            fallback: None,
                        },
                        None if keys.is_empty() => ramp(scaling)?,
                        None => {
                            return Err(ColourRefusal::NoCategories {
                                palette: palette.name,
                            });
                        }
                    },
                    Reading::Continuous => ramp(scaling)?,
                };
                Ok(ValueColours { painting, source })
            }
        }
    }

    /// The colour of a value, or of a missing one (`None`, or not a number): `None` where nothing colours it - a
    /// missing value and no `NullColor`, a key with no entry and no palette.
    pub fn colour(&self, value: Option<f64>) -> Option<Rgba> {
        let value = value.filter(|value| !value.is_nan());
        match &self.painting {
            Painting::Constant(colour) => Some(*colour),
            Painting::Mapped { map, scaling } => map.sample(value, scaling),
            Painting::ByKey { map, fallback } => match value {
                None => map.null_color.as_ref().map(Hsv::to_rgba),
                Some(key) => map.sample_key(key).or_else(|| match fallback {
                    Some(Fallback::Keys(over)) => over.sample_key(key),
                    Some(Fallback::Ramp { map, scaling }) => map.sample(Some(key), scaling),
                    None => None,
                }),
            },
        }
    }

    /// The colour of a value, as four bytes; `None` where [`ValueColours::colour`] has none.
    pub fn sample(&self, value: f64) -> Option<[u8; 4]> {
        self.colour(Some(value)).map(Rgba::to_bytes)
    }

    /// Where the colours come from.
    pub fn source(&self) -> &ColourSource {
        &self.source
    }
}
