//! A section view as a PNG - the SVG rendered in Rust (the font is
//! embedded, and `resvg` is an optional dependency whose absence returns an
//! error saying it has to be installed).
//!
//! # The same picture, not a second drawing
//!
//! The PNG is **the SVG of [`crate::draw`], rasterised** - never a layout drawn
//! again for pixels. Whatever the SVG says, the PNG shows; one drawer still
//! (ADR-0015).
//!
//! # The font travels with the code
//!
//! Every label's width was measured against Liberation Sans 2.1.5
//! (`energyml_sheet::text`), so the PNG is rendered with **that** font, embedded
//! (`fonts/LiberationSans-Regular.ttf`, SIL Open Font License 1.1 - its text is
//! `fonts/LiberationSans-OFL.txt`, which has to travel with the font). No
//! system font is read: a machine without Arial, a server without any font,
//! draws the same picture - and a label never comes out wider than the room
//! the layout gave it.
//!
//! # Off by default
//!
//! Behind the `png` feature. Without it, [`render`] refuses **by name** and
//! says how to get it: a missing capability is not an empty image (ADR-0005:
//! an absence and a mistake are not the same news).

/// Why a PNG could not be made.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PngError {
    /// This build was made without the `png` feature.
    #[error(
        "this build cannot write a PNG: it was built without the `png` feature of energyml-section \
         (rebuild with `--features png`, or the CLI with `--features png`)"
    )]
    NotBuilt,
    /// The SVG did not parse - a defect of the writer, said as such.
    #[error("the section view's SVG did not parse: {0}")]
    Svg(String),
    /// A size of zero, or too large to allocate.
    #[error("cannot make an image of {0} × {1} pixels")]
    Size(u32, u32),
    /// The PNG encoder refused.
    #[error("encoding the PNG: {0}")]
    Encode(String),
}

/// The embedded font, by the name the SVG asks for first.
#[cfg(feature = "png")]
const FONT: &[u8] = include_bytes!("../fonts/LiberationSans-Regular.ttf");

/// The family the SVG names first (`svg::write`'s `font-family`).
#[cfg(feature = "png")]
const FAMILY: &str = "Liberation Sans";

/// The PNG of an SVG written by [`crate::svg::write`], `scale` times its size in
/// pixels (2 for a sharp picture on a phone or a high-density screen).
#[cfg(feature = "png")]
pub fn render(svg: &str, scale: f32) -> Result<Vec<u8>, PngError> {
    use resvg::{tiny_skia, usvg};

    let mut options = usvg::Options {
        font_family: FAMILY.to_string(),
        ..usvg::Options::default()
    };
    {
        let fonts = options.fontdb_mut();
        fonts.load_font_data(FONT.to_vec());
        // Every generic family the SVG may fall back on resolves to the same
        // font: Arial and Helvetica are named after Liberation Sans in the
        // SVG, and they share its metrics.
        fonts.set_sans_serif_family(FAMILY);
    }
    let tree = usvg::Tree::from_str(svg, &options).map_err(|e| PngError::Svg(e.to_string()))?;
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let size = tree.size();
    let width = (size.width() * scale).ceil() as u32;
    let height = (size.height() * scale).ceil() as u32;
    let mut pixmap = tiny_skia::Pixmap::new(width, height).ok_or(PngError::Size(width, height))?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap.encode_png().map_err(|e| PngError::Encode(e.to_string()))
}

/// Without the `png` feature: the refusal, by name.
#[cfg(not(feature = "png"))]
pub fn render(svg: &str, scale: f32) -> Result<Vec<u8>, PngError> {
    let _ = (svg, scale);
    Err(PngError::NotBuilt)
}
