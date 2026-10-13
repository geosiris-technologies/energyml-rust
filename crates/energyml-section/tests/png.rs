//! The PNG export (`png::render`): the SVG rasterised with the embedded font,
//! or - without the `png` feature - a refusal that says how to get it.

use energyml_section::model::{Item, SectionKey, Side, Target, View};
use energyml_section::png;

use crate::layout::{catalogue, trace, well};

fn svg() -> String {
    let cat = catalogue(vec![
        Item::Well(well("A", 150.0, 12.0, Side::Left)),
        Item::Trace(trace("H", vec![vec![[0.0, -100.0], [1000.0, -100.0]]])),
    ]);
    energyml_section::draw(
        &cat,
        &View::new(
            Target::Section {
                key: SectionKey::Long,
            },
            [600.0, 300.0],
        ),
    )
}

#[cfg(feature = "png")]
#[test]
fn the_png_is_the_svg_at_the_scale_asked() {
    let bytes = png::render(&svg(), 2.0).expect("renders");
    // The PNG signature, then IHDR's width and height (big-endian): the size the
    // SVG says, twice.
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    assert_eq!((width, height), (1200, 600));
}

#[cfg(feature = "png")]
#[test]
fn the_labels_are_drawn_with_the_embedded_font() {
    // A picture whose text is drawn has dark pixels where an SVG without its
    // font would leave the background: compare with and without the labels.
    let with = png::render(&svg(), 1.0).unwrap();
    let without = png::render(&svg().replace("<text", "<g").replace("</text>", "</g>"), 1.0).unwrap();
    assert_ne!(with, without, "the text changed nothing: no font drew it");
}

#[cfg(not(feature = "png"))]
#[test]
fn without_the_feature_the_refusal_says_how_to_get_it() {
    let refused = png::render(&svg(), 1.0).unwrap_err();
    assert_eq!(refused, png::PngError::NotBuilt);
    assert!(refused.to_string().contains("--features png"));
}
