//! Extending a polyline or a triangulated surface past its border.
//!
//! The use case: a fault surface stops too low, and the user wants to widen it
//! by 10 m upward, 5 m downward and 8 m on the sides - with the direction read
//! as a mean tangent over a certain distance from the borders, so that one or
//! two badly placed points at the border do not make the problem worse.
//!
//! An extension **invents** geometry. Everything here keeps what was given
//! bit for bit - the input nodes come first and unchanged, the input triangles
//! too - and only **adds**, so a caller can always say which nodes were
//! measured and which were made up: `original_nodes` is where the invented ones
//! start.
//!
//! # The direction: a robust fit to a band of the border
//!
//! Not the last edge: a **band** of nodes near the border - the last `band`
//! of a polyline's length, the nodes within `band` of a surface's border node -
//! to which a line (a polyline) or a plane (a surface) is fitted. Principal
//! components (Hoppe, 1992), **reweighted** with Huber's weights (k = 1.345 σ,
//! σ the median absolute residual × 1.4826) until the weights settle: a
//! misplaced node near the border is weighted down instead of pulling the
//! direction, and the nodes left beyond 2.5 σ are **counted** - a badly picked
//! border is news in itself. The band's width is the setting that matters:
//! too narrow, it reads the noise; too wide, it smooths a real curvature away.
//!
//! # The linear extension
//!
//! The only method written: straight on along the fitted direction (the
//! CAD's *linear* extension). The *natural* one, following the curvature damped
//! to zero, is the second method and is not written.
//!
//! - **a polyline** goes on from its end node along the line fitted to its last
//!   `band` of length, oriented outward, by the distance asked, in nodes spaced
//!   at the band's mean segment length - the last one exactly at the distance;
//! - **a surface** grows by rings. Each node of its outer border moves along
//!   its **outward direction**: the bisector of its two border edges, taken in
//!   the plane fitted to its band and turned away from the surface (Move's
//!   *bisectors*). Ring `k` of `n` puts each node at `k/n` of its distance;
//!   `n` is the longest distance over the border's mean edge, rounded up. The
//!   rings are stitched into bands of triangles wound like the triangle each
//!   border edge came from, so the normals stay consistent. A node whose
//!   distance is zero does not move, and its rings are the node itself - the
//!   triangles that would fold onto it are not written.
//!
//! # How far: around, by the ends, or in the dip frame
//!
//! - [`Reach::Around`] - one distance in every outward direction;
//! - [`Reach::Ends`] - a polyline's `start` and `end`; a surface has neither;
//! - [`Reach::Dip`] - *up*, *down* and *sides*. For a surface, the frame is its
//!   **mean plane** (a robust plane through every node): *up* is its line of
//!   steepest slope, turned up; *sides* its horizontal. For a polyline it is
//!   the vertical: an end pointing up takes *up*, a level end *sides*. Between
//!   the axes the distance is a blend by the squares of the direction's
//!   components - `sides·cos²θ + vertical·sin²θ` - continuous, so there is no
//!   step at the corners (the ellipsoid of LoopStructural, without its
//!   degeneracy when an answer is zero). *Up* and *down* are measured **along
//!   the surface**, or as a **height**: then the height is turned into a length
//!   through the dip of the steepest line - the mean plane's for a surface, the
//!   end's own for a polyline - so ten metres up on a planar fault is ten metres
//!   of elevation at the top edge. On a surface flatter than `flat_below`,
//!   up and down have no meaning: refused, naming the dip measured.
//!
//! # What is refused
//!
//! - a **closed** surface: it has no border;
//! - a surface in **several pieces**: each has its own border - extended one at
//!   a time. Holes are left as they are, and counted;
//! - **fronts that cross**: a concave border whose extension would fold over
//!   itself - an edge of the last ring pointing against the border edge it came
//!   from. Only neighbouring fronts are compared; two arms of a U meeting far
//!   apart are not seen;
//! - a border that folds back on itself at a node (a spike), or turns over so
//!   that its outward direction lies along the mean plane's normal.
//!
//! TODO(reference): verify - Hoppe (1992) and Huber's weights are named without a full citation, and the 1.4826 scale factor of the median absolute residual is unreferenced; add the references (authors, year, title, venue, DOI) once checked.

use std::collections::HashMap;

use crate::outline::boundary_loops;

/// How far to extend.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Reach {
    /// A polyline's two ends, each its own distance. A surface has no ends.
    Ends {
        /// Before the first node.
        start: f64,
        /// After the last node.
        end: f64,
    },
    /// The same distance in every outward direction.
    Around(f64),
    /// Up, down and sides - see the module.
    Dip(Dip),
}

/// The dip-frame reach.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dip {
    /// Up the steepest slope (a surface), or for an end pointing up (a polyline).
    pub up: f64,
    /// Down.
    pub down: f64,
    /// Along the horizontal of the plane, or for a level end.
    pub sides: f64,
    /// `up` and `down` are heights rather than lengths along the surface.
    pub height: bool,
    /// Whether a larger z is higher - the frame says, nothing here guesses.
    pub plus_z_is_up: bool,
    /// Below this dip, in degrees, a surface has no up and no down.
    pub flat_below: f64,
}

/// Which setting a refusal is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    /// The band.
    Band,
    /// A distance.
    Distance,
    /// The frame the distances are read in - the dip frame on a flat surface.
    Toward,
}

/// Why an extension is refused.
#[derive(Debug, Clone, PartialEq)]
pub enum ExtendError {
    /// A setting that cannot be used, named.
    Setting {
        /// Which.
        which: Setting,
        /// Why.
        why: String,
    },
    /// The shape itself.
    Shape(String),
}

impl std::fmt::Display for ExtendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExtendError::Setting { why, .. } | ExtendError::Shape(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for ExtendError {}

fn setting(which: Setting, why: impl Into<String>) -> ExtendError {
    ExtendError::Setting {
        which,
        why: why.into(),
    }
}

/// More nodes than any extension somebody meant to ask for.
pub const MOST_ADDED: usize = 2_000_000;

// ---------------------------------------------------------------- vectors

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn scale(a: [f64; 3], k: f64) -> [f64; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}
fn unit(a: [f64; 3]) -> Option<[f64; 3]> {
    let n = norm(a);
    (n > 0.0 && n.is_finite()).then(|| scale(a, 1.0 / n))
}

// ---------------------------------------------------------------- the robust fit

/// The eigen-decomposition of a symmetric 3 × 3 matrix - cyclic Jacobi. The
/// values ascending, each with its unit vector.
fn eigen(mut a: [[f64; 3]; 3]) -> [(f64, [f64; 3]); 3] {
    let mut v = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..64 {
        let off = a[0][1] * a[0][1] + a[0][2] * a[0][2] + a[1][2] * a[1][2];
        let diagonal = a[0][0] * a[0][0] + a[1][1] * a[1][1] + a[2][2] * a[2][2];
        if off <= 1e-30 * diagonal.max(f64::MIN_POSITIVE) {
            break;
        }
        for (p, q) in [(0, 1), (0, 2), (1, 2)] {
            if a[p][q] == 0.0 {
                continue;
            }
            // Numerical Recipes §11.1: the rotation that zeroes a[p][q].
            let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
            let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
            let c = 1.0 / (t * t + 1.0).sqrt();
            let s = t * c;
            let mut j = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
            j[p][p] = c;
            j[q][q] = c;
            j[p][q] = s;
            j[q][p] = -s;
            // a ← jᵀ a j, v ← v j
            let mut aj = [[0.0; 3]; 3];
            for r in 0..3 {
                for col in 0..3 {
                    aj[r][col] = (0..3).map(|k| a[r][k] * j[k][col]).sum();
                }
            }
            for r in 0..3 {
                for col in 0..3 {
                    a[r][col] = (0..3).map(|k| j[k][r] * aj[k][col]).sum();
                }
            }
            let mut vj = [[0.0; 3]; 3];
            for r in 0..3 {
                for col in 0..3 {
                    vj[r][col] = (0..3).map(|k| v[r][k] * j[k][col]).sum();
                }
            }
            v = vj;
        }
    }
    let mut pairs = [0, 1, 2].map(|k| (a[k][k], [v[0][k], v[1][k], v[2][k]]));
    pairs.sort_by(|x, y| x.0.total_cmp(&y.0));
    pairs
}

/// A line or a plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Line,
    Plane,
}

/// A robust fit: a point on the line or the plane, its direction (a line) or
/// its normal (a plane), and the rank of the points left beyond 2.5 σ.
#[derive(Debug, Clone, PartialEq)]
pub struct Fitted {
    /// The weighted centre.
    pub centre: [f64; 3],
    /// The line's direction, or the plane's normal - a unit vector, its sign
    /// arbitrary.
    pub axis: [f64; 3],
    /// The points the fit weighted down as outliers, as ranks into its input.
    pub outliers: Vec<usize>,
}

/// Principal components reweighted with Huber's weights until they settle.
/// `None` when the points do not determine the fit: all at one place (a
/// line), all on one line (a plane).
fn robust(points: &[[f64; 3]], kind: Kind) -> Option<Fitted> {
    let least = match kind {
        Kind::Line => 2,
        Kind::Plane => 3,
    };
    if points.len() < least || points.iter().any(|p| p.iter().any(|c| !c.is_finite())) {
        return None;
    }
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for p in points {
        for k in 0..3 {
            low[k] = low[k].min(p[k]);
            high[k] = high[k].max(p[k]);
        }
    }
    let extent = norm(sub(high, low));
    if extent <= 0.0 {
        return None;
    }
    // What counts as *on* the fit: a relative hair.
    let tolerance = 1e-9 * extent;
    let mut weights = vec![1.0; points.len()];
    // The centre, the axis, the residuals and σ of the last pass.
    type Pass = ([f64; 3], [f64; 3], Vec<f64>, f64);
    let mut last: Option<Pass> = None;
    for _ in 0..100 {
        let total: f64 = weights.iter().sum();
        if total <= 0.0 {
            return None;
        }
        let mut centre = [0.0; 3];
        for (p, w) in points.iter().zip(&weights) {
            centre = add(centre, scale(*p, *w));
        }
        centre = scale(centre, 1.0 / total);
        let mut covariance = [[0.0; 3]; 3];
        for (p, w) in points.iter().zip(&weights) {
            let d = sub(*p, centre);
            for r in 0..3 {
                for c in 0..3 {
                    covariance[r][c] += w * d[r] * d[c];
                }
            }
        }
        let pairs = eigen(covariance);
        let largest = pairs[2].0;
        let axis = match kind {
            Kind::Line => {
                if largest <= 0.0 {
                    return None;
                }
                pairs[2].1
            }
            Kind::Plane => {
                if pairs[1].0 <= 1e-12 * largest {
                    return None;
                }
                pairs[0].1
            }
        };
        let residuals: Vec<f64> = points
            .iter()
            .map(|p| {
                let d = sub(*p, centre);
                match kind {
                    Kind::Line => norm(sub(d, scale(axis, dot(d, axis)))),
                    Kind::Plane => dot(d, axis).abs(),
                }
            })
            .collect();
        let mut sorted = residuals.clone();
        sorted.sort_by(f64::total_cmp);
        let sigma = 1.4826 * sorted[sorted.len() / 2];
        let next: Vec<f64> = residuals
            .iter()
            .map(|&r| {
                if sigma <= tolerance {
                    // More than half the points are exactly on the fit: the
                    // others are Huber's limit, weight zero.
                    if r <= tolerance { 1.0 } else { 0.0 }
                } else {
                    let k = 1.345 * sigma;
                    if r <= k { 1.0 } else { k / r }
                }
            })
            .collect();
        let settled = next.iter().zip(&weights).all(|(a, b)| (a - b).abs() < 1e-10);
        weights = next;
        last = Some((centre, axis, residuals, sigma));
        if settled {
            break;
        }
    }
    let (centre, axis, residuals, sigma) = last?;
    let beyond = (2.5 * sigma).max(tolerance);
    let outliers = residuals
        .iter()
        .enumerate()
        .filter(|(_, r)| **r > beyond)
        .map(|(k, _)| k)
        .collect();
    Some(Fitted {
        centre,
        axis,
        outliers,
    })
}

/// The robust line through `points` - public for a test to confront.
pub fn fit_line(points: &[[f64; 3]]) -> Option<Fitted> {
    robust(points, Kind::Line)
}

/// The robust plane through `points` - public for a test to confront.
pub fn fit_plane(points: &[[f64; 3]]) -> Option<Fitted> {
    robust(points, Kind::Plane)
}

// ---------------------------------------------------------------- checks

fn check_band(band: f64) -> Result<(), ExtendError> {
    if band.is_finite() && band > 0.0 {
        Ok(())
    } else {
        Err(setting(
            Setting::Band,
            format!("the band must be a finite length > 0, not {band}"),
        ))
    }
}

fn check_distance(what: &str, d: f64) -> Result<(), ExtendError> {
    if d.is_finite() && d >= 0.0 {
        Ok(())
    } else {
        Err(setting(
            Setting::Distance,
            format!("`{what}` must be a finite length ≥ 0, not {d}"),
        ))
    }
}

fn check_reach(reach: &Reach) -> Result<(), ExtendError> {
    match reach {
        Reach::Ends { start, end } => {
            check_distance("start", *start)?;
            check_distance("end", *end)
        }
        Reach::Around(d) => check_distance("distance", *d),
        Reach::Dip(dip) => {
            check_distance("up", dip.up)?;
            check_distance("down", dip.down)?;
            check_distance("sides", dip.sides)?;
            if !(dip.flat_below.is_finite() && (0.0..90.0).contains(&dip.flat_below)) {
                return Err(setting(
                    Setting::Toward,
                    format!(
                        "`flatBelow` is an angle in [0, 90) degrees, not {}",
                        dip.flat_below
                    ),
                ));
            }
            Ok(())
        }
    }
}

/// The blend of the dip frame: `sides` along the horizontal, `vertical` along
/// the steepest line, the squares of the components in between. `sin2` is the
/// square of the component along the steepest line, `rise` the sine of that
/// line's dip - what turns a height into a length.
fn blend(dip: &Dip, upward: bool, sin2: f64, rise: f64) -> f64 {
    let vertical = if upward { dip.up } else { dip.down };
    // A level direction takes none of the vertical answer - and a height over
    // a level line would divide by zero before multiplying by it.
    let steep = if sin2 <= 0.0 {
        0.0
    } else if dip.height {
        vertical * sin2 / rise
    } else {
        vertical * sin2
    };
    dip.sides * (1.0 - sin2) + steep
}

// ---------------------------------------------------------------- polylines

/// One end of a polyline, extended.
#[derive(Debug, Clone, PartialEq)]
pub struct End {
    /// The unit direction the nodes were added along, outward.
    pub direction: [f64; 3],
    /// How far the last added node is from the end node.
    pub distance: f64,
    /// How many nodes were added.
    pub added: usize,
    /// How many nodes of the band were read.
    pub band_nodes: usize,
}

/// A polyline extended: its nodes, the invented ones before and after.
#[derive(Debug, Clone, PartialEq)]
pub struct LineExtension {
    /// The nodes: `before` invented ones, the originals bit for bit, `after`
    /// invented ones.
    pub points: Vec<[f64; 3]>,
    /// Invented nodes before the first original one.
    pub before: usize,
    /// Invented nodes after the last original one.
    pub after: usize,
    /// The start, when it moved.
    pub start: Option<End>,
    /// The end, when it moved.
    pub end: Option<End>,
    /// Nodes of the two bands the fit weighted down as outliers.
    pub outliers: usize,
}

/// An end extended: what it is, the nodes added outward from it, and the
/// band's outliers.
type EndMade = (End, Vec<[f64; 3]>, usize);

/// The distance an end gets, and the direction it goes.
fn line_end(
    tail: &[[f64; 3]],
    reach: &Reach,
    is_start: bool,
    band: f64,
) -> Result<Option<EndMade>, ExtendError> {
    // `tail` runs from the end node inward.
    let mut taken = vec![tail[0]];
    let mut length = 0.0;
    let mut steps = Vec::new();
    for pair in tail.windows(2) {
        let segment = norm(sub(pair[1], pair[0]));
        if taken.len() >= 2 && length + segment > band {
            break;
        }
        length += segment;
        steps.push(segment);
        taken.push(pair[1]);
    }
    let Some(fit) = fit_line(&taken) else {
        return Err(ExtendError::Shape(format!(
            "the {} of the line has no length over its band - its last nodes are one point",
            if is_start { "start" } else { "end" }
        )));
    };
    let mut direction = fit.axis;
    let away = sub(tail[0], fit.centre);
    let along = dot(away, direction);
    if along.abs() <= 1e-12 * norm(away).max(length) {
        return Err(ExtendError::Shape(format!(
            "the band at the {} of the line folds back on itself - no outward direction",
            if is_start { "start" } else { "end" }
        )));
    }
    if along < 0.0 {
        direction = scale(direction, -1.0);
    }
    let distance = match reach {
        Reach::Ends { start, end } => {
            if is_start {
                *start
            } else {
                *end
            }
        }
        Reach::Around(d) => *d,
        Reach::Dip(dip) => {
            let upward_z = if dip.plus_z_is_up {
                direction[2]
            } else {
                -direction[2]
            };
            let sin2 = upward_z * upward_z;
            blend(dip, upward_z > 0.0, sin2, upward_z.abs())
        }
    };
    if distance <= 0.0 {
        return Ok(None);
    }
    let step = (steps.iter().sum::<f64>() / steps.len().max(1) as f64).max(f64::MIN_POSITIVE);
    let count = (distance / step).ceil().max(1.0);
    if count > MOST_ADDED as f64 {
        return Err(setting(
            Setting::Distance,
            format!(
                "{distance} at a step of {step} would add {count} nodes to one end - more than {MOST_ADDED}"
            ),
        ));
    }
    let count = count as usize;
    let added: Vec<[f64; 3]> = (1..=count)
        .map(|k| add(tail[0], scale(direction, distance * k as f64 / count as f64)))
        .collect();
    Ok(Some((
        End {
            direction,
            distance,
            added: count,
            band_nodes: taken.len(),
        },
        added,
        fit.outliers.len(),
    )))
}

/// An open polyline extended at its ends. The original nodes are kept, bit for
/// bit, between the invented ones. A line of fewer than two distinct nodes
/// has no direction and is refused.
pub fn extend_line(line: &[[f64; 3]], reach: &Reach, band: f64) -> Result<LineExtension, ExtendError> {
    check_band(band)?;
    check_reach(reach)?;
    if line.len() < 2 {
        return Err(ExtendError::Shape(format!(
            "a line of {} node(s) has no direction",
            line.len()
        )));
    }
    let reversed: Vec<[f64; 3]> = line.iter().rev().copied().collect();
    let start = line_end(line, reach, true, band)?;
    let end = line_end(&reversed, reach, false, band)?;
    let mut points = Vec::new();
    let mut outliers = 0usize;
    let before = start.as_ref().map_or(0, |(e, _, _)| e.added);
    if let Some((_, added, out)) = &start {
        points.extend(added.iter().rev());
        outliers += out;
    }
    points.extend_from_slice(line);
    let after = end.as_ref().map_or(0, |(e, _, _)| e.added);
    if let Some((_, added, out)) = &end {
        points.extend(added.iter());
        outliers += out;
    }
    Ok(LineExtension {
        points,
        before,
        after,
        start: start.map(|s| s.0),
        end: end.map(|e| e.0),
        outliers,
    })
}

// ---------------------------------------------------------------- surfaces

/// A border node moved.
#[derive(Debug, Clone, PartialEq)]
pub struct Outward {
    /// The node, a rank into the input.
    pub node: u32,
    /// Its unit outward direction, in the plane fitted to its band.
    pub direction: [f64; 3],
    /// How far its last ring went.
    pub distance: f64,
}

/// A surface extended.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceExtension {
    /// The input nodes bit for bit, then the invented ones.
    pub points: Vec<[f64; 3]>,
    /// The input triangles as they were, then the new ones.
    pub triangles: Vec<[u32; 3]>,
    /// Where the invented nodes start.
    pub original_nodes: usize,
    /// Where the new triangles start.
    pub original_triangles: usize,
    /// How many rings were laid.
    pub rings: usize,
    /// Each node of the outer border, in its order.
    pub outward: Vec<Outward>,
    /// Border loops left as they are - holes.
    pub holes: usize,
    /// Nodes that some band's fit weighted down as outliers, each once.
    pub outliers: usize,
    /// The dip of the mean plane, in degrees.
    pub dip: f64,
}

fn inside(polygon: &[[f64; 2]], p: [f64; 2]) -> bool {
    let mut odd = false;
    let n = polygon.len();
    for k in 0..n {
        let a = polygon[k];
        let b = polygon[(k + 1) % n];
        if (a[1] > p[1]) != (b[1] > p[1]) {
            let x = a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
            if p[0] < x {
                odd = !odd;
            }
        }
    }
    odd
}

/// A triangulated surface extended past its outer border. The input nodes and
/// triangles are kept as they are; only rings of nodes and bands of triangles
/// are added.
pub fn extend_surface(
    points: &[[f64; 3]],
    triangles: &[[u32; 3]],
    reach: &Reach,
    band: f64,
) -> Result<SurfaceExtension, ExtendError> {
    check_band(band)?;
    check_reach(reach)?;
    if let Reach::Ends { .. } = reach {
        return Err(setting(
            Setting::Toward,
            "a surface has no start and no end - extend it `around`, or by `dip`",
        ));
    }
    let n = points.len();
    if let Some(bad) = triangles.iter().flatten().find(|&&i| i as usize >= n) {
        return Err(ExtendError::Shape(format!(
            "a triangle names node {bad} where the surface has {n}"
        )));
    }
    let loops = boundary_loops(triangles).map_err(|e| ExtendError::Shape(e.to_string()))?;
    if loops.is_empty() {
        return Err(ExtendError::Shape(
            "the surface is closed - it has no border to extend".to_string(),
        ));
    }

    // The mean plane, over every node a triangle names.
    let mut used = vec![false; n];
    for i in triangles.iter().flatten() {
        used[*i as usize] = true;
    }
    let named: Vec<usize> = (0..n).filter(|i| used[*i]).collect();
    let cloud: Vec<[f64; 3]> = named.iter().map(|i| points[*i]).collect();
    let mean = fit_plane(&cloud).ok_or_else(|| {
        ExtendError::Shape("the surface's nodes are on one line - it has no plane".to_string())
    })?;
    let normal = mean.axis;
    let dip = normal[2].abs().min(1.0).acos().to_degrees();

    // The frame: along the horizontal, and up the steepest line.
    let (side, up_dip) = match reach {
        Reach::Dip(d) => {
            if dip < d.flat_below.max(1e-9) {
                return Err(setting(
                    Setting::Toward,
                    format!(
                        "the surface's mean plane dips {dip:.3}° - under {}°, up and down have no meaning; extend it `around`",
                        d.flat_below
                    ),
                ));
            }
            let side = unit(cross([0.0, 0.0, 1.0], normal)).expect("a dipping plane has a horizontal");
            let mut up = cross(normal, side);
            let upward = if d.plus_z_is_up { up[2] } else { -up[2] };
            if upward < 0.0 {
                up = scale(up, -1.0);
            }
            (side, up)
        }
        _ => {
            let helper = if normal[0].abs() < 0.9 {
                [1.0, 0.0, 0.0]
            } else {
                [0.0, 1.0, 0.0]
            };
            let side = unit(cross(helper, normal)).expect("a unit normal");
            (side, cross(normal, side))
        }
    };

    // The outer loop: the one no other contains, in the mean plane.
    let ring: Vec<u32> = if loops.len() == 1 {
        loops[0].clone()
    } else {
        let flat = |i: u32| {
            let d = sub(points[i as usize], mean.centre);
            [dot(d, side), dot(d, up_dip)]
        };
        let polygons: Vec<Vec<[f64; 2]>> = loops
            .iter()
            .map(|l| l.iter().map(|i| flat(*i)).collect())
            .collect();
        let outer: Vec<usize> = (0..loops.len())
            .filter(|a| !(0..loops.len()).any(|b| b != *a && inside(&polygons[b], polygons[*a][0])))
            .collect();
        if outer.len() != 1 {
            return Err(ExtendError::Shape(format!(
                "the surface is in {} pieces, each with its own border - extend them one at a time",
                outer.len().max(2)
            )));
        }
        loops[outer[0]].clone()
    };
    let holes = loops.len() - 1;

    // Each edge as its triangle states it, and the node across from it.
    let mut edges: HashMap<(u32, u32), ([u32; 2], u32)> = HashMap::new();
    for t in triangles {
        for k in 0..3 {
            let (a, b, c) = (t[k], t[(k + 1) % 3], t[(k + 2) % 3]);
            edges.insert((a.min(b), a.max(b)), ([a, b], c));
        }
    }
    let edge = |a: u32, b: u32| edges[&(a.min(b), a.max(b))];

    let count = ring.len();
    let at = |i: u32| points[i as usize];
    let mut outward = Vec::with_capacity(count);
    let mut outlier = vec![false; n];
    for k in 0..count {
        let i = ring[k];
        let prev = ring[(k + count - 1) % count];
        let next = ring[(k + 1) % count];
        let (_, across_prev) = edge(prev, i);
        let (_, across_next) = edge(i, next);
        let p = at(i);
        // The band: the nodes within `band` of this one, and always its
        // neighbours on the border and across, so three are never missing.
        let must = [i, prev, next, across_prev, across_next];
        let band2 = band * band;
        let chosen: Vec<usize> = named
            .iter()
            .copied()
            .filter(|j| must.contains(&(*j as u32)) || dot(sub(points[*j], p), sub(points[*j], p)) <= band2)
            .collect();
        let local: Vec<[f64; 3]> = chosen.iter().map(|j| points[*j]).collect();
        let fit = fit_plane(&local).ok_or_else(|| {
            ExtendError::Shape(format!("near border node {i}, the band's nodes are on one line - no plane to extend along; widen `band`"))
        })?;
        for r in &fit.outliers {
            outlier[chosen[*r]] = true;
        }
        let m = fit.axis;
        let bisector = add(
            unit(sub(p, at(prev))).unwrap_or([0.0; 3]),
            unit(sub(at(next), p)).unwrap_or([0.0; 3]),
        );
        let tangent = sub(bisector, scale(m, dot(bisector, m)));
        let Some(tangent) = unit(tangent).filter(|_| norm(tangent) > 1e-9) else {
            return Err(ExtendError::Shape(format!(
                "the border folds back on itself at node {i} - it has no outward direction there"
            )));
        };
        let mut direction = unit(cross(tangent, m)).expect("two unit vectors at right angles");
        let inward = sub(scale(add(at(across_prev), at(across_next)), 0.5), p);
        if dot(direction, inward) > 0.0 {
            direction = scale(direction, -1.0);
        }
        let distance = match reach {
            Reach::Around(d) => *d,
            Reach::Dip(d) => {
                let a = dot(direction, up_dip);
                let b = dot(direction, side);
                let h2 = a * a + b * b;
                if h2 < 1e-12 {
                    return Err(ExtendError::Shape(format!(
                        "at border node {i} the surface turns over: its outward direction lies along the mean plane's normal, where up, down and sides say nothing"
                    )));
                }
                let rise = up_dip[2].abs();
                blend(d, a > 0.0, a * a / h2, rise)
            }
            Reach::Ends { .. } => unreachable!("refused above"),
        };
        outward.push(Outward {
            node: i,
            direction,
            distance,
        });
    }

    let longest = outward.iter().map(|o| o.distance).fold(0.0, f64::max);
    if longest <= 0.0 {
        return Err(setting(
            Setting::Distance,
            "every distance is zero - there is nothing to extend",
        ));
    }
    let perimeter: f64 = (0..count)
        .map(|k| norm(sub(at(ring[(k + 1) % count]), at(ring[k]))))
        .sum();
    let step = (perimeter / count as f64).max(f64::MIN_POSITIVE);
    let rings = (longest / step).ceil().max(1.0);
    let moving = outward.iter().filter(|o| o.distance > 0.0).count();
    if rings * moving as f64 > MOST_ADDED as f64 {
        return Err(setting(
            Setting::Distance,
            format!(
                "{longest} at the border's mean edge of {step} is {rings} rings of {moving} nodes - more than {MOST_ADDED}"
            ),
        ));
    }
    let rings = rings as usize;

    // Fronts that cross: an edge of the last ring against its border edge.
    let last = |o: &Outward| add(at(o.node), scale(o.direction, o.distance));
    for k in 0..count {
        let (a, b) = (&outward[k], &outward[(k + 1) % count]);
        let before = sub(at(b.node), at(a.node));
        let after = sub(last(b), last(a));
        if dot(before, after) <= 0.0 {
            return Err(ExtendError::Shape(format!(
                "the fronts from border nodes {} and {} cross - the border is too concave there for that distance; shorten it",
                a.node, b.node
            )));
        }
    }

    let mut out = points.to_vec();
    let mut index: Vec<Vec<u32>> = vec![ring.clone()];
    for k in 1..=rings {
        let row = outward
            .iter()
            .map(|o| {
                if o.distance <= 0.0 {
                    o.node
                } else {
                    out.push(add(
                        at(o.node),
                        scale(o.direction, o.distance * k as f64 / rings as f64),
                    ));
                    (out.len() - 1) as u32
                }
            })
            .collect();
        index.push(row);
    }
    let mut made = triangles.to_vec();
    for pos in 0..count {
        let next = (pos + 1) % count;
        let ([a, _], _) = edge(ring[pos], ring[next]);
        let forward = a == ring[pos];
        for k in 1..=rings {
            let (i0, j0, i1, j1) = (
                index[k - 1][pos],
                index[k - 1][next],
                index[k][pos],
                index[k][next],
            );
            let pair = if forward {
                [[j0, i0, i1], [j0, i1, j1]]
            } else {
                [[i0, j0, j1], [i0, j1, i1]]
            };
            for t in pair {
                if t[0] != t[1] && t[1] != t[2] && t[0] != t[2] {
                    made.push(t);
                }
            }
        }
    }
    Ok(SurfaceExtension {
        points: out,
        triangles: made,
        original_nodes: n,
        original_triangles: triangles.len(),
        rings,
        outward,
        holes,
        outliers: outlier.iter().filter(|o| **o).count(),
        dip,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    /// A grid of `side × side` nodes on the plane `z = slope · y`, two
    /// triangles per cell.
    fn tilted(side: usize, slope: f64) -> (Vec<[f64; 3]>, Vec<[u32; 3]>) {
        let mut points = Vec::new();
        for j in 0..side {
            for i in 0..side {
                points.push([i as f64, j as f64, slope * j as f64]);
            }
        }
        let mut triangles = Vec::new();
        for j in 0..side - 1 {
            for i in 0..side - 1 {
                let a = (j * side + i) as u32;
                let (b, c, d) = (a + 1, a + side as u32 + 1, a + side as u32);
                triangles.push([a, b, c]);
                triangles.push([a, c, d]);
            }
        }
        (points, triangles)
    }

    #[test]
    fn the_eigenvectors_of_a_known_matrix() {
        // Diagonal in a turned frame: values 1, 2, 5.
        let pairs = eigen([[2.0, 0.0, 0.0], [0.0, 3.0, 2.0], [0.0, 2.0, 3.0]]);
        assert!(
            close(pairs[0].0, 1.0) && close(pairs[1].0, 2.0) && close(pairs[2].0, 5.0),
            "{pairs:?}"
        );
        let v = pairs[2].1;
        assert!(
            close(v[1].abs(), 0.5f64.sqrt()) && close(v[2].abs(), 0.5f64.sqrt()) && close(v[0], 0.0),
            "{v:?}"
        );
    }

    /// The robust fit ignores a misplaced node, and says so: the least-squares
    /// plane would tilt toward it.
    #[test]
    fn a_misplaced_node_is_weighted_down_and_counted() {
        let mut points: Vec<[f64; 3]> = (0..5)
            .flat_map(|j| (0..5).map(move |i| [i as f64, j as f64, 0.0]))
            .collect();
        points[24][2] = 3.0;
        let fit = fit_plane(&points).unwrap();
        assert!(
            close(fit.axis[2].abs(), 1.0),
            "the plane is z = 0 still: {:?}",
            fit.axis
        );
        assert_eq!(fit.outliers, vec![24]);
    }

    /// A straight line prolonged stays straight, each end by its own distance,
    /// at the band's step, and the originals are untouched.
    #[test]
    fn a_straight_line_goes_on_straight_by_the_distance_asked() {
        let line: Vec<[f64; 3]> = (0..5).map(|k| [2.0 * k as f64, k as f64, -(k as f64)]).collect();
        let made = extend_line(&line, &Reach::Ends { start: 3.0, end: 7.0 }, 100.0).unwrap();
        assert_eq!(&made.points[made.before..made.before + 5], line.as_slice());
        let d = unit([2.0, 1.0, -1.0]).unwrap();
        let first = made.points[0];
        let tip = *made.points.last().unwrap();
        let along = |p: [f64; 3], from: [f64; 3]| dot(sub(p, from), d);
        assert!(
            close(along(tip, line[4]), 7.0) && close(norm(sub(tip, line[4])), 7.0),
            "{tip:?}"
        );
        assert!(
            close(along(first, line[0]), -3.0) && close(norm(sub(first, line[0])), 3.0),
            "{first:?}"
        );
        // The step is the band's mean segment, √6: 3 → 2 nodes, 7 → 3.
        assert_eq!((made.before, made.after), (2, 3));
    }

    /// The band is what keeps a misplaced end from steering: with the last node
    /// off the line, the robust direction stays on it; the last edge would not.
    #[test]
    fn a_crooked_last_node_does_not_steer_the_extension() {
        let mut line: Vec<[f64; 3]> = (0..8).map(|k| [k as f64, 0.0, 0.0]).collect();
        line[7][1] = 0.4;
        let made = extend_line(&line, &Reach::Ends { start: 0.0, end: 5.0 }, 10.0).unwrap();
        let direction = made.end.unwrap().direction;
        let last_edge = unit(sub(line[7], line[6])).unwrap();
        assert!(
            direction[1].abs() < 1e-9,
            "the robust direction stays along x: {direction:?}"
        );
        assert!(
            last_edge[1] > 0.3,
            "and the last edge would have turned by: {last_edge:?}"
        );
        assert_eq!(made.outliers, 1);
    }

    /// A planar surface extended around stays **exactly** in its plane, each
    /// border node at the distance asked, and keeps every node and triangle.
    #[test]
    fn a_plane_extended_around_stays_in_its_plane() {
        let (points, triangles) = tilted(6, 0.5);
        let made = extend_surface(&points, &triangles, &Reach::Around(2.0), 1.5).unwrap();
        assert_eq!(&made.points[..points.len()], points.as_slice());
        assert_eq!(&made.triangles[..triangles.len()], triangles.as_slice());
        for p in &made.points[points.len()..] {
            assert!(close(p[2], 0.5 * p[1]), "off the plane: {p:?}");
        }
        for o in &made.outward {
            assert!(close(o.distance, 2.0));
        }
        // The border runs 20 nodes, edges of 1 · √1.25 on two sides: two rings.
        assert_eq!(made.rings, 2);
        assert_eq!(made.points.len(), points.len() + 2 * 20);
    }

    /// Up is up the dip, as a height: the top edge rises by exactly the height.
    #[test]
    fn up_as_a_height_rises_by_that_height_on_a_plane() {
        let (points, triangles) = tilted(5, 1.0);
        let dip = Dip {
            up: 10.0,
            down: 0.0,
            sides: 0.0,
            height: true,
            plus_z_is_up: true,
            flat_below: 5.0,
        };
        let made = extend_surface(&points, &triangles, &Reach::Dip(dip), 1.5).unwrap();
        assert!(close(made.dip, 45.0));
        // Node 22 is on the top edge, away from the corners.
        let top = made.outward.iter().find(|o| o.node == 22).unwrap();
        assert!(close(top.distance * top.direction[2], 10.0), "{top:?}");
        let bottom = made.outward.iter().find(|o| o.node == 2).unwrap();
        assert!(close(bottom.distance, 0.0), "down is zero: {bottom:?}");
    }

    #[test]
    fn a_flat_surface_has_no_up_and_says_its_dip() {
        let (points, triangles) = tilted(4, 0.0);
        let dip = Dip {
            up: 1.0,
            down: 1.0,
            sides: 1.0,
            height: false,
            plus_z_is_up: true,
            flat_below: 5.0,
        };
        let why = extend_surface(&points, &triangles, &Reach::Dip(dip), 1.0).unwrap_err();
        assert!(
            matches!(
                why,
                ExtendError::Setting {
                    which: Setting::Toward,
                    ..
                }
            ),
            "{why}"
        );
        assert!(why.to_string().contains("dips 0.000°"), "{why}");
    }

    #[test]
    fn a_concave_notch_whose_fronts_cross_is_refused() {
        // A U: two arms one unit apart, extended by five.
        let points = vec![
            [0.0, 0.0, 0.0],
            [3.0, 0.0, 0.0],
            [3.0, 3.0, 0.0],
            [2.0, 3.0, 0.0],
            [2.0, 1.0, 0.0],
            [1.0, 1.0, 0.0],
            [1.0, 3.0, 0.0],
            [0.0, 3.0, 0.0],
        ];
        let triangles = vec![[0, 1, 4], [1, 2, 3], [1, 3, 4], [0, 4, 5], [0, 5, 6], [0, 6, 7]];
        let why = extend_surface(&points, &triangles, &Reach::Around(5.0), 1.0).unwrap_err();
        assert!(why.to_string().contains("cross"), "{why}");
    }
}
