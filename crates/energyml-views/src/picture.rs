//! The one door to a picture: a request checked, drawn, written as SVG with the request in its metadata
//! ([`draw_svg`]); the request read back and drawn again ([`read_ask`], [`redraw`]); the PNG of a written SVG
//! ([`png`]).

use energyml_domain::package_index::PackageIndex;
use energyml_sheet::draw::{Metadata, svg};

use crate::ask::ViewAsk;
use crate::kind::{Drawn, ObjectSlot, Request, ViewContext, ViewKind, ViewKinds, ViewNote, local_type};
use crate::refusal::ViewRefusal;
use crate::setting::Settled;
use crate::style;

/// The element of a drawing's metadata that holds its request.
pub const METADATA_ELEMENT: &str = "view";
/// Its namespace.
pub const METADATA_NAMESPACE: &str = "urn:energyml:view";
/// The format of its body: a [`ViewAsk`] in JSON. A reader refuses another.
pub const METADATA_FORMAT: u32 = 1;

/// A picture, written.
#[derive(Clone, Debug, PartialEq)]
pub struct Picture {
    /// The SVG.
    pub svg: String,
    /// The request it was drawn from, settings completed with their defaults: what its metadata holds.
    pub ask: ViewAsk,
    /// What the view left out, also written in the picture.
    pub notes: Vec<ViewNote>,
}

/// A request checked against its view: the view, the request it draws from, the request completed.
fn checked<'k>(
    kinds: &'k ViewKinds,
    ask: &ViewAsk,
    index: &PackageIndex,
) -> Result<(&'k dyn ViewKind, Request, ViewAsk), ViewRefusal> {
    let Some(kind) = kinds.find(&ask.view) else {
        return Err(ViewRefusal::UnknownView {
            asked: ask.view.clone(),
            known: kinds.ids().join(", "),
        });
    };
    let settings = Settled::settle(kind.id(), &kind.settings(), &ask.settings)?;
    let objects = fit_objects(kind, &kind.objects(), &ask.objects, index)?;
    let size = ask.size.unwrap_or_else(|| kind.size());
    if !(size[0].is_finite() && size[1].is_finite() && size[0] >= 50.0 && size[1] >= 50.0) {
        return Err(ViewRefusal::BadSetting {
            name: "size".to_string(),
            why: format!("{} x {} pixels: both at least 50", size[0], size[1]),
        });
    }
    let completed = ViewAsk {
        view: kind.id().to_string(),
        objects: objects.iter().map(|(_, uri)| uri.clone()).collect(),
        settings: settings.values().clone(),
        size: Some(size),
    };
    Ok((
        kind,
        Request {
            objects,
            settings,
            size,
        },
        completed,
    ))
}

/// The objects given, each matched to the first slot (in order) that takes its type and has room; every one the
/// package holds, by its full uri; every slot filled to its least.
fn fit_objects(
    kind: &dyn ViewKind,
    slots: &[ObjectSlot],
    given: &[String],
    index: &PackageIndex,
) -> Result<Vec<(&'static str, String)>, ViewRefusal> {
    let mut placed: Vec<(&'static str, String)> = Vec::new();
    for asked in given {
        let Some(node) = index.node(asked.trim()) else {
            return Err(ViewRefusal::NotInPackage {
                object: asked.clone(),
            });
        };
        let local = local_type(&node.qualified_type);
        let slot = slots.iter().find(|slot| {
            slot.types.contains(&local)
                && placed.iter().filter(|(role, _)| *role == slot.role).count() < slot.most
        });
        let Some(slot) = slot else {
            return Err(ViewRefusal::ObjectNotTaken {
                view: kind.id().to_string(),
                object: node.uri.to_string(),
                qualified_type: node.qualified_type.to_string(),
                slots: slots_in_words(slots),
            });
        };
        placed.push((slot.role, node.uri.to_string()));
    }
    for slot in slots {
        let given = placed.iter().filter(|(role, _)| *role == slot.role).count();
        if given < slot.least {
            return Err(ViewRefusal::MissingObject {
                view: kind.id().to_string(),
                role: slot.role.to_string(),
                types: slot.types.join(", "),
                least: slot.least,
                given,
            });
        }
    }
    Ok(placed)
}

fn slots_in_words(slots: &[ObjectSlot]) -> String {
    if slots.is_empty() {
        return "no object".to_string();
    }
    slots
        .iter()
        .map(|slot| format!("{} ({}, at most {})", slot.role, slot.types.join(", "), slot.most))
        .collect::<Vec<_>>()
        .join("; ")
}

/// The display list of a request, and the request completed: for a face that draws the list itself (a window), with
/// the notes written in it as in the SVG.
///
/// # Errors
///
/// The request refused against its view, or the view's own refusal.
pub fn draw_list(
    kinds: &ViewKinds,
    ask: &ViewAsk,
    cx: &ViewContext<'_>,
) -> Result<(Drawn, ViewAsk), ViewRefusal> {
    let (kind, request, completed) = checked(kinds, ask, cx.index)?;
    let mut drawn = kind.draw(&request, cx)?;
    let mut size = drawn.list.size;
    if let Some(layer) = style::notes_layer(&mut size, drawn.list.unit, &drawn.notes) {
        drawn.list.size = size;
        drawn.list.layers.push(layer);
    }
    // The net under every view: a title, a label or a legend name that would leave the page is broken into lines.
    energyml_sheet::draw::text::fit_to_page(&mut drawn.list);
    Ok((drawn, completed))
}

/// **The door to a picture**: the request checked against its view (objects, settings, size), drawn, and written as
/// SVG with the request, completed with its defaults, in the metadata.
///
/// # Errors
///
/// The request refused against its view, the view's own refusal, or the SVG writer's.
pub fn draw_svg(kinds: &ViewKinds, ask: &ViewAsk, cx: &ViewContext<'_>) -> Result<Picture, ViewRefusal> {
    let (drawn, completed) = draw_list(kinds, ask, cx)?;
    let body = serde_json::to_string(&completed).map_err(|why| ViewRefusal::Metadata(why.to_string()))?;
    let metadata = Metadata {
        element: METADATA_ELEMENT.to_string(),
        namespace: METADATA_NAMESPACE.to_string(),
        format: METADATA_FORMAT,
        version: env!("CARGO_PKG_VERSION").to_string(),
        body,
    };
    let svg = svg::write(&drawn.list, Some(&metadata))?;
    Ok(Picture {
        svg,
        ask: completed,
        notes: drawn.notes,
    })
}

/// The request a drawing was made from.
///
/// # Errors
///
/// A drawing with no such metadata, of another format, or whose body does not read.
pub fn read_ask(svg_text: &str) -> Result<ViewAsk, ViewRefusal> {
    let metadata = svg::read_metadata(svg_text, METADATA_ELEMENT)?;
    if metadata.format != METADATA_FORMAT {
        return Err(ViewRefusal::Metadata(format!(
            "format {} where this build reads {METADATA_FORMAT}",
            metadata.format
        )));
    }
    serde_json::from_str(&metadata.body).map_err(|why| ViewRefusal::Metadata(why.to_string()))
}

/// A drawing made again from its own metadata: over the same package, the same bytes.
///
/// # Errors
///
/// What [`read_ask`] and [`draw_svg`] refuse.
pub fn redraw(kinds: &ViewKinds, svg_text: &str, cx: &ViewContext<'_>) -> Result<Picture, ViewRefusal> {
    let ask = read_ask(svg_text)?;
    draw_svg(kinds, &ask, cx)
}

/// The PNG of a written SVG, `scale` pixels per pixel: always the SVG rasterised, never a second drawing.
///
/// # Errors
///
/// Without the `png` feature of this crate, `FeatureNotEnabled` by name; with it, what the rasteriser refuses.
#[cfg(feature = "png")]
pub fn png(svg_text: &str, scale: f32) -> Result<Vec<u8>, ViewRefusal> {
    Ok(energyml_sheet::draw::png::render(svg_text, scale)?)
}

/// Without the `png` feature of this crate: the refusal, by name, even when another crate of the build turned on the
/// sheet's rasteriser (features unify), so a view answers by its own features.
#[cfg(not(feature = "png"))]
pub fn png(svg_text: &str, scale: f32) -> Result<Vec<u8>, ViewRefusal> {
    let _ = (svg_text, scale);
    Err(energyml_sheet::draw::DrawError::FeatureNotEnabled {
        feature: "png",
        what: "render a PNG",
    }
    .into())
}
