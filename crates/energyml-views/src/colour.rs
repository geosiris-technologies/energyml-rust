//! The colour of a value, by one door.
//!
//! The package decides first: [`ColourDoor`] asks `energyml_cache::colours` - the walk `energyml colors` prints -
//! for the map the file gives an object. When it gives none, a palette of [`energyml_domain::palettes`] stands in,
//! the default of the value's reading unless the request names one, and the legend says it is not from the file.
//! A palette asked for by name wins over the file: it is the user's choice. Whichever paints, the value goes through
//! [`ColorMap::sample`], the one rule: no colour is ever invented, and a value the map has no colour for is left
//! transparent.

use energyml_cache::Cache;
use energyml_cache::colours::{Wanted, of_package};
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
}

/// Where the colours of a picture come from: what its legend says.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum ColourSource {
    /// The map the file gives the object.
    File,
    /// A palette, by name: not from the file.
    Palette(String),
}

impl ColourSource {
    /// The legend's words.
    pub fn says(&self) -> String {
        match self {
            ColourSource::File => "colours: the file's map".to_string(),
            ColourSource::Palette(name) => format!("colours: palette {name} (not from the file)"),
        }
    }
}

#[derive(Clone, Debug)]
enum Painting {
    Constant([u8; 4]),
    Mapped { map: Box<ColorMap>, scaling: Scaling },
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
        match &self.painting {
            Painting::Constant(rgba) => Some(*rgba),
            Painting::Mapped { map, scaling } => map
                .sample(Some(value).filter(|v| !v.is_nan()), scaling)
                .map(bytes),
        }
    }
}

fn bytes(rgba: Rgba) -> [u8; 4] {
    let channel = |value: f64| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    [channel(rgba.r), channel(rgba.g), channel(rgba.b), channel(rgba.a)]
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
                why: format!("it paints another reading; for this one: {}", fitting.join(", ")),
            });
        }
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
