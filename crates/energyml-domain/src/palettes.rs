//! The palettes a face offers when the file says no colour map: every list is **quoted** from its source, as
//! published, never sampled from an interpolator at run time. Viridis comes from matplotlib (public domain, CC0), the
//! others from ColorBrewer (Cynthia Brewer, Apache 2.0). The provenance travels with each palette: a face says "not
//! from the file" by it.
//!
//! A palette is for one reading: a continuous ramp laid over a facies draws a progression that does not exist, and
//! qualitative hues laid over a porosity hide the order that does ([`Reading`]).
//!
//! The registry is a registry, not a `match`: the palettes a caller adds are tried **first**, and one of the same
//! name replaces a built-in ([`Palettes::register`]).
//!
//! A palette becomes a map of this crate ([`ColorMap`]), so that a palette and a map the file stores are sampled by
//! **one** rule, [`ColorMap::sample`] ([`Palette::ramp`], [`Palette::over_categories`]).

use crate::color::{ColorMap, Domain, Hsv, Interpolation, Rgba, Stop};

/// What a palette can paint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reading {
    /// A continuous value (a ramp).
    Continuous,
    /// A category (qualitative hues, one per category).
    Categorical,
}

/// Where the colours of a palette are quoted from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// matplotlib (public domain, CC0).
    Matplotlib,
    /// ColorBrewer, by Cynthia Brewer (Apache 2.0).
    ColorBrewer,
}

impl Source {
    /// The name of the source, a proper noun.
    pub fn name(self) -> &'static str {
        match self {
            Source::Matplotlib => "matplotlib",
            Source::ColorBrewer => "ColorBrewer",
        }
    }

    /// The licence of the source, as its SPDX identifier.
    pub fn licence(self) -> &'static str {
        match self {
            Source::Matplotlib => "CC0-1.0",
            Source::ColorBrewer => "Apache-2.0",
        }
    }
}

/// A named palette: its quoted colours, its source and what it can paint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    /// The published name, a proper noun, never translated.
    pub name: &'static str,
    /// Where the colours are quoted from.
    pub source: Source,
    /// What it can paint.
    pub reading: Reading,
    /// The published colours, in the published order, as RGB.
    pub colours: &'static [[u8; 3]],
}

impl Palette {
    /// The colour of the category of rank `rank` (its place among the categories painted, from 0): the colours in
    /// turn, from the first again past the last.
    pub fn category(&self, rank: usize) -> [u8; 3] {
        self.colours[rank % self.colours.len()]
    }

    /// The palette as a **continuous map**: one stop per colour, from 0 to 1 evenly, interpolated linearly **in
    /// RGB** - that is how these palettes are defined, Viridis being already perceptually uniform, and smoothing it
    /// in HSV would distort it.
    ///
    /// The stops say nothing about the values: whoever paints with it says where 0 and 1 are (a
    /// [`crate::color::Scaling`] with `min_max`). The map then goes through [`ColorMap::sample`] like a map the
    /// author stored: same null colour, same clamp, same interpolation.
    pub fn ramp(&self) -> ColorMap {
        let last = self.colours.len().saturating_sub(1);
        let stops = self
            .colours
            .iter()
            .enumerate()
            .map(|(at, rgb)| Stop {
                index: if last == 0 { 0.0 } else { at as f64 / last as f64 },
                color: hsv_of(*rgb),
            })
            .collect();
        ColorMap {
            uuid: String::new(),
            title: Some(self.name.to_string()),
            continuous: Some((Domain::Rgb, Interpolation::Linear)),
            stops,
            null_color: None,
            above_max: None,
            below_min: None,
        }
    }

    /// The palette as a **discrete map** laid over category keys: one stop at each key, the colours in turn by the
    /// rank of the key among the keys ([`Palette::category`]), so that the same key takes the same colour on every
    /// object of a group when the caller gives the union of their tables. No `min_max` goes with it: a key is looked
    /// at, never rescaled.
    ///
    /// What no table names **disappears** rather than taking a neighbour's colour: below the first key and above
    /// the last, transparent; and **between two integer keys that do not follow each other** a transparent stop at
    /// the key after the lower one, since the schema's rule for a discrete map is "the nearest lower entry" (RESQML
    /// TRG 2.2 section 4.21, [`ColorMap::sample`]) and would otherwise paint a value 3 of a table keyed 1, 2, 5 in
    /// the colour of 2.
    ///
    /// `None` when there is no finite key or the palette has no colour: a map with no stop paints nothing.
    pub fn over_categories(&self, keys: &[f64]) -> Option<ColorMap> {
        let mut keys: Vec<f64> = keys.iter().copied().filter(|key| key.is_finite()).collect();
        keys.sort_by(f64::total_cmp);
        keys.dedup();
        if keys.is_empty() || self.colours.is_empty() {
            return None;
        }
        let clear = Hsv::from_rgba(CLEAR);
        let mut stops = Vec::with_capacity(keys.len() * 2);
        for (rank, key) in keys.iter().enumerate() {
            stops.push(Stop {
                index: *key,
                color: hsv_of(self.category(rank)),
            });
            let next = keys.get(rank + 1).copied();
            if key.fract() == 0.0 && next.is_some_and(|next| next > key + 1.0) {
                stops.push(Stop {
                    index: key + 1.0,
                    color: clear.clone(),
                });
            }
        }
        Some(ColorMap {
            uuid: String::new(),
            title: Some(self.name.to_string()),
            continuous: None,
            stops,
            null_color: None,
            above_max: Some(clear.clone()),
            below_min: Some(clear),
        })
    }
}

/// Fully transparent: what is painted where a value names no category.
const CLEAR: Rgba = Rgba {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.0,
};

/// A quoted colour as a stop holds one: HSV, opaque (`Hsv::from_rgba`, whose trip back is exact).
fn hsv_of(rgb: [u8; 3]) -> Hsv {
    Hsv::from_rgba(Rgba {
        r: f64::from(rgb[0]) / 255.0,
        g: f64::from(rgb[1]) / 255.0,
        b: f64::from(rgb[2]) / 255.0,
        a: 1.0,
    })
}

/// The built-in palettes, in order: the first of each reading is its default ([`Palettes::default_for`]).
pub const BUILT_IN: &[Palette] = &[
    // matplotlib, the ten-way discretisation published with the palette.
    Palette {
        name: "Viridis",
        source: Source::Matplotlib,
        reading: Reading::Continuous,
        colours: &[
            [0x44, 0x01, 0x54],
            [0x48, 0x28, 0x78],
            [0x3e, 0x49, 0x89],
            [0x31, 0x68, 0x8e],
            [0x26, 0x82, 0x8e],
            [0x1f, 0x9e, 0x89],
            [0x35, 0xb7, 0x79],
            [0x6e, 0xce, 0x58],
            [0xb5, 0xde, 0x2b],
            [0xfd, 0xe7, 0x25],
        ],
    },
    // ColorBrewer Spectral-11, diverging.
    Palette {
        name: "Spectral",
        source: Source::ColorBrewer,
        reading: Reading::Continuous,
        colours: &[
            [0x9e, 0x01, 0x42],
            [0xd5, 0x3e, 0x4f],
            [0xf4, 0x6d, 0x43],
            [0xfd, 0xae, 0x61],
            [0xfe, 0xe0, 0x8b],
            [0xff, 0xff, 0xbf],
            [0xe6, 0xf5, 0x98],
            [0xab, 0xdd, 0xa4],
            [0x66, 0xc2, 0xa5],
            [0x32, 0x88, 0xbd],
            [0x5e, 0x4f, 0xa2],
        ],
    },
    // ColorBrewer YlGnBu-9, sequential.
    Palette {
        name: "YlGnBu",
        source: Source::ColorBrewer,
        reading: Reading::Continuous,
        colours: &[
            [0xff, 0xff, 0xd9],
            [0xed, 0xf8, 0xb1],
            [0xc7, 0xe9, 0xb4],
            [0x7f, 0xcd, 0xbb],
            [0x41, 0xb6, 0xc4],
            [0x1d, 0x91, 0xc0],
            [0x22, 0x5e, 0xa8],
            [0x25, 0x34, 0x94],
            [0x08, 0x1d, 0x58],
        ],
    },
    // ColorBrewer Set2-8, qualitative.
    Palette {
        name: "Set2",
        source: Source::ColorBrewer,
        reading: Reading::Categorical,
        colours: &[
            [0x66, 0xc2, 0xa5],
            [0xfc, 0x8d, 0x62],
            [0x8d, 0xa0, 0xcb],
            [0xe7, 0x8a, 0xc3],
            [0xa6, 0xd8, 0x54],
            [0xff, 0xd9, 0x2f],
            [0xe5, 0xc4, 0x94],
            [0xb3, 0xb3, 0xb3],
        ],
    },
    // ColorBrewer Paired-12, qualitative, for the long tables.
    Palette {
        name: "Paired",
        source: Source::ColorBrewer,
        reading: Reading::Categorical,
        colours: &[
            [0xa6, 0xce, 0xe3],
            [0x1f, 0x78, 0xb4],
            [0xb2, 0xdf, 0x8a],
            [0x33, 0xa0, 0x2c],
            [0xfb, 0x9a, 0x99],
            [0xe3, 0x1a, 0x1c],
            [0xfd, 0xbf, 0x6f],
            [0xff, 0x7f, 0x00],
            [0xca, 0xb2, 0xd6],
            [0x6a, 0x3d, 0x9a],
            [0xff, 0xff, 0x99],
            [0xb1, 0x59, 0x28],
        ],
    },
];

/// The palettes offered: the caller's first, then the built-ins, one per name.
#[derive(Clone, Debug, Default)]
pub struct Palettes {
    added: Vec<Palette>,
}

impl Palettes {
    /// The built-ins only.
    pub fn new() -> Self {
        Palettes::default()
    }

    /// Add a palette, tried before the built-ins: the same name replaces one, a new name joins the list.
    pub fn register(&mut self, palette: Palette) {
        self.added.insert(0, palette);
    }

    /// Every palette, the caller's first, one per name (the first wins).
    pub fn all(&self) -> Vec<Palette> {
        let mut out: Vec<Palette> = Vec::new();
        for palette in self.added.iter().chain(BUILT_IN) {
            if !out.iter().any(|seen| seen.name == palette.name) {
                out.push(*palette);
            }
        }
        out
    }

    /// The palettes for a reading, in order: never a ramp for a category, never hues for a value.
    pub fn for_reading(&self, reading: Reading) -> Vec<Palette> {
        self.all()
            .into_iter()
            .filter(|palette| palette.reading == reading)
            .collect()
    }

    /// The palette of this name, when there is one. Names are compared without case: `viridis` is `Viridis`.
    pub fn named(&self, name: &str) -> Option<Palette> {
        self.all()
            .into_iter()
            .find(|palette| palette.name.eq_ignore_ascii_case(name))
    }

    /// The default palette of a reading: the first one offered for it (Viridis for a value, Set2 for a category
    /// among the built-ins).
    pub fn default_for(&self, reading: Reading) -> Option<Palette> {
        self.for_reading(reading).into_iter().next()
    }
}
