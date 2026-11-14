//! Three-dimensional vectors on [`Point`]: the one place the cross product
//! and the normalisation are written.
//!
//! The handedness is the one of the placed frames: right-handed, `x × y = z`,
//! z up.

use crate::points::Point;

/// `a + b`.
pub fn add(a: Point, b: Point) -> Point {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// `a - b`.
pub fn sub(a: Point, b: Point) -> Point {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// `k a`.
pub fn scale(a: Point, k: f64) -> Point {
    [a[0] * k, a[1] * k, a[2] * k]
}

/// `a . b`.
pub fn dot(a: Point, b: Point) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// `a × b`, right-handed: `x × y = z`.
pub fn cross(a: Point, b: Point) -> Point {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// `|a|`.
pub fn norm(a: Point) -> f64 {
    dot(a, a).sqrt()
}

/// `a / |a|`, or `None` when `|a|` is not finite or not above `floor`: a
/// direction that does not exist is not invented. The floor is the caller's,
/// because what "too short to have a direction" means depends on the scale
/// of what is measured.
pub fn unit_above(a: Point, floor: f64) -> Option<Point> {
    let length = norm(a);
    (length.is_finite() && length > floor).then(|| scale(a, 1.0 / length))
}

/// `unit_above(a, f64::EPSILON)`.
pub fn unit(a: Point) -> Option<Point> {
    unit_above(a, f64::EPSILON)
}
