//! The cells of a curtain: one column per node of the frame, one row per
//! offset, each cell a quadrilateral placed by one projection.
//!
//! Folds are **marked, never straightened** and gaps are **never bridged**
//! (S7): a cell is only ever made from what the file states.

use energyml_domain::along_md::{GapRule, StepRefusal};
use energyml_geometry::around_well::projection::{ProjectionRefusal, WellAxis, WellProjection};
use energyml_sheet::draw::DrawError;
use energyml_sheet::draw::raster::node_covers;

/// Where the cells of a curtain begin and end, in the frame of the hole,
/// before anything is placed.
#[derive(Clone, Debug, PartialEq)]
pub struct CurtainLayout {
    /// The measured depths of the frame's nodes, strictly increasing.
    pub node_md: Vec<f64>,
    /// The offsets of the rows along the high side, metres, positive up;
    /// strictly monotonic, in the order of the values.
    pub rows_up: Vec<f64>,
    /// The offset of the plane along `right`, metres (0: the plane of the hole).
    pub lateral: f64,
    /// `[from, to]` in MD of each node's cells; `None`: a node with no width.
    pub md_bounds: Vec<Option<[f64; 2]>>,
    /// `[low, high]` in `dn` of each row's cells.
    pub row_bounds: Vec<[f64; 2]>,
    /// `[to of node n, from of node n + 1]` wherever the gap rule refused to bridge.
    pub gaps: Vec<[f64; 2]>,
}

/// What a curtain refuses, by name.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CurtainRefusal {
    /// The nodes do not make an axis.
    #[error("the frame's measured depths do not make a curtain: {0}")]
    Nodes(#[from] StepRefusal),
    /// Fewer than two rows.
    #[error("{given} row(s): a curtain needs two offsets to have a height")]
    TooFewRows {
        /// How many rows were given.
        given: usize,
    },
    /// The offsets of the rows are not strictly monotonic.
    #[error("the offsets of the rows stop being monotonic at row {at}")]
    RowsNotMonotonic {
        /// The first row that breaks the order, or is not finite.
        at: usize,
    },
    /// A lateral offset that is not a number.
    #[error("the lateral offset must be a finite number, not {given}")]
    BadLateral {
        /// What was given.
        given: f64,
    },
    /// A lateral plane the projection does not place.
    #[error("a lateral plane {lateral} m off the hole is not placed by the projection `{projection}`")]
    LateralNotPlaced {
        /// The lateral offset asked.
        lateral: f64,
        /// The id of the projection.
        projection: &'static str,
    },
    /// The cells cannot be bounded.
    #[error("the cells cannot be bounded: {0}")]
    Covers(#[from] DrawError),
}

impl CurtainLayout {
    /// The bounds of every cell. The bounds in MD are the shared drawing's
    /// (`draw::raster::node_covers`) capped at half the widest gap the rule
    /// bridges (S7): a bridgeable step is covered from both sides, a gap
    /// leaves an empty band in the middle.
    ///
    /// # Errors
    ///
    /// [`CurtainRefusal::Nodes`] for nodes that do not make an axis,
    /// [`CurtainRefusal::TooFewRows`], [`CurtainRefusal::RowsNotMonotonic`],
    /// [`CurtainRefusal::BadLateral`].
    pub fn new(node_md: &[f64], rows_up: &[f64], lateral: f64) -> Result<CurtainLayout, CurtainRefusal> {
        if !lateral.is_finite() {
            return Err(CurtainRefusal::BadLateral { given: lateral });
        }
        let rule = GapRule::of_axis(node_md)?;
        let covers = node_covers(node_md, Some(rule.max_gap() / 2.0))?;
        let md_bounds: Vec<Option<[f64; 2]>> = covers
            .iter()
            .map(|cover| cover.map(|cover| [cover.from, cover.to]))
            .collect();

        let mut gaps = Vec::new();
        for n in 0..node_md.len().saturating_sub(1) {
            if !rule.bridges(node_md[n + 1] - node_md[n])
                && let (Some(before), Some(after)) = (md_bounds[n], md_bounds[n + 1])
            {
                gaps.push([before[1], after[0]]);
            }
        }

        let row_bounds = row_bounds(rows_up)?;
        Ok(CurtainLayout {
            node_md: node_md.to_vec(),
            rows_up: rows_up.to_vec(),
            lateral,
            md_bounds,
            row_bounds,
            gaps,
        })
    }
}

/// `[low, high]` in `dn` of each row, in the order of the rows.
fn row_bounds(rows_up: &[f64]) -> Result<Vec<[f64; 2]>, CurtainRefusal> {
    if rows_up.len() < 2 {
        return Err(CurtainRefusal::TooFewRows { given: rows_up.len() });
    }
    if let Some(at) = rows_up.iter().position(|value| !value.is_finite()) {
        return Err(CurtainRefusal::RowsNotMonotonic { at });
    }
    let increasing = rows_up[1] > rows_up[0];
    for at in 1..rows_up.len() {
        let ordered = if increasing {
            rows_up[at] > rows_up[at - 1]
        } else {
            rows_up[at] < rows_up[at - 1]
        };
        if !ordered {
            return Err(CurtainRefusal::RowsNotMonotonic { at });
        }
    }
    let ascending: Vec<f64> = if increasing {
        rows_up.to_vec()
    } else {
        rows_up.iter().rev().copied().collect()
    };
    let covers = node_covers(&ascending, None)?;
    let mut bounds: Vec<[f64; 2]> = covers
        .iter()
        .map(|cover| match cover {
            Some(cover) => [cover.from, cover.to],
            // Unreachable: every offset is finite and there are two at least.
            None => [f64::NAN, f64::NAN],
        })
        .collect();
    if !increasing {
        bounds.reverse();
    }
    Ok(bounds)
}

/// One cell, placed.
#[derive(Clone, Debug, PartialEq)]
pub struct CurtainCell {
    /// Its four corners in the drawing `[abscissa, tvd]`, in ring order:
    /// (md from, dn low), (md to, dn low), (md to, dn high), (md from, dn high).
    pub corners: [[f64; 2]; 4],
    /// (node md, row dn), placed by the same function.
    pub centre: [f64; 2],
    /// The offset of the row along the high side, metres: what `centre` was placed at.
    pub dn: f64,
    /// The value is `values[node * rows + row]`.
    pub node: u32,
    /// The row of the cell.
    pub row: u32,
    /// The curtain folds over its own centre line here.
    pub folded: bool,
}

/// The cells, and what placing them had to say.
#[derive(Clone, Debug, PartialEq)]
pub struct CurtainGeometry {
    /// The cells, node after node.
    pub cells: Vec<CurtainCell>,
    /// `[from, to]` MD spans of folded cells, contiguous ones merged.
    pub folded: Vec<[f64; 2]>,
    /// `[from, to]` MD spans where the centre line itself runs backwards.
    pub reversed: Vec<[f64; 2]>,
    /// The gaps of the layout, not bridged.
    pub gaps: Vec<[f64; 2]>,
    /// Nodes with no cell, and why: a corner the projection refused (S4 at that node, outside the axis).
    pub missing: Vec<(u32, ProjectionRefusal)>,
}

/// Adds `[a, b]` to the spans, joining it to the last one when they touch.
fn push_span(spans: &mut Vec<[f64; 2]>, a: f64, b: f64) {
    if let Some(last) = spans.last_mut()
        && a <= last[1] + 1e-9
    {
        last[1] = last[1].max(b);
        return;
    }
    spans.push([a, b]);
}

/// The cells of a curtain, placed by one projection.
///
/// A corner that falls outside the axis because the first and last cells reach
/// half a step beyond their node is **clamped to the axis**: the cell is cut
/// where the axis ends, never extended.
///
/// # Errors
///
/// [`CurtainRefusal::LateralNotPlaced`] for a lateral plane off the hole in a
/// projection that is neither `md_tvd` nor `developed`.
pub fn curtain_cells(
    axis: &WellAxis,
    layout: &CurtainLayout,
    projection: &dyn WellProjection,
) -> Result<CurtainGeometry, CurtainRefusal> {
    if layout.lateral != 0.0 && !matches!(projection.id(), "md_tvd" | "developed") {
        return Err(CurtainRefusal::LateralNotPlaced {
            lateral: layout.lateral,
            projection: projection.id(),
        });
    }
    let [first, last] = axis.extent();
    let mut cells = Vec::new();
    let mut folded: Vec<[f64; 2]> = Vec::new();
    let mut reversed: Vec<[f64; 2]> = Vec::new();
    let mut missing = Vec::new();
    // The sign of the first centre-line step that is not zero.
    let mut first_sign = 0.0_f64;

    for (n, bounds) in layout.md_bounds.iter().enumerate() {
        let Some([from, to]) = *bounds else { continue };
        let a = from.max(first);
        let b = to.min(last);
        let md = layout.node_md[n];
        let made = node_cells(axis, layout, projection, n, a, b, md);
        match made {
            Err(refusal) => missing.push((n as u32, refusal)),
            Ok((node_cells, d0)) => {
                if first_sign == 0.0 && d0 != 0.0 {
                    first_sign = d0.signum();
                }
                if first_sign != 0.0 && d0 * first_sign < 0.0 {
                    push_span(&mut reversed, a, b);
                }
                if node_cells.iter().any(|cell| cell.folded) {
                    push_span(&mut folded, a, b);
                }
                cells.extend(node_cells);
            }
        }
    }
    Ok(CurtainGeometry {
        cells,
        folded,
        reversed,
        gaps: layout.gaps.clone(),
        missing,
    })
}

/// The cells of one node, with the step of its centre line `d0` (the abscissa
/// of its far MD minus its near MD at `dn = 0`).
fn node_cells(
    axis: &WellAxis,
    layout: &CurtainLayout,
    projection: &dyn WellProjection,
    node: usize,
    a: f64,
    b: f64,
    md: f64,
) -> Result<(Vec<CurtainCell>, f64), ProjectionRefusal> {
    let d0 = projection.place(axis, b, 0.0)?[0] - projection.place(axis, a, 0.0)?[0];
    let mut out = Vec::with_capacity(layout.rows_up.len());
    for (row, bounds) in layout.row_bounds.iter().enumerate() {
        let [lo, hi] = *bounds;
        let corners = [
            projection.place(axis, a, lo)?,
            projection.place(axis, b, lo)?,
            projection.place(axis, b, hi)?,
            projection.place(axis, a, hi)?,
        ];
        let dn = layout.rows_up[row];
        let centre = projection.place(axis, md, dn)?;
        let d_lo = corners[1][0] - corners[0][0];
        let d_hi = corners[2][0] - corners[3][0];
        out.push(CurtainCell {
            corners,
            centre,
            dn,
            node: node as u32,
            row: row as u32,
            folded: d_lo * d0 < 0.0 || d_hi * d0 < 0.0,
        });
    }
    Ok((out, d0))
}
