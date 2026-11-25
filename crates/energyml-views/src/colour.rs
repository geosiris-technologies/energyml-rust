//! The colour of a value, by one door.
//!
//! The package decides first: [`ColourDoor`] asks `energyml_cache::colours` - the walk `energyml colors` prints -
//! for the map the file gives an object. When it gives none, a palette of [`energyml_domain::palettes`] stands in,
//! the default of the value's reading unless the request names one, and the legend says it is not from the file.
//! A palette asked for by name wins over the file: it is the user's choice. Whichever paints, the value goes through
//! [`ColorMap::sample`], the one rule: no colour is ever invented, and a value the map has no colour for is left
//! transparent.

use energyml_cache::Cache;
use energyml_cache::colours::{Options, Wanted, decisions_of, of_package};
use energyml_domain::color::{ColorMap, Paint, Rgba, Scaling};
use energyml_domain::palettes::Reading;
use energyml_storage::EpcStore;
use serde::Serialize;

use crate::kind::ViewContext;
use crate::refusal::ViewRefusal;

/// The colour the package gives an object.
pub trait ColourDoor {
    /// What the package paints the object of this full uri with; `None` when nothing in it says.
    ///
    /// # Errors
    ///
    /// A package whose graphical information does not read, by its words.
    fn paint(&self, object_uri: &str) -> Result<Option<Paint>, String>;

    /// [`ColourDoor::paint`] for several objects at once: one entry per uri, in order, each what `paint` would
    /// have answered for it alone.
    ///
    /// A door that pays a cost per question (the package's sets read, an index built) overrides it to pay once;
    /// this default asks one by one.
    fn paint_all(&self, object_uris: &[&str]) -> Vec<Result<Option<Paint>, String>> {
        object_uris.iter().map(|uri| self.paint(uri)).collect()
    }
}

/// A door that knows no map: every colour comes from a palette (tests, and a package read without its colours).
pub struct NoColourMaps;

impl ColourDoor for NoColourMaps {
    fn paint(&self, _object_uri: &str) -> Result<Option<Paint>, String> {
        Ok(None)
    }
}

/// The package's own colours, through the cache: the walk of `energyml colors`, object by object.
pub struct PackageColours<'a> {
    /// The cache the walk keeps.
    pub cache: &'a Cache,
    /// The store it answers for.
    pub store: &'a EpcStore,
}

impl ColourDoor for PackageColours<'_> {
    fn paint(&self, object_uri: &str) -> Result<Option<Paint>, String> {
        let wanted = Wanted {
            object: Some(object_uri),
            ..Wanted::default()
        };
        let report = of_package(self.cache, self.store, &wanted).map_err(|why| why.to_string())?;
        Ok(report
            .rows
            .into_iter()
            .find_map(|row| row.decision)
            .map(|decision| decision.paint))
    }

    /// One walk for every object: the package's sets are read once, not once per object
    /// ([`energyml_cache::colours::decisions_of`]).
    fn paint_all(&self, object_uris: &[&str]) -> Vec<Result<Option<Paint>, String>> {
        match decisions_of(self.cache, self.store, object_uris, &Options::default()) {
            Ok(decided) => decided
                .into_iter()
                .map(|answer| answer.map(|decision| decision.map(|decision| decision.paint)))
                .collect(),
            Err(why) => object_uris.iter().map(|_| Err(why.to_string())).collect(),
        }
    }
}

/// Where the colours of a picture come from: what its legend says.
#[derive(Clone, Debug, PartialEq, Serialize)]
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

#[derive(Clone, Debug)]
enum Painting {
    Constant([u8; 4]),
    Mapped {
        map: Box<ColorMap>,
        scaling: Scaling,
    },
    /// A category: the entry of the map that has its key, else what `fallback` paints (a palette), else nothing.
    ByKey {
        map: Box<ColorMap>,
        fallback: Option<Box<Painting>>,
    },
}

impl Painting {
    fn sample(&self, value: f64) -> Option<[u8; 4]> {
        match self {
            Painting::Constant(rgba) => Some(*rgba),
            Painting::Mapped { map, scaling } => map
                .sample(Some(value).filter(|v| !v.is_nan()), scaling)
                .map(bytes),
            Painting::ByKey { map, fallback } => map
                .sample_key(value)
                .map(bytes)
                .or_else(|| fallback.as_ref().and_then(|fallback| fallback.sample(value))),
        }
    }
}

/// The colours of one picture's values.
#[derive(Clone, Debug)]
pub struct ValueColours {
    painting: Painting,
    /// Where they come from.
    pub source: ColourSource,
}

impl ValueColours {
    /// The colour of a value, RGBA; `None` where the map has none (a null value with no null colour).
    pub fn sample(&self, value: f64) -> Option<[u8; 4]> {
        self.painting.sample(value)
    }
}

fn bytes(rgba: Rgba) -> [u8; 4] {
    let channel = |value: f64| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    [channel(rgba.r), channel(rgba.g), channel(rgba.b), channel(rgba.a)]
}

/// The palette a view was asked for **by name**, the one reading of that request: the refusal when no palette has the
/// name (it lists the known ones) and when the palette paints another reading (it lists those that fit `reading`;
/// `what` ends the sentence: "for `what`: ..."). Every view that takes a `palette` setting goes through it.
///
/// # Errors
///
/// [`ViewRefusal::Palette`] in both cases.
pub fn palette_named(
    cx: &ViewContext<'_>,
    name: &str,
    reading: Reading,
    what: &str,
) -> Result<energyml_domain::palettes::Palette, ViewRefusal> {
    let Some(found) = cx.palettes.named(name) else {
        let known: Vec<&str> = cx.palettes.all().iter().map(|palette| palette.name).collect();
        return Err(ViewRefusal::Palette {
            asked: name.to_string(),
            why: format!("no palette of that name; the palettes are {}", known.join(", ")),
        });
    };
    if found.reading != reading {
        let fitting: Vec<&str> = cx
            .palettes
            .for_reading(reading)
            .iter()
            .map(|palette| palette.name)
            .collect();
        return Err(ViewRefusal::Palette {
            asked: name.to_string(),
            why: format!("it paints another reading; for {what}: {}", fitting.join(", ")),
        });
    }
    Ok(found)
}

/// The colours of the values of `object` (when it has one), over `bounds`, `logarithmic` or not.
///
/// `palette` named: that palette, refused when no palette has the name or when it is for another reading. Else the
/// map the file gives the object, with its own scaling. Else the default palette of `reading`.
///
/// # Errors
///
/// [`ViewRefusal::Palette`] for an unknown palette or one of another reading; the refusal of the colour door, by its
/// words.
pub fn value_colours(
    cx: &ViewContext<'_>,
    view: &str,
    object: Option<&str>,
    palette: Option<&str>,
    bounds: (f64, f64),
    logarithmic: bool,
    reading: Reading,
) -> Result<ValueColours, ViewRefusal> {
    let by_palette = |name: &str| -> Result<ValueColours, ViewRefusal> {
        let found = palette_named(cx, name, reading, "this one")?;
        Ok(ValueColours {
            painting: Painting::Mapped {
                map: Box::new(found.ramp()),
                scaling: Scaling {
                    min_max: Some(bounds),
                    logarithmic,
                    reverse: false,
                },
            },
            source: ColourSource::Palette(found.name.to_string()),
        })
    };
    if let Some(name) = palette {
        return by_palette(name);
    }
    if let Some(uri) = object {
        match cx
            .colours
            .paint(uri)
            .map_err(|why| ViewRefusal::computing(view, why))?
        {
            // A **category** is placed by its key: the entry that has it, and no other. A key the map has no entry for
            // takes the default palette of the reading, and the source says so.
            Some(Paint::Mapped { map, .. })
                if reading == Reading::Categorical && map.continuous.is_none() =>
            {
                return Ok(match cx.palettes.default_for(reading) {
                    Some(default) => {
                        let fallback = by_palette(default.name)?;
                        ValueColours {
                            painting: Painting::ByKey {
                                map,
                                fallback: Some(Box::new(fallback.painting)),
                            },
                            source: ColourSource::FileAndPalette(default.name.to_string()),
                        }
                    }
                    None => ValueColours {
                        painting: Painting::ByKey { map, fallback: None },
                        source: ColourSource::File,
                    },
                });
            }
            Some(Paint::Mapped { map, scaling, .. }) => {
                return Ok(ValueColours {
                    painting: Painting::Mapped { map, scaling },
                    source: ColourSource::File,
                });
            }
            Some(Paint::Constant(rgba)) => {
                return Ok(ValueColours {
                    painting: Painting::Constant(bytes(rgba)),
                    source: ColourSource::File,
                });
            }
            None => {}
        }
    }
    let Some(default) = cx.palettes.default_for(reading) else {
        return Err(ViewRefusal::Palette {
            asked: String::new(),
            why: "no palette for this reading".to_string(),
        });
    };
    by_palette(default.name)
}
