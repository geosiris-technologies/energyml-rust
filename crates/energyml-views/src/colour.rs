//! The colour of a value, by one door.
//!
//! The package decides first: [`ColourDoor`] asks `energyml_cache::colours` - the walk `energyml colors` prints -
//! for the map the file gives an object. When it gives none, a palette of [`energyml_domain::palettes`] stands in,
//! the default of the value's reading unless the request names one, and the legend says it is not from the file.
//! A palette asked for by name wins over the file: it is the user's choice. Whichever paints, the value goes through
//! [`energyml_domain::color::ValueColours`] (`ColorMap::sample`, or the entry that has the key for a category), the one rule: no colour is ever invented, and a value the map has no colour for is left
//! transparent.

use energyml_cache::Cache;
use energyml_cache::colours::{Options, Wanted, decisions_of, of_package};
use energyml_domain::color::{ColourRefusal, Paint, Scaling};
use energyml_domain::palettes::Reading;
use energyml_storage::EpcStore;

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

    /// [`ColourDoor::paint`], and what reading the colours had to say of themselves - the notes of the decision (a hue read
    /// as a fraction of a turn, an entry that named no map). A door that has none says none.
    ///
    /// # Errors
    ///
    /// As [`ColourDoor::paint`].
    fn paint_noted(&self, object_uri: &str) -> Result<(Option<Paint>, Vec<String>), String> {
        self.paint(object_uri).map(|paint| (paint, Vec::new()))
    }

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

    fn paint_noted(&self, object_uri: &str) -> Result<(Option<Paint>, Vec<String>), String> {
        let wanted = Wanted {
            object: Some(object_uri),
            ..Wanted::default()
        };
        let report = of_package(self.cache, self.store, &wanted).map_err(|why| why.to_string())?;
        Ok(report
            .rows
            .into_iter()
            .find_map(|row| row.decision)
            .map(|decision| (Some(decision.paint), decision.notes))
            .unwrap_or((None, Vec::new())))
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

pub use energyml_domain::color::{ColourSource, ValueColours};

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

/// The colours of **the categories** of `object` (a property that names a table), `keys` being the keys the caller will
/// ask a colour of: the file's colour map on the table first, **read by exact key**; the palette that stands in is laid
/// **over the keys** ([`energyml_domain::palettes::Palette::over_categories`]) - the colours in turn by the rank of
/// the key, so that two categories never share the colour of the end of a ramp - and the source says so.
///
/// `palette` named: that palette, over the keys, and it wins over the file (the user's choice). A constant colour the
/// file gives the object, or a continuous map, is read as [`value_colours`] reads it.
///
/// # Errors
///
/// [`ViewRefusal::Palette`] for an unknown palette, one of another reading, or no key to colour; the refusal of
/// the colour door, by its words.
pub fn category_colours(
    cx: &ViewContext<'_>,
    view: &str,
    object: Option<&str>,
    palette: Option<&str>,
    keys: &[f64],
) -> Result<ValueColours, ViewRefusal> {
    category_colours_noted(cx, view, object, palette, keys, &mut Vec::new())
}

/// [`category_colours`], and what reading the package's colours said (`notes`: a hue read as a fraction of a turn).
///
/// # Errors
///
/// As [`category_colours`].
pub fn category_colours_noted(
    cx: &ViewContext<'_>,
    view: &str,
    object: Option<&str>,
    palette: Option<&str>,
    keys: &[f64],
    notes: &mut Vec<String>,
) -> Result<ValueColours, ViewRefusal> {
    if let Some(name) = palette {
        let found = palette_named(cx, name, Reading::Categorical, "this one")?;
        return ValueColours::new(None, Reading::Categorical, Some(&found), None, keys, None)
            .map_err(|why| refused(name, why));
    }
    let file = file_paint(cx, view, object, notes)?;
    let stand_in = cx.palettes.default_for(Reading::Categorical);
    ValueColours::new(
        file.as_ref(),
        Reading::Categorical,
        stand_in.as_ref(),
        None,
        keys,
        None,
    )
    .map_err(|why| refused("", why))
}

/// What the package's colours give `object`, with the notes of reading them; `None` for no object, or nothing in the
/// package that colours it.
fn file_paint(
    cx: &ViewContext<'_>,
    view: &str,
    object: Option<&str>,
    notes: &mut Vec<String>,
) -> Result<Option<Paint>, ViewRefusal> {
    let Some(uri) = object else {
        return Ok(None);
    };
    let (found, said) = cx
        .colours
        .paint_noted(uri)
        .map_err(|why| ViewRefusal::computing(view, why))?;
    notes.extend(said);
    Ok(found)
}

/// The refusal of a palette that cannot paint, by the words of the door.
fn refused(asked: &str, why: ColourRefusal) -> ViewRefusal {
    ViewRefusal::Palette {
        asked: asked.to_string(),
        why: why.to_string(),
    }
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
    value_colours_noted(
        cx,
        view,
        object,
        palette,
        bounds,
        logarithmic,
        reading,
        &mut Vec::new(),
    )
}

/// [`value_colours`], and what reading the package's colours said (`notes`: a hue read as a fraction of a turn).
///
/// # Errors
///
/// As [`value_colours`].
#[allow(clippy::too_many_arguments)]
pub fn value_colours_noted(
    cx: &ViewContext<'_>,
    view: &str,
    object: Option<&str>,
    palette: Option<&str>,
    bounds: (f64, f64),
    logarithmic: bool,
    reading: Reading,
    notes: &mut Vec<String>,
) -> Result<ValueColours, ViewRefusal> {
    // A scale of its own only for a palette: a map of the file keeps the scale the file gives it.
    let scaling = Scaling {
        min_max: Some(bounds),
        logarithmic,
        reverse: false,
    };
    if let Some(name) = palette {
        let found = palette_named(cx, name, reading, "this one")?;
        return ValueColours::new(None, reading, Some(&found), Some(bounds), &[], Some(scaling))
            .map_err(|why| refused(name, why));
    }
    let file = file_paint(cx, view, object, notes)?;
    let stand_in = cx.palettes.default_for(reading);
    // Categories the file's map has no entry for take the palette over the extent: this caller knows no key.
    let own = file.is_none().then_some(scaling);
    ValueColours::new(file.as_ref(), reading, stand_in.as_ref(), Some(bounds), &[], own)
        .map_err(|why| refused("", why))
}
