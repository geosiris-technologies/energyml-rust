//! Multilevel B-spline Approximation (MBA) — Lee, Wolberg, Shin, *"Scattered
//! Data Interpolation with Multilevel B-Splines"*, IEEE Transactions on
//! Visualization and Computer Graphics, vol. 3, n° 3, 1997, p. 228-244
//! (DOI 10.1109/2945.620490). This is the §3/§6.1 core of the method, written
//! the way [`crate::idw`] and [`crate::rbf`] are: samples and query points in,
//! nothing read from a package, so it compiles to wasm with no feature flags.
//!
//! The use case is extending a mesh while keeping the general character of the
//! surface even where there are few points.
//!
//! TODO(reference): verify — the citation above (journal, pages, DOI) and the
//! section/page numbers quoted below were not re-checked against the paper;
//! the Boehm 1985 and Lyche & Mørken mentions are unverified too.
//!
//! # Why it keeps the general shape where points are sparse
//!
//! Two facts, cited in the paper:
//!
//! - **the coarsest level is fitted on every sample at once**, on a grid the
//!   paper recommends making *"as coarse as possible"* (§4.1, p. 233) — that
//!   grid's reach covers the whole domain, so it carries the overall shape
//!   even where a finer grid would have nothing;
//! - **a control point with no sample in its neighbourhood gets zero** — but
//!   zero *correction* relative to the level below (§1.2, p. 231: *"we assign
//!   zero to fij to make function f tend to zero in its neighborhood"*). The
//!   sum of levels therefore keeps the coarse shape in sparse regions, because
//!   the finer levels have nothing to correct there.
//!
//! # The trend removed first — the paper's own linear initialisation
//!
//! Left alone, BA *"tend\[s] to zero away from the data"*: a control point no
//! sample reaches is set to zero (p. 231), so the coarsest level itself sinks
//! toward 0 in a margin, and a constant of 42 came back as 37 there (the first
//! run of this module's tests). The paper answers it in §7.3
//! (p. 241-242): subtract a least-squares **plane** first, approximate the
//! residuals, add the plane back — *"the function values are no longer zero
//! away from data"*. That is what keeping the general character of the
//! surface requires, so it is the default ([`Trend::Plane`]); [`Trend::None`]
//! keeps the bare algorithm. A constant and a plane are then reproduced
//! **exactly**, everywhere in the domain — their residuals are zero, so every
//! level is zero. Samples that are all aligned (or a single one) define no
//! plane: the trend is then their **mean**, and [`MbaAnswer::trend`] says so.
//!
//! # The algorithm implemented here — *basic* MBA, not the refined one
//!
//! **Provisional (open question: refined vs basic MBA):** the paper's
//! optimisation of §4.2 collapses the `h + 1` per-level grids into one, at the
//! finest resolution, through a B-spline refinement mask (the Oslo algorithm,
//! Lyche & Mørken, specialised to a dyadic cubic doubling). The mask's coefficients
//! could not be verified against the OCR'd article, and coding an unverified
//! mask was advised against. **This module
//! sums the `h + 1` levels directly at every evaluation instead** — the
//! *basic* algorithm of §4.1, still `O(hp + mn)` in construction time (§1.3),
//! only `O(h)` rather than `O(1)` per evaluated point. The alternative —
//! implementing the refinement mask — would give `O(1)` evaluation at the cost
//! of coding a subdivision mask nobody here has verified against a primary
//! source (Boehm 1985); until that verification is done, summing is the
//! conservative choice.
//!
//! # What is refused, and what is merely set aside
//!
//! Mirroring the house rule already applied to [`crate::idw`]:
//!
//! - **a degenerate domain** (a non-finite or non-positive size, a coarsest
//!   control grid of 0 cells on an axis, a non-finite or non-positive finest
//!   spacing) is refused by name — there is no rectangle to approximate on;
//! - **a `NaN` sample position or value is set aside and counted**, never
//!   averaged — the same contract as [`crate::idw::Idw`]. If nothing usable
//!   remains, the call is refused (`Error::NoSample`) rather than silently
//!   answering `NaN` everywhere;
//! - **a sample or a query outside the domain is refused, naming it and its
//!   position** — this is the one point where MBA departs from IDW/kriging's
//!   "unreached is `NaN`" convention, and on purpose: a B-spline is simply not
//!   *defined* outside the rectangle it was built on (§1.1, §1.4 — MBA does not extrapolate beyond a boundary laid down in
//!   advance); clamping or extrapolating there would be exactly the "draw
//!   rather than refuse" the house rules forbid. A caller that wants "far away, so `NaN`" (as
//!   `estimate`'s domain-union policy needs) filters its queries
//!   against [`Domain::contains`] *before* calling [`Mba::interpolate`], so
//!   the core never has to guess what "too far" should mean;
//! - **points that are all coincident or collinear are *not* refused** — the
//!   method is explicit about this, and the reason is structural
//!   rather than a leniency: unlike a Delaunay triangulation, the least-squares
//!   grid fit of §1.2 does not invert a matrix that degenerates on aligned
//!   points; a single line of samples yields a flat ridge along it and a
//!   smooth field around it, not an error. Transposing `triangulate`'s
//!   collinearity refusal here would refuse a case this algorithm handles
//!   correctly.
//!
//! # What is declared, never guessed
//!
//! `coarsest` (the coarsest control grid, `Mba::new`) and `max_levels` (the
//! cap on how many times the grid may double, `Mba::with_max_levels`) are both
//! stated by the caller. Neither is derived from the sample count or density —
//! the house rule against a threshold calibrated on a dataset — and
//! `DEFAULT_MAX_LEVELS` is a fixed safety valve against runaway memory, not an
//! algorithmic recommendation.

/// A grid nobody meant to ask for: past this many doublings of `coarsest`,
/// `interpolate` refuses by name rather than building it — raise the cap
/// explicitly (`Mba::with_max_levels`) if it is really wanted. A fixed
/// implementation ceiling, not a threshold read off any dataset.
const DEFAULT_MAX_LEVELS: usize = 12;

/// What this module refuses, by name — see the module doc, *What is refused*.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum Error {
    #[error("{count} sample position(s) for {values} value(s) — one value per sample")]
    MismatchedLengths { count: usize, values: usize },
    #[error("the coarsest control grid must be at least 1 cell on each axis, not {coarsest:?}")]
    DegenerateCoarsest { coarsest: [usize; 2] },
    #[error(
        "the domain must be a finite rectangle with a positive size on each axis — origin {origin:?}, size {size:?}"
    )]
    DegenerateDomain { origin: [f64; 2], size: [f64; 2] },
    #[error("the finest spacing must be a finite number > 0, not {given}")]
    BadSpacing { given: f64 },
    #[error("no usable sample: {given} given, {set_aside} set aside for a NaN position or value")]
    NoSample { given: usize, set_aside: usize },
    #[error(
        "sample {at} at ({x}, {y}) lies outside the domain — origin {origin:?}, size {size:?}: MBA does not extrapolate past a domain it was not given (Lee/Wolberg/Shin 1997, §3.1, p. 229)"
    )]
    SampleOutsideDomain {
        at: usize,
        x: f64,
        y: f64,
        origin: [f64; 2],
        size: [f64; 2],
    },
    #[error(
        "query {at} at ({x}, {y}) lies outside the domain — origin {origin:?}, size {size:?}: MBA does not extrapolate past a domain it was not given (Lee/Wolberg/Shin 1997, §3.1, p. 229); filter it out (`Domain::contains`) before calling if you want it left as NaN instead of refused"
    )]
    QueryOutsideDomain {
        at: usize,
        x: f64,
        y: f64,
        origin: [f64; 2],
        size: [f64; 2],
    },
    #[error(
        "a finest spacing of {finest_spacing} over a domain of {domain_x} by {domain_y} needs {needed} level(s) past the coarsest {coarsest:?}, past the {max_levels} allowed (`Mba::with_max_levels`): raise the cap, or widen the spacing"
    )]
    TooManyLevels {
        needed: usize,
        max_levels: usize,
        coarsest: [usize; 2],
        domain_x: f64,
        domain_y: f64,
        finest_spacing: f64,
    },
    #[error(
        "the finest control grid would need {control_points} control point(s), past what this writes: raise the spacing or the coarsest grid"
    )]
    GridTooLarge { control_points: usize },
}

/// A hard technical ceiling on the finest control grid — a defensive bound
/// against `usize`/memory blow-ups, not a modelling decision. 20,000 × 20,000
/// control points is 3.2 GB of `f64` for one level alone.
const MOST_CONTROL_POINTS: usize = 400_000_000;

/// The rectangle `Ω` MBA is fitted and evaluated on — eq. (1), p. 229: *"the
/// domain Ω = {(x, y) : 0 ≤ x < m, 0 ≤ y < n}"*, here in real coordinates
/// rather than the article's grid-cell units. `origin` is `Ω`'s low corner;
/// `size` is `[width, height]`, both positive.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Domain {
    /// The rectangle's low corner, `[x0, y0]`.
    pub origin: [f64; 2],
    /// `[width, height]`, both positive and finite.
    pub size: [f64; 2],
}

impl Domain {
    /// A tolerance for the domain boundary — a query or sample landing within
    /// this of an edge is treated as on the boundary rather than outside it,
    /// which floating point arithmetic on a lattice's own nodes will do
    /// routinely (`interpolate`'s `Fitted` extent computes `Ω` from the same
    /// samples it then re-queries). Scaled to the domain, not a bare epsilon,
    /// so a domain of kilometres and one of millimetres are equally forgiving.
    fn tolerance(&self) -> f64 {
        1e-9 * self.size[0].abs().max(self.size[1].abs()).max(1.0)
    }

    /// Whether `at` lies inside `Ω`, boundary included within `Self::tolerance`.
    ///
    /// A caller that wants "outside is `NaN`" rather than "outside is refused"
    /// — `estimate`'s domain-union policy, `crates/energyml-build/src/ops/interpolate_mba.rs`
    /// — filters its queries with this before calling [`Mba::interpolate`].
    pub fn contains(&self, at: [f64; 2]) -> bool {
        let tol = self.tolerance();
        (0..2).all(|axis| {
            at[axis].is_finite()
                && at[axis] >= self.origin[axis] - tol
                && at[axis] <= self.origin[axis] + self.size[axis] + tol
        })
    }

    fn check(&self) -> Result<(), Error> {
        let finite = self.origin.iter().chain(self.size.iter()).all(|v| v.is_finite());
        if !finite || self.size[0] <= 0.0 || self.size[1] <= 0.0 {
            return Err(Error::DegenerateDomain {
                origin: self.origin,
                size: self.size,
            });
        }
        Ok(())
    }
}

/// The cubic B-spline basis, uniform, eq. in §3.1 p. 229:
/// `B0(t) = (1-t)³/6`, `B1(t) = (3t³-6t²+4)/6`, `B2(t) = (-3t³+3t²+3t+1)/6`,
/// `B3(t) = t³/6`, for `t` in `[0, 1]`.
fn basis(t: f64) -> [f64; 4] {
    let t2 = t * t;
    let t3 = t2 * t;
    [
        (1.0 - t).powi(3) / 6.0,
        (3.0 * t3 - 6.0 * t2 + 4.0) / 6.0,
        (-3.0 * t3 + 3.0 * t2 + 3.0 * t + 1.0) / 6.0,
        t3 / 6.0,
    ]
}

/// Where `px` (a coordinate already scaled to this level's cells, `0..=m`)
/// falls: the lower index `i = ⌊x⌋ - 1` of the 4-point stencil (eq. (1),
/// p. 229) and the local parameter `s = x - ⌊x⌋`.
///
/// The right edge (`px == m` exactly, which a query landing on the domain's
/// own far boundary will hit) has no cell to its right: it is folded onto the
/// last cell's far end (`⌊x⌋ = m - 1`, `s = 1.0`) rather than refused, which is
/// the standard closed-interval convention for a half-open grid — `Ω` itself
/// is checked as closed (`Domain::contains`), so a point this function would
/// refuse never reaches it.
fn locate(px: f64, m: usize) -> (i64, f64) {
    let clamped = px.clamp(0.0, m as f64);
    if clamped >= m as f64 {
        // The right edge: fold onto the last cell's far end rather than
        // reaching for a cell that does not exist. `floor_x = m - 1`,
        // `i = floor_x - 1 = m - 2`.
        (m as i64 - 2, 1.0)
    } else {
        let floor = clamped.floor();
        (floor as i64 - 1, clamped - floor)
    }
}

/// The `(m + 3) × (n + 3)` control grid of one level, and its own cell counts.
/// Storage index for control point `(i, j)`, `i` in `-1..=m+1`: `(j + 1) * (m + 3) + (i + 1)`.
struct Level {
    m: usize,
    n: usize,
    control: Vec<f64>,
}

impl Level {
    fn new(m: usize, n: usize) -> Self {
        Level {
            m,
            n,
            control: vec![0.0; (m + 3) * (n + 3)],
        }
    }

    fn stride(&self) -> usize {
        self.m + 3
    }

    /// The 16 control points a sample or query at `(px, py)` — already scaled
    /// to this level's cells — touches, as `(storage index, weight)` pairs.
    fn stencil(&self, px: f64, py: f64) -> [(usize, f64); 16] {
        let (i, s) = locate(px, self.m);
        let (j, t) = locate(py, self.n);
        let bs = basis(s);
        let bt = basis(t);
        let stride = self.stride();
        let mut out = [(0usize, 0.0); 16];
        let mut at = 0;
        for (l, &wl) in bt.iter().enumerate() {
            for (k, &wk) in bs.iter().enumerate() {
                let si = (i + k as i64 + 1) as usize;
                let sj = (j + l as i64 + 1) as usize;
                out[at] = (sj * stride + si, wk * wl);
                at += 1;
            }
        }
        out
    }

    /// `f_k(x, y)` — eq. (1), p. 229 — the value this level's spline gives at
    /// `(px, py)`.
    fn eval(&self, px: f64, py: f64) -> f64 {
        self.stencil(px, py)
            .iter()
            .map(|&(at, w)| w * self.control[at])
            .sum()
    }

    /// The BA algorithm, p. 231: fit this level's control grid to
    /// `(samples, residuals)` by accumulation — eq. (3) folded into eq. (5), no
    /// linear system. `samples` and `residuals` are already the usable ones,
    /// scaled by the caller into this level's cell units.
    fn fit(m: usize, n: usize, samples_px_py: &[(f64, f64)], residuals: &[f64]) -> Self {
        let mut level = Level::new(m, n);
        let mut denominator = vec![0.0; level.control.len()];
        for (&(px, py), &z) in samples_px_py.iter().zip(residuals) {
            let stencil = level.stencil(px, py);
            // Σ_{a,b} w_ab² over this sample's own 16 weights — eq. (3)'s
            // denominator, computed once per sample and reused for its 16
            // contributions.
            let sum_sq: f64 = stencil.iter().map(|&(_, w)| w * w).sum();
            if sum_sq <= 0.0 {
                // Cannot happen for a finite (s, t) in [0, 1]² — the basis
                // functions are never all zero at once — but a guard beats a
                // division by zero if a future change ever makes it possible.
                continue;
            }
            for &(at, w) in &stencil {
                // eq. (5): f_ij = Σ_c w_c² f_c / Σ_c w_c², where f_c (eq. (3))
                // is w_kl · z_c / sum_sq for *this* control point — so the
                // numerator's contribution is w_kl³ · z_c / sum_sq and the
                // denominator's is w_kl².
                level.control[at] += w * w * w * z / sum_sq;
                denominator[at] += w * w;
            }
        }
        for (value, den) in level.control.iter_mut().zip(&denominator) {
            // p. 231: "we assign zero to fij" when no sample's neighbourhood
            // reaches it — not a fallback, the paper's own rule (§1.2).
            *value = if *den > 0.0 { *value / den } else { 0.0 };
        }
        level
    }
}

/// Multilevel B-spline Approximation, Lee/Wolberg/Shin 1997 — the *basic*
/// algorithm (module doc): `h + 1` levels, each fitted to the residual its
/// predecessor left, summed at every evaluation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mba {
    coarsest: [usize; 2],
    max_levels: usize,
    trend: Trend,
}

/// What is taken out of the samples before the levels are fitted, and put
/// back at every evaluation — module doc, *The trend removed first*.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trend {
    /// Nothing: the bare BA/MBA, which tends to zero away from the data.
    None,
    /// A least-squares plane `z = a + b·x + c·y` — the paper's linear
    /// initialisation (§7.3, p. 241-242). The default.
    Plane,
}

/// The trend actually removed — `Plane` may fall back to a constant when the
/// samples define no plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrendUsed {
    /// None was removed.
    None,
    /// The samples' mean — asked for a plane, they were aligned or alone.
    Constant(f64),
    /// `z = a + b·x + c·y`, in the domain's own coordinates.
    Plane {
        /// The value at the origin of the coordinates.
        a: f64,
        /// The slope along x.
        b: f64,
        /// The slope along y.
        c: f64,
    },
}

impl TrendUsed {
    fn at(&self, p: [f64; 2]) -> f64 {
        match *self {
            TrendUsed::None => 0.0,
            TrendUsed::Constant(mean) => mean,
            TrendUsed::Plane { a, b, c } => a + b * p[0] + c * p[1],
        }
    }

    /// The least-squares plane through `samples`, or their mean when they
    /// define none. Centred on the samples' mean position, so the normal
    /// equations stay well conditioned for projected coordinates in the
    /// hundreds of kilometres.
    fn fit(trend: Trend, samples: &[([f64; 2], f64)]) -> TrendUsed {
        if trend == Trend::None {
            return TrendUsed::None;
        }
        let n = samples.len() as f64;
        let (mx, my, mz) = samples.iter().fold((0.0, 0.0, 0.0), |acc, (p, z)| {
            (acc.0 + p[0], acc.1 + p[1], acc.2 + z)
        });
        let (mx, my, mz) = (mx / n, my / n, mz / n);
        let (mut sxx, mut sxy, mut syy, mut sxz, mut syz) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for (p, z) in samples {
            let (dx, dy, dz) = (p[0] - mx, p[1] - my, z - mz);
            sxx += dx * dx;
            sxy += dx * dy;
            syy += dy * dy;
            sxz += dx * dz;
            syz += dy * dz;
        }
        let det = sxx * syy - sxy * sxy;
        // Aligned (or a single) sample: the 2×2 system is singular — no plane.
        // Relative, so that it does not depend on the unit of the coordinates.
        if !(det.is_finite() && det > 1e-12 * sxx.max(syy).powi(2) && det > 0.0) {
            return TrendUsed::Constant(mz);
        }
        let b = (sxz * syy - syz * sxy) / det;
        let c = (syz * sxx - sxz * sxy) / det;
        TrendUsed::Plane {
            a: mz - b * mx - c * my,
            b,
            c,
        }
    }
}

impl Mba {
    /// A coarsest control grid of `coarsest[0]` × `coarsest[1]` cells — the
    /// paper's `m0`, `n0` (§4.1, p. 233), stated by the caller and never
    /// derived from a dataset. Refuses a coarsest grid of 0 cells on either
    /// axis.
    pub fn new(coarsest: [usize; 2]) -> Result<Self, Error> {
        if coarsest[0] == 0 || coarsest[1] == 0 {
            return Err(Error::DegenerateCoarsest { coarsest });
        }
        Ok(Mba {
            coarsest,
            max_levels: DEFAULT_MAX_LEVELS,
            trend: Trend::Plane,
        })
    }

    /// Override the safety cap on how many times the coarsest grid may double
    /// before `interpolate` refuses rather than build it.
    #[must_use]
    pub fn with_max_levels(mut self, max_levels: usize) -> Self {
        self.max_levels = max_levels;
        self
    }

    /// What is removed before the levels are fitted — [`Trend::Plane`] unless
    /// told otherwise (module doc, *The trend removed first*).
    #[must_use]
    pub fn with_trend(mut self, trend: Trend) -> Self {
        self.trend = trend;
        self
    }

    /// Fit MBA to `(samples, values)` on `domain`, refining until a level's
    /// cells are no wider than `finest_spacing` on either axis (or
    /// `max_levels` is reached, refused by name), and evaluate at every
    /// `queries`.
    ///
    /// One value per query, in the queries' order. Refuses — never guesses —
    /// on a degenerate domain, too few usable samples, or a sample or query
    /// outside `domain` (module doc, *What is refused*).
    pub fn interpolate(
        &self,
        samples: &[[f64; 2]],
        values: &[f64],
        domain: Domain,
        finest_spacing: f64,
        queries: &[[f64; 2]],
    ) -> Result<MbaAnswer, Error> {
        if samples.len() != values.len() {
            return Err(Error::MismatchedLengths {
                count: samples.len(),
                values: values.len(),
            });
        }
        domain.check()?;
        if !(finest_spacing.is_finite() && finest_spacing > 0.0) {
            return Err(Error::BadSpacing {
                given: finest_spacing,
            });
        }

        // The NaN set-aside — a sample with a non-finite position or value
        // takes no part, and is counted (the house rule, already `Idw`'s).
        let usable: Vec<([f64; 2], f64)> = samples
            .iter()
            .zip(values)
            .filter(|(p, v)| v.is_finite() && p.iter().all(|c| c.is_finite()))
            .map(|(p, v)| (*p, *v))
            .collect();
        let set_aside = samples.len() - usable.len();
        if usable.is_empty() {
            return Err(Error::NoSample {
                given: samples.len(),
                set_aside,
            });
        }
        // Every *usable* sample must be inside `Ω` — a sample the caller
        // meant to set aside (NaN) never reaches this check, but one that is
        // simply outside the rectangle is refused by name rather than
        // evaluated with a stencil clamped to the boundary, which would be a
        // silent guess about where it "really" belongs.
        for (at, (p, _)) in usable.iter().enumerate() {
            if !domain.contains(*p) {
                return Err(Error::SampleOutsideDomain {
                    at,
                    x: p[0],
                    y: p[1],
                    origin: domain.origin,
                    size: domain.size,
                });
            }
        }
        for (at, q) in queries.iter().enumerate() {
            if !domain.contains(*q) {
                return Err(Error::QueryOutsideDomain {
                    at,
                    x: q[0],
                    y: q[1],
                    origin: domain.origin,
                    size: domain.size,
                });
            }
        }

        // How many doublings of `coarsest` bring a cell down to
        // `finest_spacing` or below, on the axis that needs the most —
        // ⌈log2(size / (coarsest · finest_spacing))⌉, floored at 0 (the
        // coarsest grid may already be fine enough).
        let needed_axis = |axis: usize| {
            let ratio = domain.size[axis] / (self.coarsest[axis] as f64 * finest_spacing);
            if ratio <= 1.0 {
                0i64
            } else {
                ratio.log2().ceil() as i64
            }
        };
        let needed = needed_axis(0).max(needed_axis(1)).max(0) as usize;
        if needed > self.max_levels {
            return Err(Error::TooManyLevels {
                needed,
                max_levels: self.max_levels,
                coarsest: self.coarsest,
                domain_x: domain.size[0],
                domain_y: domain.size[1],
                finest_spacing,
            });
        }
        let finest = [self.coarsest[0] << needed, self.coarsest[1] << needed];
        let finest_control_points = (finest[0] + 3).saturating_mul(finest[1] + 3);
        if finest_control_points > MOST_CONTROL_POINTS {
            return Err(Error::GridTooLarge {
                control_points: finest_control_points,
            });
        }

        // Scale a real (x, y) to level k's cell units — px in [0, m_k].
        let scale = |p: [f64; 2], m: usize, n: usize| -> (f64, f64) {
            (
                (p[0] - domain.origin[0]) / domain.size[0] * m as f64,
                (p[1] - domain.origin[1]) / domain.size[1] * n as f64,
            )
        };

        let trend = TrendUsed::fit(self.trend, &usable);
        let positions: Vec<[f64; 2]> = usable.iter().map(|(p, _)| *p).collect();
        let mut residuals: Vec<f64> = usable.iter().map(|(p, z)| z - trend.at(*p)).collect();
        let mut levels: Vec<Level> = Vec::with_capacity(needed + 1);
        for k in 0..=needed {
            let (m, n) = (self.coarsest[0] << k, self.coarsest[1] << k);
            let scaled: Vec<(f64, f64)> = positions.iter().map(|&p| scale(p, m, n)).collect();
            let level = Level::fit(m, n, &scaled, &residuals);
            // Δ_{k+1} z_c = Δ_k z_c - f_k(x_c, y_c) — the residual this level
            // leaves for the next one (§4.1, p. 232-233).
            for (residual, &(px, py)) in residuals.iter_mut().zip(&scaled) {
                *residual -= level.eval(px, py);
            }
            levels.push(level);
        }

        let out: Vec<f64> = queries
            .iter()
            .map(|&q| {
                trend.at(q)
                    + levels
                        .iter()
                        .enumerate()
                        .map(|(k, level)| {
                            let (m, n) = (self.coarsest[0] << k, self.coarsest[1] << k);
                            let (px, py) = scale(q, m, n);
                            level.eval(px, py)
                        })
                        .sum::<f64>()
            })
            .collect();

        Ok(MbaAnswer {
            values: out,
            set_aside,
            levels: needed + 1,
            finest,
            finest_spacing: [
                domain.size[0] / finest[0] as f64,
                domain.size[1] / finest[1] as f64,
            ],
            trend,
        })
    }
}

/// What an MBA fit-and-evaluate answers — the values, and what the fit
/// actually settled on (never a number nobody stated, `interpolate.rs`'s own
/// rule).
#[derive(Debug, Clone, PartialEq)]
pub struct MbaAnswer {
    /// One value per query, in the queries' order.
    pub values: Vec<f64>,
    /// Samples set aside for a `NaN` position or value — never averaged.
    pub set_aside: usize,
    /// `h + 1` — the coarsest level plus every refinement actually built.
    pub levels: usize,
    /// The finest control grid's cell counts, `[m_h, n_h]`.
    pub finest: [usize; 2],
    /// The finest grid's *actual* cell size — rarely exactly the
    /// `finest_spacing` asked for, since the grid only ever doubles.
    pub finest_spacing: [f64; 2],
    /// The trend removed before the levels were fitted — a plane asked for
    /// comes back a constant when the samples defined none.
    pub trend: TrendUsed,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn domain(origin: [f64; 2], size: [f64; 2]) -> Domain {
        Domain { origin, size }
    }

    /// **Exact interpolation when `d ≥ 4`** (§5.1, p. 235-236): samples spaced by at least 4 finest cells come back
    /// exactly at their own positions. To break: flip the sign of `w_kl²`
    /// in the denominator of eq. (5) — the value at the vertex would no longer
    /// be exactly `z_c`.
    #[test]
    fn samples_spaced_past_4_cells_are_reproduced_exactly() {
        let mba = Mba::new([1, 1]).unwrap();
        // A 16×16 finest grid (coarsest 1×1, 4 doublings) over [0,16]×[0,16]:
        // samples 4 cells apart share no control point.
        let samples = [[2.0, 2.0], [6.0, 2.0], [10.0, 2.0], [2.0, 10.0]];
        let values = [1.0, 5.0, -3.0, 9.0];
        let dom = domain([0.0, 0.0], [16.0, 16.0]);
        let answer = mba.interpolate(&samples, &values, dom, 1.0, &samples).unwrap();
        for (got, want) in answer.values.iter().zip(&values) {
            assert!((got - want).abs() < 1e-9, "{got} != {want}");
        }
    }

    /// **A constant field is reproduced exactly**, including outside the extent
    /// of the points — same test as `idw.rs`'s
    /// `a_constant_field_is_reproduced_exactly`, but here the property comes
    /// from the polynomial reproduction of cubic B-splines (degree ≤ 3), not
    /// from a weighted mean. To break: remove the coarsest level (keep only
    /// the fine one) and the field would fall back to zero far from the
    /// samples instead of to the constant.
    #[test]
    fn a_constant_field_is_reproduced_exactly_even_away_from_samples() {
        let mba = Mba::new([2, 2]).unwrap();
        let samples = [[1.0, 1.0], [9.0, 1.0], [1.0, 9.0], [9.0, 9.0], [5.0, 5.0]];
        let values = [7.0; 5];
        let dom = domain([0.0, 0.0], [10.0, 10.0]);
        let queries = [[5.0, 5.0], [0.5, 0.5], [9.5, 9.5], [2.0, 8.0]];
        let answer = mba.interpolate(&samples, &values, dom, 2.0, &queries).unwrap();
        for value in &answer.values {
            assert!((value - 7.0).abs() < 1e-9, "{value} != 7.0");
        }
    }

    /// **A plane (degree 1) is reproduced exactly, at every resolution** — a
    /// direct consequence of the polynomial reproduction of uniform cubic
    /// B-splines (de Boor, *A Practical Guide to Splines*).
    /// Two different control grids (`coarsest` = [1,1] and [2,3]) give the
    /// same result. To break: swap `i` and `j` in `Level::stencil` and the
    /// plane would drift as soon as it is not symmetric in x/y.
    #[test]
    fn a_plane_is_reproduced_exactly_whatever_the_coarsest_grid() {
        let plane = |x: f64, y: f64| 2.0 + 0.3 * x - 0.7 * y;
        let samples: Vec<[f64; 2]> = (0..7)
            .flat_map(|i| (0..7).map(move |j| [i as f64, j as f64]))
            .collect();
        let values: Vec<f64> = samples.iter().map(|p| plane(p[0], p[1])).collect();
        let dom = domain([0.0, 0.0], [6.0, 6.0]);
        let queries = [[3.0, 3.0], [0.2, 5.8], [6.0, 0.0]];
        for coarsest in [[1usize, 1], [2, 3]] {
            let mba = Mba::new(coarsest).unwrap();
            let answer = mba.interpolate(&samples, &values, dom, 1.0, &queries).unwrap();
            for (q, got) in queries.iter().zip(&answer.values) {
                let want = plane(q[0], q[1]);
                assert!(
                    (got - want).abs() < 1e-6,
                    "coarsest {coarsest:?}: {got} != {want} at {q:?}"
                );
            }
        }
    }

    /// **Adding a level cannot move the function away from the data**
    /// (§4.1): the RMS error at the samples does not rise as `h` grows.
    /// To break: sum the levels with alternating signs (`+`, `-`, `+`, …) and
    /// the error would rise at the third level.
    #[test]
    fn the_error_at_the_samples_does_not_grow_with_more_levels() {
        let f = |x: f64, y: f64| (x * 0.7).sin() * (y * 0.5).cos() * 5.0 + x * 0.2;
        let samples: Vec<[f64; 2]> = (0..10)
            .flat_map(|i| (0..10).map(move |j| [i as f64 + 0.5, j as f64 + 0.3]))
            .collect();
        let values: Vec<f64> = samples.iter().map(|p| f(p[0], p[1])).collect();
        let dom = domain([0.0, 0.0], [10.0, 10.0]);
        let rms = |spacing: f64| -> f64 {
            let mba = Mba::new([1, 1]).unwrap();
            let answer = mba
                .interpolate(&samples, &values, dom, spacing, &samples)
                .unwrap();
            let se: f64 = answer
                .values
                .iter()
                .zip(&values)
                .map(|(a, b)| (a - b) * (a - b))
                .sum();
            (se / samples.len() as f64).sqrt()
        };
        let coarse_rms = rms(5.0); // fewer levels
        let fine_rms = rms(0.5); // more levels
        assert!(
            fine_rms <= coarse_rms + 1e-9,
            "finer ({fine_rms}) should not be worse than coarser ({coarse_rms})"
        );
    }

    /// **A textbook case**: `f2(x, y) = tanh(9(y-x)+1)/9`,
    /// Lee/Wolberg/Shin 1997 §7.2 p. 241 — read cleanly from the OCR, unlike
    /// the refinement mask. Not the article's own RMS (implementation choices
    /// — `h`, the grid — are not specified there), only an order of
    /// magnitude on a smooth, well-sampled function.
    #[test]
    fn the_articles_f2_is_approximated_to_the_right_order_of_magnitude() {
        let f2 = |x: f64, y: f64| ((9.0 * (y - x) + 1.0).tanh()) / 9.0;
        let samples: Vec<[f64; 2]> = (0..15)
            .flat_map(|i| (0..15).map(move |j| [i as f64 / 14.0, j as f64 / 14.0]))
            .collect();
        let values: Vec<f64> = samples.iter().map(|p| f2(p[0], p[1])).collect();
        let dom = domain([0.0, 0.0], [1.0, 1.0]);
        let mba = Mba::new([2, 2]).unwrap();
        let queries: Vec<[f64; 2]> = (0..21)
            .flat_map(|i| (0..21).map(move |j| [i as f64 / 20.0, j as f64 / 20.0]))
            .collect();
        let answer = mba.interpolate(&samples, &values, dom, 0.05, &queries).unwrap();
        let max_err = queries
            .iter()
            .zip(&answer.values)
            .map(|(q, got)| (got - f2(q[0], q[1])).abs())
            .fold(0.0, f64::max);
        assert!(
            max_err < 0.05,
            "max error {max_err} on a smooth, densely sampled function"
        );
    }

    /// **NaN in the input**: set aside and counted, never averaged.
    #[test]
    fn a_nan_sample_is_set_aside_and_counted() {
        let mba = Mba::new([1, 1]).unwrap();
        let samples = [[1.0, 1.0], [5.0, 5.0], [3.0, 3.0]];
        let values = [1.0, 3.0, f64::NAN];
        let dom = domain([0.0, 0.0], [10.0, 10.0]);
        let answer = mba
            .interpolate(&samples, &values, dom, 1.0, &[[1.0, 1.0]])
            .unwrap();
        assert_eq!(answer.set_aside, 1);
    }

    #[test]
    fn every_refusal_is_named() {
        let mba = Mba::new([1, 1]).unwrap();
        assert_eq!(
            Mba::new([0, 1]),
            Err(Error::DegenerateCoarsest { coarsest: [0, 1] })
        );
        let dom = domain([0.0, 0.0], [10.0, 10.0]);
        // Degenerate domain.
        assert_eq!(
            mba.interpolate(&[[0.0, 0.0]], &[1.0], domain([0.0, 0.0], [0.0, 10.0]), 1.0, &[]),
            Err(Error::DegenerateDomain {
                origin: [0.0, 0.0],
                size: [0.0, 10.0]
            })
        );
        // Inconsistent lengths.
        assert_eq!(
            mba.interpolate(&[[0.0, 0.0]], &[1.0, 2.0], dom, 1.0, &[]),
            Err(Error::MismatchedLengths { count: 1, values: 2 })
        );
        // Non-positive spacing.
        assert_eq!(
            mba.interpolate(&[[0.0, 0.0]], &[1.0], dom, 0.0, &[]),
            Err(Error::BadSpacing { given: 0.0 })
        );
        // No usable sample.
        assert_eq!(
            mba.interpolate(&[[0.0, 0.0]], &[f64::NAN], dom, 1.0, &[]),
            Err(Error::NoSample {
                given: 1,
                set_aside: 1
            })
        );
        // Query outside the domain.
        assert_eq!(
            mba.interpolate(&[[1.0, 1.0]], &[1.0], dom, 1.0, &[[50.0, 50.0]]),
            Err(Error::QueryOutsideDomain {
                at: 0,
                x: 50.0,
                y: 50.0,
                origin: [0.0, 0.0],
                size: [10.0, 10.0]
            })
        );
        // Sample outside the domain.
        assert_eq!(
            mba.interpolate(&[[50.0, 50.0]], &[1.0], dom, 1.0, &[]),
            Err(Error::SampleOutsideDomain {
                at: 0,
                x: 50.0,
                y: 50.0,
                origin: [0.0, 0.0],
                size: [10.0, 10.0]
            })
        );
        // Too many levels for the declared cap.
        let capped = Mba::new([1, 1]).unwrap().with_max_levels(2);
        assert!(matches!(
            capped.interpolate(&[[1.0, 1.0]], &[1.0], dom, 1e-6, &[]),
            Err(Error::TooManyLevels { max_levels: 2, .. })
        ));
    }

    /// **Not affine-invariant** (§8, Conclusion, p. 243): rotating the same
    /// scatter by 45° inside a domain that always encloses it gives a
    /// *different* result at the centre. This test **must** fail to be
    /// invariant — if it found the two surfaces equal, it would have
    /// implemented something other than MBA (a method invariant under
    /// isometries, like IDW or RBF).
    #[test]
    fn the_result_is_not_affine_invariant_under_rotation() {
        let base = [[1.0, 4.0], [4.0, 1.0], [7.0, 6.0], [3.0, 8.0], [8.0, 8.0]];
        let values = [1.0, 4.0, -2.0, 6.0, 0.5];
        let dom = domain([-5.0, -5.0], [20.0, 20.0]);
        let query = [[4.5, 4.5]];
        let mba = Mba::new([1, 1]).unwrap();
        let plain = mba.interpolate(&base, &values, dom, 1.0, &query).unwrap();

        let angle = std::f64::consts::FRAC_PI_4;
        let (c, s) = (angle.cos(), angle.sin());
        // Rotate the samples about the query point: the query is unmoved in
        // world coordinates, only the samples' position relative to the fixed
        // control grid changes.
        let [qx, qy] = query[0];
        let rotated: Vec<[f64; 2]> = base
            .iter()
            .map(|&[x, y]| {
                let (dx, dy) = (x - qx, y - qy);
                [qx + dx * c - dy * s, qy + dx * s + dy * c]
            })
            .collect();
        let turned = mba.interpolate(&rotated, &values, dom, 1.0, &query).unwrap();

        assert!(
            (plain.values[0] - turned.values[0]).abs() > 1e-6,
            "rotating the samples about a fixed grid should change the fitted value: {} vs {}",
            plain.values[0],
            turned.values[0]
        );
    }

    /// A query exactly on the domain's own far edge — the boundary
    /// `locate` folds onto the last cell rather than refusing (module doc,
    /// `locate`). To break: remove the folding in `locate` and this test
    /// goes red through a panic (index out of bounds) rather than through a
    /// wrong value.
    #[test]
    fn a_query_on_the_domains_far_edge_does_not_panic_or_refuse() {
        let mba = Mba::new([2, 2]).unwrap();
        let samples: Vec<[f64; 2]> = (0..5)
            .flat_map(|i| (0..5).map(move |j| [i as f64 * 2.0, j as f64 * 2.0]))
            .collect();
        let values: Vec<f64> = samples.iter().map(|_| 3.0).collect();
        let dom = domain([0.0, 0.0], [8.0, 8.0]);
        let answer = mba
            .interpolate(&samples, &values, dom, 1.0, &[[8.0, 8.0], [0.0, 0.0]])
            .unwrap();
        for value in &answer.values {
            assert!((value - 3.0).abs() < 1e-9);
        }
    }

    /// **Without the trend, the bare algorithm sinks toward zero away from
    /// the data** — the paper's own words (§7.3, p. 241-242) and the reason
    /// [`Trend::Plane`] is the default. Samples of 42 in a corner of a wide
    /// domain: far from them, the bare fit is well below 42, the default one
    /// is 42. To break: make `Trend::None` the default, or drop the trend from
    /// the evaluation — one of the two halves fails.
    #[test]
    fn without_the_trend_the_fit_sinks_away_from_the_data() {
        let samples: Vec<[f64; 2]> = (0..4)
            .flat_map(|i| (0..4).map(move |j| [i as f64, j as f64]))
            .collect();
        let values = vec![42.0; samples.len()];
        let dom = domain([0.0, 0.0], [40.0, 40.0]);
        let far = [[35.0, 35.0]];
        let bare = Mba::new([2, 2])
            .unwrap()
            .with_trend(Trend::None)
            .interpolate(&samples, &values, dom, 2.0, &far)
            .unwrap();
        let kept = Mba::new([2, 2])
            .unwrap()
            .interpolate(&samples, &values, dom, 2.0, &far)
            .unwrap();
        assert!(
            bare.values[0] < 41.0,
            "the bare fit far from the data: {} — it should have sunk",
            bare.values[0]
        );
        assert!(
            (kept.values[0] - 42.0).abs() < 1e-9,
            "with the trend: {}",
            kept.values[0]
        );
        assert_eq!(bare.trend, TrendUsed::None);
    }

    /// **Aligned samples define no plane: the trend is their mean, and the
    /// answer says so.** To break: fit the plane regardless — the 2×2 system is
    /// singular and the slopes come back infinite or NaN.
    #[test]
    fn aligned_samples_fall_back_to_their_mean_and_say_so() {
        let samples: Vec<[f64; 2]> = (0..5).map(|i| [i as f64 * 2.0, i as f64 * 2.0]).collect();
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let answer = Mba::new([2, 2])
            .unwrap()
            .interpolate(
                &samples,
                &values,
                domain([0.0, 0.0], [10.0, 10.0]),
                2.0,
                &[[1.0, 9.0]],
            )
            .unwrap();
        assert_eq!(answer.trend, TrendUsed::Constant(3.0));
        assert!(answer.values[0].is_finite());
    }
}
