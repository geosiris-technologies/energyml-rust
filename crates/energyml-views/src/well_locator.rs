//! `well-locator`: a well laid flat, and a mark at a measured depth.
//!
//! The well's line comes from `energyml_geometry::around_well::projection` ([`locator_line`], refined until the MD
//! read back along it is right), in one of the projections of [`WellProjections`] or a vertical section; the
//! interval a frame carries data over is brought forward ([`locator_spans`]); the mark at a depth is
//! [`locator_mark`]. This module only lays them out.

use energyml_core::value::FieldValue;
use energyml_domain::md_extent::{self, ExtentReaders};
use energyml_domain::package_index::PackageIndex;
use energyml_geometry::around_well::projection::{
    LocatorLine, Plan, VerticalSection, WellAxis, WellProjection, WellProjections, locator_line,
    locator_mark, locator_spans,
};
use energyml_geometry::well::{MdRange, PlacedPath};
use energyml_sheet::draw::{Anchor, DisplayList, Element, Item, Layer, PageUnit, Rect, Stroke, SymbolKind};
use serde_json::Value;

use crate::kind::{
    Drawn, ObjectSlot, Offer, Request, ViewContext, ViewKind, ViewNote, local_type, offers_of_types,
    title_or_uri,
};
use crate::refusal::ViewRefusal;
use crate::setting::ViewSetting;
use crate::style::{self, PlotFrame, short};

/// The id of the view.
pub const ID: &str = "well-locator";

/// The types it shows: a frame (through its trajectory) or a trajectory.
pub const TYPES: &[&str] = &["WellboreFrameRepresentation", "WellboreTrajectoryRepresentation"];

/// The projections it offers, the first the default.
pub const MODES: &[&str] = &["md_tvd", "md_bar", "developed", "plan", "vertical_section"];

/// The view.
pub struct WellLocator;

impl ViewKind for WellLocator {
    fn id(&self) -> &'static str {
        ID
    }

    fn name(&self) -> &'static str {
        "Well locator"
    }

    fn about(&self) -> &'static str {
        "a well laid flat - MD bar, MD against TVD, developed, plan or vertical section - with the interval a frame carries data over, and a mark at a measured depth"
    }

    fn objects(&self) -> Vec<ObjectSlot> {
        vec![ObjectSlot {
            role: "well",
            types: TYPES,
            least: 1,
            most: 1,
            about: "a wellbore frame (drawn along its trajectory) or a wellbore trajectory",
        }]
    }

    fn settings(&self) -> Vec<ViewSetting> {
        vec![
            ViewSetting::choice("mode", MODES, "how the well is laid flat").default(Value::from(MODES[0])),
            ViewSetting::number(
                "md",
                "the measured depth marked, in the well's MD unit; none: no mark",
            ),
            ViewSetting::number(
                "azimuth",
                "for `vertical_section`: the azimuth of the section plane, degrees clockwise from grid north",
            )
            .uom("dega")
            .bounds(Some(0.0), Some(360.0)),
            ViewSetting::number(
                "step",
                "the spacing of the samples along the hole before refining, in the MD unit",
            )
            .bounds(Some(0.001), None)
            .default(Value::from(10.0)),
        ]
    }

    fn size(&self) -> [f64; 2] {
        [420.0, 640.0]
    }

    fn offers(&self, index: &PackageIndex) -> Vec<Offer> {
        offers_of_types(index, ID, TYPES, |_| true)
    }

    fn draw(&self, request: &Request, cx: &ViewContext<'_>) -> Result<Drawn, ViewRefusal> {
        let refuse = |why: String| ViewRefusal::computing(ID, why);
        let uri = request
            .object("well")
            .ok_or_else(|| refuse("no well given".to_string()))?;
        let object = cx.source.read_object_at(uri).map_err(refuse)?;
        let is_frame = local_type(object.as_reflect().type_info().name) == "WellboreFrameRepresentation";
        let mut notes = Vec::new();

        // The trajectory: the object, or the one the frame names.
        let trajectory = if is_frame {
            match object.as_reflect().field("Trajectory") {
                Some(FieldValue::Object(reference)) => cx.source.read_object_of(reference).map_err(refuse)?,
                _ => return Err(refuse("the frame names no trajectory".to_string())),
            }
        } else {
            object.clone_box()
        };
        let path =
            PlacedPath::of(trajectory.as_reflect(), cx.source).map_err(|why| refuse(why.to_string()))?;
        let extent = path.md_extent();
        let md_uom = path.md_uom().to_string();
        if extent[0] > 0.0 {
            notes.push(ViewNote {
                key: "head-portion-not-drawn",
                says: format!(
                    "the hole from the MD datum down to the first point of the trajectory ({} {md_uom}) is not drawn",
                    short(extent[0])
                ),
            });
        }

        // The projection: one of the registry, or a vertical section through the head.
        let settings = &request.settings;
        let mode = settings.text("mode").unwrap_or(MODES[0]);
        let step = settings.number("step").unwrap_or(10.0);
        let axis = WellAxis::new(&path, extent, step).map_err(|why| refuse(why.to_string()))?;
        let standard = WellProjections::standard();
        let section;
        let projection: &dyn WellProjection = if mode == "vertical_section" {
            let Some(azimuth) = settings.number("azimuth") else {
                return Err(ViewRefusal::MissingSetting {
                    view: ID.to_string(),
                    name: "azimuth".to_string(),
                    about: "a vertical section needs the azimuth of its plane".to_string(),
                });
            };
            let origin = Plan
                .place(&axis, extent[0], 0.0)
                .map_err(|why| refuse(why.to_string()))?;
            section = VerticalSection {
                azimuth: azimuth.to_radians(),
                origin,
            };
            &section
        } else {
            standard
                .get(mode)
                .ok_or_else(|| refuse(format!("no projection `{mode}`")))?
        };
        let span = (extent[1] - extent[0]).abs().max(1.0);
        let line = locator_line(&axis, projection, span * 1.0e-4).map_err(|why| refuse(why.to_string()))?;

        // What the frame carries data over.
        let mut highlighted = Vec::new();
        if is_frame {
            match md_extent::of_object(object.as_reflect(), cx.source, &ExtentReaders::builtin()) {
                Ok(frame) => {
                    let (from, to) = (frame.from.min(frame.to), frame.from.max(frame.to));
                    highlighted = locator_spans(&line, &[[from, to]]);
                }
                Err(why) => notes.push(ViewNote {
                    key: "frame-extent-not-read",
                    says: format!("the interval the frame covers is not brought forward: {why}"),
                }),
            }
        }

        // The mark.
        let mark = match settings.number("md") {
            None => None,
            Some(md) => {
                let mark = locator_mark(&axis, projection, md).map_err(|why| refuse(why.to_string()))?;
                match mark.range {
                    MdRange::Within => {}
                    MdRange::BeforeFirst { first } => notes.push(ViewNote {
                        key: "mark-clamped",
                        says: format!(
                            "MD {} is above the first point ({}): marked there",
                            short(md),
                            short(first)
                        ),
                    }),
                    // `locator_mark` refuses a NaN before: never reached, refused all the same.
                    MdRange::NotANumber => return Err(refuse("the MD asked is not a number".to_string())),
                    MdRange::AfterLast { last } => notes.push(ViewNote {
                        key: "mark-clamped",
                        says: format!(
                            "MD {} is below the last point ({}): marked there",
                            short(md),
                            short(last)
                        ),
                    }),
                }
                Some(mark)
            }
        };

        let title = title_or_uri(cx.index, uri);
        let list = picture(
            request.size,
            mode,
            &md_uom,
            settings.number("azimuth"),
            &line,
            &highlighted,
            mark.map(|m| (m.at, m.normal, m.md)),
            uri,
            &title,
        );
        Ok(Drawn { list, notes })
    }
}

/// The axis titles, which way y runs, and whether both axes share one scale, for a mode.
fn axes_of(mode: &str, md_uom: &str, azimuth: Option<f64>) -> (String, String, bool, bool) {
    match mode {
        "md_bar" => (String::new(), format!("MD ({md_uom})"), true, false),
        "md_tvd" => (
            format!("MD ({md_uom})"),
            "TVD below the MD datum (m)".to_string(),
            true,
            false,
        ),
        "developed" => (
            "length along the plan (m)".to_string(),
            "TVD below the MD datum (m)".to_string(),
            true,
            true,
        ),
        "plan" => ("easting (m)".to_string(), "northing (m)".to_string(), false, true),
        _ => (
            format!("along the azimuth {}° (m)", short(azimuth.unwrap_or(0.0))),
            "TVD below the MD datum (m)".to_string(),
            true,
            true,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn picture(
    size: [f64; 2],
    mode: &str,
    md_uom: &str,
    azimuth: Option<f64>,
    line: &LocatorLine,
    highlighted: &[Vec<[f64; 2]>],
    mark: Option<([f64; 2], Option<[f64; 2]>, f64)>,
    uri: &str,
    title: &str,
) -> DisplayList {
    let (x_title, y_title, y_down, equal) = axes_of(mode, md_uom, azimuth);
    let area = Rect {
        x: 64.0,
        y: 40.0,
        w: size[0] - 64.0 - 24.0,
        h: size[1] - 40.0 - 52.0,
    };
    let bounds = |pick: usize| {
        let values = line.points.iter().map(|p| p[pick]);
        let low = values.clone().fold(f64::INFINITY, f64::min);
        let high = values.fold(f64::NEG_INFINITY, f64::max);
        [low, high]
    };
    let frame = PlotFrame::new(area, bounds(0), bounds(1), y_down, equal);

    let mut list = DisplayList::new(size, PageUnit::Pixel);
    list.title = format!("{ID} - {mode}");
    list.background = Some(style::PAPER);
    let mut header = Layer::new("title");
    header.elements.push(Element::new(vec![style::label_item(
        [area.x, 24.0],
        format!("{title} - {mode}"),
        style::text(style::TITLE, style::INK, Anchor::Start),
    )]));
    list.layers.push(header);
    list.layers.push(style::axes_layer(&frame, &x_title, &y_title));

    let mut well = Layer::new("well");
    well.clip = Some(area);
    let placed: Vec<[f64; 2]> = line.points.iter().map(|p| frame.place(*p)).collect();
    well.elements.push(Element::keyed(
        uri.to_string(),
        "well",
        vec![Item::Polyline {
            points: placed,
            stroke: Stroke::solid(style::SUBJECT, 5.0),
        }],
    ));
    for piece in highlighted {
        well.elements.push(Element::keyed(
            format!("{uri}#data"),
            "span",
            vec![Item::Polyline {
                points: piece.iter().map(|p| frame.place(*p)).collect(),
                stroke: Stroke::solid(style::HIGHLIGHT, 7.0),
            }],
        ));
    }
    list.layers.push(well);

    if let Some((at, normal, md)) = mark {
        let mut layer = Layer::new("mark");
        let centre = frame.place(at);
        // The normal is `[-t1, t0]` of the tangent `t` in data units. On the page the tangent is `[t0 sx, t1 sy]`
        // (the scales of the two axes, signed), so its perpendicular is `[n0 sy, n1 sx]`.
        let sx = (frame.x.page[1] - frame.x.page[0]) / (frame.x.data[1] - frame.x.data[0]);
        let sy = (frame.y.page[1] - frame.y.page[0]) / (frame.y.data[1] - frame.y.data[0]);
        let across = normal.and_then(|n| {
            let page = [n[0] * sy, n[1] * sx];
            let length = page[0].hypot(page[1]);
            (length.is_finite() && length > 0.0).then(|| [page[0] / length, page[1] / length])
        });
        let mut items = Vec::new();
        if let Some(across) = across {
            items.push(Item::Polyline {
                points: vec![
                    [centre[0] - across[0] * 14.0, centre[1] - across[1] * 14.0],
                    [centre[0] + across[0] * 14.0, centre[1] + across[1] * 14.0],
                ],
                stroke: Stroke::solid(style::MARK, 2.5),
            });
        }
        items.push(Item::Symbol {
            at: centre,
            kind: SymbolKind::Point,
            size: 9.0,
            fill: Some(style::MARK),
            stroke: style::MARK,
        });
        items.push(style::label_item(
            [centre[0] + 10.0, centre[1] - 8.0],
            format!("MD {} {md_uom}", short(md)),
            style::text(style::TEXT, style::MARK, Anchor::Start),
        ));
        layer
            .elements
            .push(Element::keyed(format!("{uri}#md={}", short(md)), "mark", items));
        list.layers.push(layer);
    }
    list
}
