//! Vectors on `[f64; 3]` and, in [`plan`], on `[f64; 2]`: the one place the
//! cross product, the normalisation, the distance of a point to a segment and
//! the intersection of two segments are written.
//!
//! The handedness is the one of the placed frames: right-handed, `x × y = z`,
//! z up. A 3D point is a plain `[f64; 3]`, which is also what
//! `energyml_geometry::points::Point` is; `energyml_geometry::vector`
//! re-exports this module, so a caller that depends on either crate reaches
//! the same functions.
//!
//! This module is in `energyml-geostat` because that crate depends on no
//! EnergyML crate and must stay so, while every crate that needs vector
//! arithmetic can depend on it: it is pure Rust, with no native dependency, and
//! compiles to wasm.

/// `a + b`.
pub fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// `a - b`.
pub fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// `k a`.
pub fn scale(a: [f64; 3], k: f64) -> [f64; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}

/// `a . b`.
pub fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// `a × b`, right-handed: `x × y = z`.
pub fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// `|a|`.
pub fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

/// `|b - a|`: the distance between two points.
pub fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    norm(sub(b, a))
}

/// `a / |a|`, or `None` when `|a|` is not finite or not above `floor`: a
/// direction that does not exist is not invented. The floor is the caller's,
/// because what "too short to have a direction" means depends on the scale
/// of what is measured.
pub fn unit_above(a: [f64; 3], floor: f64) -> Option<[f64; 3]> {
    let length = norm(a);
    (length.is_finite() && length > floor).then(|| scale(a, 1.0 / length))
}

/// `unit_above(a, f64::EPSILON)`.
pub fn unit(a: [f64; 3]) -> Option<[f64; 3]> {
    unit_above(a, f64::EPSILON)
}

/// The distance from `b` to the line through `a` and `c`, in 3D. Coincident
/// `a` and `c` (or a chord whose length is not finite) make the chord a point;
/// the distance is then to `a`.
pub fn distance_to_chord(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> f64 {
    let ab = sub(b, a);
    let ac = sub(c, a);
    let ac_len = norm(ac);
    if ac_len == 0.0 || !ac_len.is_finite() {
        return norm(ab);
    }
    norm(cross(ab, ac)) / ac_len
}

/// The same operations in the plan, on `[f64; 2]`.
pub mod plan {
    /// `a + b`.
    pub fn add(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
        [a[0] + b[0], a[1] + b[1]]
    }

    /// `a - b`.
    pub fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
        [a[0] - b[0], a[1] - b[1]]
    }

    /// `k a`.
    pub fn scale(a: [f64; 2], k: f64) -> [f64; 2] {
        [a[0] * k, a[1] * k]
    }

    /// `a . b`.
    pub fn dot(a: [f64; 2], b: [f64; 2]) -> f64 {
        a[0] * b[0] + a[1] * b[1]
    }

    /// The z component of `a × b`: twice the signed area of the triangle
    /// `0 a b`, positive when `b` is anticlockwise from `a`.
    pub fn cross(a: [f64; 2], b: [f64; 2]) -> f64 {
        a[0] * b[1] - a[1] * b[0]
    }

    /// `|a|`, by `hypot`.
    pub fn norm(a: [f64; 2]) -> f64 {
        a[0].hypot(a[1])
    }

    /// `|a - b|`, by `hypot`: the distance between two plan positions.
    pub fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
        (a[0] - b[0]).hypot(a[1] - b[1])
    }

    /// The distance from `p` to the segment `a b`, and the parameter in
    /// `[0, 1]` of its nearest point (`0` when the segment has no length).
    pub fn segment_distance(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> (f64, f64) {
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

    /// The distance from `p` to the segment `a b`.
    pub fn distance_to_segment(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
        segment_distance(p, a, b).0
    }

    /// Twice the signed area of the triangle `a b c`: > 0 anticlockwise, < 0
    /// clockwise, 0 collinear.
    pub fn orient(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    }

    /// Whether `c` lies on the closed segment `a`-`b`, `c` being collinear with
    /// it.
    pub fn on_segment(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> bool {
        c[0] >= a[0].min(b[0]) && c[0] <= a[0].max(b[0]) && c[1] >= a[1].min(b[1]) && c[1] <= a[1].max(b[1])
    }

    /// Whether the closed segments `a`-`b` and `c`-`d` share at least one point.
    ///
    /// The textbook orientation test, exact (no tolerance), endpoints included:
    /// a proper crossing has `c` and `d` on opposite sides of `ab` **and** `a`,
    /// `b` on opposite sides of `cd`; the collinear cases are settled by a
    /// bounding check.
    pub fn segments_meet(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
        let o1 = orient(a, b, c);
        let o2 = orient(a, b, d);
        let o3 = orient(c, d, a);
        let o4 = orient(c, d, b);
        if ((o1 > 0.0 && o2 < 0.0) || (o1 < 0.0 && o2 > 0.0))
            && ((o3 > 0.0 && o4 < 0.0) || (o3 < 0.0 && o4 > 0.0))
        {
            return true;
        }
        (o1 == 0.0 && on_segment(a, b, c))
            || (o2 == 0.0 && on_segment(a, b, d))
            || (o3 == 0.0 && on_segment(c, d, a))
            || (o4 == 0.0 && on_segment(c, d, b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cross_is_right_handed() {
        assert_eq!(cross([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]), [0.0, 0.0, 1.0]);
    }

    #[test]
    fn distance_is_symmetric_and_three_four_five() {
        assert_eq!(distance([0.0; 3], [3.0, 4.0, 0.0]), 5.0);
        assert_eq!(distance([3.0, 4.0, 0.0], [0.0; 3]), 5.0);
    }

    #[test]
    fn a_chord_of_no_length_measures_to_its_point() {
        assert_eq!(distance_to_chord([0.0; 3], [3.0, 4.0, 0.0], [0.0; 3]), 5.0);
        assert_eq!(distance_to_chord([0.0; 3], [1.0, 2.0, 0.0], [2.0, 0.0, 0.0]), 2.0);
    }

    #[test]
    fn a_unit_vector_of_nothing_is_nothing() {
        assert_eq!(unit([0.0; 3]), None);
        assert_eq!(unit_above([1e-13, 0.0, 0.0], 1e-12), None);
        assert_eq!(unit_above([0.0, 2.0, 0.0], 0.0), Some([0.0, 1.0, 0.0]));
    }

    #[test]
    fn the_plan_cross_is_positive_anticlockwise() {
        assert_eq!(plan::cross([1.0, 0.0], [0.0, 1.0]), 1.0);
        assert_eq!(plan::cross([0.0, 1.0], [1.0, 0.0]), -1.0);
    }

    #[test]
    fn the_nearest_point_of_a_segment_is_clamped_to_its_ends() {
        let (a, b) = ([0.0, 0.0], [10.0, 0.0]);
        assert_eq!(plan::segment_distance([5.0, 3.0], a, b), (3.0, 0.5));
        assert_eq!(plan::segment_distance([-4.0, 3.0], a, b), (5.0, 0.0));
        assert_eq!(plan::segment_distance([14.0, 3.0], a, b), (5.0, 1.0));
        // A segment of no length is its point.
        assert_eq!(plan::segment_distance([3.0, 4.0], a, a), (5.0, 0.0));
    }

    #[test]
    fn segments_meet_on_a_crossing_a_touch_and_a_collinear_overlap_but_not_apart() {
        let m = plan::segments_meet;
        assert!(m([0.0, 0.0], [2.0, 2.0], [0.0, 2.0], [2.0, 0.0]));
        assert!(m([0.0, 0.0], [2.0, 0.0], [2.0, 0.0], [2.0, 2.0]));
        assert!(m([0.0, 0.0], [2.0, 0.0], [1.0, 0.0], [3.0, 0.0]));
        assert!(!m([0.0, 0.0], [1.0, 0.0], [2.0, 0.0], [3.0, 0.0]));
        assert!(!m([0.0, 0.0], [1.0, 1.0], [2.0, 0.0], [3.0, 1.0]));
        // Break it: make `on_segment` return true unconditionally and the
        // collinear case apart fails.
    }
}
