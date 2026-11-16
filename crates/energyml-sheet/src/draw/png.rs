//! A view as a PNG: **its SVG, rasterised** - the only way this crate makes a
//! PNG, so a PNG always shows what its SVG shows.
//!
//! # The same picture, not a second drawing
//!
//! One layout, two outputs (ADR-0015): [`render`] takes the SVG text a view
//! exports and draws it with `resvg`; [`render_list`] is exactly
//! `render(&svg::write(list, None)?, scale)`, a shorthand for a face that holds
//! the list. Nothing here composes a picture of its own: the rasters of a view
//! are `<image>` elements of its SVG, drawn by `resvg` like the rest.
//!
//! # The font travels with the code
//!
//! Every label's width was measured against Liberation Sans
//! ([`crate::draw::text`]), so the PNG is rendered with **that** font,
//! embedded. No system font is read: the same picture on every machine.
//!
//! # Images: an embedded PNG, or a refusal
//!
//! `resvg` omits an image it cannot read and says nothing. This module refuses
//! instead (ADR-0005): the image resolver given to `usvg` accepts a PNG
//! embedded as a `data:` URL and nothing else; any other `<image>` is a named
//! refusal (`ImageNotPng`, `ImageNotEmbedded`, `ImageUnreadable`, `Size`).
//! It also means no file is ever read from a path found in an SVG.
//!
//! # Off by default
//!
//! Behind the `png` feature. Without it, both functions refuse by name
//! (`FeatureNotEnabled`): a missing capability is not an empty image.

use crate::draw::list::DisplayList;
use crate::draw::svg::DrawError;
#[cfg(feature = "png")]
use crate::draw::{raster::MAX_RASTER_PIXELS, text};

/// The PNG of an SVG written by `svg::write` (or by any writer that names
/// Liberation Sans first), `scale` times its size in pixels (a `scale` that is
/// not finite or not positive is 1).
///
/// The rasters the SVG embeds are drawn when each is a PNG in a `data:` URL;
/// any other `<image>` is refused by name, never left out of the picture.
#[cfg(feature = "png")]
pub fn render(svg: &str, scale: f32) -> Result<Vec<u8>, DrawError> {
    use resvg::{tiny_skia, usvg};
    use std::sync::{Arc, Mutex};

    let first: Arc<Mutex<Option<DrawError>>> = Arc::new(Mutex::new(None));
    let mut options = usvg::Options {
        font_family: text::FONT_FAMILY.to_string(),
        image_href_resolver: embedded_png_only(&first),
        ..usvg::Options::default()
    };
    {
        let fonts = options.fontdb_mut();
        fonts.load_font_data(text::FONT.to_vec());
        // Every generic family the SVG may fall back on resolves to the same
        // font: Arial and Helvetica share the metrics of Liberation Sans.
        fonts.set_sans_serif_family(text::FONT_FAMILY);
    }
    let tree = usvg::Tree::from_str(svg, &options).map_err(|e| DrawError::Svg(e.to_string()))?;
    // The SVG parses, but one of its images would not have been drawn.
    if let Some(refusal) = take(&first) {
        return Err(refusal);
    }
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let size = tree.size();
    let width = (f64::from(size.width()) * f64::from(scale)).ceil() as u64;
    let height = (f64::from(size.height()) * f64::from(scale)).ceil() as u64;
    if width == 0 || height == 0 || width.saturating_mul(height) > MAX_RASTER_PIXELS {
        return Err(DrawError::Size {
            width,
            height,
            limit: MAX_RASTER_PIXELS,
        });
    }
    let mut pixmap = tiny_skia::Pixmap::new(width as u32, height as u32).ok_or(DrawError::Size {
        width,
        height,
        limit: MAX_RASTER_PIXELS,
    })?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap.encode_png().map_err(|e| DrawError::Encode(e.to_string()))
}

/// Without the `png` feature: the refusal, by name.
#[cfg(not(feature = "png"))]
pub fn render(svg: &str, scale: f32) -> Result<Vec<u8>, DrawError> {
    let _ = (svg, scale);
    Err(DrawError::FeatureNotEnabled {
        feature: "png",
        what: "render a PNG",
    })
}

/// Exactly `render(&svg::write(list, None)?, scale)`: a shorthand for a face
/// that holds the list, never a second drawing (`svg::write` validates the
/// list, so every PNG is of a valid list).
pub fn render_list(list: &DisplayList, scale: f32) -> Result<Vec<u8>, DrawError> {
    render(&crate::draw::svg::write(list, None)?, scale)
}

#[cfg(feature = "png")]
fn take(slot: &std::sync::Mutex<Option<DrawError>>) -> Option<DrawError> {
    slot.lock().unwrap_or_else(|p| p.into_inner()).take()
}

/// Keeps `refusal` in `slot` when it is the first.
#[cfg(feature = "png")]
fn keep(slot: &std::sync::Mutex<Option<DrawError>>, refusal: DrawError) {
    let mut held = slot.lock().unwrap_or_else(|p| p.into_inner());
    if held.is_none() {
        *held = Some(refusal);
    }
}

/// The resolver `usvg` calls for every `<image>`: an embedded PNG that decodes,
/// or a refusal kept in `first` (and `None` returned, so the tree parses and
/// `render` then rejects it).
#[cfg(feature = "png")]
fn embedded_png_only(
    first: &std::sync::Arc<std::sync::Mutex<Option<DrawError>>>,
) -> resvg::usvg::ImageHrefResolver<'static> {
    use resvg::usvg::{ImageHrefResolver, ImageKind};
    use std::sync::Arc;

    let for_data = Arc::clone(first);
    let for_string = Arc::clone(first);
    ImageHrefResolver {
        resolve_data: Box::new(move |mime, data, _options| match check_png(mime, &data) {
            Ok(()) => Some(ImageKind::PNG(data)),
            Err(refusal) => {
                keep(&for_data, refusal);
                None
            }
        }),
        resolve_string: Box::new(move |href, _options| {
            keep(
                &for_string,
                DrawError::ImageNotEmbedded {
                    href: href.chars().take(120).collect(),
                },
            );
            None
        }),
    }
}

/// Whether an embedded image is a PNG that `resvg` will draw, checked in the
/// order: the mime type, the signature, the size in the header (before any
/// decoding: no 64 MB allocation for a header that lies), then the decoder
/// `resvg` itself uses, so an image that passes is drawn and one that fails
/// would have been omitted without a word.
#[cfg(feature = "png")]
fn check_png(mime: &str, data: &[u8]) -> Result<(), DrawError> {
    const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
    if mime != "image/png" {
        return Err(DrawError::ImageNotPng {
            mime: mime.to_string(),
        });
    }
    if data.len() < 24 || &data[..8] != SIGNATURE {
        return Err(DrawError::ImageUnreadable {
            why: "no PNG signature".to_string(),
        });
    }
    // The IHDR chunk holds the width, then the height, big-endian
    // (W3C PNG Specification, Second Edition, IHDR chunk).
    // TODO(verify): the section number of the IHDR chunk in the W3C text.
    let width = u64::from(u32::from_be_bytes([data[16], data[17], data[18], data[19]]));
    let height = u64::from(u32::from_be_bytes([data[20], data[21], data[22], data[23]]));
    if width == 0 || height == 0 || width.saturating_mul(height) > MAX_RASTER_PIXELS {
        return Err(DrawError::Size {
            width,
            height,
            limit: MAX_RASTER_PIXELS,
        });
    }
    resvg::tiny_skia::Pixmap::decode_png(data)
        .map(|_| ())
        .map_err(|e| DrawError::ImageUnreadable { why: e.to_string() })
}
