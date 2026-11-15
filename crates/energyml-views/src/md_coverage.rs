//! `md-coverage`: which objects cover which measured depths of a well.
//!
//! The extent of each object comes from `energyml_domain::md_extent::of_object` (a trajectory's interval, a frame's
//! nodes, a WITSML log's index interval), the objects of a well from `energyml_domain::well_members`, and the laying
//! of the extents side by side from `md_extent::coverage`: one group per comparable MD reference, one band for what
//! states none, and the alignments the person declares (`align`), never guessed. This module only lays it out:
//!
//! - one **band** per group, side by side, the MD running down the page, each band with its own graduated axis (two
//!   groups are not comparable, so they never share one axis);
//! - one **bar** per object, at its extent, keyed by the object's uri;
//! - a **hatched** bar for an extent inherited through a link, a **cross** on a bar written the wrong way round, a
//!   **dashed** outline on a bar placed by an alignment;
//! - **apart**, at the foot of the page: what is not on the MD axis (a time index), and, in the band of unstated
//!   references, what no reference says.

use std::collections::BTreeMap;

use energyml_domain::md_extent::{
    self, Alignment, Coverage, CoverageItem, CoverageNote, ExtentAxis, ExtentBasis, ExtentReaders,
    ExtentRefusal,
};
use energyml_domain::package_index::{NodeState, PackageIndex};
use energyml_domain::well_members::{WellMembers, well_members_of_index};
use energyml_sheet::draw::ticks::{label, nice_ticks};
use energyml_sheet::draw::{
    Anchor, Colour, DisplayList, Element, Hatch, Item, Layer, PageUnit, Paint, Rect, Stroke, SymbolKind,
    TextStyle,
};
use serde_json::Value;

use crate::kind::{Drawn, ObjectSlot, Offer, Request, ViewContext, ViewKind, ViewNote};
use crate::refusal::ViewRefusal;
use crate::setting::{Settled, ViewSetting};
use crate::style::{self, PlotFrame, short};

/// The id of the view.
pub const ID: &str = "md-coverage";

/// What it takes: a well (a feature, or an interpretation, which stands for the whole well) or the objects that
/// carry an extent in MD, by their local type name.
pub const TYPES: &[&str] = &[
    "WellboreFeature",
    "WellboreInterpretation",
    "WellboreTrajectoryRepresentation",
    "WellboreFrameRepresentation",
    "SeismicWellboreFrameRepresentation",
    "BlockedWellboreRepresentation",
    "WellboreMarkerFrameRepresentation",
    "DeviationSurveyRepresentation",
    "Trajectory",
    "Log",
    "ChannelSet",
    "Channel",
];

/// The view.
pub struct MdCoverage;

impl ViewKind for MdCoverage {
    fn id(&self) -> &'static str {
        ID
    }

    fn about(&self) -> &'static str {
        "which objects cover which measured depths of a well: a band per MD reference, a bar per object at its extent, hatching for an extent inherited through a link"
    }

    fn objects(&self) -> Vec<ObjectSlot> {
        vec![ObjectSlot {
            role: "object",
            types: TYPES,
            least: 1,
            most: usize::MAX,
            about: "a well (a wellbore feature or interpretation: every trajectory, frame and marker frame it has) or objects that carry an MD extent",
        }]
    }

    fn settings(&self) -> Vec<ViewSetting> {
        vec![
            ViewSetting::json(
                "align",
                "alignments you declare, a JSON list of {\"item\": uri, \"onto\": uri}: count the item's MDs as the other's are counted; never guessed, written on the picture",
            ),
            ViewSetting::number(
                "md_from",
                "the top of every band's axis, in the unit of each band; none: the least extent shown",
            ),
            ViewSetting::number(
                "md_to",
                "the bottom of every band's axis, in the unit of each band; none: the greatest extent shown",
            ),
        ]
    }

    fn size(&self) -> [f64; 2] {
        [720.0, 520.0]
    }

    fn offers(&self, index: &PackageIndex) -> Vec<Offer> {
        well_members_of_index(index)
            .into_iter()
            .filter(|well| {
                !(well.trajectories.is_empty() && well.frames.is_empty() && well.markers.is_empty())
            })
            .map(|well| Offer {
                view: ID,
                objects: vec![well.key],
                title: well.title,
                missing: Vec::new(),
            })
            .collect()
    }

    fn draw(&self, request: &Request, cx: &ViewContext<'_>) -> Result<Drawn, ViewRefusal> {
        let refuse = |why: String| ViewRefusal::computing(ID, why);
        let mut notes: Vec<ViewNote> = Vec::new();

        // The objects: a well stands for its members; any other object stands for itself.
        let wells = well_members_of_index(cx.index);
        let mut uris: Vec<String> = Vec::new();
        let mut heading: Vec<String> = Vec::new();
        for asked in request.objects_of("object") {
            match well_of(&wells, asked) {
                Some(well) => {
                    let members: Vec<&str> = well
                        .trajectories
                        .iter()
                        .chain(&well.frames)
                        .chain(&well.markers)
                        .map(|member| member.uri.as_str())
                        .collect();
                    if members.is_empty() {
                        return Err(refuse(format!(
                            "the well `{}` has no trajectory, frame or marker frame: nothing carries a depth",
                            well.title
                        )));
                    }
                    heading.push(well.title.clone());
                    for member in members {
                        if !uris.iter().any(|known| known == member) {
                            uris.push(member.to_string());
                        }
                    }
                }
                None => {
                    heading.push(title_of(cx.index, asked));
                    if !uris.iter().any(|known| known == asked) {
                        uris.push(asked.to_string());
                    }
                }
            }
        }

        // The extent of each, by the door of extents. Triangulation: the bars are placed from these, and a test reads
        // the same door.
        let readers = ExtentReaders::builtin();
        let mut titles: BTreeMap<String, String> = BTreeMap::new();
        let mut items: Vec<CoverageItem> = Vec::new();
        let mut off_axis: Vec<(String, ExtentAxis)> = Vec::new();
        for uri in &uris {
            let title = title_of(cx.index, uri);
            titles.insert(uri.clone(), title.clone());
            let object = match cx.source.read_object_at(uri) {
                Ok(object) => object,
                Err(why) => {
                    notes.push(ViewNote {
                        key: "extent-not-read",
                        says: format!("{title}: the object is not read: {why}"),
                    });
                    continue;
                }
            };
            match md_extent::of_object(object.as_reflect(), cx.source, &readers) {
                Ok(extent) => items.push(CoverageItem {
                    key: uri.clone(),
                    extent,
                }),
                Err(ExtentRefusal::NotDepth { axis }) => off_axis.push((uri.clone(), axis)),
                Err(why) => notes.push(ViewNote {
                    key: "extent-not-read",
                    says: format!("{title}: no extent on the MD axis: {why}"),
                }),
            }
        }

        let alignments = alignments_of(&request.settings, cx.index)?;
        let coverage = md_extent::coverage(items, &alignments);
        let name = |uri: &str| {
            titles
                .get(uri)
                .cloned()
                .unwrap_or_else(|| title_of(cx.index, uri))
        };
        for note in &coverage.notes {
            match note {
                CoverageNote::Incomparable {
                    item,
                    first_of_group,
                    why,
                } => notes.push(ViewNote {
                    key: "incomparable-md-reference",
                    says: format!(
                        "{} is counted apart from {}: {why}",
                        name(item),
                        if first_of_group.is_empty() {
                            "the first of its band".to_string()
                        } else {
                            name(first_of_group)
                        }
                    ),
                }),
                CoverageNote::UnknownAlignment { alignment } => notes.push(ViewNote {
                    key: "unknown-alignment",
                    says: format!(
                        "the alignment of {} onto {} is ignored: the second is not among the objects drawn",
                        name(&alignment.item),
                        name(&alignment.onto)
                    ),
                }),
                CoverageNote::Reversed { item } => notes.push(ViewNote {
                    key: "reversed-interval",
                    says: format!(
                        "{}: the interval is written from a deeper MD to a shallower one; drawn as written, marked with a cross",
                        name(item)
                    ),
                }),
                CoverageNote::OffAxis { item, axis } => {
                    if !off_axis.iter().any(|(known, _)| known == item) {
                        off_axis.push((item.clone(), *axis));
                    }
                }
            }
        }
        for alignment in &coverage.applied {
            notes.push(ViewNote {
                key: "aligned-by-assumption",
                says: format!(
                    "aligned by assumption: the MDs of {} are counted as those of {} (dashed outline)",
                    name(&alignment.item),
                    name(&alignment.onto)
                ),
            });
        }

        if coverage.groups.iter().all(|group| group.bars.is_empty()) {
            let apart = if off_axis.is_empty() {
                String::new()
            } else {
                format!(" ({} off the MD axis)", off_axis.len())
            };
            let said: Vec<String> = notes.iter().map(|note| note.says.clone()).collect();
            return Err(refuse(format!(
                "no object gives an extent on the MD axis{apart}{}",
                if said.is_empty() {
                    String::new()
                } else {
                    format!(": {}", said.join("; "))
                }
            )));
        }

        let heading = match heading.len() {
            1 => heading.remove(0),
            count => format!("{count} objects"),
        };
        let spec = CoverageSpec {
            coverage: &coverage,
            titles: &titles,
            off_axis: &off_axis,
            size: request.size,
            from: request.settings.number("md_from"),
            to: request.settings.number("md_to"),
            heading: &heading,
        };
        let list = coverage_list(&spec).map_err(refuse)?;
        Ok(Drawn { list, notes })
    }
}

/// The well an object asked stands for: the feature or interpretation that keys a well of the index.
fn well_of<'a>(wells: &'a [WellMembers], asked: &str) -> Option<&'a WellMembers> {
    wells
        .iter()
        .find(|well| well.key == asked || well.interpretations.iter().any(|one| one == asked))
}

/// The title of an object, else its uri.
fn title_of(index: &PackageIndex, uri: &str) -> String {
    index
        .node(uri)
        .filter(|node| node.state == NodeState::Present)
        .and_then(|node| node.title.as_deref())
        .unwrap_or(uri)
        .to_string()
}

/// The `align` setting: a list of `{"item": uri, "onto": uri}`, each uri one the package holds (a bare uuid is
/// changed into its full uri here, at the entry).
fn alignments_of(settings: &Settled, index: &PackageIndex) -> Result<Vec<Alignment>, ViewRefusal> {
    let bad = |why: String| ViewRefusal::BadSetting {
        name: "align".to_string(),
        why,
    };
    let Some(value) = settings.get("align") else {
        return Ok(Vec::new());
    };
    if value.is_null() {
        return Ok(Vec::new());
    }
    let Some(list) = value.as_array() else {
        return Err(bad(
            "a list of {\"item\": uri, \"onto\": uri} is expected".to_string()
        ));
    };
    let resolve = |entry: &Value, field: &str| -> Result<String, ViewRefusal> {
        let Some(text) = entry.get(field).and_then(Value::as_str) else {
            return Err(bad(format!("every alignment needs a text `{field}`")));
        };
        index
            .uri_of(text.trim())
            .map(str::to_string)
            .ok_or_else(|| bad(format!("`{text}` is no object of the package")))
    };
    list.iter()
        .map(|entry| {
            Ok(Alignment {
                item: resolve(entry, "item")?,
                onto: resolve(entry, "onto")?,
            })
        })
        .collect()
}

/// What [`coverage_list`] lays out.
pub struct CoverageSpec<'a> {
    /// The coverage: the groups, their bars.
    pub coverage: &'a Coverage,
    /// The title of each object, by the key of its bar; a key without one is written as it is.
    pub titles: &'a BTreeMap<String, String>,
    /// What is not on the MD axis, by key, with its axis: written apart.
    pub off_axis: &'a [(String, ExtentAxis)],
    /// Width and height, in pixels.
    pub size: [f64; 2],
    /// The top of every band's axis, in each band's unit; `None`: the least extent shown.
    pub from: Option<f64>,
    /// The bottom of every band's axis; `None`: the greatest extent shown.
    pub to: Option<f64>,
    /// The heading of the picture.
    pub heading: &'a str,
}

const MARGIN_LEFT: f64 = 8.0;
const MARGIN_RIGHT: f64 = 16.0;
const AXIS_WIDTH: f64 = 60.0;
const BAND_GAP: f64 = 10.0;
const TOP: f64 = 72.0;
const BOTTOM: f64 = 16.0;
const LINE: f64 = 14.0;
/// The most off-axis lines written; the rest is counted.
const OFF_AXIS_LINES: usize = 6;

/// The picture of a coverage: the bands, the bars, what is apart. Pure: the same spec, the same list.
///
/// # Errors
///
/// A sentence when nothing can be laid (no bar, bands that do not fit the width, a height too small, bounds that
/// leave no interval).
pub fn coverage_list(spec: &CoverageSpec<'_>) -> Result<DisplayList, String> {
    let groups = &spec.coverage.groups;
    if groups.iter().all(|group| group.bars.is_empty()) {
        return Err("no bar to lay on an axis".to_string());
    }
    let [width, height] = spec.size;
    let count = groups.len();
    let slot = (width - MARGIN_LEFT - MARGIN_RIGHT) / count as f64;
    let plot_width = slot - AXIS_WIDTH - BAND_GAP;
    if plot_width < 16.0 {
        return Err(format!(
            "{count} bands do not fit a page {width} pixels wide: at least 16 pixels are needed for each"
        ));
    }
    let off_shown = spec.off_axis.len().min(OFF_AXIS_LINES);
    let off_block = if spec.off_axis.is_empty() {
        0.0
    } else {
        10.0 + LINE * (1 + off_shown + usize::from(spec.off_axis.len() > OFF_AXIS_LINES)) as f64
    };
    let plot_height = height - TOP - BOTTOM - off_block;
    if plot_height < 40.0 {
        return Err(format!(
            "a page {height} pixels high leaves {plot_height:.0} pixels for the bands: at least 40 are needed"
        ));
    }

    let mut list = DisplayList::new(spec.size, PageUnit::Pixel);
    list.title = format!("{ID} - {}", spec.heading);
    list.background = Some(style::PAPER);

    let mut heads = Layer::new("title");
    heads.elements.push(Element::new(vec![style::label_item(
        [MARGIN_LEFT + 2.0, 24.0],
        format!("MD coverage - {}", spec.heading),
        style::text(style::TITLE, style::INK, Anchor::Start),
    )]));

    let mut bands: Vec<Layer> = Vec::new();
    for (at, group) in groups.iter().enumerate() {
        let slot_x = MARGIN_LEFT + slot * at as f64;
        let area = Rect {
            x: slot_x + AXIS_WIDTH,
            y: TOP,
            w: plot_width,
            h: plot_height,
        };

        // The band's header: its unit, then how its MDs are counted.
        let unit = group
            .uom
            .as_deref()
            .map(|uom| format!(" ({uom})"))
            .unwrap_or_default();
        let (first, second) = match &group.reference {
            Some(reference) => (
                format!("MD{unit}"),
                format!(
                    "trajectory {}{}",
                    first_chars(&reference.trajectory),
                    reference
                        .datum
                        .as_deref()
                        .map(|datum| format!(", datum {}", first_chars(datum)))
                        .unwrap_or_default()
                ),
            ),
            None => (
                format!("MD{unit}, reference not stated"),
                "not compared with the other bands".to_string(),
            ),
        };
        heads.elements.push(Element::new(vec![
            style::label_item(
                [slot_x + 4.0, 46.0],
                first,
                style::text(style::TEXT, style::INK, Anchor::Start),
            ),
            style::label_item(
                [slot_x + 4.0, 60.0],
                second,
                style::text(style::SMALL, style::MUTED, Anchor::Start),
            ),
        ]));

        // The band's axis: the least and the greatest MD of its bars, unless the person set them.
        let (mut low, mut high) = (f64::INFINITY, f64::NEG_INFINITY);
        for bar in &group.bars {
            low = low.min(bar.from.min(bar.to));
            high = high.max(bar.from.max(bar.to));
        }
        let set = spec.from.is_some() || spec.to.is_some();
        if let Some(from) = spec.from {
            low = from;
        }
        if let Some(to) = spec.to {
            high = to;
        }
        if !(low.is_finite() && high.is_finite()) || (set && low >= high) {
            return Err(format!(
                "the bounds of the axis leave no interval: from {} to {}",
                short(low),
                short(high)
            ));
        }
        let frame = PlotFrame::new(area, [0.0, 1.0], [low, high], true, false);
        list.layers.push(axis_layer(at, &frame, &format!("MD{unit}")));

        // The bars, side by side across the band, in the order of the items.
        let mut layer = Layer::new(format!("bars-{at}"));
        layer.clip = Some(area);
        let room = area.w / group.bars.len().max(1) as f64;
        let thick = (room * 0.7).clamp(2.0, 26.0);
        for (place, bar) in group.bars.iter().enumerate() {
            let (top, bottom) = (bar.from.min(bar.to), bar.from.max(bar.to));
            let reversed = bar.from > bar.to;
            let y = frame.y.at(top);
            let y_end = frame.y.at(bottom);
            let centre = area.x + room * (place as f64 + 0.5);
            let rect = Rect {
                x: centre - thick / 2.0,
                y,
                w: thick,
                h: (y_end - y).max(1.5),
            };
            let fill = match bar.basis {
                ExtentBasis::ThroughLink { .. } => Paint::Hatch(Hatch {
                    colour: style::SUBJECT,
                    spacing: 4.0,
                    angle: 45.0,
                    width: 1.0,
                }),
                ExtentBasis::Declared | ExtentBasis::Measured => Paint::Solid(Colour::rgba(
                    style::SUBJECT.r,
                    style::SUBJECT.g,
                    style::SUBJECT.b,
                    0x55,
                )),
            };
            let stroke = if bar.aligned {
                Stroke {
                    dash: vec![4.0, 3.0],
                    ..Stroke::solid(style::MARK, 1.5)
                }
            } else {
                Stroke::solid(style::SUBJECT, 1.0)
            };
            let name = spec
                .titles
                .get(&bar.key)
                .cloned()
                .unwrap_or_else(|| bar.key.clone());
            let mut items = vec![
                Item::Rect {
                    rect,
                    corner: 0.0,
                    fill: Some(fill),
                    stroke: Some(stroke),
                },
                Item::Text {
                    at: [centre + 3.5, (rect.y + rect.h - 3.0).max(area.y + 3.0)],
                    text: name.clone(),
                    style: TextStyle {
                        rotate: -90.0,
                        ..style::text(style::SMALL, style::INK, Anchor::Start)
                    },
                },
            ];
            if reversed {
                items.push(Item::Symbol {
                    at: [centre, rect.y - 6.0],
                    kind: SymbolKind::Cross,
                    size: 8.0,
                    fill: None,
                    stroke: style::MARK,
                });
            }
            let mut element = Element::keyed(bar.key.clone(), "bar", items);
            element.title = format!(
                "{name}: {} to {}{}",
                short(bar.from),
                short(bar.to),
                group
                    .uom
                    .as_deref()
                    .map(|uom| format!(" {uom}"))
                    .unwrap_or_default()
            );
            element.data.push((
                "basis".to_string(),
                match bar.basis {
                    ExtentBasis::Declared => "declared",
                    ExtentBasis::Measured => "measured",
                    ExtentBasis::ThroughLink { .. } => "through-link",
                }
                .to_string(),
            ));
            if reversed {
                element.data.push(("reversed".to_string(), "true".to_string()));
            }
            if bar.aligned {
                element.data.push(("aligned".to_string(), "true".to_string()));
            }
            layer.elements.push(element);
        }
        bands.push(layer);
    }
    list.layers.insert(0, heads);
    list.layers.extend(bands);

    // Apart: what is not on the MD axis.
    if !spec.off_axis.is_empty() {
        let mut layer = Layer::new("off-axis");
        let mut y = height - BOTTOM - off_block + 20.0;
        layer.elements.push(Element::new(vec![style::label_item(
            [MARGIN_LEFT + 2.0, y],
            "Off the MD axis, not drawn:",
            style::text(style::TEXT, style::INK, Anchor::Start),
        )]));
        for (key, axis) in spec.off_axis.iter().take(OFF_AXIS_LINES) {
            y += LINE;
            let name = spec.titles.get(key).cloned().unwrap_or_else(|| key.clone());
            layer.elements.push(Element::keyed(
                key.clone(),
                "off-axis",
                vec![style::label_item(
                    [MARGIN_LEFT + 14.0, y],
                    format!("{name} - {}", axis_word(*axis)),
                    style::text(style::SMALL, style::MUTED, Anchor::Start),
                )],
            ));
        }
        if spec.off_axis.len() > OFF_AXIS_LINES {
            y += LINE;
            layer.elements.push(Element::new(vec![style::label_item(
                [MARGIN_LEFT + 14.0, y],
                format!("and {} more", spec.off_axis.len() - OFF_AXIS_LINES),
                style::text(style::SMALL, style::MUTED, Anchor::Start),
            )]));
        }
        list.layers.push(layer);
    }
    list.validate().map_err(|why| why.to_string())?;
    Ok(list)
}

/// The frame of a band, its horizontal grid and its graduated MD axis. (`style::axes_layer` graduates a second
/// axis across, which a band has not: the bars are side by side with no scale between them.)
fn axis_layer(band: usize, frame: &PlotFrame, title: &str) -> Layer {
    let mut layer = Layer::new(format!("axes-{band}"));
    let area = frame.area;
    let mut grid = Vec::new();
    let mut marks = Vec::new();
    let range = [
        frame.y.data[0].min(frame.y.data[1]),
        frame.y.data[0].max(frame.y.data[1]),
    ];
    if let Ok(ticks) = nice_ticks(range[0], range[1], (area.h / 50.0).max(2.0)) {
        for value in ticks.values.iter().copied() {
            let y = frame.y.at(value);
            grid.push(Item::Polyline {
                points: vec![[area.x, y], [area.x + area.w, y]],
                stroke: Stroke::solid(style::GRID, 1.0),
            });
            marks.push(style::label_item(
                [area.x - 5.0, y + 3.5],
                label(value, ticks.step),
                style::text(style::SMALL, style::MUTED, Anchor::End),
            ));
        }
    }
    layer.elements.push(Element::new(grid));
    layer.elements.push(Element::new(vec![Item::Rect {
        rect: area,
        corner: 0.0,
        fill: None,
        stroke: Some(Stroke::solid(style::MUTED, 1.0)),
    }]));
    marks.push(Item::Text {
        at: [area.x - 42.0, area.y + area.h / 2.0],
        text: title.to_string(),
        style: TextStyle {
            rotate: -90.0,
            ..style::text(style::TEXT, style::MUTED, Anchor::Middle)
        },
    });
    layer.elements.push(Element::new(marks));
    layer
}

fn axis_word(axis: ExtentAxis) -> &'static str {
    match axis {
        ExtentAxis::Md => "measured depth",
        ExtentAxis::Tvd => "true vertical depth",
        ExtentAxis::Time => "time",
    }
}

/// The first eight characters of an identifier.
fn first_chars(text: &str) -> String {
    text.chars().take(8).collect()
}
