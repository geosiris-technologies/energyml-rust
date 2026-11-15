//! `image-track`: a property with several values per element, one matrix row per depth, as an image.
//!
//! The values come from [`read_plane`] (one plane of the matrix: the dimension shown across, the others fixed or
//! reduced by a [`ComponentPick`]); the depth of each row from the frame the property sits on
//! ([`strata_by_md::node_mds`]); the columns' positions from [`AxisSources`]; the scale from
//! [`energyml_domain::stats::scale`]; the rows and columns of pixels from `energyml_sheet::draw::raster`
//! ([`node_covers`] or [`interval_covers`], [`rows_for`], [`columns_for`], [`paint_grid`]); the colours from
//! [`value_colours`], one door. This module only lays them out: the MD axis down the left, the image, the
//! components along the top of the axis below it, a colour bar on the right.

use std::sync::Arc;

use energyml_domain::along_md::GapRule;
use energyml_domain::component_axes::{AxisSources, AxisSubject, AxisValues};
use energyml_domain::package_index::{NodeState, PackageIndex};
use energyml_domain::palettes::Reading;
use energyml_domain::property::shape::{ComponentPick, ElementShape, Reduction};
use energyml_domain::property::{self, IndexableElement};
use energyml_domain::series::matrix::{MatrixNote, read_plane};
use energyml_domain::stats::quantile::QuantileMethod;
use energyml_domain::stats::scale::{
    self, Fraction, ScaleKind, ScaleSpec, auto_bounds, propose_log, resolve,
};
use energyml_domain::strata_by_md;
use energyml_sheet::draw::raster::{
    ColumnMap, Cover, GridView, RowMap, RowPick, RowReduces, columns_for, interval_covers, node_covers,
    paint_grid, rows_for,
};
use energyml_sheet::draw::ticks::{label, log_label, log_ticks, nice_ticks};
use energyml_sheet::draw::{Anchor, Colour, DisplayList, Element, Item, Layer, PageUnit, Paint, Rect};
use serde_json::Value;

use crate::colour::value_colours;
use crate::kind::{Drawn, ObjectSlot, Offer, Request, ViewContext, ViewKind, ViewNote, local_type};
use crate::refusal::ViewRefusal;
use crate::setting::ViewSetting;
use crate::style::{self, PlotFrame};

/// The id of the view.
pub const ID: &str = "image-track";

/// The types it shows.
pub const TYPES: &[&str] = &["ContinuousProperty", "DiscreteProperty"];

/// The scales it offers, the first the default.
pub const SCALES: &[&str] = &["auto", "linear", "log"];

/// The reductions of a pixel row that holds several samples: the ids of `RowReduces::with_builtins`.
pub const REDUCES: &[&str] = &["nearest", "mean", "min", "max"];

/// The frame type the property must sit on.
const FRAME: &str = "WellboreFrameRepresentation";

/// Room left of the plot (MD axis), right of it (colour bar), above (title) and below (axis, legend), pixels.
const LEFT: f64 = 70.0;
const RIGHT: f64 = 110.0;
const TOP: f64 = 44.0;
const BOTTOM: f64 = 56.0;

/// The bands a colour bar is cut in.
const BANDS: usize = 100;

/// The view.
pub struct ImageTrack;

impl ViewKind for ImageTrack {
    fn id(&self) -> &'static str {
        ID
    }

    fn about(&self) -> &'static str {
        "a property with several values per element on a wellbore frame as an image: depth down, one component of the matrix across, the value as a colour"
    }

    fn objects(&self) -> Vec<ObjectSlot> {
        vec![ObjectSlot {
            role: "property",
            types: TYPES,
            least: 1,
            most: 1,
            about: "a property with several values per element, on the nodes or the intervals of a wellbore frame",
        }]
    }

    fn settings(&self) -> Vec<ViewSetting> {
        vec![
            ViewSetting::json(
                "pick",
                "the plane shown: \"all\", {\"along\": axis, \"at\": [index per dimension]} (optionally \"reduced\": [dimensions], \"by\": \"mean\"|\"min\"|\"max\"|\"norm\"), {\"at\": [indices]} or {\"reduce\": id}; none: the first index of every other dimension",
            ),
            ViewSetting::number(
                "md_from",
                "the top of the depth window, in the frame's MD unit; none: the first sample",
            ),
            ViewSetting::number(
                "md_to",
                "the bottom of the depth window, in the frame's MD unit; none: the last sample",
            ),
            ViewSetting::choice(
                "scale",
                SCALES,
                "linear or logarithmic; auto: logarithmic when the values span several decades",
            )
            .default(Value::from(SCALES[0])),
            ViewSetting::number(
                "min",
                "the lower bound of the value scale; none: the 1st percentile of the plane",
            ),
            ViewSetting::number(
                "max",
                "the upper bound of the value scale; none: the 99th percentile of the plane",
            ),
            ViewSetting::text(
                "palette",
                "the palette to paint with, by name; none: the file's colour map, else a default",
            ),
            ViewSetting::choice(
                "reduce",
                REDUCES,
                "how a pixel row that holds several samples shows one value",
            )
            .default(Value::from(REDUCES[0])),
        ]
    }

    fn size(&self) -> [f64; 2] {
        [560.0, 640.0]
    }

    fn offers(&self, index: &PackageIndex) -> Vec<Offer> {
        index
            .nodes()
            .filter(|node| {
                node.state == NodeState::Present && TYPES.contains(&local_type(&node.qualified_type))
            })
            .filter(|node| {
                let Some(sheet) = index.sheet(&node.uri) else {
                    return false;
                };
                let on_a_frame = sheet
                    .supporting_representation
                    .as_deref()
                    .and_then(|uuid| index.node(uuid))
                    .is_some_and(|support| &*support.type_key == FRAME);
                sheet.values_per_element() > 1
                    && sheet.lookup.is_none()
                    && matches!(
                        sheet.indexable,
                        IndexableElement::Nodes | IndexableElement::Intervals
                    )
                    && on_a_frame
            })
            .map(|node| Offer {
                view: ID,
                objects: vec![node.uri.to_string()],
                title: node.title.as_deref().unwrap_or(&node.uuid).to_string(),
                missing: Vec::new(),
            })
            .collect()
    }

    fn draw(&self, request: &Request, cx: &ViewContext<'_>) -> Result<Drawn, ViewRefusal> {
        let refuse = |why: String| ViewRefusal::computing(ID, why);
        let uri = request
            .object("property")
            .ok_or_else(|| refuse("no property given".to_string()))?;
        let object = cx.source.read_object_at(uri).map_err(refuse)?;
        let kind = local_type(object.as_reflect().type_info().name);
        if !TYPES.contains(&kind) {
            return Err(refuse(format!("`{kind}` is not a property this view shows")));
        }
        let property = property::read(object.as_reflect()).map_err(|why| refuse(why.to_string()))?;
        let mut notes = Vec::new();

        // The frame, and the depth of its nodes.
        let frame_uri = property
            .supporting_representation_uri
            .clone()
            .ok_or_else(|| refuse("the property names no supporting representation".to_string()))?;
        let frame = cx.source.read_object_at(&frame_uri).map_err(refuse)?;
        let support = local_type(frame.as_reflect().type_info().name);
        if support != FRAME {
            return Err(refuse(format!(
                "the property sits on a `{support}`: this view shows the properties of a wellbore frame"
            )));
        }
        let mds =
            strata_by_md::node_mds(frame.as_reflect(), cx.source).map_err(|why| refuse(why.to_string()))?;
        let md_uom = strata_by_md::md_reference(frame.as_reflect(), cx.source)
            .ok()
            .and_then(|reference| reference.uom);

        // The plane.
        let shape = ElementShape::of_property(&property).map_err(|why| refuse(why.to_string()))?;
        if shape.is_scalar() {
            return Err(refuse(
                "the property holds one value per element: that is a curve, not an image".to_string(),
            ));
        }
        let settings = &request.settings;
        let (pick, defaulted) = pick_of(settings.get("pick"), &shape)?;
        if defaulted {
            notes.push(ViewNote {
                key: "pick-defaulted",
                says: format!(
                    "no plane asked: the first index of every dimension but the one shown ({} of {} dimension(s))",
                    describe(&pick),
                    shape.rank()
                ),
            });
        }
        let plane = read_plane(uri, &property, mds.len() as u64, &pick, cx.source)
            .map_err(|why| refuse(why.to_string()))?;
        for note in &plane.notes {
            if let MatrixNote::SupportFromCount { declared, used } = note {
                notes.push(ViewNote {
                    key: "support-from-count",
                    says: format!(
                        "the property declares its values on the {declared:?} but its count fits the {used:?}: read as the {used:?}"
                    ),
                });
            }
        }

        // The rows: what each sample of the matrix covers along the depth.
        let covers: Vec<Option<Cover>> = if plane.rows == mds.len() {
            let rule = GapRule::of_axis(&mds)
                .map_err(|why| refuse(format!("the MD axis of the frame cannot be drawn: {why}")))?;
            node_covers(&mds, Some(rule.max_gap() / 2.0)).map_err(|why| refuse(why.to_string()))?
        } else if plane.rows + 1 == mds.len() {
            interval_covers(&mds[..mds.len() - 1], &mds[1..]).map_err(|why| refuse(why.to_string()))?
        } else {
            return Err(refuse(format!(
                "{} row(s) fit neither the {} node(s) nor the intervals of the frame",
                plane.rows,
                mds.len()
            )));
        };
        let (data_top, data_base) = extent_of(&covers)
            .ok_or_else(|| refuse("no sample of the frame has a finite depth".to_string()))?;
        let window = [
            settings.number("md_from").unwrap_or(data_top),
            settings.number("md_to").unwrap_or(data_base),
        ];
        if !window[0].is_finite() || !window[1].is_finite() || window[0] >= window[1] {
            return Err(refuse(format!(
                "the depth window {} to {} is empty",
                short(window[0]),
                short(window[1])
            )));
        }

        // The columns: the axis of the dimension shown, else the ranks.
        let across = across_of(&pick, &shape);
        let (positions, cyclic, x_title) = match across {
            Some(dim) => {
                let subject = AxisSubject {
                    uri,
                    shape: &shape,
                    facets: &property.facets,
                    links: &[],
                    rows: Some(plane.rows as u64),
                };
                let resolved = AxisSources::with_builtins()
                    .resolve(&subject, cx.source, cx.source)
                    .map_err(|why| refuse(why.to_string()))?;
                let axis = &resolved.axes[dim];
                if resolved.ranks_only().contains(&dim) {
                    notes.push(ViewNote {
                        key: "axis-ranks-only",
                        says: format!(
                            "the file gives no axis for dimension {dim}: the ranks 0 to {} stand in",
                            plane.cols.saturating_sub(1)
                        ),
                    });
                }
                if matches!(axis.values, AxisValues::Labels(_)) {
                    notes.push(ViewNote {
                        key: "axis-labels-not-drawn",
                        says: format!(
                            "the components of dimension {dim} have names, not numbers: the ranks are graduated, the names are not written"
                        ),
                    });
                }
                let title = match (&axis.name, &axis.uom) {
                    (Some(name), Some(uom)) => format!("{name} ({uom})"),
                    (Some(name), None) => name.clone(),
                    (None, Some(uom)) => format!("component ({uom})"),
                    (None, None) => "component (rank)".to_string(),
                };
                (positions_of(&axis.values), axis.cyclic, title)
            }
            None => (
                (0..plane.cols).map(|k| k as f64).collect(),
                None,
                "component (rank)".to_string(),
            ),
        };
        let column_covers = node_covers(&positions, (positions.len() == 1).then_some(0.5))
            .map_err(|why| refuse(format!("the axis of the components cannot be drawn: {why}")))?;
        let (x_from, x_to) = extent_of(&column_covers)
            .ok_or_else(|| refuse("no component has a finite position".to_string()))?;

        // The pixels of the plot.
        let size = request.size;
        let width = (size[0] - LEFT - RIGHT).floor();
        let height = (size[1] - TOP - BOTTOM).floor();
        if width < 1.0 || height < 1.0 {
            return Err(refuse("the picture is too small to hold a plot".to_string()));
        }
        let area = Rect {
            x: LEFT,
            y: TOP,
            w: width,
            h: height,
        };
        let row_map = rows_for(&covers, window, height as u32).map_err(|why| refuse(why.to_string()))?;
        if row_map.blank == row_map.picks.len() {
            return Err(refuse(format!(
                "no sample lies in the depth window {} to {}",
                short(window[0]),
                short(window[1])
            )));
        }
        let column_map = columns_for(&column_covers, [x_from, x_to], width as u32, cyclic)
            .map_err(|why| refuse(why.to_string()))?;
        if row_map.blank > 0 {
            notes.push(ViewNote {
                key: "md-gaps",
                says: format!(
                    "{} of {} pixel rows lie in a gap of the depth axis (no sample, or a gap wider than the rule bridges): left empty",
                    row_map.blank,
                    row_map.picks.len()
                ),
            });
        }
        let reduce_id = settings.text("reduce").unwrap_or(REDUCES[0]);
        let reduces = RowReduces::with_builtins();
        let reduce = reduces
            .find(reduce_id)
            .ok_or_else(|| refuse(format!("no row reduction `{reduce_id}`")))?;
        if row_map.most_per_pixel > 1 {
            notes.push(ViewNote {
                key: "rows-reduced",
                says: format!(
                    "up to {} samples fall in one pixel row: shown by `{reduce_id}`",
                    row_map.most_per_pixel
                ),
            });
        }

        // The scale of the values, and their colours.
        let (scale, kind) = value_scale(
            settings.text("scale").unwrap_or(SCALES[0]),
            settings,
            &plane.values,
            &mut notes,
        )
        .map_err(refuse)?;
        let log = kind == ScaleKind::Log10;
        let colours = value_colours(
            cx,
            ID,
            Some(uri),
            settings.text("palette"),
            (scale.min(), scale.max()),
            log,
            Reading::Continuous,
        )?;
        let colour = |value: f64| -> Option<[u8; 4]> {
            if log && value <= 0.0 {
                None
            } else {
                colours.sample(value)
            }
        };
        let raster = paint_grid(
            GridView {
                rows: plane.rows,
                cols: plane.cols,
                values: &plane.values,
            },
            &row_map,
            &column_map,
            reduce,
            &colour,
        )
        .map_err(|why| refuse(why.to_string()))?;
        let counts = raster.counts();
        if counts.not_coloured > 0 {
            notes.push(ViewNote {
                key: "cells-not-coloured",
                says: format!(
                    "{} pixel(s) hold a value the colours give no colour (absent{}): left transparent",
                    counts.not_coloured,
                    if log {
                        ", or not above zero on a logarithmic scale"
                    } else {
                        ""
                    }
                ),
            });
        }

        // The page.
        let title = property
            .title
            .clone()
            .or_else(|| {
                cx.index
                    .node(uri)
                    .and_then(|node| node.title.as_deref().map(str::to_string))
            })
            .unwrap_or_else(|| uri.to_string());
        let y_title = match &md_uom {
            Some(uom) => format!("MD ({uom})"),
            None => "MD".to_string(),
        };
        let frame_plot = PlotFrame {
            area,
            x: style::Scale {
                data: [x_from, x_to],
                page: [area.x, area.x + area.w],
            },
            y: style::Scale {
                data: window,
                page: [area.y, area.y + area.h],
            },
        };
        let mut list = DisplayList::new(size, PageUnit::Pixel);
        list.title = format!("{ID} - {title}");
        list.background = Some(style::PAPER);

        let mut header = Layer::new("title");
        header.elements.push(Element::new(vec![style::label_item(
            [area.x, 24.0],
            format!("{title} - {}", describe(&pick)),
            style::text(style::TITLE, style::INK, Anchor::Start),
        )]));
        list.layers.push(header);
        list.layers
            .push(style::axes_layer(&frame_plot, &x_title, &y_title));

        let mut image = Layer::new("image");
        image.elements.push(Element::keyed(
            uri.to_string(),
            "image",
            vec![Item::Image {
                rect: area,
                raster: Arc::new(raster),
            }],
        ));
        list.layers.push(image);

        let mut outline = Layer::new("frame");
        outline.elements.push(Element::new(vec![Item::Rect {
            rect: area,
            corner: 0.0,
            fill: None,
            stroke: Some(energyml_sheet::draw::Stroke::solid(style::MUTED, 1.0)),
        }]));
        list.layers.push(outline);
        list.layers
            .push(hit_layer(uri, area, &row_map, &column_map, &covers));
        list.layers
            .push(legend_layer(area, &scale, log, &colours, property.uom.as_deref()));
        Ok(Drawn { list, notes })
    }
}

/// The plane asked for, and whether it is the default. `None` or JSON: `"all"`, `{"along": 0, "at": [0, 40]}`
/// (with `"reduced": [1]` and `"by": "mean"` to project dimensions), `{"at": [3, 1]}`, `{"reduce": "max"}`.
fn pick_of(value: Option<&Value>, shape: &ElementShape) -> Result<(ComponentPick, bool), ViewRefusal> {
    let bad = |why: String| ViewRefusal::BadSetting {
        name: "pick".to_string(),
        why,
    };
    let rank = shape.rank();
    let Some(value) = value else {
        let pick = if rank <= 1 {
            ComponentPick::All
        } else {
            ComponentPick::Along {
                axis: 0,
                at: vec![0; rank],
            }
        };
        return Ok((pick, rank > 1));
    };
    if value.as_str() == Some("all") {
        return Ok((ComponentPick::All, false));
    }
    let Some(map) = value.as_object() else {
        return Err(bad("expected \"all\" or an object".to_string()));
    };
    let indices = |key: &str| -> Result<Option<Vec<usize>>, ViewRefusal> {
        match map.get(key) {
            None => Ok(None),
            Some(list) => list
                .as_array()
                .and_then(|items| {
                    items
                        .iter()
                        .map(|item| item.as_u64().map(|n| n as usize))
                        .collect::<Option<Vec<usize>>>()
                })
                .map(Some)
                .ok_or_else(|| bad(format!("`{key}` is a list of whole numbers"))),
        }
    };
    let by = |map: &serde_json::Map<String, Value>| -> Result<Reduction, ViewRefusal> {
        let id = map
            .get("by")
            .and_then(Value::as_str)
            .ok_or_else(|| bad("`reduced` needs `by`: the id of a reduction".to_string()))?;
        Reduction::from_id(id).ok_or_else(|| bad(format!("no reduction `{id}`")))
    };
    if let Some(axis) = map.get("along") {
        let axis =
            axis.as_u64()
                .ok_or_else(|| bad("`along` is the number of a dimension".to_string()))? as usize;
        let at = indices("at")?.unwrap_or_else(|| vec![0; rank]);
        let reduced = indices("reduced")?.unwrap_or_default();
        let how = if reduced.is_empty() {
            Reduction::Mean
        } else {
            by(map)?
        };
        return Ok((ComponentPick::along_reduced(axis, at, reduced, how), false));
    }
    if let Some(at) = indices("at")? {
        return Ok((ComponentPick::At(at), false));
    }
    if let Some(id) = map.get("reduce").and_then(Value::as_str) {
        let how = Reduction::from_id(id).ok_or_else(|| bad(format!("no reduction `{id}`")))?;
        return Ok((ComponentPick::Reduce(how), false));
    }
    Err(bad("expected `along`, `at` or `reduce`".to_string()))
}

/// The pick in words, for the title and the notes.
fn describe(pick: &ComponentPick) -> String {
    match pick {
        ComponentPick::All => "all components".to_string(),
        ComponentPick::At(at) => format!("component {at:?}"),
        ComponentPick::Along { axis, at } => format!("along dimension {axis} at {at:?}"),
        ComponentPick::Reduce(by) => format!("reduced by {}", by.id()),
        ComponentPick::AlongReduced {
            axis,
            at,
            reduced,
            by,
        } => {
            format!(
                "along dimension {axis} at {at:?}, {reduced:?} reduced by {}",
                by.id()
            )
        } // `ComponentPick` is not `non_exhaustive`: every pick above.
    }
}

/// The dimension the columns of the plane stand for: the one shown, or the only one of a one-dimension shape.
/// `None`: the columns are components of no single dimension (one component, a reduction, several dimensions
/// flattened).
fn across_of(pick: &ComponentPick, shape: &ElementShape) -> Option<usize> {
    match pick {
        ComponentPick::Along { axis, .. } | ComponentPick::AlongReduced { axis, .. } => Some(*axis),
        ComponentPick::All if shape.rank() == 1 => Some(0),
        _ => None,
    }
}

/// The positions of the values of an axis. Names and ranks: the ranks.
fn positions_of(values: &AxisValues) -> Vec<f64> {
    match values {
        AxisValues::Regular { start, step, count } => (0..*count).map(|k| start + k as f64 * step).collect(),
        AxisValues::Listed(list) => list.clone(),
        AxisValues::Labels(names) => (0..names.len()).map(|k| k as f64).collect(),
        AxisValues::Ranks(count) => (0..*count).map(|k| k as f64).collect(),
    }
}

/// The least start and the greatest end of the covers that exist.
fn extent_of(covers: &[Option<Cover>]) -> Option<(f64, f64)> {
    covers.iter().flatten().fold(None, |found, cover| match found {
        None => Some((cover.from, cover.to)),
        Some((from, to)) => Some((from.min(cover.from), to.max(cover.to))),
    })
}

/// The resolved scale and its kind. `auto`: logarithmic when the plane's values span the decades
/// [`propose_log`] asks for. The automatic bounds are the plane's, whole, never the window's: a zoom must not
/// change what a value looks like.
fn value_scale(
    choice: &str,
    settings: &crate::setting::Settled,
    values: &[f64],
    notes: &mut Vec<ViewNote>,
) -> Result<(scale::Scale, ScaleKind), String> {
    let resolved = |kind: ScaleKind| -> Result<scale::Scale, String> {
        let (min, max) = (settings.number("min"), settings.number("max"));
        let auto = if min.is_some() && max.is_some() {
            None
        } else {
            Some(auto_bounds(values, kind, QuantileMethod::Auto).map_err(|why| why.to_string())?)
        };
        resolve(
            &ScaleSpec {
                kind,
                min,
                max,
                reverse: false,
            },
            auto,
        )
        .map_err(|why| why.to_string())
    };
    match choice {
        "linear" => Ok((resolved(ScaleKind::Linear)?, ScaleKind::Linear)),
        "log" => Ok((resolved(ScaleKind::Log10)?, ScaleKind::Log10)),
        _ => {
            let linear = resolved(ScaleKind::Linear)?;
            if propose_log(linear.decades()) {
                let log = resolved(ScaleKind::Log10)?;
                notes.push(ViewNote {
                    key: "scale-log-proposed",
                    says: format!(
                        "the values span {} decades: a logarithmic scale is used (ask `linear` to change it)",
                        short(linear.decades().unwrap_or(0.0))
                    ),
                });
                Ok((log, ScaleKind::Log10))
            } else {
                Ok((linear, ScaleKind::Linear))
            }
        }
    }
}

/// The elements a user can point at: a row (a sample of the matrix) and a column (a component), as invisible
/// rectangles over the plot, merged where neighbouring pixels show the same one.
fn hit_layer(uri: &str, area: Rect, rows: &RowMap, columns: &ColumnMap, covers: &[Option<Cover>]) -> Layer {
    let mut layer = Layer::new("hit");
    let mut runs: Vec<(usize, usize, usize)> = Vec::new();
    for (y, pick) in rows.picks.iter().enumerate() {
        let sample = match *pick {
            RowPick::Sample(i) => i,
            RowPick::Bucket { nearest, .. } => nearest,
            RowPick::Blank => continue,
        };
        extend(&mut runs, sample, y);
    }
    for (sample, from, to) in runs {
        let mut element = Element::keyed(
            format!("{uri}#row={sample}"),
            "row",
            vec![invisible(Rect {
                x: area.x,
                y: area.y + from as f64,
                w: area.w,
                h: (to - from) as f64,
            })],
        );
        if let Some(cover) = covers.get(sample).copied().flatten() {
            element.data.push(("md".to_string(), short(cover.at)));
        }
        layer.elements.push(element);
    }
    let mut runs: Vec<(usize, usize, usize)> = Vec::new();
    for (x, pick) in columns.picks.iter().enumerate() {
        let Some(component) = *pick else { continue };
        extend(&mut runs, component, x);
    }
    for (component, from, to) in runs {
        layer.elements.push(Element::keyed(
            format!("{uri}#col={component}"),
            "column",
            vec![invisible(Rect {
                x: area.x + from as f64,
                y: area.y,
                w: (to - from) as f64,
                h: area.h,
            })],
        ));
    }
    layer
}

/// Pixel `at` shows `id`: the run of `id` that ends there grows by one, else a run starts.
fn extend(runs: &mut Vec<(usize, usize, usize)>, id: usize, at: usize) {
    if let Some(last) = runs.last_mut() {
        if last.0 == id && last.2 == at {
            last.2 = at + 1;
            return;
        }
    }
    runs.push((id, at, at + 1));
}

fn invisible(rect: Rect) -> Item {
    Item::Rect {
        rect,
        corner: 0.0,
        fill: None,
        stroke: None,
    }
}

/// The colour bar at the right of the plot, its graduations, the unit and where the colours come from.
fn legend_layer(
    area: Rect,
    scale: &scale::Scale,
    log: bool,
    colours: &crate::colour::ValueColours,
    uom: Option<&str>,
) -> Layer {
    let mut layer = Layer::new("legend");
    let bar = Rect {
        x: area.x + area.w + 24.0,
        y: area.y,
        w: 14.0,
        h: area.h,
    };
    let band = bar.h / BANDS as f64;
    let mut bands = Vec::new();
    for k in 0..BANDS {
        let fraction = 1.0 - (k as f64 + 0.5) / BANDS as f64;
        if let Some([r, g, b, a]) = colours.sample(scale.value_at(fraction)) {
            bands.push(Item::Rect {
                rect: Rect {
                    x: bar.x,
                    y: bar.y + k as f64 * band,
                    w: bar.w,
                    h: band + 0.5,
                },
                corner: 0.0,
                fill: Some(Paint::Solid(Colour::rgba(r, g, b, a))),
                stroke: None,
            });
        }
    }
    layer.elements.push(Element::new(bands));
    let mut marks = vec![Item::Rect {
        rect: bar,
        corner: 0.0,
        fill: None,
        stroke: Some(energyml_sheet::draw::Stroke::solid(style::MUTED, 1.0)),
    }];
    let target = (bar.h / 40.0).max(2.0);
    let ticks: Vec<(f64, String)> = if log {
        log_ticks(scale.min(), scale.max(), target)
            .map(|ticks| ticks.values.into_iter().map(|v| (v, log_label(v))).collect())
            .unwrap_or_default()
    } else {
        nice_ticks(scale.min(), scale.max(), target)
            .map(|ticks| {
                let step = ticks.step;
                ticks.values.into_iter().map(|v| (v, label(v, step))).collect()
            })
            .unwrap_or_default()
    };
    for (value, text) in ticks {
        if let Fraction::At(at) = scale::fraction(value, scale) {
            let y = bar.y + bar.h * (1.0 - at);
            marks.push(style::label_item(
                [bar.x + bar.w + 4.0, y + 3.5],
                text,
                style::text(style::SMALL, style::MUTED, Anchor::Start),
            ));
        }
    }
    if let Some(uom) = uom {
        marks.push(style::label_item(
            [bar.x, bar.y - 8.0],
            uom,
            style::text(style::TEXT, style::MUTED, Anchor::Start),
        ));
    }
    marks.push(style::label_item(
        [area.x, area.y + area.h + 46.0],
        colours.source.says(),
        style::text(style::SMALL, style::MUTED, Anchor::Start),
    ));
    layer.elements.push(Element::new(marks));
    layer
}

/// A number written short: no trailing zeros, two decimals at most.
fn short(value: f64) -> String {
    let text = format!("{value:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}
