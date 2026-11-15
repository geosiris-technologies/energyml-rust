//! The colour of a value: the file's map first, a palette else, a palette named winning over the file; never an
//! invented colour.

use energyml_domain::color::{ColorMap, Domain, Hsv, Interpolation, Paint, Rgba, Scaling, Stop};
use energyml_domain::palettes::Reading;
use energyml_views::{ColourDoor, ColourSource, ViewRefusal, value_colours};

use crate::fixtures::{NO_MAPS, Opened, package};

/// A door that gives every object a two-stop map, red to blue over 0 to 10.
struct RedToBlue;

impl ColourDoor for RedToBlue {
    fn paint(&self, _object_uri: &str) -> Result<Option<Paint>, String> {
        let stop = |index: f64, r: f64, b: f64| Stop {
            index,
            color: Hsv::from_rgba(Rgba { r, g: 0.0, b, a: 1.0 }),
        };
        Ok(Some(Paint::Mapped {
            map: Box::new(ColorMap {
                uuid: String::new(),
                title: None,
                continuous: Some((Domain::Rgb, Interpolation::Linear)),
                stops: vec![stop(0.0, 1.0, 0.0), stop(10.0, 0.0, 1.0)],
                null_color: None,
                above_max: None,
                below_min: None,
            }),
            scaling: Scaling {
                min_max: None,
                logarithmic: false,
                reverse: false,
            },
            value_vector_index: None,
        }))
    }
}

#[test]
fn without_a_map_the_default_palette_paints_and_says_so() {
    let opened = Opened::of(package(50.0, &[20.0, 80.0]));
    let cx = opened.context(&NO_MAPS);
    let colours = value_colours(
        &cx,
        "test",
        Some("eml:///x"),
        None,
        (0.0, 1.0),
        false,
        Reading::Continuous,
    )
    .expect("colours");
    assert_eq!(colours.source, ColourSource::Palette("Viridis".to_string()));
    // The first colour Viridis publishes, quoted in the palette: #440154.
    assert_eq!(colours.sample(0.0), Some([0x44, 0x01, 0x54, 255]));
    assert_eq!(
        colours.sample(1.0),
        Some([0xfd, 0xe7, 0x25, 255]),
        "the last one at the top of the bounds"
    );
    // Break it: sample the ramp without the bounds, and 1.0 is not the top.
}

#[test]
fn the_files_map_paints_before_any_palette() {
    let opened = Opened::of(package(50.0, &[20.0, 80.0]));
    let cx = opened.context(&RedToBlue);
    let colours = value_colours(
        &cx,
        "test",
        Some("eml:///x"),
        None,
        (0.0, 1.0),
        false,
        Reading::Continuous,
    )
    .expect("colours");
    assert_eq!(colours.source, ColourSource::File);
    assert_eq!(
        colours.sample(0.0),
        Some([255, 0, 0, 255]),
        "red at 0, by the file's own range"
    );
    assert_eq!(
        colours.sample(10.0),
        Some([0, 0, 255, 255]),
        "blue at 10, not at the bounds given"
    );
    // Break it: ask the palettes before the door.
}

#[test]
fn a_palette_named_wins_over_the_file_and_is_read_without_case() {
    let opened = Opened::of(package(50.0, &[20.0, 80.0]));
    let cx = opened.context(&RedToBlue);
    let colours = value_colours(
        &cx,
        "test",
        Some("eml:///x"),
        Some("spectral"),
        (0.0, 1.0),
        false,
        Reading::Continuous,
    )
    .expect("colours");
    assert_eq!(colours.source, ColourSource::Palette("Spectral".to_string()));
    // Break it: ask the door first even when a palette is named.
}

#[test]
fn an_unknown_palette_or_one_of_another_reading_is_refused_by_name() {
    let opened = Opened::of(package(50.0, &[20.0, 80.0]));
    let cx = opened.context(&NO_MAPS);
    let refused = value_colours(
        &cx,
        "test",
        None,
        Some("rainbow"),
        (0.0, 1.0),
        false,
        Reading::Continuous,
    )
    .expect_err("refused");
    assert!(
        matches!(&refused, ViewRefusal::Palette { why, .. } if why.contains("Viridis")),
        "{refused}"
    );
    let refused = value_colours(
        &cx,
        "test",
        None,
        Some("Set2"),
        (0.0, 1.0),
        false,
        Reading::Continuous,
    )
    .expect_err("refused");
    assert!(
        matches!(&refused, ViewRefusal::Palette { why, .. } if why.contains("another reading")),
        "{refused}"
    );
    // Break it: fall back on the default palette for a name that does not exist.
}

#[test]
fn a_value_the_map_has_no_colour_for_is_left_transparent() {
    let opened = Opened::of(package(50.0, &[20.0, 80.0]));
    let cx = opened.context(&NO_MAPS);
    let colours =
        value_colours(&cx, "test", None, None, (0.0, 1.0), false, Reading::Continuous).expect("colours");
    assert_eq!(
        colours.sample(f64::NAN),
        None,
        "a null value takes no colour: none is invented"
    );
    // Break it: sample a NaN as the bottom of the ramp.
}
