//! Fake, dated data for tests: a vertical polyline descending from each point
//! of a cloud.
//!
//! The need: from a point cloud, create many vertical polylines with either a
//! fixed number of cuts or a random number between two bounds, either an exact
//! z or a z between two bounds for the end (each point being the head of a
//! well), and cuts that are either regular or fluctuating.
//!
//! # A length, not an absolute z
//!
//! The request is for "the z of the end" — a wish that reads naturally as an absolute
//! coordinate, but a fixed z would put the free end above or below the head
//! depending on where in the frame the cloud sits, which is not what a
//! synthetic wellhead means. What is asked here instead is a **length**: how
//! far the polyline descends from its head, the same for every point of the
//! cloud. Which way "descends" is is not a convention of this module — it is
//! read from the CRS (`up`), exactly as the requirement says: the cloud's CRS decides, not a convention
//! of the operation.
//!
//! # Deterministic from a seed
//!
//! Chance has a seed, and the seed is an input: the same seed rebuilds the same
//! polylines. No source of randomness lives in this crate on
//! its own, and none is reached for here either: [`SplitMix64`] is a fixed,
//! public-domain generator, seeded once per point from the run's seed and the
//! point's rank, so a cloud read in a different order still gives every point
//! the same draw it always had, and adding a point never reshuffles the
//! others'.
//!
//! TODO(reference): verify — SplitMix64 is described as a public-domain generator without a citation; add the original reference once checked.

use crate::Error;

/// A tiny, deterministic generator — [SplitMix64](https://prng.di.unimi.it/splitmix64.c),
/// public domain, chosen because it needs no crate of its own (this one stays
/// dependency-free and wasm-compatible) and is exactly the generator its name
/// promises: splitting one seed into as many independent streams as there are
/// points, in `O(1)` per point rather than by walking a single stream.
struct SplitMix64(u64);

impl SplitMix64 {
    /// One independent stream per point: the run's seed, mixed with the
    /// point's rank rather than walked to — a cloud of a million points does
    /// not cost a million steps to reach the last one.
    fn for_point(seed: u64, index: usize) -> Self {
        let mixed = (index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        SplitMix64(seed ^ mixed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `[0, 1)`, 53 bits of it.
    fn next_f64(&mut self) -> f64 {
        ((self.next_u64() >> 11) as f64) * (1.0 / (1u64 << 53) as f64)
    }

    /// Uniform integer in `[low, high]`, inclusive.
    fn next_range_u32(&mut self, low: u32, high: u32) -> u32 {
        if high <= low {
            return low;
        }
        let span = u64::from(high - low) + 1;
        low + (self.next_u64() % span) as u32
    }
}

/// How many cuts a polyline is divided into: `cuts + 1` nodes, the head
/// included.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Cuts {
    /// Exactly this many, every polyline.
    Fixed(u32),
    /// Uniform between two bounds, inclusive — drawn once per point.
    Random { min: u32, max: u32 },
}

impl Cuts {
    fn validate(&self) -> Result<(), Error> {
        match *self {
            Cuts::Fixed(count) if count >= 1 => Ok(()),
            Cuts::Fixed(count) => Err(Error::BadCuts { given: f64::from(count) }),
            Cuts::Random { min, max } if min >= 1 && max >= min => Ok(()),
            Cuts::Random { min, max } => Err(Error::BadCutsRange { min: f64::from(min), max: f64::from(max) }),
        }
    }

    fn draw(&self, rng: &mut SplitMix64) -> u32 {
        match *self {
            Cuts::Fixed(count) => count,
            Cuts::Random { min, max } => rng.next_range_u32(min, max),
        }
    }
}

/// How long a polyline is, from its head to its free end.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Length {
    /// Exactly this long, every polyline.
    Fixed(f64),
    /// Uniform between two bounds — drawn once per point.
    Random { min: f64, max: f64 },
}

impl Length {
    fn validate(&self) -> Result<(), Error> {
        match *self {
            Length::Fixed(length) if length.is_finite() && length >= 0.0 => Ok(()),
            Length::Fixed(length) => Err(Error::BadLength { given: length }),
            Length::Random { min, max } if min.is_finite() && max.is_finite() && min >= 0.0 && max >= min => Ok(()),
            Length::Random { min, max } => Err(Error::BadLengthRange { min, max }),
        }
    }

    fn draw(&self, rng: &mut SplitMix64) -> f64 {
        match *self {
            Length::Fixed(length) => length,
            Length::Random { min, max } => min + rng.next_f64() * (max - min),
        }
    }
}

/// How the cuts are placed along the length.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Spacing {
    /// Evenly: every segment the same length.
    Regular,
    /// Evenly, jittered by up to `fraction` of the even segment length.
    Fluctuating { fraction: f64 },
}

impl Spacing {
    fn validate(&self) -> Result<(), Error> {
        match *self {
            Spacing::Regular => Ok(()),
            Spacing::Fluctuating { fraction } if (0.0..=1.0).contains(&fraction) => Ok(()),
            Spacing::Fluctuating { fraction } => Err(Error::BadFluctuation { given: fraction }),
        }
    }

    /// One relative weight per segment (`cuts` of them), summing to `cuts` on
    /// average — never zero, so two nodes never land on the same point.
    fn weights(&self, cuts: u32, rng: &mut SplitMix64) -> Vec<f64> {
        match *self {
            Spacing::Regular => vec![1.0; cuts as usize],
            Spacing::Fluctuating { fraction } => (0..cuts)
                .map(|_| (1.0 + (rng.next_f64() * 2.0 - 1.0) * fraction).max(1e-6))
                .collect(),
        }
    }
}

/// One polyline made up.
#[derive(Debug, Clone)]
pub struct Polyline {
    /// Its nodes, the head first.
    pub points: Vec<[f64; 3]>,
    /// How many cuts it was drawn with.
    pub cuts: u32,
    /// How long it is.
    pub length: f64,
}

/// A vertical polyline from each point of `heads`.
///
/// `up` is the frame's vertical sense (`true`: `z` increases upward, and a
/// polyline descends by subtracting its length; `false`: `z` increases
/// downward, and it adds). `seed` makes every draw reproducible: the same
/// seed, the same cloud, the same polylines.
pub fn vertical_polylines(
    heads: &[[f64; 3]],
    cuts: &Cuts,
    length: &Length,
    spacing: &Spacing,
    up: bool,
    seed: u64,
) -> Result<Vec<Polyline>, Error> {
    cuts.validate()?;
    length.validate()?;
    spacing.validate()?;
    if heads.is_empty() {
        return Err(Error::NoHeads);
    }
    let mut out = Vec::with_capacity(heads.len());
    for (index, head) in heads.iter().enumerate() {
        let mut rng = SplitMix64::for_point(seed, index);
        let n = cuts.draw(&mut rng);
        let total = length.draw(&mut rng);
        let weights = spacing.weights(n, &mut rng);
        let sum: f64 = weights.iter().sum();
        let mut points = Vec::with_capacity(n as usize + 1);
        points.push(*head);
        let mut travelled = 0.0;
        for weight in &weights {
            travelled += weight / sum * total;
            let z = if up { head[2] - travelled } else { head[2] + travelled };
            points.push([head[0], head[1], z]);
        }
        out.push(Polyline { points, cuts: n, length: total });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn end_lands_at_the_requested_length_in_the_up_direction() {
        let heads = [[0.0, 0.0, 100.0]];
        let made = vertical_polylines(&heads, &Cuts::Fixed(3), &Length::Fixed(30.0), &Spacing::Regular, true, 1).unwrap();
        assert_eq!(made[0].points.len(), 4);
        assert_eq!(made[0].points[0], [0.0, 0.0, 100.0]);
        let last = *made[0].points.last().unwrap();
        assert!((last[2] - 70.0).abs() < 1e-9, "{last:?}");
        // Regular spacing: every segment 10 long.
        for pair in made[0].points.windows(2) {
            assert!((pair[0][2] - pair[1][2] - 10.0).abs() < 1e-9);
        }
    }

    #[test]
    fn direction_depends_on_the_crs_not_on_a_convention() {
        let heads = [[0.0, 0.0, 100.0]];
        let down = vertical_polylines(&heads, &Cuts::Fixed(1), &Length::Fixed(10.0), &Spacing::Regular, false, 1).unwrap();
        assert!((down[0].points[1][2] - 110.0).abs() < 1e-9);
    }

    #[test]
    fn a_seed_reproduces_the_same_draw() {
        let heads = [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [20.0, 0.0, 0.0]];
        let cuts = Cuts::Random { min: 2, max: 8 };
        let length = Length::Random { min: 5.0, max: 50.0 };
        let spacing = Spacing::Fluctuating { fraction: 0.3 };
        let a = vertical_polylines(&heads, &cuts, &length, &spacing, true, 42).unwrap();
        let b = vertical_polylines(&heads, &cuts, &length, &spacing, true, 42).unwrap();
        for (one, other) in a.iter().zip(&b) {
            assert_eq!(one.cuts, other.cuts);
            assert_eq!(one.points.len(), other.points.len());
            for (p, q) in one.points.iter().zip(&other.points) {
                assert_eq!(p, q);
            }
        }
        // And a different seed draws differently — else the test could not fail.
        let c = vertical_polylines(&heads, &cuts, &length, &spacing, true, 43).unwrap();
        assert!(a.iter().zip(&c).any(|(one, other)| one.cuts != other.cuts || one.length != other.length));
    }

    #[test]
    fn a_single_tip_does_not_merge_two_points() {
        // A point added at the end must not change the draw of the ones before it —
        // each point's stream is independent of how many others there are.
        let a = vertical_polylines(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]], &Cuts::Random { min: 1, max: 20 }, &Length::Fixed(10.0), &Spacing::Regular, true, 7).unwrap();
        let b = vertical_polylines(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]], &Cuts::Random { min: 1, max: 20 }, &Length::Fixed(10.0), &Spacing::Regular, true, 7).unwrap();
        assert_eq!(a[0].cuts, b[0].cuts);
        assert_eq!(a[1].cuts, b[1].cuts);
    }

    #[test]
    fn a_zero_cut_count_is_refused() {
        let heads = [[0.0, 0.0, 0.0]];
        let why = vertical_polylines(&heads, &Cuts::Fixed(0), &Length::Fixed(10.0), &Spacing::Regular, true, 1).unwrap_err();
        assert!(matches!(why, Error::BadCuts { .. }), "{why}");
    }

    #[test]
    fn an_empty_cloud_is_refused() {
        let why = vertical_polylines(&[], &Cuts::Fixed(1), &Length::Fixed(10.0), &Spacing::Regular, true, 1).unwrap_err();
        assert!(matches!(why, Error::NoHeads));
    }
}
