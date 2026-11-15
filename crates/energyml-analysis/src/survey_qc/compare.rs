//! The gap between a survey and another path, MD by MD, and the turn about the vertical that fits two paths.

/// One sample of a path in the datum frame: metres, x east and y north (of a frame the caller names), z up,
/// origin at the MD datum.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PathSample {
    /// The measured depth, metres.
    pub md: f64,
    /// `[x east, y north, z up]`, metres.
    pub at: [f64; 3],
}

/// Where the gap is measured.
#[derive(Debug, Clone, PartialEq)]
pub enum CompareAt {
    /// At these MDs (the other path's nodes, or the survey's stations), kept where both paths exist.
    Mds(Vec<f64>),
    /// Every `step` metres over the overlap, both ends included.
    Step(f64),
}

/// The most samples one comparison takes.
pub const MOST_SAMPLES: usize = 100_000;

/// Whether the two paths share their horizontal axes, or only the vertical one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frames {
    /// Both in one frame: the gap is measured as it stands.
    Same,
    /// The same up to a turn about the vertical through the datum (the survey's north is not known):
    /// the turn that fits best is found and applied to the survey before the gap is measured.
    TurnedAboutVertical,
}

/// The gap at one MD, in metres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PositionGap {
    /// The measured depth.
    pub md: f64,
    /// The 3D distance (after the turn, under `TurnedAboutVertical`).
    pub total: f64,
    /// The other path's z minus the survey's: positive where the other path is higher. Turn-invariant.
    pub vertical: f64,
    /// The horizontal distance (after the turn).
    pub horizontal: f64,
    /// The other's horizontal distance from the datum minus the survey's. Turn-invariant.
    pub radial: f64,
}

/// A survey compared with another path.
#[derive(Debug, Clone, PartialEq)]
pub struct SurveyComparison {
    /// One gap per sample.
    pub gaps: Vec<PositionGap>,
    /// Index of the largest `total` in `gaps` (the first one on a tie).
    pub largest: usize,
    /// The arithmetic mean of the `total`s.
    pub mean_total: f64,
    /// The turn found, under `TurnedAboutVertical`.
    pub fit: Option<RotationFit>,
}

/// What a comparison refuses, by name.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CompareError {
    /// The two paths do not overlap in measured depth.
    #[error("the two paths share no measured depth: the survey runs {survey:?}, the other {other:?}")]
    NoOverlap {
        /// The survey's first and last MD.
        survey: [f64; 2],
        /// The other path's first and last MD.
        other: [f64; 2],
    },
    /// The step is not a positive number.
    #[error("a step of {step} m is not a positive number")]
    BadStep {
        /// The step given.
        step: f64,
    },
    /// More samples than one comparison takes.
    #[error("{count} samples, more than the {most} one comparison takes")]
    TooManySamples {
        /// How many were asked for.
        count: usize,
        /// The most there can be.
        most: usize,
    },
    /// The two sample lists do not have the same length.
    #[error("{survey} survey sample(s) against {other}: a gap is measured sample by sample")]
    LengthsDiffer {
        /// Samples of the survey.
        survey: usize,
        /// Samples of the other path.
        other: usize,
    },
    /// Two samples at the same index are at different measured depths.
    #[error("sample {index}: MD {survey} on the survey and {other} on the other path")]
    MdsDiffer {
        /// The sample, from 0.
        index: usize,
        /// The survey's MD there.
        survey: f64,
        /// The other path's MD there.
        other: f64,
    },
    /// A sample holds a number that is not finite.
    #[error("sample {index} is not a finite number")]
    NotFinite {
        /// The sample, from 0.
        index: usize,
    },
    /// Nothing to compare.
    #[error("no sample to compare")]
    NoSample,
    /// The turn about the vertical could not be fitted.
    #[error(transparent)]
    Fit(#[from] FitError),
}

/// The MDs to sample both paths at, over their overlap (`survey` and `other`: first and last MD of each).
pub fn sample_mds(survey: [f64; 2], other: [f64; 2], at: &CompareAt) -> Result<Vec<f64>, CompareError> {
    let lo = survey[0].max(other[0]);
    let hi = survey[1].min(other[1]);
    if lo.is_nan() || hi.is_nan() || lo >= hi {
        return Err(CompareError::NoOverlap { survey, other });
    }
    let mds: Vec<f64> = match at {
        CompareAt::Mds(values) => {
            let mut kept: Vec<f64> = values
                .iter()
                .copied()
                .filter(|md| md.is_finite() && *md >= lo && *md <= hi)
                .collect();
            kept.sort_by(|a, b| a.total_cmp(b));
            kept.dedup();
            if kept.len() > MOST_SAMPLES {
                return Err(CompareError::TooManySamples {
                    count: kept.len(),
                    most: MOST_SAMPLES,
                });
            }
            kept
        }
        CompareAt::Step(step) => {
            if !step.is_finite() || *step <= 0.0 {
                return Err(CompareError::BadStep { step: *step });
            }
            let whole = ((hi - lo) / step).floor();
            // Refused before anything is allocated; the count is in floating point so that a tiny step cannot overflow.
            let count = whole + 2.0;
            if count > MOST_SAMPLES as f64 {
                return Err(CompareError::TooManySamples {
                    count: count.min(usize::MAX as f64) as usize,
                    most: MOST_SAMPLES,
                });
            }
            let whole = whole as usize;
            let mut mds: Vec<f64> = (0..=whole).map(|i| (lo + i as f64 * step).min(hi)).collect();
            match mds.last().copied() {
                Some(last) if hi - last > 1e-9 => mds.push(hi),
                Some(_) => {
                    if let Some(last) = mds.last_mut() {
                        *last = hi;
                    }
                }
                None => {}
            }
            mds
        }
    };
    if mds.is_empty() {
        return Err(CompareError::NoSample);
    }
    Ok(mds)
}

/// The turn about the vertical that brings the survey's horizontal positions closest to the other path's
/// (least squares), and what is left after it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RotationFit {
    /// Degrees, clockwise seen from above (an azimuth increment): the survey's azimuths plus this angle
    /// are the other path's.
    pub angle: f64,
    /// The largest distance left between a turned survey position and the other's, metres.
    pub largest_residual: f64,
    /// The root mean square of those distances, metres.
    pub rms_residual: f64,
    /// How many positions were fitted.
    pub samples: usize,
}

/// What a fit cannot be.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum FitError {
    /// The two lists do not have the same length.
    #[error("{survey} survey position(s) against {other}")]
    LengthsDiffer {
        /// Positions of the survey.
        survey: usize,
        /// Positions of the other path.
        other: usize,
    },
    /// Every position is on the vertical of the datum.
    #[error("no horizontal extent: every position is on the vertical of the datum, no turn can be read")]
    NoHorizontalExtent,
}

/// `[east, north]` turned clockwise by `degrees`, seen from above.
fn turned(a: [f64; 2], degrees: f64) -> [f64; 2] {
    let (sin, cos) = degrees.to_radians().sin_cos();
    [a[0] * cos + a[1] * sin, a[1] * cos - a[0] * sin]
}

/// The turn that fits `survey` to `other` (`[x east, y north]`, metres, from the datum), the least squares one:
/// with `a = survey[i]`, `b = other[i]`, `S = sum (a.n b.e - a.e b.n)`, `C = sum (a.n b.n + a.e b.e)` and the angle
/// `atan2(S, C)`, in `(-180, 180]`.
pub fn rotation_fit(survey: &[[f64; 2]], other: &[[f64; 2]]) -> Result<RotationFit, FitError> {
    if survey.len() != other.len() {
        return Err(FitError::LengthsDiffer {
            survey: survey.len(),
            other: other.len(),
        });
    }
    let (mut s, mut c, mut norms) = (0.0_f64, 0.0_f64, 0.0_f64);
    for (a, b) in survey.iter().zip(other) {
        s += a[1] * b[0] - a[0] * b[1];
        c += a[1] * b[1] + a[0] * b[0];
        norms += a[0].hypot(a[1]) * b[0].hypot(b[1]);
    }
    let tolerance = 1e-12 * norms;
    if survey.is_empty() || !(s.abs() > tolerance || c.abs() > tolerance) {
        return Err(FitError::NoHorizontalExtent);
    }
    let mut angle = s.atan2(c).to_degrees();
    if angle <= -180.0 {
        angle += 360.0;
    }
    let (mut largest, mut squares) = (0.0_f64, 0.0_f64);
    for (a, b) in survey.iter().zip(other) {
        let a = turned(*a, angle);
        let residual = (b[0] - a[0]).hypot(b[1] - a[1]);
        largest = largest.max(residual);
        squares += residual * residual;
    }
    Ok(RotationFit {
        angle,
        largest_residual: largest,
        rms_residual: (squares / survey.len() as f64).sqrt(),
        samples: survey.len(),
    })
}

/// The gap between two paths sampled at the same MDs, in the datum frame.
pub fn compare(
    survey: &[PathSample],
    other: &[PathSample],
    frames: Frames,
) -> Result<SurveyComparison, CompareError> {
    if survey.len() != other.len() {
        return Err(CompareError::LengthsDiffer {
            survey: survey.len(),
            other: other.len(),
        });
    }
    if survey.is_empty() {
        return Err(CompareError::NoSample);
    }
    for (index, (a, b)) in survey.iter().zip(other).enumerate() {
        let finite = a.md.is_finite()
            && b.md.is_finite()
            && a.at.iter().all(|v| v.is_finite())
            && b.at.iter().all(|v| v.is_finite());
        if !finite {
            return Err(CompareError::NotFinite { index });
        }
        if (a.md - b.md).abs() > 1e-9 * a.md.abs().max(b.md.abs()).max(1.0) {
            return Err(CompareError::MdsDiffer {
                index,
                survey: a.md,
                other: b.md,
            });
        }
    }
    let fit = match frames {
        Frames::Same => None,
        Frames::TurnedAboutVertical => {
            let a: Vec<[f64; 2]> = survey.iter().map(|p| [p.at[0], p.at[1]]).collect();
            let b: Vec<[f64; 2]> = other.iter().map(|p| [p.at[0], p.at[1]]).collect();
            Some(rotation_fit(&a, &b)?)
        }
    };
    let angle = fit.map_or(0.0, |f| f.angle);
    let mut gaps = Vec::with_capacity(survey.len());
    for (a, b) in survey.iter().zip(other) {
        let xy = if fit.is_some() {
            turned([a.at[0], a.at[1]], angle)
        } else {
            [a.at[0], a.at[1]]
        };
        let (dx, dy, dz) = (b.at[0] - xy[0], b.at[1] - xy[1], b.at[2] - a.at[2]);
        gaps.push(PositionGap {
            md: a.md,
            total: (dx * dx + dy * dy + dz * dz).sqrt(),
            vertical: dz,
            horizontal: dx.hypot(dy),
            radial: b.at[0].hypot(b.at[1]) - a.at[0].hypot(a.at[1]),
        });
    }
    let mut largest = 0;
    for (index, gap) in gaps.iter().enumerate() {
        if gap.total > gaps[largest].total {
            largest = index;
        }
    }
    let mean_total = gaps.iter().map(|g| g.total).sum::<f64>() / gaps.len() as f64;
    Ok(SurveyComparison {
        gaps,
        largest,
        mean_total,
        fit,
    })
}
