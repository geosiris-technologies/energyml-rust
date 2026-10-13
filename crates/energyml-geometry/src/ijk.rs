//! IJK grids: pillars, split coordinate lines, and cells.
//!
//! The most numerous representation in the reference corpus — 30 of them — and
//! the one a reservoir application is really asking for.
//!
//! # The model, in one paragraph
//!
//! An IJK grid is not a box of points. It is a set of **coordinate lines**
//! (pillars) running through the model, each carrying `Nk + 1` points, one per
//! layer boundary. There are `(Ni + 1) × (Nj + 1)` pillars in the regular
//! lattice, and a cell takes its eight corners from the four pillars around it
//! at two consecutive levels. The `Points` array is therefore shaped
//! `(Nk + 1) × lines × 3` and indexed `k × lines + line` — layer first, pillar
//! second.
//!
//! # Faults, which is where it gets interesting
//!
//! A fault means two cells that share a pillar do **not** share its points:
//! the rock is displaced. RESQML expresses that with **split coordinate
//! lines** — extra lines appended after the regular pillars, each saying which
//! pillar it duplicates and which columns use it instead.
//!
//! So resolving a corner is a lookup, not an index:
//!
//! ```text
//! line = split line for (pillar, column), if one is declared
//!      = the pillar itself, otherwise
//! ```
//!
//! Getting that wrong does not fail. It produces a grid that draws, with the
//! cells on one side of every fault pulled onto the other side's pillars —
//! plausible everywhere except at the faults, which is exactly where anyone
//! looking at the model is looking. The reference package is called *"two
//! faulted sugar cubes"* for a reason: two cells, one fault, and the two
//! answers differ by the 50 m throw.
//!
//! # Two spellings
//!
//! RESQML 2.0.1 calls the element `SplitCoordinateLines`; RESQML 2.2 calls it
//! `ColumnLayerSplitCoordinateLines`. Same structure inside. Both are accepted,
//! which is the rule [`crate::readers`] states: a version that respells the
//! insides is handled *inside* the reader.
//!
//! # Winding is measured, not assumed
//!
//! VTK requires the first four corners of a hexahedron to form a face whose
//! right-hand normal points at the opposite face. Whether the natural IJK order
//! satisfies that depends on `GridIsRighthanded`, on `KDirection`, and on
//! whether the CRS flipped z — three flags that interact, and each of the eight
//! combinations is a chance to be wrong.
//!
//! So it is **computed**, per cell, from the cell's own signed volume — see
//! `oriented`. A cell that comes out inside-out renders as a hole, which is
//! another failure that looks like working software with nothing in it.
//!
//! Per cell rather than per grid, and the corpus says why it has to be. In
//! `h+g_PaleoGTM` the grid `01e02598` declares `GridIsRighthanded = true`, and
//! 379 of its 29 401 cells come out wound the other way: pillar row `j = 49`
//! has folded back past `j = 48` — its y values sit *below* both neighbouring
//! rows — so twenty-four columns of that grid are reversed in plan view. That
//! is the file's own geometry, confirmed against the HDF5 with `h5py`, and a
//! reader that took the flag as the answer would emit those cells inside out.

use std::collections::HashMap;

use energyml_core::value::{FieldValue, Reflect};
use energyml_domain::values;

use crate::error::{Error, Result};
use crate::geometry::{Patch, Topology};
use crate::points::{self, Point};
use crate::source::ValueSource;
use crate::values::scalar_i64;

/// Read an `IjkGridRepresentation` into one patch of cells.
pub fn read(representation: &dyn Reflect, source: &dyn ValueSource) -> Result<Vec<Patch>> {
    let at = "IjkGridRepresentation";
    let ni = dimension(representation, "Ni", at)?;
    let nj = dimension(representation, "Nj", at)?;
    let nk = dimension(representation, "Nk", at)?;

    let Some(geometry) = field_object(representation, "Geometry") else {
        // A grid may legitimately have none: a **local grid** is a refinement
        // of a window of its parent, and its points are derived from the
        // parent's rather than stored. See [`crate::parent`].
        return crate::parent::ijk_grid(representation, source, [ni, nj, nk], at);
    };
    let Some(array) = field_object(geometry, "Points") else {
        return Err(Error::malformed(at, "a geometry with no `Points`"));
    };

    // The splits are read **before** the points, and that ordering is the fix
    // for a real bug rather than a tidiness: a parametric array cannot evaluate
    // a faulted grid without knowing which pillar each split line duplicates.
    // See [`crate::parametric::read_on_lines`].
    let pillars = (ni + 1) * (nj + 1);
    let splits = split_lines(geometry, source, pillars, at)?;
    let lines = pillars + splits.count;

    let where_ = format!("{at}.Geometry.Points");
    // The geometry names the CRS its numbers are in, and a parametric pillar
    // stores a depth where a coordinate goes: the same clause as a well's
    // vertical line (`parametric::Vertical`).
    let vertical = crate::parametric::Vertical::read(geometry, source);
    let coordinates = if array.type_info().name.ends_with("ParametricArray") {
        // Coordinate line `c` evaluates against parametric line `c` for a
        // pillar, and against the line of the pillar it duplicates for a split.
        let on: Vec<usize> = (0..pillars).chain(splits.of_pillar.iter().copied()).collect();
        crate::parametric::read_on_lines(array, source, &where_, Some(&on), vertical)?
    } else {
        points::read(array, source, &where_, vertical)?
    };
    let tops = layer_tops(representation, source, nk, at)?;
    let levels = tops.last().map(|top| top + 2).unwrap_or(nk + 1);

    // The shape is the one thing that says whether this reader understood the
    // file at all, so it is checked before anything is indexed.
    if coordinates.len() != levels * lines {
        return Err(Error::malformed(
            at,
            format!(
                "{} point(s) for a {ni}×{nj}×{nk} grid with {pillars} pillar(s), {} split \
                 line(s) and {levels} level(s): expected {}",
                coordinates.len(),
                splits.count,
                levels * lines
            ),
        ));
    }

    // The two flags the standard makes authoritative. Read here rather than
    // inferred from the coordinates, because RESQML says in as many words that
    // they win:
    //
    //   `PillarGeometryIsDefined` — "Indicator that a pillar has at least one
    //   node with a defined cell geometry. **If the indicator does not indicate
    //   that the pillar geometry is defined, then this over-rides any other
    //   node geometry specification.**" (RESQML TRG v2.2 §5.1; mandatory)
    //
    //   `CellGeometryIsDefined` — "Indicator that a cell has a defined
    //   geometry. **If the indicator shows that the cell geometry is NOT
    //   defined, then this attribute overrides any other node geometry
    //   specification.**" (§5.1; optional)
    //
    // Note what they do *not* say: neither promises the coordinates will be
    // NaN. A writer may mark a cell undefined and still leave finite numbers on
    // its pillars, and until these were read such a grid came out whole. The
    // NaN check below stays — it catches the writers that mark nothing.
    let pillar_defined = flags(geometry, source, "PillarGeometryIsDefined");
    let cell_defined = flags(geometry, source, "CellGeometryIsDefined");

    let mut cells: Vec<[u32; 8]> = Vec::with_capacity(ni * nj * nk);

    let mut numbering: Vec<u32> = Vec::new();
    let mut skipped = 0usize;
    let mut layer = 0usize;
    for top in &tops {
        for j in 0..nj {
            for i in 0..ni {
                let column = j * ni + i;
                // Cell index in RESQML's own order — i fastest, then j, then k
                // — which is the order both flag arrays are in.
                let cell_index = layer * nj * ni + column;
                if says_undefined(&cell_defined, cell_index) {
                    skipped += 1;
                    continue;
                }
                // The four pillars around this column, anticlockwise seen from
                // above: (i,j), (i+1,j), (i+1,j+1), (i,j+1).
                let corners = [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)];
                let mut node = [0u32; 8];
                // Not `k` and `k + 1`: a K gap inserts an extra level between
                // two layers, so the level a cell's top sits at comes from
                // `tops` rather than from the loop counter.
                let (top, bottom) = (*top, top + 1);
                let mut undefined_pillar = false;
                for (corner, (ci, cj)) in corners.iter().enumerate() {
                    let pillar = cj * (ni + 1) + ci;
                    undefined_pillar |= says_undefined(&pillar_defined, pillar);
                    let line = splits.line_for(pillar, column).unwrap_or(pillar);
                    node[corner] = (top * lines + line) as u32;
                    node[corner + 4] = (bottom * lines + line) as u32;
                }
                // Two reasons a cell has no shape, and both end the same way.
                // The flag is the standard's answer and outranks the numbers;
                // the NaN is what a writer that states no flag leaves behind.
                // Counted rather than drawn.
                if undefined_pillar
                    || node
                        .iter()
                        .any(|n| !coordinates[*n as usize].iter().all(|c| c.is_finite()))
                {
                    skipped += 1;
                    continue;
                }
                // **Which cell this is, in the file's own numbering.** Kept
                // because the push above is conditional: a cell the flag marks
                // undefined, or whose pillars carry no location, is skipped —
                // so a position in `cells` is not the number RESQML calls the
                // cell as soon as a grid has one hole. Anything addressing a
                // cell by that number (a grid connection set names pairs of
                // them) would otherwise be right on an unbroken grid and
                // quietly wrong on the faulted ones.
                numbering.push(cell_index as u32);
                cells.push(oriented(node, &coordinates));
            }
        }
        layer += 1;
    }

    if cells.is_empty() && skipped > 0 {
        return Err(Error::malformed(
            at,
            format!("all {skipped} cell(s) have at least one undefined pillar"),
        ));
    }

    Ok(vec![Patch {
        index: 0,
        node_offset: 0,
        points: coordinates,
        topology: Topology::Hexahedra {
            cells,
            dimensions: [ni, nj, nk],
            // Empty when nothing was dropped: a position *is* the index then,
            // and a grid of ten million cells does not pay for a list saying so.
            index: if skipped == 0 { Vec::new() } else { numbering },
        },
        closed: Vec::new(),
    }])
}

/// Put the eight corners in the order VTK asks for.
///
/// The rule VTK states: the right-hand normal of face `(0,1,2,3)` points at
/// face `(4,5,6,7)`. Rather than deduce that from `GridIsRighthanded`,
/// `KDirection` and whatever the CRS did to z, it is measured on the cell
/// itself — see the module header for why the flags cannot answer it.
///
/// # What is measured, and why it is not the face normal
///
/// The obvious reading of VTK's sentence is arithmetic: take the normal of
/// face `(0,1,2,3)` and dot it with the vector to the centre of `(4,5,6,7)`.
/// That was the first implementation, and it was later found to give a
/// **negative answer for both orderings** of the same cell — so it swapped,
/// changed nothing, and emitted the cell inside out.
///
/// The cause is that a cell corner is not a plane. `h+g_PaleoGTM` has cells
/// whose quadrilateral face is warped through 33 m while the cell itself is
/// 2 m thick, because one of its four pillars is collapsed over seventeen of
/// the nineteen levels. The face's normal is then dominated by its own tilt
/// rather than by the direction of the cell, and dotting it against a 2 m
/// separation says nothing.
///
/// So the quantity measured is the **signed volume** of the trilinear
/// hexahedron, by the divergence theorem over its six faces, each split into
/// four triangles at its own centre. It is the same statement — VTK's rule and
/// a positive volume agree on any cell where the rule is well defined — but it
/// is a property of the whole cell rather than of one of its faces, and it
/// **negates exactly** when the two faces are swapped. That last point is what
/// makes the decision total: one of the two orderings is right unless the
/// volume is zero, and a zero-volume cell has no right answer to give.
///
/// A 5-tetrahedron decomposition would be the cheaper spelling and is wrong
/// here: it picks a diagonal, so on a warped cell it is not symmetric under
/// the swap and the two orderings disagree about the magnitude as well as the
/// sign.
pub(crate) fn oriented(node: [u32; 8], points: &[Point]) -> [u32; 8] {
    if signed_volume(node, points) < 0.0 {
        [
            node[4], node[5], node[6], node[7], node[0], node[1], node[2], node[3],
        ]
    } else {
        // Zero included: a cell that pinches out to nothing is drawn in the
        // order it arrived rather than flipped on the sign of a rounding error.
        node
    }
}

/// The signed volume of the hexahedron `node` describes, positive when its
/// corners are in the order VTK asks for.
///
/// The points are taken relative to the cell's own mean before anything is
/// multiplied. Without that the sum is a difference of terms of order
/// `coordinate³` — a corpus grid sits at x ≈ 13 000 m, so the terms run to
/// 10¹² while the answer is 10² — and the sign of a thin cell would be decided
/// by cancellation rather than by its shape.
fn signed_volume(node: [u32; 8], points: &[Point]) -> f64 {
    // Each face wound so its right-hand normal points *out* of the cell when
    // the corners are in VTK's order.
    const FACES: [[usize; 4]; 6] = [
        [0, 3, 2, 1],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [1, 2, 6, 5],
        [2, 3, 7, 6],
        [3, 0, 4, 7],
    ];
    let mut origin = [0.0f64; 3];
    for index in node {
        let point = points[index as usize];
        for (axis, value) in origin.iter_mut().enumerate() {
            *value += point[axis] / 8.0;
        }
    }
    let at = |corner: usize| {
        let point = points[node[corner] as usize];
        [point[0] - origin[0], point[1] - origin[1], point[2] - origin[2]]
    };

    let mut volume = 0.0;
    for face in FACES {
        let mut middle = [0.0f64; 3];
        for corner in face {
            let point = at(corner);
            for (axis, value) in middle.iter_mut().enumerate() {
                *value += point[axis] / 4.0;
            }
        }
        // Triangle fan from the face's own centre, so a warped face is
        // integrated rather than approximated by one plane.
        for index in 0..4 {
            let (a, b) = (at(face[index]), at(face[(index + 1) % 4]));
            let u = [a[0] - middle[0], a[1] - middle[1], a[2] - middle[2]];
            let w = [b[0] - middle[0], b[1] - middle[1], b[2] - middle[2]];
            let normal = [
                u[1] * w[2] - u[2] * w[1],
                u[2] * w[0] - u[0] * w[2],
                u[0] * w[1] - u[1] * w[0],
            ];
            volume += (normal[0] * middle[0] + normal[1] * middle[1] + normal[2] * middle[2]) / 6.0;
        }
    }
    volume
}

/// Which extra coordinate line a column uses in place of a pillar.
#[derive(Default)]
pub(crate) struct Splits {
    /// How many split lines there are, appended after the regular pillars.
    pub(crate) count: usize,
    /// `(pillar, column)` → the line index to use instead of `pillar`.
    lookup: HashMap<(usize, usize), usize>,
    /// Which pillar each split line duplicates, in split order.
    ///
    /// A split coordinate line is a second column of points on **the same
    /// geometric line** as the pillar it splits. The lookup above answers "what
    /// line does this column use"; this answers "what line *is* this", which is
    /// what a parametric evaluation needs and what the lookup cannot say
    /// without being searched backwards.
    pub(crate) of_pillar: Vec<usize>,
}

impl Splits {
    fn line_for(&self, pillar: usize, column: usize) -> Option<usize> {
        self.lookup.get(&(pillar, column)).copied()
    }

    /// A column inside the given window of parent cells that uses a split line,
    /// if there is one.
    ///
    /// What a local grid asks before deriving its geometry from this grid: a
    /// child node on a pillar the fault has split has one position per side,
    /// and [`crate::parent`] refuses rather than pick. A split line used by a
    /// column *outside* the window changes nothing here — the window's own
    /// pillars still hold the values its columns use.
    pub(crate) fn within(&self, ni: usize, origin: [usize; 2], span: [usize; 2]) -> Option<usize> {
        self.lookup
            .keys()
            .map(|(_, column)| *column)
            .filter(|column| {
                let (i, j) = (column % ni.max(1), column / ni.max(1));
                i >= origin[0] && i < origin[0] + span[0] && j >= origin[1] && j < origin[1] + span[1]
            })
            .min()
    }
}

/// The two spellings of the same element.
const SPLIT_FIELDS: &[&str] = &["SplitCoordinateLines", "ColumnLayerSplitCoordinateLines"];

pub(crate) fn split_lines(
    geometry: &dyn Reflect,
    source: &dyn ValueSource,
    pillars: usize,
    at: &str,
) -> Result<Splits> {
    let Some(element) = SPLIT_FIELDS
        .iter()
        .find_map(|field| field_object(geometry, field))
    else {
        // No faults. The common case for a synthetic grid, and not an error.
        return Ok(Splits::default());
    };
    let count = scalar_i64(element, "Count").unwrap_or(0).max(0) as usize;
    if count == 0 {
        return Ok(Splits::default());
    }

    let Some(indices) = field_object(element, "PillarIndices") else {
        return Err(Error::malformed(
            at,
            "split coordinate lines with no `PillarIndices`",
        ));
    };
    let split_of = crate::values::read_integers(indices, source, &format!("{at}.PillarIndices"))?;
    if split_of.len() != count {
        return Err(Error::malformed(
            at,
            format!(
                "{count} split coordinate line(s) declared, {} pillar index/indices given",
                split_of.len()
            ),
        ));
    }

    let Some(jagged) = field_object(element, "ColumnsPerSplitCoordinateLine") else {
        return Err(Error::malformed(
            at,
            "split coordinate lines with no `ColumnsPerSplitCoordinateLine`",
        ));
    };
    // A jagged array: one row per split line, holding the columns that use it.
    // Rows differ in length because a split line may serve one column at the
    // edge of a fault or three at a corner.
    let rows = values::jagged(jagged, source, &format!("{at}.ColumnsPerSplitCoordinateLine"))?;
    if rows.len() != count {
        return Err(Error::malformed(
            at,
            format!(
                "{count} split coordinate line(s) declared, {} row(s) of columns",
                rows.len()
            ),
        ));
    }

    // The lookup holds the **absolute** line index, not the split's own: split
    // lines are appended after the regular pillars, so line `n` of the splits
    // is line `pillars + n` of the points array. Storing the relative index and
    // adding later is the same mistake in a place that is harder to see.
    let mut lookup = HashMap::new();
    for (split, row) in rows.iter().enumerate() {
        let pillar = split_of[split].max(0) as usize;
        let Some(columns) = row.integer() else {
            return Err(Error::malformed(
                at,
                "the columns of a split line are not integers",
            ));
        };
        for column in columns {
            lookup.insert((pillar, (*column).max(0) as usize), pillars + split);
        }
    }
    Ok(Splits {
        count,
        lookup,
        of_pillar: split_of.iter().map(|pillar| (*pillar).max(0) as usize).collect(),
    })
}

/// Which level each layer's top sits at.
///
/// Without gaps this is just `k`. With them it is not, and the difference is a
/// whole extra plane of points.
///
/// A **K gap** is a stratigraphic hiatus: two layers that are not in contact,
/// with a void between them. RESQML represents it by giving the grid an extra
/// level of coordinate-line points, so `Points` holds `Nk + 1 + gaps` levels
/// rather than `Nk + 1`, and layer `k` no longer sits between levels `k` and
/// `k + 1`.
///
/// Reading a gapped grid as if it had none does not fail — it produces cells
/// spanning the void, each one layer too tall, and a model whose total
/// thickness is wrong by the size of the gaps.
pub(crate) fn layer_tops(
    representation: &dyn Reflect,
    source: &dyn ValueSource,
    nk: usize,
    at: &str,
) -> Result<Vec<usize>> {
    let Some(gaps) = field_object(representation, "KGaps") else {
        return Ok((0..nk).collect());
    };
    let count = scalar_i64(gaps, "Count").unwrap_or(0).max(0) as usize;
    if count == 0 {
        return Ok((0..nk).collect());
    }
    let Some(array) = field_object(gaps, "GapAfterLayer") else {
        return Err(Error::malformed(at, "`KGaps` with no `GapAfterLayer`"));
    };
    // One flag per interface between layers, so `Nk - 1` of them.
    let after = values::read(array, source, &format!("{at}.KGaps.GapAfterLayer"))?;
    let after = after
        .boolean()
        .ok_or_else(|| Error::malformed(at, "`GapAfterLayer` is not a boolean array"))?
        .to_vec();
    if after.len() + 1 != nk {
        return Err(Error::malformed(
            at,
            format!(
                "`GapAfterLayer` holds {} flag(s) for {nk} layer(s): expected {}",
                after.len(),
                nk.saturating_sub(1)
            ),
        ));
    }
    let declared = after.iter().filter(|gap| **gap).count();
    if declared != count {
        return Err(Error::malformed(
            at,
            format!("`KGaps` declares {count} gap(s), `GapAfterLayer` marks {declared}"),
        ));
    }

    let mut tops = Vec::with_capacity(nk);
    let mut level = 0usize;
    for k in 0..nk {
        tops.push(level);
        level += 1;
        if after.get(k).copied().unwrap_or(false) {
            level += 1; // the void: one level belongs to neither layer
        }
    }
    Ok(tops)
}

fn dimension(object: &dyn Reflect, field: &str, at: &str) -> Result<usize> {
    match scalar_i64(object, field) {
        Some(value) if value > 0 => Ok(value as usize),
        Some(value) => Err(Error::malformed(at, format!("`{field}` is {value}"))),
        None => Err(Error::malformed(at, format!("no `{field}`"))),
    }
}

fn field_object<'a>(object: &'a dyn Reflect, field: &str) -> Option<&'a dyn Reflect> {
    match object.field(field)? {
        FieldValue::Object(child) => Some(child),
        _ => None,
    }
}

/// One of the grid's `…IsDefined` boolean arrays, when it holds one.
///
/// `None` covers three cases that are all the same answer — the field is
/// absent, it is not a boolean array, or it could not be read (its external
/// dataset is missing, say). In every one of them **nothing is known**, and the
/// caller falls back to the coordinates. Treating an unreadable flag as `false`
/// would delete a grid because a `.h5` was not at hand.
fn flags(geometry: &dyn Reflect, source: &dyn ValueSource, field: &str) -> Option<Vec<bool>> {
    let FieldValue::Object(array) = geometry.field(field)? else {
        return None;
    };
    let values = values::read(array, source, field).ok()?;
    values.boolean().map(|flags| flags.to_vec())
}

/// Whether a flag array says this element is **not** defined.
///
/// Out of range is *not* "undefined": a flag array shorter than the grid is a
/// malformed file, and reading past its end as `false` would erase the cells it
/// says nothing about. Silence means the coordinates decide.
fn says_undefined(flags: &Option<Vec<bool>>, index: usize) -> bool {
    flags
        .as_ref()
        .and_then(|flags| flags.get(index))
        .is_some_and(|defined| !defined)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unit cube already in VTK's order, and the same cube with its two
    /// faces exchanged.
    ///
    /// The ground truth here is arithmetic, not a measurement: face `(0,1,2,3)`
    /// of this cube lies at z = 0 wound anticlockwise seen from above, so its
    /// right-hand normal points at z = 1, which is where face `(4,5,6,7)` is.
    /// That is VTK's rule met, so `oriented` must leave it alone — and must put
    /// the exchanged one back.
    fn cube() -> Vec<Point> {
        vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 1.0, 1.0],
        ]
    }

    #[test]
    fn a_cube_in_vtks_order_is_left_alone_and_its_mirror_is_put_back() {
        let points = cube();
        let right = [0, 1, 2, 3, 4, 5, 6, 7];
        let wrong = [4, 5, 6, 7, 0, 1, 2, 3];
        assert_eq!(oriented(right, &points), right);
        assert_eq!(oriented(wrong, &points), right);
        // And the measure itself is the volume, in the unit the cube states.
        assert!((signed_volume(right, &points) - 1.0).abs() < 1e-12);
        assert!((signed_volume(wrong, &points) + 1.0).abs() < 1e-12);
    }

    /// The cell that made this function change, kept with its own numbers.
    ///
    /// `h+g_PaleoGTM`, grid `01e02598`, cell 26447: one of its four pillars is
    /// collapsed over seventeen of the grid's nineteen levels, so the cell is
    /// 20 m thick at one corner and 2 m at the other three while its faces are
    /// warped through 33 m. The old measure — the normal of face `(0,1,2,3)`
    /// against the vector to face `(4,5,6,7)` — is **negative for both**
    /// orderings of this cell (-3389.9 natural, -626.3 swapped), so it swapped
    /// and emitted the cell inside out.
    ///
    /// The coordinates below are the file's own, read back from the HDF5 and
    /// checked against `h5py` before being written here. What the assertion
    /// claims is not one of them: it is that the ordering `oriented` returns is
    /// the one with the positive volume, and that the two orderings disagree
    /// about the sign — which is the property the old measure lacked.
    #[test]
    fn the_twisted_cell_of_a_collapsed_pillar_is_oriented_by_its_volume() {
        // Level 16 first, which is the order the reader assembles.
        let points: Vec<Point> = vec![
            [13591.28125, 13687.5, 3637.706298828125],
            [13568.0126953125, 13850.400390625, 3622.7626953125],
            [13585.7861328125, 13711.5, 3628.719482421875],
            [13591.95703125, 13574.7880859375, 3645.5654296875],
            [13608.0986328125, 13687.5, 3658.21923828125],
            [13569.248046875, 13850.7216796875, 3624.921875],
            [13586.5732421875, 13712.0, 3630.77099609375],
            [13593.09375, 13575.03515625, 3647.5302734375],
        ];
        let natural = [0, 1, 2, 3, 4, 5, 6, 7];
        let swapped = [4, 5, 6, 7, 0, 1, 2, 3];

        let (a, b) = (signed_volume(natural, &points), signed_volume(swapped, &points));
        assert!(
            a > 0.0 && b < 0.0,
            "the two orderings must disagree about the sign: {a} and {b}"
        );
        // Symmetric to the last bit that a f64 can carry at this magnitude.
        assert!((a + b).abs() < 1e-6 * a.abs(), "{a} is not the negative of {b}");
        assert_eq!(oriented(natural, &points), natural);
        assert_eq!(oriented(swapped, &points), natural);

        // And the measure this replaced, kept so its failure stays visible:
        // it calls both orderings wrong, which is how the cell escaped.
        assert!(face_normal_measure(natural, &points) < 0.0);
        assert!(face_normal_measure(swapped, &points) < 0.0);
    }

    /// The measure `oriented` used originally, kept only so the test above can
    /// state what was wrong with it.
    fn face_normal_measure(node: [u32; 8], points: &[Point]) -> f64 {
        let at = |corner: usize| points[node[corner] as usize];
        let mut normal = [0.0f64; 3];
        for index in 0..4 {
            let (current, next) = (at(index), at((index + 1) % 4));
            normal[0] += (current[1] - next[1]) * (current[2] + next[2]);
            normal[1] += (current[2] - next[2]) * (current[0] + next[0]);
            normal[2] += (current[0] - next[0]) * (current[1] + next[1]);
        }
        let centre = |from: usize| {
            let mut sum = [0.0f64; 3];
            for corner in from..from + 4 {
                let point = at(corner);
                for (axis, value) in sum.iter_mut().enumerate() {
                    *value += point[axis] / 4.0;
                }
            }
            sum
        };
        let (a, b) = (centre(0), centre(4));
        (0..3).map(|axis| normal[axis] * (b[axis] - a[axis])).sum()
    }

    /// A cell that pinches out to nothing keeps the order it arrived in.
    ///
    /// Not a preference: `h+g_PaleoGTM` has eleven of them, where a layer has
    /// closed completely, and their volume is zero to the last bit. There is no
    /// right answer to give, so the one thing worth promising is that the same
    /// file yields the same cell twice.
    #[test]
    fn a_flat_cell_is_not_flipped_by_a_rounding_error() {
        let flat: Vec<Point> = vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
        ];
        let node = [0, 1, 2, 3, 4, 5, 6, 7];
        assert_eq!(signed_volume(node, &flat), 0.0);
        assert_eq!(oriented(node, &flat), node);
    }
}
