//! `property-insight`: the automatic analysis of one property, as a figure.
//!
//! The figure has four blocks, each the lay-out of one computing of `energyml-domain`:
//!
//! 1. **the shape**, as written facts: the property's own words ([`property::read`]) and what the description found
//!    ([`Description`]: counts, moments, quantiles, the typed [`Observation`]s);
//! 2. **the distribution**: a histogram of the [`Binned`](energyml_domain::stats::Binned) the description holds, on a
//!    logarithmic axis when the values span several decades, with the modes and the threshold it found;
//! 3. **the relation** with a sister property ([`PackageIndex::siblings`]): the density of the pairs
//!    ([`PairWalk`]) as a raster, and the 1:1 diagonal;
//! 4. **the variation along each axis**: one horizontal bar per axis, the strongest marked.
//!
//! The values are read slab by slab ([`PropertyChunks`]) and described by the one door ([`Describer`]). This module
//! computes none of it: it draws what those doors answer. A block whose computing refuses is a [`ViewNote`] and the
//! other blocks are drawn: a doubtful value is said, never refused (the property stays usable everywhere).

use std::sync::Arc;

use energyml_domain::package_index::PackageIndex;
use energyml_domain::palettes::Reading;
use energyml_domain::property::chunks::{
    AxisId, ChunkSelection, PairWalk, PropertyChunks, keep_ordered_axes,
};
use energyml_domain::property::kinship::Kinship;
use energyml_domain::property::shape::{ComponentPick, ElementAxes, element_axes};
use energyml_domain::property::{self, Property, PropertyKindRef};
use energyml_domain::source::ArraySource;
use energyml_domain::stats::binned::Binned;
use energyml_domain::stats::cells::CellRule;
use energyml_domain::stats::describe::{
    DescribeRefusal, Describer, Description, Observation, Plan, ScaleChoice, ValueChunks,
};
use energyml_domain::stats::modes::{DEFAULT_BINS, MAX_BINS, StatMethods, ThresholdMeasure};
use energyml_domain::stats::pairs::{DENSITY_SIZE, PairSummary};
use energyml_domain::stats::scale::{Fraction, Scale as ValueScale, ScaleKind};
use energyml_sheet::draw::raster::Raster;
use energyml_sheet::draw::ticks::{label, log_label, log_ticks, nice_ticks};
use energyml_sheet::draw::{
    Anchor, Colour, DisplayList, Element, Item, Layer, PageUnit, Paint, Rect, Stroke, SymbolKind,
};
use serde_json::Value;

use crate::colour::{ValueColours, value_colours};
use crate::kind::{
    Drawn, ObjectSlot, Offer, Request, ViewContext, ViewKind, ViewNote, local_type, offers_of_types,
};
use crate::refusal::ViewRefusal;
use crate::setting::ViewSetting;
use crate::style;

/// The id of the view.
pub const ID: &str = "property-insight";

/// The types it shows: a property of numbers (a lookup table makes codes, described by their categories).
pub const TYPES: &[&str] = &["ContinuousProperty", "DiscreteProperty"];

/// The scales it offers, the first the default.
pub const SCALES: &[&str] = &["auto", "linear", "log"];

/// Room left of the plots (the value axis and its title) and right of them, pixels.
const LEFT: f64 = 64.0;
const RIGHT: f64 = 24.0;
/// The height of one line of facts, pixels.
const LINE: f64 = 15.0;
/// The height of the histogram's plot area, pixels.
const HISTOGRAM_HEIGHT: f64 = 190.0;
/// The side of the density's plot area, pixels.
const DENSITY_SIDE: f64 = 300.0;
/// The height of one row of the variation block, pixels.
const AXIS_ROW: f64 = 24.0;
/// The room of the labels of the variation block, pixels.
const LABEL_WIDTH: f64 = 190.0;
/// The room left of the end of a bar for its value, pixels.
const VALUE_WIDTH: f64 = 70.0;
/// How many pieces the 1:1 diagonal is cut in.
const DIAGONAL_STEPS: usize = 48;

/// The view.
pub struct PropertyInsight;

impl ViewKind for PropertyInsight {
    fn id(&self) -> &'static str {
        ID
    }

    fn about(&self) -> &'static str {
        "the automatic analysis of a property: its shape in facts, the distribution of its values (log when they span decades), its relation with a sister property, and the axis along which it varies most"
    }

    fn objects(&self) -> Vec<ObjectSlot> {
        vec![ObjectSlot {
            role: "property",
            types: TYPES,
            least: 1,
            most: 1,
            about: "a continuous or discrete property, on any support",
        }]
    }

    fn settings(&self) -> Vec<ViewSetting> {
        vec![
            ViewSetting::json(
                "component",
                "the components read: \"all\", {\"at\": [indices]}, {\"along\": axis, \"at\": [index per dimension]} or {\"reduce\": id}; none: all of them",
            ),
            ViewSetting::text(
                "sibling",
                "the sister property to compare with, by full uri; \"none\": no comparison; none given: the first sister by direction, else the first of the same kind",
            ),
            ViewSetting::choice(
                "scale",
                SCALES,
                "linear or logarithmic; auto: logarithmic when the values span several decades",
            )
            .default(Value::from(SCALES[0])),
            ViewSetting::integer("bins", "how many bars the histogram has")
                .bounds(Some(1.0), Some(MAX_BINS as f64))
                .default(Value::from(DEFAULT_BINS as i64)),
            ViewSetting::text(
                "palette",
                "the palette to paint with, by name; none: the file's colour map, else a default",
            ),
            ViewSetting::number(
                "ratio_below",
                "with a sister: the share of the pairs whose ratio sister / this property is below this value is written; none: not counted",
            ),
        ]
    }

    fn size(&self) -> [f64; 2] {
        [760.0, 760.0]
    }

    fn offers(&self, index: &PackageIndex) -> Vec<Offer> {
        offers_of_types(index, ID, TYPES, |node| {
            index.sheet(&node.uri).is_some_and(|sheet| sheet.lookup.is_none())
        })
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
        let settings = &request.settings;
        let pick = pick_of(settings.get("component"))?;
        let selection = ChunkSelection {
            pick: pick.clone(),
            ..ChunkSelection::default()
        };
        let plan = plan_of(settings);
        let rule = ratio_rule(settings.number("ratio_below"))?;
        let mut notes: Vec<ViewNote> = Vec::new();

        let title = property
            .title
            .clone()
            .filter(|title| !title.is_empty())
            .or_else(|| {
                cx.index
                    .node(uri)
                    .and_then(|node| node.title.as_deref().map(str::to_string))
            })
            .unwrap_or_else(|| uri.to_string());

        // The description: nothing else is computed without it.
        let described = describe_property(uri, &property, &plan, &selection, cx.source);
        let described = match described {
            Ok(described) => Some(described),
            Err(why) => {
                notes.push(ViewNote {
                    key: "description-refused",
                    says: format!(
                        "the values are not described, so the distribution, the relation and the variation are not drawn: {why}"
                    ),
                });
                None
            }
        };
        if let Some(described) = &described {
            if let Some(declared) = described.chunks.read_as_scalars() {
                notes.push(ViewNote {
                    key: "read-as-scalars",
                    says: format!(
                        "the property declares {declared} values per element and holds one: read as one value per element"
                    ),
                });
            }
            if !described.unordered.is_empty() {
                notes.push(ViewNote {
                    key: "axes-unordered-not-measured",
                    says: "the elements have no order the guide states: no variation is measured across them"
                        .to_string(),
                });
            }
        }

        // The colours: the values' own (the file's map, else a palette), over the scale of the histogram.
        let scale = described.as_ref().and_then(|d| d.description.scale);
        let colours = match &scale {
            Some(scale) => Some(value_colours(
                cx,
                ID,
                Some(uri),
                settings.text("palette"),
                (scale.min(), scale.max()),
                scale.kind() == ScaleKind::Log10,
                Reading::Continuous,
            )?),
            None => None,
        };

        // The facts, then the page.
        let words = |rank: usize| -> String {
            match &described {
                Some(d) => axis_words(d.chunks.axis_ids(), d.chunks.dims(), rank),
                None => format!("axis {rank}"),
            }
        };
        let facts = facts_of(&property, &pick, &plan, described.as_ref(), &words);

        let width = request.size[0];
        let mut layers: Vec<Layer> = Vec::new();
        let mut titles: Vec<Item> = vec![style::label_item(
            [LEFT, 24.0],
            format!("{title} - property insight"),
            style::text(style::TITLE, style::INK, Anchor::Start),
        )];
        let mut y = 50.0;

        // Block 1: the shape.
        titles.push(heading(y, "Shape"));
        y += 16.0;
        let mut form = Layer::new("form");
        for (slug, words) in &facts {
            y += LINE;
            form.elements.push(Element::keyed(
                format!("{uri}#fact={slug}"),
                "fact",
                vec![style::label_item(
                    [LEFT, y],
                    words.clone(),
                    style::text(style::TEXT, style::INK, Anchor::Start),
                )],
            ));
        }
        layers.push(form);
        y += 26.0;

        if let (Some(described), Some(scale), Some(colours)) = (&described, scale, &colours) {
            // Block 2: the distribution.
            let unit = property.uom.as_deref();
            let x_title = match (unit, scale.kind()) {
                (Some(unit), ScaleKind::Log10) => format!("{title} ({unit}), logarithmic scale"),
                (Some(unit), ScaleKind::Linear) => format!("{title} ({unit})"),
                (None, ScaleKind::Log10) => format!("{title}, logarithmic scale"),
                (None, ScaleKind::Linear) => title.clone(),
            };
            match described.description.binned.as_ref() {
                Some(binned) if binned.counts.iter().any(|count| *count > 0) => {
                    titles.push(heading(y, "Distribution"));
                    let area = Rect {
                        x: LEFT,
                        y: y + 20.0,
                        w: width - LEFT - RIGHT,
                        h: HISTOGRAM_HEIGHT,
                    };
                    layers.extend(distribution_layers(
                        uri,
                        area,
                        &described.description,
                        binned,
                        &scale,
                        colours,
                        &x_title,
                    ));
                    layers.push(footer_layer(
                        "distribution-footer",
                        [LEFT, area.y + area.h + 46.0],
                        format!(
                            "{} below and {} above the scale ({} not finite{}); {}",
                            binned.below,
                            binned.above,
                            binned.not_finite,
                            if scale.kind() == ScaleKind::Log10 {
                                format!(", {} not above zero", binned.not_positive)
                            } else {
                                String::new()
                            },
                            colours.source.says()
                        ),
                    ));
                    y = area.y + area.h + 66.0;
                }
                _ => notes.push(ViewNote {
                    key: "no-histogram",
                    says: "no histogram: the values have no range to divide, or none lies inside the scale"
                        .to_string(),
                }),
            }

            // Block 3: the relation with a sister.
            match relation_of(
                uri,
                described,
                &plan,
                &selection,
                rule,
                cx,
                settings.text("sibling"),
            ) {
                Ok(Some(relation)) => {
                    let density_colours = value_colours(
                        cx,
                        ID,
                        None,
                        settings.text("palette"),
                        (0.0, 1.0),
                        false,
                        Reading::Continuous,
                    )?;
                    let side = (width - LEFT - RIGHT).min(DENSITY_SIDE);
                    titles.push(heading(y, "Relation"));
                    let area = Rect {
                        x: LEFT,
                        y: y + 20.0,
                        w: side,
                        h: side,
                    };
                    let y_title = match &relation.unit {
                        Some(unit) => format!("{} ({unit})", relation.title),
                        None => relation.title.clone(),
                    };
                    let x_density = match unit {
                        Some(unit) => format!("{title} ({unit})"),
                        None => title.clone(),
                    };
                    match relation_layers(
                        uri,
                        area,
                        &relation,
                        &density_colours,
                        &x_density,
                        &y_title,
                        &facts_of_relation(&relation, rule),
                    ) {
                        Ok((relation_layers, diagonal)) => {
                            layers.extend(relation_layers);
                            if !diagonal {
                                notes.push(ViewNote {
                                    key: "diagonal-not-drawn",
                                    says: "the two scales share no value: the 1:1 diagonal is not drawn"
                                        .to_string(),
                                });
                            }
                            y = area.y + area.h + 66.0;
                        }
                        Err(why) => notes.push(ViewNote {
                            key: "relation-not-drawn",
                            says: format!("the relation with the sister is not drawn: {why}"),
                        }),
                    }
                }
                Ok(None) => notes.push(ViewNote {
                    key: "no-sibling",
                    says: "no sister property to compare with: the relation is not drawn".to_string(),
                }),
                Err(why) => notes.push(ViewNote {
                    key: "relation-not-drawn",
                    says: format!("the relation with the sister is not drawn: {why}"),
                }),
            }

            // Block 4: the variation along each axis.
            if described.description.axes.is_empty() {
                notes.push(ViewNote {
                    key: "no-axis-variation",
                    says: "no variation along an axis is measured for this property".to_string(),
                });
            } else {
                titles.push(heading(
                    y,
                    if scale.kind() == ScaleKind::Log10 {
                        "Variation along each axis (mean absolute step of log10 of the value)"
                    } else {
                        "Variation along each axis (mean absolute step)"
                    },
                ));
                let (layer, end) = variation_layer(uri, y + 20.0, width, &described.description, &words);
                layers.push(layer);
                y = end + 10.0;
            }
        } else if described.is_some() {
            notes.push(ViewNote {
                key: "no-histogram",
                says: "no histogram: the values have no range to divide (constant, or none finite), so there is no scale"
                    .to_string(),
            });
        }

        let size = [width, request.size[1].max(y + 8.0)];
        let mut list = DisplayList::new(size, PageUnit::Pixel);
        list.title = format!("{ID} - {title}");
        list.background = Some(style::PAPER);
        let mut header = Layer::new("title");
        header.elements.push(Element::new(titles));
        list.layers.push(header);
        list.layers.extend(layers);
        Ok(Drawn { list, notes })
    }
}

// ---------------------------------------------------------------------------
// Reading and describing
// ---------------------------------------------------------------------------

/// A property read slab by slab, and what the description found of it.
struct Described<'a> {
    chunks: PropertyChunks<'a>,
    description: Description,
    /// The axes whose variation was not measured: the elements have no order.
    unordered: Vec<usize>,
}

/// The elements a property hangs on, read from its supporting representation.
fn elements_of(property: &Property<'_>, source: &dyn ArraySource) -> Result<ElementAxes, String> {
    let support = property
        .supporting_representation_uri
        .as_deref()
        .ok_or_else(|| "the property names no supporting representation".to_string())?;
    let support = source.read_object_at(support)?;
    element_axes(support.as_reflect(), property.indexable).map_err(|why| why.to_string())
}

/// Read a property by the slab and describe it: the one door, [`Describer`].
fn describe_property<'a>(
    uri: &str,
    property: &Property<'a>,
    plan: &Plan,
    selection: &ChunkSelection,
    source: &'a dyn ArraySource,
) -> Result<Described<'a>, String> {
    let elements = elements_of(property, source)?;
    let chunks = PropertyChunks::new(uri, property, elements, None, None, selection, source)
        .map_err(|why| why.to_string())?;
    let mut description = run(&chunks, plan.clone()).map_err(|why| why.to_string())?;
    let unordered = keep_ordered_axes(&mut description, chunks.axis_ids());
    Ok(Described {
        chunks,
        description,
        unordered,
    })
}

/// The description of a source, stepped to the end.
fn run(source: &dyn ValueChunks, plan: Plan) -> Result<Description, DescribeRefusal> {
    let mut describer = Describer::new(source, plan, StatMethods::builtin())?;
    describer.step(source, usize::MAX)?;
    describer.finish()
}

/// The plan the settings ask for: the scale and the bars; every other choice is the description's own default.
fn plan_of(settings: &crate::setting::Settled) -> Plan {
    let scale = match settings.text("scale").unwrap_or(SCALES[0]) {
        "linear" => ScaleChoice::Linear,
        "log" => ScaleChoice::Log10,
        _ => ScaleChoice::Auto,
    };
    Plan {
        scale,
        bins: settings
            .integer("bins")
            .map_or(DEFAULT_BINS, |bins| bins.max(1) as usize),
        ..Plan::default()
    }
}

/// The rule of the ratio, checked: a threshold that is not a number is a bad setting.
fn ratio_rule(below: Option<f64>) -> Result<Option<CellRule>, ViewRefusal> {
    let Some(below) = below else { return Ok(None) };
    let rule = CellRule::Below(below);
    rule.check().map_err(|why| ViewRefusal::BadSetting {
        name: "ratio_below".to_string(),
        why: why.to_string(),
    })?;
    Ok(Some(rule))
}

/// The components asked for: `"all"`, `{"at": [..]}`, `{"along": axis, "at": [..]}` or `{"reduce": id}`.
fn pick_of(value: Option<&Value>) -> Result<ComponentPick, ViewRefusal> {
    match value {
        None => Ok(ComponentPick::All),
        Some(value) => crate::pick::component_pick("component", value, 0),
    }
}

/// What a sister adds: the pairs, the two scales, and how it is called.
struct Relation {
    summary: PairSummary,
    x: ValueScale,
    y: ValueScale,
    title: String,
    unit: Option<String>,
    kinship: Option<Kinship>,
}

/// The sister to compare with, and the pairs of the two properties; `Ok(None)` when there is none to compare with.
///
/// The sister is the one asked for (a full uri, found by the index: one door), `none` for no comparison, else the
/// first sister by direction, else the first of the same kind (a time step or a polar pair is a sister to look at,
/// not one to compare by default).
fn relation_of<'a>(
    uri: &str,
    this: &Described<'a>,
    plan: &Plan,
    selection: &ChunkSelection,
    rule: Option<CellRule>,
    cx: &ViewContext<'a>,
    asked: Option<&str>,
) -> Result<Option<Relation>, String> {
    let siblings = cx.index.siblings(uri);
    let (sibling_uri, kinship) = match asked {
        Some("none") => return Ok(None),
        Some(named) => {
            let found = cx
                .index
                .uri_of(named)
                .ok_or_else(|| format!("the package holds no `{named}`"))?;
            if found.eq_ignore_ascii_case(uri) {
                return Err("a property is not its own sister".to_string());
            }
            let kinship = siblings
                .iter()
                .find(|sibling| sibling.uri.eq_ignore_ascii_case(found))
                .map(|sibling| sibling.kinship.clone());
            (found.to_string(), kinship)
        }
        None => {
            let chosen = siblings
                .iter()
                .find(|sibling| matches!(sibling.kinship, Kinship::Direction { .. }))
                .or_else(|| {
                    siblings
                        .iter()
                        .find(|sibling| matches!(sibling.kinship, Kinship::SameKind))
                });
            match chosen {
                Some(sibling) => (sibling.uri.clone(), Some(sibling.kinship.clone())),
                None => return Ok(None),
            }
        }
    };
    let object = cx.source.read_object_at(&sibling_uri)?;
    let property = property::read(object.as_reflect()).map_err(|why| why.to_string())?;
    let other = describe_property(&sibling_uri, &property, plan, selection, cx.source)?;
    let (Some(x), Some(y)) = (this.description.scale, other.description.scale) else {
        return Err("a property has no scale (constant, or none of its values is finite)".to_string());
    };
    let mut walk = PairWalk::new(&this.chunks, &other.chunks, x, y, DENSITY_SIZE, rule)
        .map_err(|why| why.to_string())?;
    walk.step(&this.chunks, &other.chunks, usize::MAX)
        .map_err(|why| why.to_string())?;
    let summary = walk.finish().map_err(|why| why.to_string())?;
    let title = property
        .title
        .clone()
        .filter(|title| !title.is_empty())
        .or_else(|| {
            cx.index
                .node(&sibling_uri)
                .and_then(|node| node.title.as_deref().map(str::to_string))
        })
        .unwrap_or_else(|| sibling_uri.clone());
    Ok(Some(Relation {
        summary,
        x,
        y,
        title,
        unit: property.uom.clone(),
        kinship,
    }))
}

// ---------------------------------------------------------------------------
// The words
// ---------------------------------------------------------------------------

/// A number written short: three decimals at most, no trailing zeros, an exponent when very large or very small.
fn num(value: f64) -> String {
    if !value.is_finite() {
        return value.to_string();
    }
    if value != 0.0 && (value.abs() >= 1.0e6 || value.abs() < 1.0e-3) {
        return format!("{value:.3e}");
    }
    let text = format!("{value:.3}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// A share of 1 written as a percentage.
fn percent(share: f64) -> String {
    format!("{} %", num(share * 100.0))
}

/// What an axis of the values read stands for.
fn axis_words(ids: &[AxisId], dims: &[u64], rank: usize) -> String {
    let length = dims.get(rank).map_or(String::new(), |len| format!(" ({len})"));
    match ids.get(rank) {
        Some(AxisId::Time) => format!("time{length}"),
        Some(AxisId::Element { dim, ordered }) => {
            format!(
                "elements, dimension {dim}{}{length}",
                if *ordered { "" } else { ", unordered" }
            )
        }
        Some(AxisId::Component { dim }) => format!("components, dimension {dim}{length}"),
        None => format!("axis {rank}"),
    }
}

fn kind_words(kind: &PropertyKindRef) -> String {
    match kind {
        PropertyKindRef::Standard { name } => name.clone(),
        PropertyKindRef::Object { uuid, title } => title.clone().unwrap_or_else(|| uuid.clone()),
    }
}

fn pick_words(pick: &ComponentPick) -> String {
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
        } => format!(
            "along dimension {axis} at {at:?}, {reduced:?} reduced by {}",
            by.id()
        ),
    }
}

fn scale_word(kind: ScaleKind) -> &'static str {
    match kind {
        ScaleKind::Linear => "linear",
        ScaleKind::Log10 => "logarithmic",
    }
}

/// One typed observation, in a sentence.
fn observation_words(observation: &Observation, axis: &dyn Fn(usize) -> String) -> String {
    match observation {
        Observation::SpansDecades { decades, proposed } => format!(
            "the positive values span {} decades: a {} scale is proposed",
            num(*decades),
            scale_word(*proposed)
        ),
        Observation::FileSaysLinear { decades } => format!(
            "the file maps the values linearly, and they span {} decades",
            num(*decades)
        ),
        Observation::OutsideFileMinMax { count, share } => format!(
            "{count} value(s) ({}) lie outside the MinMax the file declares",
            percent(*share)
        ),
        Observation::NotANumber { count, share } => {
            format!("{count} value(s) ({}) are not finite", percent(*share))
        }
        Observation::NullValues { count, share } => format!(
            "{count} value(s) ({}) equal the declared NullValue",
            percent(*share)
        ),
        Observation::NonPositiveInLog { count } => format!(
            "{count} value(s) at or below zero cannot be placed on a logarithmic scale: counted apart"
        ),
        Observation::LogRefused(why) => {
            format!("a logarithmic scale was refused, the linear one is kept: {why}")
        }
        Observation::BinMethodUndefined { method } => {
            format!("the bin method `{method}` is undefined for these values: a fixed number of bars is used")
        }
        Observation::Modes { count } => format!("the histogram has {count} mode(s)"),
        Observation::Threshold(threshold) => {
            let between = threshold
                .between
                .map(|(low, high)| format!(" between the modes {} and {}", num(low), num(high)))
                .unwrap_or_default();
            let measure = match threshold.measure {
                ThresholdMeasure::ValleyDepth(depth) => format!("valley depth {}", num(depth)),
                ThresholdMeasure::Separability(share) => format!("separability {}", num(share)),
            };
            format!(
                "a threshold between two populations at {}{between} ({measure}, method {})",
                num(threshold.at),
                threshold.method
            )
        }
        Observation::NoThreshold(why) => format!("no threshold: {why}"),
        Observation::ConstantAlong { axis: rank } => format!("the values do not vary along {}", axis(*rank)),
        Observation::Jump(jump) => format!(
            "a seam along {} at position {}: the step is {} times the median step",
            axis(jump.axis),
            jump.at,
            num(jump.ratio_to_median)
        ),
        Observation::Strongest { axis: rank, ratio } => format!(
            "the values vary most along {}: {} times the mean of the other axes",
            axis(*rank),
            num(*ratio)
        ),
        Observation::Sampled { visited, total } => {
            format!("only {visited} of the {total} values were visited")
        }
        Observation::Sketched { relative_error } => format!(
            "the quantiles and the histogram come from a sketch, within a relative error of {}",
            num(*relative_error)
        ),
        other => format!("{other:?}"),
    }
}

/// The written facts of the shape block, each under a stable word: what the property says of itself, then what the
/// description found. A fact is a statement, never a refusal.
fn facts_of(
    property: &Property<'_>,
    pick: &ComponentPick,
    plan: &Plan,
    described: Option<&Described<'_>>,
    axis: &dyn Fn(usize) -> String,
) -> Vec<(String, String)> {
    let mut facts: Vec<(String, String)> = Vec::new();
    let mut fact = |slug: &str, words: String| facts.push((slug.to_string(), words));
    fact(
        "property",
        format!(
            "{}, on the {}, {:?} value(s) per element",
            local_type(&property.qualified_type),
            property.indexable,
            property.value_count_per_element
        ),
    );
    let unit = property
        .uom
        .as_deref()
        .map_or("no unit".to_string(), |unit| format!("unit {unit}"));
    let kind = property
        .kind
        .as_ref()
        .map_or("no kind".to_string(), |kind| format!("kind {}", kind_words(kind)));
    fact("unit", format!("{unit}, {kind}"));
    for (at, facet) in property.facets.iter().enumerate() {
        fact(
            &format!("facet-{at}"),
            format!(
                "facet {} = {}{}",
                facet.kind,
                facet.value,
                if facet.value.is_extension() {
                    " (an extension: its meaning is the producer's)"
                } else {
                    ""
                }
            ),
        );
    }
    fact("component", format!("read as {}", pick_words(pick)));
    let Some(described) = described else {
        return facts;
    };
    let description = &described.description;
    let total: u64 = description.dims.iter().product();
    fact(
        "dimensions",
        format!(
            "read as dimensions {:?}, {total} value(s), {} visited",
            description.dims, description.sampling.visited
        ),
    );
    let counts = &description.accumulator;
    fact(
        "counts",
        format!(
            "{} finite, {} not a number, {} infinite, {} null",
            counts.finite(),
            counts.not_a_number(),
            counts.infinite(),
            counts.null()
        ),
    );
    if let Some(moments) = &description.moments {
        fact(
            "moments",
            format!(
                "from {} to {}, mean {}, deviation {}",
                num(moments.min),
                num(moments.max),
                num(moments.mean),
                num(moments.deviation)
            ),
        );
    }
    if let Some(quantiles) = &description.quantiles {
        let list: Vec<String> = quantiles
            .at
            .iter()
            .zip(&quantiles.values)
            .map(|(at, value)| format!("p{} {}", num(at * 100.0), num(*value)))
            .collect();
        fact("quantiles", list.join(", "));
    }
    if let Some(scale) = &description.scale {
        fact(
            "scale",
            format!(
                "histogram on a {} scale, from {} to {} (the 1st to the 99th percentile), {} bar(s) asked",
                scale_word(scale.kind()),
                num(scale.min()),
                num(scale.max()),
                plan.bins
            ),
        );
    }
    for (at, observation) in description.observations.iter().enumerate() {
        fact(&format!("observation-{at}"), observation_words(observation, axis));
    }
    facts
}

/// The facts written beside the density.
fn facts_of_relation(relation: &Relation, rule: Option<CellRule>) -> Vec<String> {
    let summary = &relation.summary;
    let mut lines = vec![format!("sister: {}", relation.title)];
    if let Some(kinship) = &relation.kinship {
        lines.push(match kinship {
            Kinship::Direction { this, other } => format!(
                "told apart by their {} facet: {} and {}",
                this.kind, this.value, other.value
            ),
            Kinship::TimeStep { .. } => "two time steps of one property".to_string(),
            Kinship::PolarPair { .. } => "a radius and its angles".to_string(),
            Kinship::SameKind => "same kind, shape and unit".to_string(),
        });
    } else {
        lines.push("chosen by hand: not found among the sisters of the index".to_string());
    }
    lines.push(format!(
        "{} pair(s), {} dropped (a member not finite), {} with a zero denominator",
        summary.pairs, summary.dropped, summary.undefined_ratio
    ));
    lines.push(format!("{} pair(s) outside the scales", summary.density.outside));
    if let Some(correlation) = summary.correlation {
        lines.push(format!("correlation {} (on the scales shown)", num(correlation)));
    }
    if !summary.ratio_quantiles.is_empty() {
        let list: Vec<String> = summary
            .ratio_quantiles
            .iter()
            .map(|(at, value)| format!("p{} {}", num(at * 100.0), num(*value)))
            .collect();
        lines.push(format!("ratio sister / this: {}", list.join(", ")));
    }
    if let (Some(CellRule::Below(limit)), Some(share)) = (rule, summary.share_matched) {
        lines.push(format!(
            "ratio below {} in {} of the pairs ({} of {})",
            num(limit),
            percent(share),
            summary.matched,
            summary.pairs - summary.undefined_ratio
        ));
    }
    lines
}

// ---------------------------------------------------------------------------
// The drawing
// ---------------------------------------------------------------------------

fn heading(y: f64, words: &str) -> Item {
    style::label_item(
        [LEFT, y],
        words,
        style::text(style::TEXT + 1.0, style::SUBJECT, Anchor::Start),
    )
}

fn footer_layer(name: &str, at: [f64; 2], words: String) -> Layer {
    let mut layer = Layer::new(name);
    layer.elements.push(Element::new(vec![style::label_item(
        at,
        words,
        style::text(style::SMALL, style::MUTED, Anchor::Start),
    )]));
    layer
}

/// The graduations of a value scale as `(fraction along the axis, label)`, each at a value the scale places.
fn value_ticks(scale: &ValueScale, target: f64) -> Vec<(f64, String)> {
    let values: Vec<(f64, String)> = match scale.kind() {
        ScaleKind::Log10 => log_ticks(scale.min(), scale.max(), target)
            .map(|ticks| ticks.values.into_iter().map(|v| (v, log_label(v))).collect())
            .unwrap_or_default(),
        ScaleKind::Linear => nice_ticks(scale.min(), scale.max(), target)
            .map(|ticks| {
                let step = ticks.step;
                ticks.values.into_iter().map(|v| (v, label(v, step))).collect()
            })
            .unwrap_or_default(),
    };
    values
        .into_iter()
        .filter_map(|(value, words)| match scale.fraction(value) {
            Fraction::At(at) => Some((at, words)),
            _ => None,
        })
        .collect()
}

/// The graduations of a count axis, from 0 to `most`.
fn count_ticks(most: u64, target: f64) -> Vec<(f64, String)> {
    nice_ticks(0.0, most as f64, target)
        .map(|ticks| {
            let step = ticks.step;
            ticks
                .values
                .into_iter()
                .filter(|value| *value >= 0.0 && *value <= most as f64)
                .map(|value| (value / most as f64, label(value, step)))
                .collect()
        })
        .unwrap_or_default()
}

/// The frame of a plot, its grid and its graduations, each at a fraction along its axis (a value axis can be
/// logarithmic), by `style::graduated_layer`: `x` runs left to right, `y` bottom to top.
fn plot_axes(
    name: &str,
    area: Rect,
    x_ticks: &[(f64, String)],
    y_ticks: &[(f64, String)],
    x_title: &str,
    y_title: &str,
) -> Layer {
    let x: Vec<style::Tick> = x_ticks
        .iter()
        .map(|(at, words)| style::Tick {
            at: area.x + at * area.w,
            label: words.clone(),
        })
        .collect();
    let y: Vec<style::Tick> = y_ticks
        .iter()
        .map(|(at, words)| style::Tick {
            at: area.y + area.h * (1.0 - at),
            label: words.clone(),
        })
        .collect();
    style::graduated_layer(name, area, Some((&x, x_title)), (&y, y_title))
}

/// The histogram: one bar per bin of the [`Binned`], as high as its count, coloured at its middle value; the modes
/// and the threshold the description found.
fn distribution_layers(
    uri: &str,
    area: Rect,
    description: &Description,
    binned: &Binned,
    scale: &ValueScale,
    colours: &ValueColours,
    x_title: &str,
) -> Vec<Layer> {
    let most = binned.counts.iter().copied().max().unwrap_or(0).max(1);
    let axes = plot_axes(
        "distribution-axes",
        area,
        &value_ticks(scale, (area.w / 90.0).max(2.0)),
        &count_ticks(most, (area.h / 45.0).max(2.0)),
        x_title,
        "count",
    );
    let bins = binned.counts.len();
    let width = area.w / bins as f64;
    let mut bars = Layer::new("distribution");
    for (at, count) in binned.counts.iter().enumerate() {
        let height = *count as f64 / most as f64 * area.h;
        let (from, to) = binned.edges(at);
        let fill = colours
            .sample(binned.center(at))
            .map(|[r, g, b, a]| Paint::Solid(Colour::rgba(r, g, b, a)));
        let mut element = Element::keyed(
            format!("{uri}#bin={at}"),
            "bin",
            vec![Item::Rect {
                rect: Rect {
                    x: area.x + at as f64 * width,
                    y: area.y + area.h - height,
                    w: width,
                    h: height,
                },
                corner: 0.0,
                fill,
                stroke: Some(Stroke::solid(style::PAPER, 0.5)),
            }],
        );
        element.data.push(("count".to_string(), count.to_string()));
        element.data.push(("from".to_string(), num(from)));
        element.data.push(("to".to_string(), num(to)));
        bars.elements.push(element);
    }
    let mut marks = Layer::new("distribution-marks");
    for (at, mode) in description.modes.iter().enumerate() {
        let Fraction::At(fraction) = scale.fraction(mode.at) else {
            continue;
        };
        let x = area.x + fraction * area.w;
        let top = area.y + area.h - mode.height as f64 / most as f64 * area.h;
        let mut element = Element::keyed(
            format!("{uri}#mode={at}"),
            "mode",
            vec![
                Item::Symbol {
                    at: [x, top - 8.0],
                    kind: SymbolKind::TriangleDown,
                    size: 8.0,
                    fill: Some(style::HIGHLIGHT),
                    stroke: style::HIGHLIGHT,
                },
                style::label_item(
                    [x, top - 16.0],
                    num(mode.at),
                    style::text(style::SMALL, style::INK, Anchor::Middle),
                ),
            ],
        );
        element.data.push(("at".to_string(), num(mode.at)));
        marks.elements.push(element);
    }
    if let Some(threshold) = &description.threshold
        && let Fraction::At(fraction) = scale.fraction(threshold.at)
    {
        let x = area.x + fraction * area.w;
        let mut element = Element::keyed(
            format!("{uri}#threshold"),
            "threshold",
            vec![
                Item::Polyline {
                    points: vec![[x, area.y], [x, area.y + area.h]],
                    stroke: Stroke {
                        dash: vec![4.0, 3.0],
                        ..Stroke::solid(style::MARK, 1.5)
                    },
                },
                style::label_item(
                    [x + 4.0, area.y + 10.0],
                    format!("threshold {}", num(threshold.at)),
                    style::text(style::SMALL, style::MARK, Anchor::Start),
                ),
            ],
        );
        element.data.push(("at".to_string(), num(threshold.at)));
        marks.elements.push(element);
    }
    vec![axes, bars, marks]
}

/// The density of the pairs as a raster (the counts in log, as the same ramp the values use gives them), the axes of
/// the two scales, the 1:1 diagonal, and the facts beside it. The second value is whether the diagonal was drawn.
fn relation_layers(
    uri: &str,
    area: Rect,
    relation: &Relation,
    colours: &ValueColours,
    x_title: &str,
    y_title: &str,
    lines: &[String],
) -> Result<(Vec<Layer>, bool), String> {
    let grid = &relation.summary.density;
    let n = grid.n;
    let most = grid.counts.iter().copied().max().unwrap_or(0);
    if most == 0 {
        return Err("no pair falls inside the two scales".to_string());
    }
    let mut rgba = vec![0u8; n * n * 4];
    for row in 0..n {
        for col in 0..n {
            let count = grid.counts[row * n + col];
            if count == 0 {
                continue;
            }
            let t = ((1 + count) as f64).ln() / ((1 + most) as f64).ln();
            if let Some(pixel) = colours.sample(t) {
                // Row 0 of the grid is the lowest value of y: the bottom of the image.
                let at = ((n - 1 - row) * n + col) * 4;
                rgba[at..at + 4].copy_from_slice(&pixel);
            }
        }
    }
    let raster = Raster::from_rgba(n as u32, n as u32, rgba).map_err(|why| why.to_string())?;
    let axes = plot_axes(
        "relation-axes",
        area,
        &value_ticks(&relation.x, (area.w / 90.0).max(2.0)),
        &value_ticks(&relation.y, (area.h / 60.0).max(2.0)),
        x_title,
        y_title,
    );
    let mut image = Layer::new("relation");
    let mut density = Element::keyed(
        format!("{uri}#density"),
        "density",
        vec![Item::Image {
            rect: area,
            raster: Arc::new(raster),
        }],
    );
    density
        .data
        .push(("pairs".to_string(), relation.summary.pairs.to_string()));
    density.data.push(("grid".to_string(), n.to_string()));
    image.elements.push(density);

    // The 1:1 diagonal: where a value of this property equals a value of the sister, on the two scales.
    let mut runs: Vec<Vec<[f64; 2]>> = Vec::new();
    let mut open: Vec<[f64; 2]> = Vec::new();
    for step in 0..=DIAGONAL_STEPS {
        let at = step as f64 / DIAGONAL_STEPS as f64;
        match relation.y.fraction(relation.x.value_at(at)) {
            Fraction::At(up) => open.push([area.x + at * area.w, area.y + area.h * (1.0 - up)]),
            _ => {
                if open.len() >= 2 {
                    runs.push(std::mem::take(&mut open));
                } else {
                    open.clear();
                }
            }
        }
    }
    if open.len() >= 2 {
        runs.push(open);
    }
    let diagonal = !runs.is_empty();
    if diagonal {
        let items = runs
            .into_iter()
            .map(|points| Item::Polyline {
                points,
                stroke: Stroke {
                    dash: vec![5.0, 4.0],
                    ..Stroke::solid(style::MARK, 1.5)
                },
            })
            .collect();
        image
            .elements
            .push(Element::keyed(format!("{uri}#diagonal"), "diagonal", items));
    }
    let mut beside = Layer::new("relation-facts");
    for (at, line) in lines.iter().enumerate() {
        beside.elements.push(Element::keyed(
            format!("{uri}#relation={at}"),
            "fact",
            vec![style::label_item(
                [area.x + area.w + 30.0, area.y + 12.0 + at as f64 * LINE],
                line.clone(),
                style::text(style::TEXT, style::INK, Anchor::Start),
            )],
        ));
    }
    let footer = footer_layer(
        "relation-footer",
        [area.x, area.y + area.h + 46.0],
        format!(
            "the counts in a logarithmic ramp, from 1 to {most} pair(s) a cell; {}",
            colours.source.says()
        ),
    );
    Ok((vec![axes, image, beside, footer], diagonal))
}

/// The variation along each axis: one horizontal bar per axis of the description, the strongest marked. Returns the
/// layer and the page position under it.
fn variation_layer(
    uri: &str,
    top: f64,
    width: f64,
    description: &Description,
    axis: &dyn Fn(usize) -> String,
) -> (Layer, f64) {
    let longest = description
        .axes
        .iter()
        .filter_map(|variation| variation.mean_abs_step)
        .fold(0.0_f64, f64::max);
    let room = (width - LEFT - RIGHT - LABEL_WIDTH - VALUE_WIDTH).max(1.0);
    let strongest = description.strongest.map(|(rank, _)| rank);
    let mut layer = Layer::new("variation");
    for (row, variation) in description.axes.iter().enumerate() {
        let y = top + row as f64 * AXIS_ROW;
        let bar_x = LEFT + LABEL_WIDTH;
        let mut items = vec![style::label_item(
            [bar_x - 8.0, y + 14.0],
            axis(variation.axis),
            style::text(style::TEXT, style::INK, Anchor::End),
        )];
        let marked = strongest == Some(variation.axis);
        let written = match variation.mean_abs_step {
            Some(step) => {
                let length = if longest > 0.0 { step / longest * room } else { 0.0 };
                items.push(Item::Rect {
                    rect: Rect {
                        x: bar_x,
                        y: y + 3.0,
                        w: length,
                        h: AXIS_ROW - 8.0,
                    },
                    corner: 0.0,
                    fill: Some(Paint::Solid(if marked {
                        style::HIGHLIGHT
                    } else {
                        style::SUBJECT
                    })),
                    stroke: None,
                });
                if variation.constant {
                    "constant".to_string()
                } else {
                    num(step)
                }
            }
            None => "no step".to_string(),
        };
        items.push(style::label_item(
            [
                bar_x
                    + variation.mean_abs_step.map_or(0.0, |step| {
                        if longest > 0.0 { step / longest * room } else { 0.0 }
                    })
                    + 6.0,
                y + 14.0,
            ],
            if marked {
                format!("{written}  (strongest)")
            } else {
                written
            },
            style::text(style::SMALL, style::MUTED, Anchor::Start),
        ));
        let mut element = Element::keyed(format!("{uri}#axis={}", variation.axis), "axis", items);
        if marked {
            element.data.push(("strongest".to_string(), "true".to_string()));
        }
        if let Some(step) = variation.mean_abs_step {
            element.data.push(("mean-abs-step".to_string(), num(step)));
        }
        layer.elements.push(element);
    }
    (layer, top + description.axes.len() as f64 * AXIS_ROW)
}
