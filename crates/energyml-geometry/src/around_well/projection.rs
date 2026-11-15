//! Laying the well flat: one formula per projection, in a registry, plus the locator (the whole hole as a 2D
//! line, and the mark of one measured depth on it).
//!
//! A point `(md, dn)` of the hole's frame (`dn` metres along the high side) becomes a 2D point. The true
//! vertical depth is counted **below the MD datum** (S6): a path that does not reach MD 0 has no datum, and the
//! projections that need one refuse by name. The position of a mark is always `PlacedPath::at(md)` projected,
//! never a length taken on the 2D line (on the plan view of a J-shaped well the projected line is shorter than
//! the MD).
//!
//! | id | x | y |
//! |---|---|---|
//! | `md_bar` | `0` | `md` |
//! | `md_tvd` | `md` | `tvd(Q)` |
//! | `developed` | `s(md) + dn cos I` | `tvd(Q)` |
//! | `vertical_section` | `(E(Q) - E0) sin a + (N(Q) - N0) cos a` | `tvd(Q)` |
//! | `plan` | `E(Q)` | `N(Q)` |
//!
//! With `Q = P + dn h`, `h` the high side, `I` the inclination, `s` the cumulated plan length.

use crate::around_well::hole_frame::{FrameRefusal, MIN_INCLINATION_DEGREES, offset_point};
use crate::points::Point;
use crate::units;
use crate::well::{MdRange, PlacedPath};

/// The most samples an axis takes.
pub const MAX_AXIS_SAMPLES: usize = 1_000_000;

/// The deepest bisection of an interval of the locator line.
const MAX_REFINE_DEPTH: u32 = 16;

/// Why a projection cannot be laid, by name.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum ProjectionRefusal {
    /// The projection needs the trajectory.
    #[error("this projection needs the trajectory, and only an MD extent is known")]
    NoPath,
    /// The path does not reach MD 0.
    #[error("the path does not reach MD 0: no datum to count a true vertical depth from")]
    NoDatum,
    /// East and north in the placed plane are not known.
    #[error("the CRS states no usable order of its horizontal axes: east and north are not known")]
    AxisOrderUnknown,
    /// A depth outside the axis.
    #[error("MD {md} is outside [{first}, {last}]")]
    MdOutside {
        /// The depth asked.
        md: f64,
        /// The first depth of the axis.
        first: f64,
        /// The last depth of the axis.
        last: f64,
    },
    /// A range that is not an increasing pair of numbers.
    #[error("the MD range [{from}, {to}] is not an increasing pair of numbers")]
    BadRange {
        /// The start of the range.
        from: f64,
        /// The end of the range.
        to: f64,
    },
    /// More samples than an axis takes.
    #[error("{samples} samples, more than {max}: take a larger step")]
    TooManySamples {
        /// The samples the range would take.
        samples: usize,
        /// The most an axis takes.
        max: usize,
    },
    /// A point off the hole in a projection that has no such point.
    #[error("projection `{projection}` lays no point off the hole")]
    OffsetNotSupported {
        /// The id of the projection.
        projection: &'static str,
    },
    /// The line has no direction there.
    #[error("at MD {md}: the line has no direction")]
    NoDirection {
        /// The measured depth.
        md: f64,
    },
    /// A tolerance that is not a positive number.
    #[error("a tolerance of {tolerance} m draws nothing true")]
    BadTolerance {
        /// The tolerance given.
        tolerance: f64,
    },
    /// An MD unit that is no length this build converts.
    #[error("the MD unit `{uom}` is not a length this build converts")]
    MdUnit {
        /// The unit as the file spells it.
        uom: String,
    },
    /// A refusal of the frame of the hole.
    #[error(transparent)]
    Frame(#[from] FrameRefusal),
}

/// The well sampled once over a range: what every projection reads.
#[derive(Clone, Debug)]
pub struct WellAxis {
    /// `None` for an axis built from an MD extent alone (`md_bar` only).
    pub path: Option<PlacedPath>,
    /// Ascending: the range's ends, the knots inside it, and the steps.
    pub md: Vec<f64>,
    /// Placed points at `md` (empty without a path).
    pub at: Vec<Point>,
    /// Cumulated horizontal length from `md[0]`, metres (empty without a path).
    pub plan_length: Vec<f64>,
}

fn bad_range(from: f64, to: f64) -> bool {
    !from.is_finite() || !to.is_finite() || from > to
}

impl WellAxis {
    /// Samples `md_range` (within the path's extent): its ends, every knot inside, and every `step`
    /// (`step <= 0`: knots and ends only). At most [`MAX_AXIS_SAMPLES`], counted before anything is evaluated.
    pub fn new(
        path: &PlacedPath,
        md_range: [f64; 2],
        step: f64,
    ) -> Result<WellAxis, ProjectionRefusal> {
        let [from, to] = md_range;
        if bad_range(from, to) {
            return Err(ProjectionRefusal::BadRange { from, to });
        }
        let [first, last] = path.md_extent();
        for md in [from, to] {
            if md < first || md > last {
                return Err(ProjectionRefusal::MdOutside { md, first, last });
            }
        }
        let inside = path.knots().iter().filter(|&&k| k > from && k < to).count();
        let stepped = if step.is_finite() && step > 0.0 {
            ((to - from) / step).floor()
        } else {
            0.0
        };
        let estimate = 2.0 + inside as f64 + stepped;
        if estimate > MAX_AXIS_SAMPLES as f64 {
            return Err(ProjectionRefusal::TooManySamples {
                samples: estimate as usize,
                max: MAX_AXIS_SAMPLES,
            });
        }

        let mut md = vec![from, to];
        md.extend(path.knots().iter().copied().filter(|&k| k > from && k < to));
        if step.is_finite() && step > 0.0 {
            let mut i = 1.0_f64;
            loop {
                let m = from + i * step;
                if m >= to {
                    break;
                }
                md.push(m);
                i += 1.0;
            }
        }
        md.sort_by(|a, b| a.total_cmp(b));
        let mut merged: Vec<f64> = Vec::with_capacity(md.len());
        for m in md {
            if merged.last().is_none_or(|&last| m - last > 1e-9) {
                merged.push(m);
            }
        }

        let at: Vec<Point> = merged.iter().map(|&m| path.at(m).point).collect();
        let mut plan_length = Vec::with_capacity(at.len());
        let mut total = 0.0;
        for (j, point) in at.iter().enumerate() {
            if j > 0 {
                total += (point[0] - at[j - 1][0]).hypot(point[1] - at[j - 1][1]);
            }
            plan_length.push(total);
        }
        Ok(WellAxis {
            path: Some(path.clone()),
            md: merged,
            at,
            plan_length,
        })
    }

    /// The axis of `md_bar` when there is no trajectory: an MD extent alone.
    pub fn md_only(md_extent: [f64; 2]) -> Result<WellAxis, ProjectionRefusal> {
        let [from, to] = md_extent;
        if bad_range(from, to) {
            return Err(ProjectionRefusal::BadRange { from, to });
        }
        let md = if from == to { vec![from] } else { vec![from, to] };
        Ok(WellAxis {
            path: None,
            md,
            at: Vec::new(),
            plan_length: Vec::new(),
        })
    }

    /// `[md[0], md[last]]`.
    pub fn extent(&self) -> [f64; 2] {
        [self.md[0], self.md[self.md.len() - 1]]
    }

    /// s(md): the cumulated plan length at the sample below, plus the plan distance to the point at `md`.
    pub fn plan_length_at(&self, md: f64) -> Result<f64, ProjectionRefusal> {
        self.within(md)?;
        let path = self.path.as_ref().ok_or(ProjectionRefusal::NoPath)?;
        let j = self.md.partition_point(|&m| m <= md).saturating_sub(1);
        let p = path.at(md).point;
        Ok(self.plan_length[j] + (p[0] - self.at[j][0]).hypot(p[1] - self.at[j][1]))
    }

    fn within(&self, md: f64) -> Result<(), ProjectionRefusal> {
        let [first, last] = self.extent();
        if md.is_nan() || md < first || md > last {
            return Err(ProjectionRefusal::MdOutside { md, first, last });
        }
        Ok(())
    }
}

/// One way to lay a well flat.
pub trait WellProjection: Send + Sync {
    /// Its name in the registry and in a view's recipe.
    fn id(&self) -> &'static str;
    /// Whether one unit along x is one metre on the ground (decides the exaggeration wording).
    fn abscissa_is_length(&self) -> bool;
    /// The point (md, dn) - dn metres along the high side - laid flat.
    fn place(&self, axis: &WellAxis, md: f64, dn: f64) -> Result<[f64; 2], ProjectionRefusal>;
    /// The image of the hole's direction at `md`, per metre along the hole; `None` when it vanishes.
    fn direction(&self, axis: &WellAxis, md: f64) -> Result<Option<[f64; 2]>, ProjectionRefusal>;
    /// Back from a laid point to `[md, dn]`, when the projection can say. `None` by default.
    fn invert(&self, _axis: &WellAxis, _xy: [f64; 2]) -> Option<[f64; 2]> {
        None
    }
}

// -- shared helpers ------------------------------------------------------------------------------

/// What every projection needs at one depth: the centre `p`, the point `q = p + dn h`, and the inclination.
struct Pose<'a> {
    path: &'a PlacedPath,
    q: Point,
    sin_i: f64,
    cos_i: f64,
}

fn pose(axis: &WellAxis, md: f64, dn: f64) -> Result<Pose<'_>, ProjectionRefusal> {
    axis.within(md)?;
    let path = axis.path.as_ref().ok_or(ProjectionRefusal::NoPath)?;
    let direction = path.direction_at(md);
    if dn != 0.0 && direction.is_none() {
        return Err(ProjectionRefusal::NoDirection { md });
    }
    let q = offset_point(path, md, dn)?;
    let (sin_i, cos_i) = direction.map_or((0.0, 0.0), |t| (t[0].hypot(t[1]), -t[2]));
    Ok(Pose {
        path,
        q,
        sin_i,
        cos_i,
    })
}

fn datum_of(path: &PlacedPath) -> Result<Point, ProjectionRefusal> {
    path.datum().ok_or(ProjectionRefusal::NoDatum)
}

fn compass_of(path: &PlacedPath) -> Result<energyml_domain::crs::CompassAxes, ProjectionRefusal> {
    path.compass().ok_or(ProjectionRefusal::AxisOrderUnknown)
}

fn east_of(compass: &energyml_domain::crs::CompassAxes, p: Point) -> f64 {
    p[0] * compass.east[0] + p[1] * compass.east[1]
}

fn north_of(compass: &energyml_domain::crs::CompassAxes, p: Point) -> f64 {
    p[0] * compass.north[0] + p[1] * compass.north[1]
}

/// The direction of the hole at `md`, placed.
fn tangent(axis: &WellAxis, md: f64) -> Result<Point, ProjectionRefusal> {
    axis.within(md)?;
    let path = axis.path.as_ref().ok_or(ProjectionRefusal::NoPath)?;
    path.direction_at(md)
        .ok_or(ProjectionRefusal::NoDirection { md })
}

/// A 2D vector, or `None` when it vanishes.
fn nonzero(v: [f64; 2]) -> Option<[f64; 2]> {
    let length = v[0].hypot(v[1]);
    (length.is_finite() && length > 1e-12).then_some(v)
}

/// MD units per metre along the hole: 1 for an axis with no trajectory.
fn md_per_metre(axis: &WellAxis) -> Result<f64, ProjectionRefusal> {
    match &axis.path {
        None => Ok(1.0),
        Some(path) => {
            let metres = units::metres(1.0, path.md_uom()).map_err(|_| ProjectionRefusal::MdUnit {
                uom: path.md_uom().to_string(),
            })?;
            Ok(1.0 / metres)
        }
    }
}

fn min_sin_inclination() -> f64 {
    MIN_INCLINATION_DEGREES.to_radians().sin()
}

/// The offset `dn` that puts `y` on the sample's ordinate (the ordinate being `tvd(P) - dn sin I`).
fn offset_for_ordinate(axis: &WellAxis, md: f64, y: f64) -> Option<f64> {
    let path = axis.path.as_ref()?;
    let datum = path.datum()?;
    let t = path.direction_at(md)?;
    let sin_i = t[0].hypot(t[1]);
    if sin_i.is_nan() || sin_i < min_sin_inclination() {
        return None;
    }
    let p = path.at(md).point;
    Some((datum[2] - p[2] - y) / sin_i)
}

/// The inverse of a projection whose ordinate is `tvd(P) - dn sin I`: the sign changes of the abscissa error
/// along the axis, each refined by bisection; the candidate nearest the hole (smallest `|dn|`) wins.
#[allow(clippy::needless_range_loop)]
fn invert_by_scan(
    projection: &dyn WellProjection,
    axis: &WellAxis,
    xy: [f64; 2],
) -> Option<[f64; 2]> {
    let error_at = |md: f64| -> Option<(f64, f64)> {
        let dn = offset_for_ordinate(axis, md, xy[1])?;
        let x = projection.place(axis, md, dn).ok()?[0];
        Some((dn, x - xy[0]))
    };
    let values: Vec<Option<(f64, f64)>> = axis.md.iter().map(|&md| error_at(md)).collect();
    let mut best: Option<[f64; 2]> = None;
    let mut consider = |candidate: [f64; 2]| {
        if best.is_none_or(|b| candidate[1].abs() < b[1].abs()) {
            best = Some(candidate);
        }
    };
    for i in 0..values.len() {
        let Some((dn, f)) = values[i] else { continue };
        if f == 0.0 {
            consider([axis.md[i], dn]);
        }
        let Some(Some((_, f_next))) = values.get(i + 1) else {
            continue;
        };
        if f * f_next < 0.0 {
            let (mut lo, mut hi, mut f_lo) = (axis.md[i], axis.md[i + 1], f);
            let mut found: Option<[f64; 2]> = None;
            for _ in 0..60 {
                let mid = (lo + hi) / 2.0;
                let Some((dn_mid, f_mid)) = error_at(mid) else {
                    found = None;
                    break;
                };
                found = Some([mid, dn_mid]);
                if f_mid == 0.0 || (hi - lo).abs() < 1e-9 {
                    break;
                }
                if (f_mid < 0.0) == (f_lo < 0.0) {
                    lo = mid;
                    f_lo = f_mid;
                } else {
                    hi = mid;
                }
            }
            if let Some(candidate) = found {
                consider(candidate);
            }
        }
    }
    best
}

// -- the entries ---------------------------------------------------------------------------------

/// `md_bar`: the MD extent alone, as a bar: `(0, md)`.
pub struct MdBar;

/// `md_tvd`: `(md, tvd)`.
pub struct MdTvd;

/// `developed`: the plan length along x, the true vertical depth along y.
pub struct Developed;

/// `plan`: east and north.
pub struct Plan;

/// `vertical_section`: the abscissa along a vertical plane of `azimuth`, the true vertical depth along y.
/// `azimuth`: radians, clockwise from grid north; `origin`: `[east, north]`, metres.
pub struct VerticalSection {
    /// Radians, clockwise from grid north.
    pub azimuth: f64,
    /// `[east, north]` in metres, the origin of the section.
    pub origin: [f64; 2],
}

impl VerticalSection {
    /// The one formula of the vertical section: `(e - origin[0]) sin a + (n - origin[1]) cos a` for a point
    /// `[e, n]` in the same frame as `origin` (WITSML TRG 2.1 section 16.7 p. 352, section 16.10 p. 361).
    pub fn abscissa(&self, east_north: [f64; 2]) -> f64 {
        let (sin, cos) = self.azimuth.sin_cos();
        (east_north[0] - self.origin[0]) * sin + (east_north[1] - self.origin[1]) * cos
    }
}

impl WellProjection for MdBar {
    fn id(&self) -> &'static str {
        "md_bar"
    }

    fn abscissa_is_length(&self) -> bool {
        false
    }

    fn place(&self, axis: &WellAxis, md: f64, dn: f64) -> Result<[f64; 2], ProjectionRefusal> {
        axis.within(md)?;
        if dn != 0.0 {
            return Err(ProjectionRefusal::OffsetNotSupported {
                projection: self.id(),
            });
        }
        Ok([0.0, md])
    }

    fn direction(&self, axis: &WellAxis, md: f64) -> Result<Option<[f64; 2]>, ProjectionRefusal> {
        axis.within(md)?;
        Ok(nonzero([0.0, md_per_metre(axis)?]))
    }

    fn invert(&self, _axis: &WellAxis, xy: [f64; 2]) -> Option<[f64; 2]> {
        Some([xy[1], 0.0])
    }
}

impl WellProjection for MdTvd {
    fn id(&self) -> &'static str {
        "md_tvd"
    }

    fn abscissa_is_length(&self) -> bool {
        false
    }

    fn place(&self, axis: &WellAxis, md: f64, dn: f64) -> Result<[f64; 2], ProjectionRefusal> {
        let pose = pose(axis, md, dn)?;
        let datum = datum_of(pose.path)?;
        Ok([md, datum[2] - pose.q[2]])
    }

    fn direction(&self, axis: &WellAxis, md: f64) -> Result<Option<[f64; 2]>, ProjectionRefusal> {
        let t = tangent(axis, md)?;
        Ok(nonzero([md_per_metre(axis)?, -t[2]]))
    }

    fn invert(&self, axis: &WellAxis, xy: [f64; 2]) -> Option<[f64; 2]> {
        let md = xy[0];
        axis.within(md).ok()?;
        let dn = offset_for_ordinate(axis, md, xy[1])?;
        Some([md, dn])
    }
}

impl WellProjection for Developed {
    fn id(&self) -> &'static str {
        "developed"
    }

    fn abscissa_is_length(&self) -> bool {
        true
    }

    fn place(&self, axis: &WellAxis, md: f64, dn: f64) -> Result<[f64; 2], ProjectionRefusal> {
        let pose = pose(axis, md, dn)?;
        let datum = datum_of(pose.path)?;
        let s = axis.plan_length_at(md)?;
        Ok([s + dn * pose.cos_i, datum[2] - pose.q[2]])
    }

    fn direction(&self, axis: &WellAxis, md: f64) -> Result<Option<[f64; 2]>, ProjectionRefusal> {
        let t = tangent(axis, md)?;
        Ok(nonzero([t[0].hypot(t[1]), -t[2]]))
    }

    fn invert(&self, axis: &WellAxis, xy: [f64; 2]) -> Option<[f64; 2]> {
        invert_by_scan(self, axis, xy)
    }
}

impl WellProjection for Plan {
    fn id(&self) -> &'static str {
        "plan"
    }

    fn abscissa_is_length(&self) -> bool {
        true
    }

    fn place(&self, axis: &WellAxis, md: f64, dn: f64) -> Result<[f64; 2], ProjectionRefusal> {
        let pose = pose(axis, md, dn)?;
        let compass = compass_of(pose.path)?;
        Ok([east_of(&compass, pose.q), north_of(&compass, pose.q)])
    }

    fn direction(&self, axis: &WellAxis, md: f64) -> Result<Option<[f64; 2]>, ProjectionRefusal> {
        let t = tangent(axis, md)?;
        let path = axis.path.as_ref().ok_or(ProjectionRefusal::NoPath)?;
        let compass = compass_of(path)?;
        Ok(nonzero([east_of(&compass, t), north_of(&compass, t)]))
    }
}

impl WellProjection for VerticalSection {
    fn id(&self) -> &'static str {
        "vertical_section"
    }

    fn abscissa_is_length(&self) -> bool {
        true
    }

    fn place(&self, axis: &WellAxis, md: f64, dn: f64) -> Result<[f64; 2], ProjectionRefusal> {
        let pose = pose(axis, md, dn)?;
        let datum = datum_of(pose.path)?;
        let compass = compass_of(pose.path)?;
        let x = self.abscissa([east_of(&compass, pose.q), north_of(&compass, pose.q)]);
        Ok([x, datum[2] - pose.q[2]])
    }

    fn direction(&self, axis: &WellAxis, md: f64) -> Result<Option<[f64; 2]>, ProjectionRefusal> {
        let t = tangent(axis, md)?;
        let path = axis.path.as_ref().ok_or(ProjectionRefusal::NoPath)?;
        let compass = compass_of(path)?;
        let (sin, cos) = self.azimuth.sin_cos();
        let along = east_of(&compass, t) * sin + north_of(&compass, t) * cos;
        Ok(nonzero([along, -t[2]]))
    }

    fn invert(&self, axis: &WellAxis, xy: [f64; 2]) -> Option<[f64; 2]> {
        invert_by_scan(self, axis, xy)
    }
}

/// The projections, the caller's additions tried first.
pub struct WellProjections {
    entries: Vec<Box<dyn WellProjection>>,
}

impl WellProjections {
    /// An empty registry.
    pub fn new() -> Self {
        WellProjections {
            entries: Vec::new(),
        }
    }

    /// `md_bar`, `md_tvd`, `developed` and `plan`. `vertical_section` is built by the caller: it needs an
    /// azimuth and an origin.
    pub fn standard() -> Self {
        WellProjections {
            entries: vec![
                Box::new(MdBar),
                Box::new(MdTvd),
                Box::new(Developed),
                Box::new(Plan),
            ],
        }
    }

    /// Puts an entry before every entry already there.
    pub fn add(&mut self, entry: Box<dyn WellProjection>) {
        self.entries.insert(0, entry);
    }

    /// The first entry of that id.
    pub fn get(&self, id: &str) -> Option<&dyn WellProjection> {
        self.entries
            .iter()
            .find(|entry| entry.id() == id)
            .map(|entry| entry.as_ref())
    }

    /// The ids, in the order they are tried.
    pub fn ids(&self) -> Vec<&'static str> {
        self.entries.iter().map(|entry| entry.id()).collect()
    }
}

impl Default for WellProjections {
    fn default() -> Self {
        Self::standard()
    }
}

// -- the locator ---------------------------------------------------------------------------------

/// The whole hole as a 2D polyline, fine enough for a tolerance in metres.
#[derive(Clone, Debug, PartialEq)]
pub struct LocatorLine {
    /// The points of the line.
    pub points: Vec<[f64; 2]>,
    /// The MD of each point.
    pub md: Vec<f64>,
    /// Every point within `tolerance` of the first: the line is a point (a vertical hole in plan).
    pub degenerate: bool,
}

/// Where one MD falls in the projection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LocatorMark {
    /// The MD asked.
    pub md: f64,
    /// `PlacedPath::at(md)` projected, the MD clamped to the axis first.
    pub at: [f64; 2],
    /// Unit, perpendicular to the line there: the direction of the tick. `None` where the line has no direction.
    pub normal: Option<[f64; 2]>,
    /// Within the axis, or clamped to its end and said.
    pub range: MdRange,
}

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// The distance from `p` to the segment `a b`, and the parameter in [0, 1] of its nearest point.
fn segment_distance(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> (f64, f64) {
    let ab = [b[0] - a[0], b[1] - a[1]];
    let length2 = ab[0] * ab[0] + ab[1] * ab[1];
    let u = if length2 > 0.0 {
        (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / length2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let nearest = [a[0] + u * ab[0], a[1] + u * ab[1]];
    (distance(p, nearest), u)
}

#[allow(clippy::too_many_arguments)]
fn refine(
    axis: &WellAxis,
    projection: &dyn WellProjection,
    (m0, p0): (f64, [f64; 2]),
    (m1, p1): (f64, [f64; 2]),
    tolerance: f64,
    depth: u32,
    points: &mut Vec<[f64; 2]>,
    mds: &mut Vec<f64>,
) -> Result<(), ProjectionRefusal> {
    if depth < MAX_REFINE_DEPTH {
        let mid = (m0 + m1) / 2.0;
        let pm = projection.place(axis, mid, 0.0)?;
        if segment_distance(pm, p0, p1).0 > tolerance {
            refine(axis, projection, (m0, p0), (mid, pm), tolerance, depth + 1, points, mds)?;
            return refine(axis, projection, (mid, pm), (m1, p1), tolerance, depth + 1, points, mds);
        }
    }
    points.push(p1);
    mds.push(m1);
    Ok(())
}

/// The whole hole as a 2D polyline, fine enough for `tolerance` metres (each interval of the axis is cut at its
/// middle while the middle point is farther than `tolerance` from the chord, down to a depth of 16).
pub fn locator_line(
    axis: &WellAxis,
    projection: &dyn WellProjection,
    tolerance: f64,
) -> Result<LocatorLine, ProjectionRefusal> {
    if !tolerance.is_finite() || tolerance <= 0.0 {
        return Err(ProjectionRefusal::BadTolerance { tolerance });
    }
    let samples: Vec<(f64, [f64; 2])> = axis
        .md
        .iter()
        .map(|&md| projection.place(axis, md, 0.0).map(|p| (md, p)))
        .collect::<Result<_, _>>()?;
    let mut points = vec![samples[0].1];
    let mut mds = vec![samples[0].0];
    for pair in samples.windows(2) {
        refine(axis, projection, pair[0], pair[1], tolerance, 0, &mut points, &mut mds)?;
    }
    let degenerate = points.iter().all(|&p| distance(p, points[0]) <= tolerance);
    Ok(LocatorLine {
        points,
        md: mds,
        degenerate,
    })
}

/// Where `md` falls in the projection. The position is `PlacedPath::at(md)` projected (through `place`), never
/// a length measured on the 2D line.
pub fn locator_mark(
    axis: &WellAxis,
    projection: &dyn WellProjection,
    md: f64,
) -> Result<LocatorMark, ProjectionRefusal> {
    if md.is_nan() {
        return Err(ProjectionRefusal::BadRange { from: md, to: md });
    }
    let [first, last] = axis.extent();
    let clamped = md.clamp(first, last);
    let range = if md < first {
        MdRange::BeforeFirst { first }
    } else if md > last {
        MdRange::AfterLast { last }
    } else {
        MdRange::Within
    };
    let at = projection.place(axis, clamped, 0.0)?;
    let normal = projection
        .direction(axis, clamped)?
        .and_then(|d| nonzero([-d[1], d[0]]))
        .map(|n| {
            let length = n[0].hypot(n[1]);
            [n[0] / length, n[1] / length]
        });
    Ok(LocatorMark {
        md: clamped,
        at,
        normal,
        range,
    })
}

/// The MD of the line's point nearest `xy`, interpolated along its segment; `None` for an empty or degenerate
/// line.
pub fn locator_md_at(line: &LocatorLine, xy: [f64; 2]) -> Option<f64> {
    if line.degenerate || line.points.len() < 2 {
        return None;
    }
    let mut best: Option<(f64, f64)> = None;
    for i in 0..line.points.len() - 1 {
        let (d, u) = segment_distance(xy, line.points[i], line.points[i + 1]);
        if best.is_none_or(|(best_d, _)| d < best_d) {
            best = Some((d, line.md[i] + u * (line.md[i + 1] - line.md[i])));
        }
    }
    best.map(|(_, md)| md)
}

/// The point of the line at `md`, interpolated linearly along its segment.
fn line_point_at(line: &LocatorLine, md: f64) -> Option<[f64; 2]> {
    let n = line.md.len();
    if n == 0 {
        return None;
    }
    if n == 1 {
        return Some(line.points[0]);
    }
    let i = line.md.partition_point(|&m| m <= md).clamp(1, n - 1) - 1;
    let (m0, m1) = (line.md[i], line.md[i + 1]);
    let u = if m1 > m0 { (md - m0) / (m1 - m0) } else { 0.0 };
    let (p0, p1) = (line.points[i], line.points[i + 1]);
    Some([p0[0] + u * (p1[0] - p0[0]), p0[1] + u * (p1[1] - p0[1])])
}

/// The pieces of the line inside each MD span (frames, data), ends interpolated. A span that is not increasing,
/// or does not meet the line, gives an empty piece.
pub fn locator_spans(line: &LocatorLine, spans: &[[f64; 2]]) -> Vec<Vec<[f64; 2]>> {
    spans
        .iter()
        .map(|&[a, b]| {
            let (Some(&first), Some(&last)) = (line.md.first(), line.md.last()) else {
                return Vec::new();
            };
            if a.is_nan() || b.is_nan() || a >= b || b <= first || a >= last {
                return Vec::new();
            }
            let (lo, hi) = (a.max(first), b.min(last));
            let mut piece = Vec::new();
            piece.extend(line_point_at(line, lo));
            for (m, p) in line.md.iter().zip(&line.points) {
                if *m > lo && *m < hi {
                    piece.push(*p);
                }
            }
            piece.extend(line_point_at(line, hi));
            piece
        })
        .collect()
}
