//! What lies under the pointer, and the way back from the data to a pixel.

use energyml_domain::along_md::{GapRule, Interpolations, Nature, interpolate_row};
use energyml_domain::units;
use energyml_geometry::around_well::projection::WellProjections;

use super::cells::CurtainRefusal;
use super::layout::{View, plot_frame};
use super::model::{AlongWellRefusal, Model, ProjectionChoice};

/// What lies under a pixel: numbers only; the sentence is the face's.
#[derive(Clone, Debug, PartialEq)]
pub struct Readout {
    /// The measured depth under the pixel.
    pub md: f64,
    /// The offset along the high side under the pixel.
    pub dn: f64,
    /// The abscissa of the drawing.
    pub abscissa: f64,
    /// The true vertical depth.
    pub tvd: f64,
    /// The node of the cell under the pixel, when there is a cell.
    pub node: Option<u32>,
    /// The row of the cell.
    pub row: Option<u32>,
    /// The value, when the cell has one.
    pub value: Option<f64>,
    /// The unit of the value, as the file writes it.
    pub unit: Option<String>,
    /// `r_above - dn` at the node, when the boundaries are there.
    pub to_above: Option<f64>,
    /// `dn - (-r_below)` at the node, when the boundaries are there.
    pub to_below: Option<f64>,
    /// The nearest horizon point in the same column: (surface uri, signed distance in `dn`).
    pub nearest_horizon: Option<(String, f64)>,
}

/// The node whose bounds in MD contain `md`.
fn node_at(model: &Model, md: f64) -> Option<usize> {
    let curtain = model.curtain.as_ref()?;
    curtain.layout.md_bounds.iter().position(|bounds| match bounds {
        Some([from, to]) => md >= *from && md <= *to,
        None => false,
    })
}

/// The node nearest to `md`, whether or not it has a cell.
fn nearest_node(model: &Model, md: f64) -> Option<usize> {
    model
        .node_md
        .iter()
        .enumerate()
        .min_by(|a, b| (a.1 - md).abs().total_cmp(&(b.1 - md).abs()))
        .map(|(node, _)| node)
}

/// The value between nodes along the MD, within the row, by one of the
/// domain's interpolations (never across the rows, WP03 C13).
fn interpolated(
    model: &Model,
    id: &str,
    md: f64,
    row: usize,
) -> Result<Option<f64>, AlongWellRefusal> {
    let methods = Interpolations::builtin();
    let method = methods
        .get(id)
        .ok_or_else(|| AlongWellRefusal::UnknownInterpolation {
            id: id.to_string(),
            known: methods.ids(),
        })?;
    let Some(curtain) = &model.curtain else {
        return Ok(None);
    };
    let rows = curtain.layout.rows_up.len();
    let nodes = &model.node_md;
    let Some(lower) = nodes
        .windows(2)
        .position(|pair| md >= pair[0] && md <= pair[1])
    else {
        return Ok(None);
    };
    let (a, b) = (nodes[lower], nodes[lower + 1]);
    let t = (md - a) / (b - a);
    let rule = GapRule::of_axis(nodes).map_err(CurtainRefusal::from)?;
    let angle = curtain
        .columns
        .unit
        .as_deref()
        .is_some_and(|uom| units::radians(1.0, uom).is_ok());
    // What the model knows of the values: floats of a cube, so neither
    // categories nor integers; an angle when the unit says so.
    let nature = Nature {
        angle,
        ..Nature::default()
    };
    let values = &curtain.columns.values;
    let row_value = |node: usize| values[node * rows + row];
    let found = interpolate_row(
        &[row_value(lower)],
        &[row_value(lower + 1)],
        t,
        b - a,
        &rule,
        method,
        &nature,
    )?;
    Ok(found.values.first().copied().filter(|value| !value.is_nan()))
}

/// What lies under `pixel`: the measured depth and offset by the way back of
/// the projection (`WellProjection::invert`), the cell and its value, the
/// distances to the boundaries and to the nearest horizon of the column.
///
/// `interpolation`: `None`, the cell's own value (nearest, the default: no
/// value made up); `Some(id)`, one of the domain's `Interpolations`, along the
/// MD within the row.
///
/// `Ok(None)`: the pixel is outside the frame, or the projection cannot say
/// (`plan` and `md_bar` do not invert).
///
/// # Errors
///
/// An unknown projection or interpolation, an interpolation that refuses.
pub fn pick(
    model: &Model,
    view: &View,
    projections: &WellProjections,
    pixel: [f64; 2],
    interpolation: Option<&str>,
) -> Result<Option<Readout>, AlongWellRefusal> {
    let choice = ProjectionChoice::of(&model.request);
    let projection = choice.get(&model.request.projection, projections)?;
    let plot = plot_frame(model, view);
    if !plot.contains(pixel) {
        return Ok(None);
    }
    let data = plot.to_data(pixel);
    let Some([md, dn]) = projection.invert(&model.axis, data) else {
        return Ok(None);
    };

    let mut readout = Readout {
        md,
        dn,
        abscissa: data[0],
        tvd: data[1],
        node: None,
        row: None,
        value: None,
        unit: None,
        to_above: None,
        to_below: None,
        nearest_horizon: None,
    };

    if let (Some(curtain), Some(node)) = (&model.curtain, node_at(model, md)) {
        let rows = curtain.layout.rows_up.len();
        let row = curtain
            .layout
            .row_bounds
            .iter()
            .position(|[low, high]| dn >= *low && dn <= *high);
        if let Some(row) = row {
            readout.node = Some(node as u32);
            readout.row = Some(row as u32);
            readout.unit = curtain.columns.unit.clone();
            readout.value = match interpolation {
                None => Some(curtain.columns.values[node * rows + row]).filter(|value| !value.is_nan()),
                Some(id) => interpolated(model, id, md, row)?,
            };
        }
    }

    if let (Some(found), Some(node)) = (&model.boundaries, nearest_node(model, md)) {
        if let Some(above) = found.above[node] {
            readout.to_above = Some(above.dn - dn);
        }
        if let Some(below) = found.below[node] {
            readout.to_below = Some(dn - below.dn);
        }
    }

    if let Some(node) = nearest_node(model, md) {
        let column_md = model.node_md[node];
        let mut best: Option<(String, f64)> = None;
        for trace in &model.horizons {
            for point in trace.spans.iter().flatten().filter(|p| p.md == column_md) {
                let distance = point.dn - dn;
                if best
                    .as_ref()
                    .is_none_or(|(_, kept)| distance.abs() < kept.abs())
                {
                    best = Some((trace.surface.clone(), distance));
                }
            }
        }
        readout.nearest_horizon = best;
    }
    Ok(Some(readout))
}

/// The pixel of `(md, dn)`: [`pick`]'s way back. `None` where the projection
/// refuses the point.
pub fn pixel_of(
    model: &Model,
    view: &View,
    projections: &WellProjections,
    md: f64,
    dn: f64,
) -> Option<[f64; 2]> {
    let choice = ProjectionChoice::of(&model.request);
    let projection = choice.get(&model.request.projection, projections).ok()?;
    let placed = projection.place(&model.axis, md, dn).ok()?;
    Some(plot_frame(model, view).to_pixel(placed))
}
