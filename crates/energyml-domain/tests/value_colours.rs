//! `ValueColours`: the one door from a value to a colour (ADR 0056).
//!
//! Every map is built here, by hand. The expected colours are worked from the map's own stops - hue 0 is red, 120 green,
//! 240 blue - and from the palettes' quoted colours (Set2's fourth is `#e78ac3`, Viridis starts at `#440154` and ends at
//! `#fde725`).
//!
//! What it would take to make each one fail: reading a category by the nearest lower entry, applying a caller's extent
//! to a file's map, dropping the file's own scale, giving a missing value a colour, or taking a palette for the file's.

use energyml_domain::color::{
    ColorMap, ColourRefusal, ColourSource, Domain, Hsv, Interpolation, Paint, Rgba, Scaling, Stop,
    ValueColours,
};
use energyml_domain::palettes::{Palette, Palettes, Reading};

fn hsv(hue: f64) -> Hsv {
    Hsv {
        hue,
        saturation: 1.0,
        value: 1.0,
        alpha: 1.0,
        title: None,
    }
}

fn map(continuous: bool, stops: &[(f64, f64)], null: Option<f64>) -> ColorMap {
    ColorMap {
        uuid: "f0000000-0000-4000-8000-000000000001".to_string(),
        title: None,
        continuous: continuous.then_some((Domain::Hsv, Interpolation::Linear)),
        stops: stops
            .iter()
            .map(|(index, hue)| Stop {
                index: *index,
                color: hsv(*hue),
            })
            .collect(),
        null_color: null.map(hsv),
        above_max: None,
        below_min: None,
    }
}

fn mapped(map: ColorMap, scaling: Scaling) -> Paint {
    Paint::Mapped {
        map: Box::new(map),
        scaling,
        value_vector_index: None,
    }
}

fn set2() -> Palette {
    Palettes::new().named("Set2").expect("a category palette")
}

fn hex(colour: Option<Rgba>) -> Option<String> {
    colour.map(Rgba::to_hex)
}

/// A discrete map of the file read as categories is read **by key**: 20 has its entry, 25 has none and takes nothing of 20's;
/// a key the map lacks (40) takes the palette laid over the keys - Set2's colour of its rank, the fourth - and the source
/// says the palette is not the file's.
///
/// Break it: in `ValueColours::colour`, answer a `ByKey` painting with `map.sample` (the nearest lower entry).
#[test]
fn a_category_is_read_by_its_key_and_a_missing_key_takes_the_palette() {
    let file = mapped(
        map(false, &[(10.0, 0.0), (20.0, 120.0), (35.0, 240.0)], None),
        Scaling::default(),
    );
    let palette = set2();
    let colours = ValueColours::new(
        Some(&file),
        Reading::Categorical,
        Some(&palette),
        None,
        &[10.0, 20.0, 35.0, 40.0],
        None,
    )
    .expect("colours");
    assert_eq!(
        hex(colours.colour(Some(20.0))).as_deref(),
        Some("#00ff00ff"),
        "its own entry"
    );
    assert_eq!(
        hex(colours.colour(Some(40.0))).as_deref(),
        Some("#e78ac3ff"),
        "Set2's fourth colour"
    );
    assert_eq!(
        colours.colour(Some(25.0)),
        None,
        "25 is no key, and takes nothing of 20"
    );
    assert_eq!(
        colours.source(),
        &ColourSource::FileAndPalette("Set2".to_string())
    );
    assert!(colours.source().says().contains("not from the file"));

    // With no palette to stand in, the key has no colour and the source is the file's alone.
    let alone = ValueColours::new(Some(&file), Reading::Categorical, None, None, &[], None).expect("colours");
    assert_eq!(alone.colour(Some(40.0)), None);
    assert_eq!(alone.source(), &ColourSource::File);
}

/// A continuous map with `MinMax` places the value on **the property's range**: what lies outside takes the extreme
/// colour of the map, red below and blue above.
///
/// Break it: in `ValueColours::new`, drop the scaling the file gives (`Scaling::default()` for a file's map).
#[test]
fn a_measure_with_min_max_is_clamped_to_the_extreme_colours() {
    let scaling = Scaling {
        min_max: Some((100.0, 200.0)),
        ..Scaling::default()
    };
    let file = mapped(map(true, &[(0.0, 0.0), (1.0, 240.0)], None), scaling);
    let colours =
        ValueColours::new(Some(&file), Reading::Continuous, None, None, &[], None).expect("colours");
    assert_eq!(
        hex(colours.colour(Some(50.0))).as_deref(),
        Some("#ff0000ff"),
        "below the range: the first colour"
    );
    assert_eq!(
        hex(colours.colour(Some(500.0))).as_deref(),
        Some("#0000ffff"),
        "above it: the last"
    );
    assert_eq!(colours.source(), &ColourSource::File);
}

/// Without `MinMax`, the map's own range of indices stands for the range of the values (ADR 0025) - and **the extent a
/// caller asks for is not applied to a file's map**: the colour of 15 on a map from 10 to 20 is the middle one whatever
/// the extent.
///
/// Break it: in `ValueColours::new`, give the file's map the caller's extent as its `min_max`.
#[test]
fn a_measure_without_min_max_keeps_the_range_of_its_indices_whatever_the_extent() {
    let file = mapped(map(true, &[(10.0, 0.0), (20.0, 240.0)], None), Scaling::default());
    let own = ValueColours::new(Some(&file), Reading::Continuous, None, None, &[], None).expect("colours");
    let asked = ValueColours::new(
        Some(&file),
        Reading::Continuous,
        None,
        Some((0.0, 1000.0)),
        &[],
        None,
    )
    .expect("colours");
    assert_eq!(
        hex(own.colour(Some(5.0))).as_deref(),
        Some("#ff0000ff"),
        "below the first index"
    );
    assert_eq!(
        hex(own.colour(Some(25.0))).as_deref(),
        Some("#0000ffff"),
        "above the last"
    );
    assert_eq!(
        own.colour(Some(15.0)),
        asked.colour(Some(15.0)),
        "the extent is not the file's scale"
    );
    // Half way from hue 0 to hue 240, in HSV: hue 120, green.
    assert_eq!(hex(own.colour(Some(15.0))).as_deref(), Some("#00ff00ff"));
}

/// A palette that stands in for a measure lays its colours on **the extent the caller gives** - it has no scale of its own
/// - and refuses to paint without one; the source says it is a palette.
///
/// Break it: in `ValueColours::new`, fall back on `(0.0, 1.0)` when there is no extent.
#[test]
fn a_continuous_palette_takes_the_callers_extent_and_needs_one() {
    let viridis = Palettes::new().named("Viridis").expect("a continuous palette");
    let colours = ValueColours::new(
        None,
        Reading::Continuous,
        Some(&viridis),
        Some((0.0, 10.0)),
        &[],
        None,
    )
    .expect("colours");
    assert_eq!(hex(colours.colour(Some(0.0))).as_deref(), Some("#440154ff"));
    assert_eq!(hex(colours.colour(Some(10.0))).as_deref(), Some("#fde725ff"));
    assert_eq!(colours.source(), &ColourSource::Palette("Viridis".to_string()));

    let refused = ValueColours::new(None, Reading::Continuous, Some(&viridis), None, &[], None)
        .expect_err("no extent, no scale");
    assert_eq!(refused, ColourRefusal::NeedsExtent { palette: "Viridis" });
    let nothing = ValueColours::new(None, Reading::Continuous, None, Some((0.0, 1.0)), &[], None)
        .expect_err("no colour at all");
    assert_eq!(nothing, ColourRefusal::NoColours);
}

/// A missing value takes the map's `NullColor` and nothing else - no colour is invented -, a constant colour colours every
/// value, a missing one included - and a number that is no number is a missing value.
///
/// Break it: in `ValueColours::colour`, sample a `None` as the first stop.
#[test]
fn a_missing_value_takes_the_null_colour_or_nothing() {
    let with_null = mapped(
        map(true, &[(0.0, 0.0), (1.0, 240.0)], Some(120.0)),
        Scaling::default(),
    );
    let without = mapped(map(true, &[(0.0, 0.0), (1.0, 240.0)], None), Scaling::default());
    let by_key = mapped(map(false, &[(1.0, 0.0)], Some(120.0)), Scaling::default());
    let read = |paint: &Paint, reading| {
        ValueColours::new(Some(paint), reading, None, None, &[], None).expect("colours")
    };

    assert_eq!(
        hex(read(&with_null, Reading::Continuous).colour(None)).as_deref(),
        Some("#00ff00ff")
    );
    assert_eq!(
        hex(read(&with_null, Reading::Continuous).colour(Some(f64::NAN))).as_deref(),
        Some("#00ff00ff")
    );
    assert_eq!(read(&without, Reading::Continuous).colour(None), None);
    assert_eq!(
        hex(read(&by_key, Reading::Categorical).colour(None)).as_deref(),
        Some("#00ff00ff")
    );

    let constant = Paint::Constant(hsv(240.0).to_rgba());
    let always = read(&constant, Reading::Continuous);
    assert_eq!(hex(always.colour(None)).as_deref(), Some("#0000ffff"));
    assert_eq!(hex(always.colour(Some(3.0))).as_deref(), Some("#0000ffff"));
}
