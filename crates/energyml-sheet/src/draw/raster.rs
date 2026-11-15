//! Value images: a grid of values, or placed quadrilaterals, painted into an
//! RGBA [`Raster`] pixel for pixel.
//!
//! The colour is a closure of the caller: the one door to colours stays with
//! the caller (a colour map read here would be a second one). A value the
//! caller refuses to colour (`None`) is **counted** and left transparent, never
//! painted with a colour that looks like data; a pixel nothing covers is
//! counted apart (`blank`) and stays transparent: a hole stays a hole
//! (ADR-0005).
//!
//! The same [`Raster`] goes to a face (its bytes laid as they are) and to the
//! SVG (an `<image>` that `svg::write` embeds as a PNG); the PNG of a view is
//! that SVG rasterised ([`crate::draw::png`]), never a second composition.

use crate::draw::svg::DrawError;

/// The most pixels a raster may hold: 4096 x 4096 (64 MB of RGBA), a bound a
/// WebAssembly page can allocate. Larger is refused, never truncated.
pub const MAX_RASTER_PIXELS: u64 = 16_777_216;

/// How the pixels of a raster came out: they sum to width x height.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct PaintCounts {
    /// Pixels the colour closure coloured.
    pub painted: usize,
    /// Pixels no sample or quad covers: transparent, a hole kept as a hole.
    pub blank: usize,
    /// Pixels whose value the closure refused to colour (`None`): transparent.
    pub not_coloured: usize,
}

/// An RGBA image, straight alpha, rows top to bottom.
#[derive(Clone, Debug, PartialEq)]
pub struct Raster {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    counts: PaintCounts,
}

fn check_size(width: u32, height: u32) -> Result<(), DrawError> {
    let pixels = u64::from(width) * u64::from(height);
    if width == 0 || height == 0 || pixels > MAX_RASTER_PIXELS {
        return Err(DrawError::Size {
            width: u64::from(width),
            height: u64::from(height),
            limit: MAX_RASTER_PIXELS,
        });
    }
    Ok(())
}

impl Raster {
    /// A transparent raster; refuses 0 or more than [`MAX_RASTER_PIXELS`]
    /// pixels (`Size`).
    pub fn transparent(width: u32, height: u32) -> Result<Raster, DrawError> {
        check_size(width, height)?;
        let pixels = width as usize * height as usize;
        Ok(Raster {
            width,
            height,
            rgba: vec![0; pixels * 4],
            counts: PaintCounts {
                painted: 0,
                blank: pixels,
                not_coloured: 0,
            },
        })
    }

    /// Wraps bytes the caller made (`RasterBytes` when the length is not
    /// `width x height x 4`). Its counts: the pixels with some opacity are
    /// `painted`, the others `blank`.
    pub fn from_rgba(width: u32, height: u32, rgba: Vec<u8>) -> Result<Raster, DrawError> {
        check_size(width, height)?;
        let expected = width as usize * height as usize * 4;
        if rgba.len() != expected {
            return Err(DrawError::RasterBytes {
                width,
                height,
                expected,
                found: rgba.len(),
            });
        }
        let painted = rgba.as_chunks::<4>().0.iter().filter(|p| p[3] != 0).count();
        let pixels = expected / 4;
        Ok(Raster {
            width,
            height,
            rgba,
            counts: PaintCounts {
                painted,
                blank: pixels - painted,
                not_coloured: 0,
            },
        })
    }

    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The pixels, `r g b a` each, straight alpha, rows top to bottom.
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    /// The pixels, by value.
    pub fn into_rgba(self) -> Vec<u8> {
        self.rgba
    }

    /// How the last painting call left the pixels.
    pub fn counts(&self) -> PaintCounts {
        self.counts
    }

    /// The pixel at column `x`, row `y`; `None` outside.
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let at = (y as usize * self.width as usize + x as usize) * 4;
        Some([
            self.rgba[at],
            self.rgba[at + 1],
            self.rgba[at + 2],
            self.rgba[at + 3],
        ])
    }

    fn put(&mut self, x: usize, y: usize, pixel: [u8; 4]) {
        let at = (y * self.width as usize + x) * 4;
        self.rgba[at..at + 4].copy_from_slice(&pixel);
    }
}

/// What one sample covers along an axis: `[from, to)`, with `at` its own
/// position (a node's MD, an interval's middle).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cover {
    /// Start of the covered range (included).
    pub from: f64,
    /// The sample's own position.
    pub at: f64,
    /// End of the covered range (excluded).
    pub to: f64,
}

/// Node samples: each covers half the way to each neighbour, capped at
/// `half_cap` on each side when given (the caller's gap rule, S7); the first
/// and the last mirror their only neighbour. A non-finite position is `None`
/// and is skipped when looking for neighbours.
pub fn node_covers(positions: &[f64], half_cap: Option<f64>) -> Result<Vec<Option<Cover>>, DrawError> {
    if let Some(c) = half_cap
        && (!c.is_finite() || c <= 0.0)
    {
        return Err(DrawError::BadGap { gap: c });
    }
    let finite: Vec<usize> = (0..positions.len())
        .filter(|&i| positions[i].is_finite())
        .collect();
    for pair in finite.windows(2) {
        if positions[pair[1]] <= positions[pair[0]] {
            return Err(DrawError::NotIncreasing { at: pair[1] });
        }
    }
    let mut out = vec![None; positions.len()];
    match finite.len() {
        0 => {}
        1 => {
            let c = half_cap.ok_or(DrawError::SingleSampleNeedsWidth)?;
            let p = positions[finite[0]];
            out[finite[0]] = Some(Cover {
                from: p - c,
                at: p,
                to: p + c,
            });
        }
        n => {
            let cap = |v: f64| half_cap.map_or(v, |c| v.min(c));
            for (k, &i) in finite.iter().enumerate() {
                let p = positions[i];
                let before = (k > 0).then(|| (p - positions[finite[k - 1]]) / 2.0);
                let after = (k + 1 < n).then(|| (positions[finite[k + 1]] - p) / 2.0);
                // `n >= 2`: every sample has at least one neighbour.
                let down = before.or(after).unwrap_or(0.0);
                let up = after.or(before).unwrap_or(0.0);
                out[i] = Some(Cover {
                    from: p - cap(down),
                    at: p,
                    to: p + cap(up),
                });
            }
        }
    }
    Ok(out)
}

/// Interval samples: each covers exactly `[top, base)` (RESQML TRG 2.2 §12.13
/// p. 310, "to organize association of properties on intervals between MD
/// points"). An interval of no thickness, or with a non-finite end, covers no
/// pixel (`None`).
pub fn interval_covers(tops: &[f64], bases: &[f64]) -> Result<Vec<Option<Cover>>, DrawError> {
    if tops.len() != bases.len() {
        return Err(DrawError::GridShape {
            rows: tops.len(),
            cols: 1,
            expected: tops.len(),
            found: bases.len(),
        });
    }
    let mut out = Vec::with_capacity(tops.len());
    let mut last_from = f64::NEG_INFINITY;
    for (i, (&top, &base)) in tops.iter().zip(bases).enumerate() {
        if top.is_finite() && base.is_finite() && top < base {
            if top < last_from {
                return Err(DrawError::NotIncreasing { at: i });
            }
            last_from = top;
            out.push(Some(Cover {
                from: top,
                at: (top + base) / 2.0,
                to: base,
            }));
        } else {
            out.push(None);
        }
    }
    Ok(out)
}

/// What one row of pixels shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowPick {
    /// One sample.
    Sample(usize),
    /// Several samples fall in the row: `from..to` (absolute indices, `to`
    /// excluded) are reduced; `nearest` is the one closest to the row's centre.
    Bucket {
        /// First sample of the bucket.
        from: usize,
        /// One past the last sample of the bucket.
        to: usize,
        /// The sample nearest to the row's centre (absolute index).
        nearest: usize,
    },
    /// Nothing covers or lies in this row: a hole, kept.
    Blank,
}

/// What every row of pixels shows.
#[derive(Clone, Debug, PartialEq)]
pub struct RowMap {
    /// One pick per row of pixels, top to bottom.
    pub picks: Vec<RowPick>,
    /// How many rows are `Blank`.
    pub blank: usize,
    /// The most samples one row holds (1 when any row shows a sample, 0 when all are blank).
    pub most_per_pixel: usize,
}

/// What every column of pixels shows.
#[derive(Clone, Debug, PartialEq)]
pub struct ColumnMap {
    /// One sample (or none) per column of pixels, left to right.
    pub picks: Vec<Option<usize>>,
    /// How many columns show nothing.
    pub blank: usize,
}

/// The checks `rows_for` and `columns_for` share; the width of one pixel.
fn check_axis(covers: &[Option<Cover>], window: [f64; 2], pixels: u32) -> Result<f64, DrawError> {
    if pixels == 0 {
        return Err(DrawError::NoPixels { pixels });
    }
    let [w0, w1] = window;
    if !w0.is_finite() || !w1.is_finite() || w0 >= w1 {
        return Err(DrawError::BadWindow { from: w0, to: w1 });
    }
    let mut last = f64::NEG_INFINITY;
    for (i, c) in covers.iter().enumerate() {
        if let Some(c) = c
            && c.at.is_finite()
        {
            if c.at < last {
                return Err(DrawError::NotIncreasing { at: i });
            }
            last = c.at;
        }
    }
    Ok((w1 - w0) / f64::from(pixels))
}

/// Fills the unset pixels whose centre lies in a cover shifted by `offset`;
/// the first cover in order wins.
fn sweep(covers: &[Option<Cover>], w0: f64, h: f64, offset: f64, out: &mut [Option<usize>]) {
    let pixels = out.len();
    for (i, c) in covers.iter().enumerate() {
        let Some(c) = c else { continue };
        let (lo, hi) = (c.from + offset, c.to + offset);
        if !lo.is_finite() || !hi.is_finite() {
            continue;
        }
        let start = (((lo - w0) / h - 0.5).floor() - 1.0).clamp(0.0, pixels as f64) as usize;
        for (r, slot) in out.iter_mut().enumerate().skip(start) {
            let centre = w0 + (r as f64 + 0.5) * h;
            if centre >= hi {
                break;
            }
            if centre >= lo && slot.is_none() {
                *slot = Some(i);
            }
        }
    }
}

/// Rows of pixels over `window` (`[top, bottom]`, increasing, y down).
///
/// Row `r` covers `[a, b)` with centre `c`. Several samples whose position is
/// in `[a, b)` make a [`RowPick::Bucket`]; otherwise the first cover that
/// holds `c` is the row's sample; otherwise a single sample lying in the row
/// is drawn even when its cover is narrower than a pixel; otherwise the row
/// is `Blank`: a hole is never filled by the nearest sample. A bucket spans
/// the absolute indices `from..to`, which can include samples whose cover is
/// `None` (a non-finite position): the caller's values for them are reduced
/// too, as the caller gave them.
pub fn rows_for(covers: &[Option<Cover>], window: [f64; 2], pixels: u32) -> Result<RowMap, DrawError> {
    let h = check_axis(covers, window, pixels)?;
    let w0 = window[0];
    let present: Vec<(usize, f64)> = covers
        .iter()
        .enumerate()
        .filter_map(|(i, c)| c.as_ref().filter(|c| c.at.is_finite()).map(|c| (i, c.at)))
        .collect();
    let mut holding: Vec<Option<usize>> = vec![None; pixels as usize];
    sweep(covers, w0, h, 0.0, &mut holding);

    let mut picks = Vec::with_capacity(pixels as usize);
    let mut blank = 0;
    let mut most = 0;
    for (r, holder) in holding.iter().enumerate() {
        let a = w0 + r as f64 * h;
        let b = w0 + (r as f64 + 1.0) * h;
        let c = w0 + (r as f64 + 0.5) * h;
        let lo = present.partition_point(|&(_, at)| at < a);
        let hi = present.partition_point(|&(_, at)| at < b);
        let inside = hi - lo;
        most = most.max(inside);
        let pick = if inside >= 2 {
            let mut nearest = present[lo];
            for &(i, at) in &present[lo + 1..hi] {
                if (at - c).abs() < (nearest.1 - c).abs() {
                    nearest = (i, at);
                }
            }
            RowPick::Bucket {
                from: present[lo].0,
                to: present[hi - 1].0 + 1,
                nearest: nearest.0,
            }
        } else if let Some(i) = holder {
            RowPick::Sample(*i)
        } else if inside == 1 {
            RowPick::Sample(present[lo].0)
        } else {
            RowPick::Blank
        };
        match pick {
            RowPick::Blank => blank += 1,
            _ => most = most.max(1),
        }
        picks.push(pick);
    }
    Ok(RowMap {
        picks,
        blank,
        most_per_pixel: most,
    })
}

/// Columns of pixels over `window` (`[left, right]`); `period`: a cyclic axis
/// (360 for degrees), for which the covers also hold one period before and one
/// after. No bucket: a column of pixels shows one component.
pub fn columns_for(
    covers: &[Option<Cover>],
    window: [f64; 2],
    pixels: u32,
    period: Option<f64>,
) -> Result<ColumnMap, DrawError> {
    let h = check_axis(covers, window, pixels)?;
    if let Some(p) = period
        && (!p.is_finite() || p <= 0.0)
    {
        return Err(DrawError::BadWindow {
            from: window[0],
            to: window[1],
        });
    }
    let w0 = window[0];
    let mut picks: Vec<Option<usize>> = vec![None; pixels as usize];
    sweep(covers, w0, h, 0.0, &mut picks);
    if let Some(p) = period {
        // `c - p` in a cover is `c` in the cover moved by `+p`.
        sweep(covers, w0, h, p, &mut picks);
        sweep(covers, w0, h, -p, &mut picks);
    }
    let blank = picks.iter().filter(|p| p.is_none()).count();
    Ok(ColumnMap { picks, blank })
}

/// How several samples falling in one row become one value.
pub trait RowReduce: Send + Sync {
    /// The reduction's name in the registry.
    fn id(&self) -> &'static str;
    /// `values` are the bucket's, in order; `nearest` indexes into them.
    fn reduce(&self, values: &[f64], nearest: usize) -> f64;
}

/// `nearest`: the sample closest to the row's centre.
pub struct NearestReduce;
/// `mean`: the mean of the finite values; NaN when none.
pub struct MeanReduce;
/// `min`: the smallest finite value; NaN when none.
pub struct MinReduce;
/// `max`: the largest finite value; NaN when none.
pub struct MaxReduce;

impl RowReduce for NearestReduce {
    fn id(&self) -> &'static str {
        "nearest"
    }
    fn reduce(&self, values: &[f64], nearest: usize) -> f64 {
        values.get(nearest).copied().unwrap_or(f64::NAN)
    }
}

impl RowReduce for MeanReduce {
    fn id(&self) -> &'static str {
        "mean"
    }
    fn reduce(&self, values: &[f64], _nearest: usize) -> f64 {
        let (sum, count) = values
            .iter()
            .filter(|v| v.is_finite())
            .fold((0.0, 0usize), |(s, n), v| (s + v, n + 1));
        if count == 0 { f64::NAN } else { sum / count as f64 }
    }
}

impl RowReduce for MinReduce {
    fn id(&self) -> &'static str {
        "min"
    }
    fn reduce(&self, values: &[f64], _nearest: usize) -> f64 {
        values
            .iter()
            .copied()
            .filter(|v| v.is_finite())
            .reduce(f64::min)
            .unwrap_or(f64::NAN)
    }
}

impl RowReduce for MaxReduce {
    fn id(&self) -> &'static str {
        "max"
    }
    fn reduce(&self, values: &[f64], _nearest: usize) -> f64 {
        values
            .iter()
            .copied()
            .filter(|v| v.is_finite())
            .reduce(f64::max)
            .unwrap_or(f64::NAN)
    }
}

/// The reductions: built-ins, the caller's additions tried first
/// (the registry of the section layout, ADR-0006).
pub struct RowReduces(Vec<Box<dyn RowReduce>>);

impl RowReduces {
    /// `nearest`, `mean`, `min`, `max`.
    pub fn with_builtins() -> Self {
        RowReduces(vec![
            Box::new(NearestReduce),
            Box::new(MeanReduce),
            Box::new(MinReduce),
            Box::new(MaxReduce),
        ])
    }

    /// Adds a reduction, tried before every one already there (so a caller's
    /// addition shadows a built-in of the same id).
    pub fn register(&mut self, reduce: Box<dyn RowReduce>) {
        self.0.insert(0, reduce);
    }

    /// The first reduction of this id.
    pub fn find(&self, id: &str) -> Option<&dyn RowReduce> {
        self.0.iter().find(|r| r.id() == id).map(|r| &**r)
    }
}

impl Default for RowReduces {
    fn default() -> Self {
        Self::with_builtins()
    }
}

/// A grid of values, row-major (`values[row * cols + col]`): a plane of a
/// matrix, a density.
#[derive(Clone, Copy, Debug)]
pub struct GridView<'a> {
    /// Number of rows.
    pub rows: usize,
    /// Number of columns.
    pub cols: usize,
    /// `rows x cols` values, row-major.
    pub values: &'a [f64],
}

/// Paints a grid: one pixel per (row pick, column pick). A `Blank` row or a
/// column with no pick is transparent (`blank`); a bucket is reduced by
/// `reduce`; `colour` colours the value (a NaN is passed as it is: the caller
/// says what a NaN becomes) or refuses it (`None`: transparent, counted in
/// `not_coloured`). `painted + blank + not_coloured == width x height`.
pub fn paint_grid(
    grid: GridView<'_>,
    rows: &RowMap,
    cols: &ColumnMap,
    reduce: &dyn RowReduce,
    colour: &dyn Fn(f64) -> Option<[u8; 4]>,
) -> Result<Raster, DrawError> {
    let expected = grid.rows.saturating_mul(grid.cols);
    if grid.values.len() != expected {
        return Err(DrawError::GridShape {
            rows: grid.rows,
            cols: grid.cols,
            expected,
            found: grid.values.len(),
        });
    }
    for pick in &rows.picks {
        let last = match *pick {
            RowPick::Blank => continue,
            RowPick::Sample(i) => i,
            RowPick::Bucket { from, to, nearest } => {
                if from >= to || nearest < from || nearest >= to {
                    return Err(DrawError::PickOutOfRange {
                        axis: "rows",
                        pick: nearest,
                        len: grid.rows,
                    });
                }
                to - 1
            }
        };
        if last >= grid.rows {
            return Err(DrawError::PickOutOfRange {
                axis: "rows",
                pick: last,
                len: grid.rows,
            });
        }
    }
    for j in cols.picks.iter().flatten() {
        if *j >= grid.cols {
            return Err(DrawError::PickOutOfRange {
                axis: "columns",
                pick: *j,
                len: grid.cols,
            });
        }
    }
    let width = u32::try_from(cols.picks.len()).unwrap_or(u32::MAX);
    let height = u32::try_from(rows.picks.len()).unwrap_or(u32::MAX);
    let mut raster = Raster::transparent(width, height)?;
    let mut counts = PaintCounts::default();
    let mut bucket: Vec<f64> = Vec::new();
    for (y, pick) in rows.picks.iter().enumerate() {
        for (x, column) in cols.picks.iter().enumerate() {
            let value = match (*pick, *column) {
                (RowPick::Blank, _) | (_, None) => None,
                (RowPick::Sample(i), Some(j)) => Some(grid.values[i * grid.cols + j]),
                (RowPick::Bucket { from, to, nearest }, Some(j)) => {
                    bucket.clear();
                    bucket.extend((from..to).map(|k| grid.values[k * grid.cols + j]));
                    Some(reduce.reduce(&bucket, nearest - from))
                }
            };
            match value {
                None => counts.blank += 1,
                Some(v) => match colour(v) {
                    Some(pixel) => {
                        raster.put(x, y, pixel);
                        counts.painted += 1;
                    }
                    None => counts.not_coloured += 1,
                },
            }
        }
    }
    raster.counts = counts;
    Ok(raster)
}

/// Quads, each four corners in the raster's pixels (x right, y down), painted
/// into `raster` in the order given: a later quad wins where two overlap (the
/// caller orders them, farthest first). `colour` takes the quad's index (the
/// caller knows its value and its colour); `None`: the quad clears what it
/// covers, so the nearest cell decides even when it has no colour. A pixel is
/// painted when its centre lies inside the quad (odd-even rule: a crossed
/// quadrilateral is painted as two triangles). A quad with a non-finite corner
/// is skipped. The raster keeps what it held where no quad of this call
/// passes. Returns what this call did, also stored as the raster's counts
/// (`blank`: pixels no quad of this call covers).
pub fn paint_quads(
    raster: &mut Raster,
    quads: &[[[f64; 2]; 4]],
    colour: &dyn Fn(usize) -> Option<[u8; 4]>,
) -> PaintCounts {
    let width = raster.width as usize;
    let height = raster.height as usize;
    // 0: not touched by this call, 1: painted, 2: cleared.
    let mut state = vec![0u8; width * height];
    for (k, quad) in quads.iter().enumerate() {
        if quad.iter().any(|p| !p[0].is_finite() || !p[1].is_finite()) {
            continue;
        }
        let paint = colour(k);
        let (mut y_min, mut y_max) = (f64::INFINITY, f64::NEG_INFINITY);
        for p in quad {
            y_min = y_min.min(p[1]);
            y_max = y_max.max(p[1]);
        }
        let first = (y_min - 0.5).floor().clamp(0.0, height as f64) as usize;
        let last = (y_max + 0.5).ceil().clamp(0.0, height as f64) as usize;
        for j in first..last.min(height) {
            let yc = j as f64 + 0.5;
            let mut xs = Vec::with_capacity(4);
            for e in 0..4 {
                let (p, q) = (quad[e], quad[(e + 1) % 4]);
                // Half-open: a vertex on the line is counted once.
                if (p[1] <= yc) != (q[1] <= yc) {
                    xs.push(p[0] + (yc - p[1]) * (q[0] - p[0]) / (q[1] - p[1]));
                }
            }
            xs.sort_by(f64::total_cmp);
            for span in xs.as_chunks::<2>().0 {
                let from = (span[0] - 0.5).ceil().clamp(0.0, width as f64) as usize;
                let to = (span[1] - 0.5).ceil().clamp(0.0, width as f64) as usize;
                for i in from..to {
                    match paint {
                        Some(pixel) => {
                            raster.put(i, j, pixel);
                            state[j * width + i] = 1;
                        }
                        None => {
                            raster.put(i, j, [0, 0, 0, 0]);
                            state[j * width + i] = 2;
                        }
                    }
                }
            }
        }
    }
    let counts = PaintCounts {
        painted: state.iter().filter(|&&s| s == 1).count(),
        not_coloured: state.iter().filter(|&&s| s == 2).count(),
        blank: state.iter().filter(|&&s| s == 0).count(),
    };
    raster.counts = counts;
    counts
}

/// The raster as PNG bytes. With the `png` feature only: without it, a
/// `FeatureNotEnabled` refusal.
///
/// The pixels are premultiplied by tiny-skia's own function, then encoded (and
/// un-premultiplied) by it: an opaque or a transparent pixel comes back exactly,
/// a half-transparent one can lose a bit. This PNG is for a face that wants the
/// raster alone; the PNG of a view is its SVG rasterised ([`crate::draw::png`]).
#[cfg(feature = "png")]
pub fn encode_png(raster: &Raster) -> Result<Vec<u8>, DrawError> {
    use resvg::tiny_skia::{ColorU8, IntSize, Pixmap};

    let size_error = || DrawError::Size {
        width: u64::from(raster.width),
        height: u64::from(raster.height),
        limit: MAX_RASTER_PIXELS,
    };
    let mut data = Vec::with_capacity(raster.rgba.len());
    for p in raster.rgba.as_chunks::<4>().0 {
        let c = ColorU8::from_rgba(p[0], p[1], p[2], p[3]).premultiply();
        data.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    let size = IntSize::from_wh(raster.width, raster.height).ok_or_else(size_error)?;
    let pixmap = Pixmap::from_vec(data, size).ok_or_else(size_error)?;
    pixmap.encode_png().map_err(|e| DrawError::Encode(e.to_string()))
}

/// Without the `png` feature: the refusal, by name.
#[cfg(not(feature = "png"))]
pub fn encode_png(raster: &Raster) -> Result<Vec<u8>, DrawError> {
    let _ = raster;
    Err(DrawError::FeatureNotEnabled {
        feature: "png",
        what: "encode a raster as PNG",
    })
}
