//! `table-curve`: the curves of a table, like KrPc (relative permeabilities and capillary pressure against the
//! saturation) or PVT.
//!
//! The pairs of columns are the library's proposal (`energyml_domain::table::curve_pairs` over
//! `classify_column`), or the ones a `pairs` setting names. The points of a curve are the cells of
//! `Table::read_cells`, in the order of the rows; a unit is converted by `energyml_domain::units::convert` only. The
//! colours of the curves are the categorical palette of `energyml_domain::palettes`. This module lays them out: one
//! polyline per pair with its points, a legend per column with its title and unit, and a second ordinate axis when
//! two families of units cross (a pressure and a number with no unit).

use energyml_domain::package_index::PackageIndex;
use energyml_domain::palettes::Reading;
use energyml_domain::property_kind::Resolver;
use energyml_domain::table::{
    Cell, CellState, ColumnCells, ColumnClass, Table, TableSheet, classify_column, curve_pairs, resolve_kinds,
};
use energyml_domain::units::{self, Dimension};
use energyml_sheet::draw::{
    Anchor, Colour, DisplayList, Element, Item, Layer, PageUnit, Rect, Stroke, SymbolKind, TextStyle,
};
use energyml_sheet::text::short;
use serde_json::Value;

use crate::colour::ColourSource;
use crate::kind::{
    Drawn, ObjectSlot, Offer, Request, ViewContext, ViewKind, ViewNote, column_name, local_type, table_name,
    title_or_uri,
};
use crate::refusal::ViewRefusal;
use crate::setting::ViewSetting;
use crate::style::{self, PlotFrame};

/// The id of the view.
pub const ID: &str = "table-curve";

/// The types it takes: a column-based table, and the 2.0.1 double lookup.
pub const TYPES: &[&str] = &["ColumnBasedTable", "DoubleTableLookup"];

/// The view.
pub struct TableCurve;

impl ViewKind for TableCurve {
    fn id(&self) -> &'static str {
        ID
    }

    fn name(&self) -> &'static str {
        "Table curve"
    }

    fn about(&self) -> &'static str {
        "the curves of a table (KrPc, PVT): one curve per pair of numeric columns, a legend per column with its unit, a second ordinate axis when two families of units cross"
    }

    fn objects(&self) -> Vec<ObjectSlot> {
        vec![ObjectSlot {
            role: "table",
            types: TYPES,
            least: 1,
            most: 1,
            about: "a column-based table or a double table lookup",
        }]
    }

    fn settings(&self) -> Vec<ViewSetting> {
        vec![
            ViewSetting::json(
                "pairs",
                "the curves to draw, a list of couples of columns `[x, y]` (a column by its rank from 0, or by its title); none: the pairs the library proposes",
            ),
            ViewSetting::text(
                "palette",
                "the categorical palette the curves are coloured with; none: the default one",
            ),
        ]
    }

    fn size(&self) -> [f64; 2] {
        [900.0, 560.0]
    }

    fn offers(&self, index: &PackageIndex) -> Vec<Offer> {
        let mut offers: Vec<Offer> = index
            .tables()
            .filter(|(_, sheet)| TYPES.contains(&local_type(&sheet.qualified_type)))
            .filter(|(_, sheet)| {
                // Without the kinds: a depth column then reads as a real one, which is numeric all the same.
                let classes: Vec<ColumnClass> = sheet
                    .columns
                    .iter()
                    .map(|column| classify_column(column, None, None))
                    .collect();
                !curve_pairs(sheet, &classes).is_empty()
            })
            .map(|(uri, sheet)| Offer {
                view: ID,
                objects: vec![uri.to_string()],
                title: table_name(sheet),
                missing: Vec::new(),
                proposed: Vec::new(),
                part: None,
            })
            .collect();
        offers.sort_by(|a, b| a.objects.cmp(&b.objects));
        offers
    }

    fn draw(&self, request: &Request, cx: &ViewContext<'_>) -> Result<Drawn, ViewRefusal> {
        let refuse = |why: String| ViewRefusal::computing(ID, why);
        let uri = request
            .object("table")
            .ok_or_else(|| refuse("no table given".to_string()))?;
        let sheet = cx
            .index
            .table_sheet(uri)
            .ok_or_else(|| refuse(format!("`{uri}` has no sheet: the table does not read")))?;
        let object = cx.source.read_object_at(uri).map_err(refuse)?;
        let table = Table::read(object.as_reflect()).map_err(|why| refuse(why.to_string()))?;
        if table.columns.len() != sheet.columns.len() {
            return Err(refuse(format!(
                "the sheet of `{uri}` has {} columns and the table {}",
                sheet.columns.len(),
                table.columns.len()
            )));
        }
        let cells = table.read_cells(cx.source);

        // What each column is.
        let resolver = Resolver::new(cx.source);
        let kinds = resolve_kinds(sheet, &resolver);
        let classes: Vec<ColumnClass> = sheet
            .columns
            .iter()
            .enumerate()
            .map(|(at, column)| {
                classify_column(
                    column,
                    kinds.get(at).and_then(|kind| kind.as_ref()),
                    cells.get(at).and_then(|read| read.as_ref().ok()),
                )
            })
            .collect();

        // The pairs: the setting's, which wins, else the library's proposal.
        let pairs = match request.settings.get("pairs") {
            Some(asked) => pairs_asked(asked, sheet, &classes)?,
            None => {
                let proposed = curve_pairs(sheet, &classes);
                if proposed.is_empty() {
                    return Err(refuse(format!(
                        "the table `{}` has no pair of numeric columns to draw as a curve",
                        table_name(sheet)
                    )));
                }
                proposed
            }
        };

        // The palette: a categorical one, the request's or the default.
        let palette = match request.settings.text("palette") {
            Some(name) => crate::colour::palette_named(cx, name, Reading::Categorical, "curves")?,
            None => cx
                .palettes
                .default_for(Reading::Categorical)
                .ok_or_else(|| ViewRefusal::Palette {
                    asked: String::new(),
                    why: "no categorical palette".to_string(),
                })?,
        };

        // The curves, from the cells.
        let mut notes: Vec<ViewNote> = Vec::new();
        let mut axes: Vec<Axis> = Vec::new();
        let mut curves: Vec<Curve> = Vec::new();
        let mut x_unit: Option<Option<String>> = None;
        let mut x_columns: Vec<usize> = Vec::new();
        for (x, y) in pairs {
            let x_values = numbers(sheet, &cells, x)?;
            let y_values = match numbers(sheet, &cells, y) {
                Ok(values) => values,
                Err(why) => {
                    notes.push(ViewNote {
                        key: "curve-not-drawn",
                        says: format!("the curve of `{}` is not drawn: {why}", column_name(sheet, y)),
                    });
                    continue;
                }
            };
            if x_values.len() != y_values.len() {
                return Err(refuse(format!(
                    "`{}` has {} rows and `{}` has {}: a row is the same position in every column, so no curve is drawn",
                    column_name(sheet, x),
                    x_values.len(),
                    column_name(sheet, y),
                    y_values.len()
                )));
            }
            // The abscissa unit is the first curve's; another is converted to it, or refused.
            let this_x = uom_of(sheet, x);
            let shown_x = x_unit.get_or_insert_with(|| this_x.clone()).clone();
            let x_factor = convert_between(this_x.as_deref(), shown_x.as_deref())
                .map_err(|why| refuse(format!("the abscissa `{}`: {why}", column_name(sheet, x))))?;
            if !x_columns.contains(&x) {
                x_columns.push(x);
            }
            // The ordinate axis of its family of units: the first two families get one each.
            let this_y = uom_of(sheet, y);
            let family = family_of(this_y.as_deref());
            let axis = match axes.iter().position(|axis| axis.family == family) {
                Some(at) => at,
                None if axes.len() < 2 => {
                    axes.push(Axis {
                        family,
                        uom: this_y.clone().filter(|_| !is_unitless(this_y.as_deref())),
                        columns: Vec::new(),
                    });
                    axes.len() - 1
                }
                None => {
                    notes.push(ViewNote {
                        key: "curve-not-drawn",
                        says: format!(
                            "the curve of `{}` is not drawn: a third family of units ({}), and a plot has two ordinate axes",
                            column_name(sheet, y),
                            this_y.as_deref().unwrap_or("no unit")
                        ),
                    });
                    continue;
                }
            };
            let to_axis = convert_between(this_y.as_deref(), axes[axis].uom.as_deref().or(this_y.as_deref()))
                .map_err(|why| refuse(format!("the ordinate `{}`: {why}", column_name(sheet, y))))?;
            let mut points = Vec::new();
            let mut left_out = 0usize;
            for (row, (xv, yv)) in x_values.iter().zip(&y_values).enumerate() {
                match (xv, yv) {
                    (Some(xv), Some(yv)) => {
                        points.push((row, [xv * x_factor.0 + x_factor.1, yv * to_axis.0 + to_axis.1]))
                    }
                    _ => left_out += 1,
                }
            }
            if left_out > 0 {
                notes.push(ViewNote {
                    key: "rows-not-drawn",
                    says: format!(
                        "`{}` against `{}`: {left_out} rows have no value in one of the two columns and are not drawn",
                        column_name(sheet, y),
                        column_name(sheet, x)
                    ),
                });
            }
            if points.is_empty() {
                notes.push(ViewNote {
                    key: "curve-not-drawn",
                    says: format!(
                        "the curve of `{}` is not drawn: it has no row with both values",
                        column_name(sheet, y)
                    ),
                });
                continue;
            }
            axes[axis].columns.push(y);
            curves.push(Curve { x, y, axis, points });
        }
        if curves.is_empty() {
            return Err(refuse(format!(
                "no curve of the table `{}` could be drawn",
                table_name(sheet)
            )));
        }

        let title = title_or_uri(cx.index, uri);
        let list = picture(
            request.size,
            uri,
            &title,
            sheet,
            &curves,
            &axes,
            &x_columns,
            x_unit.flatten().as_deref(),
            &palette,
        );
        Ok(Drawn { list, notes })
    }
}

/// One curve: the columns, the ordinate axis it is drawn against, and its points as `(row, [x, y])` in the units of
/// the axes, in the order of the rows.
struct Curve {
    x: usize,
    y: usize,
    axis: usize,
    points: Vec<(usize, [f64; 2])>,
}

/// A family of units: what two columns must share to be drawn against one axis.
#[derive(Clone, Debug, PartialEq)]
enum Family {
    /// A family `energyml_domain::units` converts within.
    Dimension(Dimension),
    /// No unit.
    Unitless,
    /// A unit the conversion table does not know: only itself.
    Other(String),
}

/// One ordinate axis.
struct Axis {
    family: Family,
    /// The unit the axis is drawn in: the first column's.
    uom: Option<String>,
    /// The columns drawn against it.
    columns: Vec<usize>,
}

fn is_unitless(uom: Option<&str>) -> bool {
    matches!(uom, None | Some("Euc"))
}

fn family_of(uom: Option<&str>) -> Family {
    match uom {
        _ if is_unitless(uom) => Family::Unitless,
        Some(unit) => match Dimension::of(unit) {
            Some(dimension) => Family::Dimension(dimension),
            None => Family::Other(unit.to_string()),
        },
        None => Family::Unitless,
    }
}

/// The affine map `(scale, offset)` from one unit to another: the identity when they are the same or when the
/// column has no unit, a conversion of `energyml_domain::units::convert` otherwise, refused when it cannot be.
fn convert_between(from: Option<&str>, to: Option<&str>) -> Result<(f64, f64), String> {
    match (from, to) {
        (Some(from), Some(to)) if from != to && !is_unitless(Some(from)) && !is_unitless(Some(to)) => {
            // Every table unit converts by a factor: two values give the scale, and zero the offset.
            let zero = units::convert(0.0, from, to).map_err(|why| why.to_string())?;
            let one = units::convert(1.0, from, to).map_err(|why| why.to_string())?;
            Ok((one - zero, zero))
        }
        (from, to) if from == to || (is_unitless(from) && is_unitless(to)) => Ok((1.0, 0.0)),
        (from, to) => Err(format!(
            "the unit {} cannot be brought to {}",
            from.unwrap_or("none"),
            to.unwrap_or("none")
        )),
    }
}

/// The unit of a column, from the sheet.
fn uom_of(sheet: &TableSheet, column: usize) -> Option<String> {
    sheet
        .columns
        .get(column)
        .and_then(|found| found.uom.as_deref())
        .map(str::to_string)
}

/// A column with its unit, as the legend and the axes write it.
fn column_label(sheet: &TableSheet, column: usize) -> String {
    match uom_of(sheet, column) {
        Some(uom) => format!("{} ({uom})", column_name(sheet, column)),
        None => column_name(sheet, column),
    }
}

/// The numbers of a column, one per row, in the order of the rows; `None` where the cell is no number (an absence,
/// a text). The cells are `Table::read_cells`'s: the one reading.
fn numbers(
    sheet: &TableSheet,
    cells: &[Result<ColumnCells<'static>, energyml_domain::table::TableRefusal>],
    column: usize,
) -> Result<Vec<Option<f64>>, ViewRefusal> {
    let name = column_name(sheet, column);
    let read = match cells.get(column) {
        Some(Ok(read)) => read,
        Some(Err(why)) => return Err(ViewRefusal::computing(ID, format!("column `{name}`: {why}"))),
        None => return Err(ViewRefusal::computing(ID, format!("no column {column}"))),
    };
    let rows = read
        .rows(&name)
        .map_err(|why| ViewRefusal::computing(ID, why.to_string()))?;
    Ok((0..rows)
        .map(|row| match read.cell(row) {
            Some(CellState::Value(Cell::Number(value))) if value.is_finite() => Some(value),
            _ => None,
        })
        .collect())
}

/// The pairs a `pairs` setting names, checked: columns that exist, numeric and scalar, two different ones.
fn pairs_asked(
    asked: &Value,
    sheet: &TableSheet,
    classes: &[ColumnClass],
) -> Result<Vec<(usize, usize)>, ViewRefusal> {
    let bad = |why: String| ViewRefusal::BadSetting {
        name: "pairs".to_string(),
        why,
    };
    let Some(list) = asked.as_array() else {
        return Err(bad(
            "a list of couples of columns, like [[0, 1], [0, 2]]".to_string()
        ));
    };
    if list.is_empty() {
        return Err(bad("the list is empty: name at least one couple".to_string()));
    }
    let column_of = |value: &Value| -> Result<usize, ViewRefusal> {
        if let Some(rank) = value.as_u64() {
            return if (rank as usize) < sheet.columns.len() {
                Ok(rank as usize)
            } else {
                Err(bad(format!(
                    "there is no column {rank}: the table has {}",
                    sheet.columns.len()
                )))
            };
        }
        if let Some(title) = value.as_str() {
            let found: Vec<usize> = sheet
                .columns
                .iter()
                .enumerate()
                .filter(|(_, column)| column.title.as_deref() == Some(title))
                .map(|(at, _)| at)
                .collect();
            return match found.as_slice() {
                [one] => Ok(*one),
                [] => Err(bad(format!("no column titled `{title}`"))),
                _ => Err(bad(format!(
                    "{} columns are titled `{title}`: name one by its rank",
                    found.len()
                ))),
            };
        }
        Err(bad(format!(
            "`{value}` is neither a column rank nor a column title"
        )))
    };
    let mut out = Vec::new();
    for couple in list {
        let Some([x, y]) = couple
            .as_array()
            .map(Vec::as_slice)
            .and_then(|c| <&[Value; 2]>::try_from(c).ok())
        else {
            return Err(bad(format!("`{couple}` is not a couple [x, y]")));
        };
        let (x, y) = (column_of(x)?, column_of(y)?);
        if x == y {
            return Err(bad(format!(
                "the couple names the column `{}` twice",
                column_name(sheet, x)
            )));
        }
        for column in [x, y] {
            if !matches!(
                classes.get(column),
                Some(ColumnClass::Real { .. } | ColumnClass::Depth { .. } | ColumnClass::Integer)
            ) {
                return Err(ViewRefusal::computing(
                    ID,
                    format!(
                        "column `{}` is {:?}, not a number: it cannot be a curve",
                        column_name(sheet, column),
                        classes
                            .get(column)
                            .map(|class| format!("{class:?}"))
                            .unwrap_or_default()
                    ),
                ));
            }
        }
        out.push((x, y));
    }
    Ok(out)
}

/// The axis title of a set of columns: their titles, and the unit.
fn axis_title(sheet: &TableSheet, columns: &[usize], uom: Option<&str>) -> String {
    let mut titles: Vec<String> = Vec::new();
    for column in columns {
        let name = column_name(sheet, *column);
        if !titles.contains(&name) {
            titles.push(name);
        }
    }
    match uom {
        Some(uom) if !is_unitless(Some(uom)) => format!("{} ({uom})", titles.join(", ")),
        _ => titles.join(", "),
    }
}

fn ink(rgb: [u8; 3]) -> Colour {
    Colour::rgb(rgb[0], rgb[1], rgb[2])
}

#[allow(clippy::too_many_arguments)]
fn picture(
    size: [f64; 2],
    uri: &str,
    title: &str,
    sheet: &TableSheet,
    curves: &[Curve],
    axes: &[Axis],
    x_columns: &[usize],
    x_uom: Option<&str>,
    palette: &energyml_domain::palettes::Palette,
) -> DisplayList {
    let second = axes.len() == 2;
    let legend_height = 14.0 * (curves.len() as f64 + 1.0) + 8.0;
    let area = Rect {
        x: 64.0,
        y: 40.0,
        w: (size[0] - 64.0 - if second { 72.0 } else { 24.0 }).max(20.0),
        h: (size[1] - 40.0 - 52.0 - legend_height).max(20.0),
    };
    let range = |keep: &dyn Fn(&Curve) -> bool, pick: usize| {
        let values = curves
            .iter()
            .filter(|curve| keep(curve))
            .flat_map(|curve| curve.points.iter().map(move |(_, p)| p[pick]));
        let low = values.clone().fold(f64::INFINITY, f64::min);
        let high = values.fold(f64::NEG_INFINITY, f64::max);
        [low, high]
    };
    let x_range = range(&|_| true, 0);
    let frames: Vec<PlotFrame> = (0..axes.len())
        .map(|at| PlotFrame::new(area, x_range, range(&|curve| curve.axis == at, 1), false, false))
        .collect();

    let mut list = DisplayList::new(size, PageUnit::Pixel);
    list.title = format!("{ID} - {title}");
    list.background = Some(style::PAPER);
    let mut header = Layer::new("title");
    header.elements.push(Element::new(vec![style::label_item(
        [area.x, 24.0],
        title,
        style::text(style::TITLE, style::INK, Anchor::Start),
    )]));
    list.layers.push(header);
    let x_title = axis_title(sheet, x_columns, x_uom);
    let left_title = axis_title(sheet, &axes[0].columns, axes[0].uom.as_deref());
    list.layers
        .push(style::axes_layer(&frames[0], &x_title, &left_title));
    if second {
        list.layers.push(right_axis_layer(
            &frames[1],
            &axis_title(sheet, &axes[1].columns, axes[1].uom.as_deref()),
        ));
    }

    // The curves, clipped to the plot area.
    let mut layer = Layer::new("curves");
    layer.clip = Some(area);
    for (rank, curve) in curves.iter().enumerate() {
        let colour = ink(palette.category(rank));
        let frame = &frames[curve.axis];
        let dash = if curve.axis == 1 {
            vec![6.0, 3.0]
        } else {
            Vec::new()
        };
        let placed: Vec<[f64; 2]> = curve.points.iter().map(|(_, p)| frame.place(*p)).collect();
        if placed.len() >= 2 {
            let mut stroke = Stroke::solid(colour, 2.0);
            stroke.dash = dash;
            layer.elements.push(Element::keyed(
                format!("{uri}#col={}", curve.y),
                "curve",
                vec![Item::Polyline {
                    points: placed.clone(),
                    stroke,
                }],
            ));
        }
        for ((row, data), at) in curve.points.iter().zip(&placed) {
            let mut point = Element::keyed(
                format!("{uri}#col={}&row={row}", curve.y),
                "point",
                vec![Item::Symbol {
                    at: *at,
                    kind: SymbolKind::Circle,
                    size: 5.0,
                    fill: Some(colour),
                    stroke: colour,
                }],
            );
            point.title = format!("row {row}: {}, {}", short(data[0], 4), short(data[1], 4));
            layer.elements.push(point);
        }
    }
    list.layers.push(layer);

    // The legend: one line per column, then where the colours come from.
    let mut legend = Layer::new("legend");
    let top = area.y + area.h + 46.0;
    for (rank, curve) in curves.iter().enumerate() {
        let colour = ink(palette.category(rank));
        let y = top + 14.0 * rank as f64;
        let mut stroke = Stroke::solid(colour, 2.0);
        if curve.axis == 1 {
            stroke.dash = vec![6.0, 3.0];
        }
        let side = if curve.axis == 1 { ", right axis" } else { "" };
        legend.elements.push(Element::keyed(
            format!("{uri}#col={}", curve.y),
            "legend",
            vec![
                Item::Polyline {
                    points: vec![[area.x, y], [area.x + 24.0, y]],
                    stroke,
                },
                style::label_item(
                    [area.x + 30.0, y + 3.5],
                    format!(
                        "{} against {}{side}",
                        column_label(sheet, curve.y),
                        column_name(sheet, curve.x)
                    ),
                    style::text(style::TEXT, style::INK, Anchor::Start),
                ),
            ],
        ));
    }
    legend.elements.push(Element::new(vec![style::label_item(
        [area.x, top + 14.0 * curves.len() as f64 + 3.5],
        ColourSource::Palette(palette.name.to_string()).says(),
        style::text(style::SMALL, style::MUTED, Anchor::Start),
    )]));
    list.layers.push(legend);
    list
}

/// The graduations and the title of the second ordinate axis, on the right of the plot. The grid is the first
/// axis's.
fn right_axis_layer(frame: &PlotFrame, y_title: &str) -> Layer {
    let mut layer = Layer::new("axis-right");
    let area = frame.area;
    let mut marks = Vec::new();
    // The graduations are the plot's (`style::linear_ticks`): the same values the left axis writes.
    for tick in style::linear_ticks(&frame.y, (area.h / 60.0).max(2.0)) {
        marks.push(Item::Polyline {
            points: vec![[area.x + area.w, tick.at], [area.x + area.w + 4.0, tick.at]],
            stroke: Stroke::solid(style::MUTED, 1.0),
        });
        marks.push(style::label_item(
            [area.x + area.w + 7.0, tick.at + 3.5],
            tick.label,
            style::text(style::SMALL, style::MUTED, Anchor::Start),
        ));
    }
    marks.push(Item::Text {
        at: [area.x + area.w + 62.0, area.y + area.h / 2.0],
        text: y_title.to_string(),
        style: TextStyle {
            rotate: 90.0,
            ..style::text(style::TEXT, style::MUTED, Anchor::Middle)
        },
    });
    layer.elements.push(Element::new(marks));
    layer
}
