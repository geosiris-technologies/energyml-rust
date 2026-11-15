//! The curtain as an image of the plot.

use energyml_sheet::draw::raster::{Raster, paint_quads};
use energyml_sheet::draw::DrawError;

use super::cells::CurtainGeometry;
use super::layout::PlotFrame;

/// What painting the curtain counted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ImageCounts {
    /// Cells painted.
    pub painted: usize,
    /// Cells left transparent because their value is NaN (absent, or the
    /// `NullValue` the reading turned into NaN).
    pub no_value: usize,
}

/// The curtain as an image of the plot: farthest cells from the hole first, so
/// that where two columns overlap the cell nearer the hole wins; a gap between
/// two columns stays transparent. `colour` is the caller's (the face's look,
/// the studio's colour map): this crate holds no palette.
///
/// The raster has the size of the plot frame, rounded to the pixel.
///
/// # Errors
///
/// [`DrawError::Size`] for a frame that is no image,
/// [`DrawError::GridShape`] when `values` does not hold a value for a cell.
pub fn curtain_image(
    geometry: &CurtainGeometry,
    values: &[f64],
    rows: usize,
    colour: &dyn Fn(f64) -> Option<[u8; 4]>,
    plot: &PlotFrame,
) -> Result<(Raster, ImageCounts), DrawError> {
    let [x, y, w, h] = plot.frame;
    let mut raster = Raster::transparent(w.round() as u32, h.round() as u32)?;

    let mut order: Vec<&super::cells::CurtainCell> = geometry.cells.iter().collect();
    for cell in &order {
        let needed = (cell.node as usize + 1) * rows;
        if rows == 0 || values.len() < needed {
            return Err(DrawError::GridShape {
                rows: cell.node as usize + 1,
                cols: rows,
                expected: needed,
                found: values.len(),
            });
        }
    }
    // Farthest from the hole first (the last painted wins), then node by node.
    order.sort_by(|a, b| b.dn.abs().total_cmp(&a.dn.abs()).then(a.node.cmp(&b.node)));

    let mut counts = ImageCounts::default();
    let mut quads: Vec<[[f64; 2]; 4]> = Vec::with_capacity(order.len());
    let mut colours: Vec<Option<[u8; 4]>> = Vec::with_capacity(order.len());
    for cell in &order {
        let value = values[cell.node as usize * rows + cell.row as usize];
        let painted = if value.is_nan() {
            counts.no_value += 1;
            None
        } else {
            colour(value)
        };
        if painted.is_some() {
            counts.painted += 1;
        }
        colours.push(painted);
        let mut quad = [[0.0; 2]; 4];
        for (k, corner) in cell.corners.iter().enumerate() {
            let pixel = plot.to_pixel(*corner);
            quad[k] = [pixel[0] - x, pixel[1] - y];
        }
        quads.push(quad);
    }
    paint_quads(&mut raster, &quads, &|k| colours[k]);
    Ok((raster, counts))
}
