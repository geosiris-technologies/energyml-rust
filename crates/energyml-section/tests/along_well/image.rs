//! `along_well::image`: the curtain painted as an image of the plot.

use energyml_section::along_well::{CurtainCell, CurtainGeometry, PlotFrame, curtain_image};

fn cell(corners: [[f64; 2]; 4], dn: f64, node: u32, row: u32) -> CurtainCell {
    let centre = [
        (corners[0][0] + corners[2][0]) / 2.0,
        (corners[0][1] + corners[2][1]) / 2.0,
    ];
    CurtainCell {
        corners,
        centre,
        dn,
        node,
        row,
        folded: false,
    }
}

fn square(x: f64, y: f64, side: f64) -> [[f64; 2]; 4] {
    [[x, y], [x + side, y], [x + side, y + side], [x, y + side]]
}

fn plot() -> PlotFrame {
    PlotFrame {
        frame: [0.0, 0.0, 100.0, 100.0],
        window: [0.0, 0.0, 100.0, 100.0],
        scale: [1.0, 1.0],
    }
}

fn geometry(cells: Vec<CurtainCell>) -> CurtainGeometry {
    CurtainGeometry {
        cells,
        folded: Vec::new(),
        reversed: Vec::new(),
        gaps: Vec::new(),
        missing: Vec::new(),
    }
}

const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const NOTHING: [u8; 4] = [0, 0, 0, 0];

fn colour(value: f64) -> Option<[u8; 4]> {
    if value == 1.0 {
        Some(RED)
    } else if value == 2.0 {
        Some(BLUE)
    } else {
        None
    }
}

/// Two cells overlap on `[40, 60] x [40, 60]`: the one at offset 5 m from the hole (red) is nearer than the
/// one at 30 m (blue), so it wins the pixels they share; the cell of 30 m shows where it is alone; a pixel
/// covered by no cell stays transparent; a NaN is counted and paints nothing.
///
/// To break it: paint from the nearest to the farthest (the shared pixel turns blue).
#[test]
fn the_cell_nearer_the_hole_wins_where_columns_overlap() {
    let near = cell(square(10.0, 10.0, 50.0), 5.0, 0, 0);
    let far = cell(square(40.0, 40.0, 50.0), 30.0, 1, 1);
    let absent = cell(square(0.0, 90.0, 10.0), 40.0, 0, 1);
    // values[node * rows + row], rows = 2: (0, 0) -> 0, (0, 1) -> 1, (1, 1) -> 3.
    let values = [1.0, f64::NAN, f64::NAN, 2.0];
    let (raster, counts) = curtain_image(&geometry(vec![near, far, absent]), &values, 2, &colour, &plot()).unwrap();
    assert_eq!(raster.pixel(50, 50), Some(RED));
    assert_eq!(raster.pixel(80, 80), Some(BLUE));
    assert_eq!(raster.pixel(20, 20), Some(RED));
    assert_eq!(raster.pixel(5, 5), Some(NOTHING));
    assert_eq!(raster.pixel(95, 20), Some(NOTHING));
    assert_eq!(raster.pixel(5, 95), Some(NOTHING));
    assert_eq!(counts.painted, 2);
    assert_eq!(counts.no_value, 1);
}

/// A cell that has no value in `values` is not painted by guess: the image refuses by name.
///
/// To break it: index the values with `get(..).unwrap_or(NAN)` (no refusal).
#[test]
fn a_cell_with_no_value_in_the_array_is_a_refusal() {
    let lonely = cell(square(10.0, 10.0, 10.0), 5.0, 3, 0);
    let refused = curtain_image(&geometry(vec![lonely]), &[1.0, 2.0], 1, &colour, &plot());
    assert!(refused.is_err());
}
