//! **A survey read back from its stations** - the inclination, the azimuth and
//! the dogleg of a wellbore at each station, from where the stations are and
//! their measured depths: the minimum curvature method, inverted. And where
//! each station is from the datum - its vertical depth and its displacement.
//!
//! Written for `witsml-wells` in `energyml-build` (a synthetic model): a WITSML 2.1 `TrajectoryStation`
//! carries `Incl` and `Azi` (WITSML TRG 2.1 §16.10 p. 356), and the RESQML
//! trajectories of a package carry positions and measured depths - a linear
//! spline through the stations `well-from-plan` surveyed, a vertical line, a
//! line through a borehole's passes. Nothing here reads a package or knows a
//! frame: the caller turns its frame into north, east and **down**.
//!
//! # The method, and why it inverts
//!
//! The minimum curvature method joins two stations by **the circular arc
//! tangent to both directions** (API Bulletin D20, 1985; the form of Sawaryn &
//! Thorogood, SPE 84246, 2005 - `well_plan` holds the forward formula). With
//! unit directions `t1`, `t2`, the dogleg `β` between them and `ΔMD` of hole,
//! its chord is
//!
//! ```text
//! Δp = ΔMD / 2 · RF(β) · (t1 + t2),     RF(β) = (2 / β) · tan(β / 2)
//! ```
//!
//! so the chord **bisects** the two directions - it lies along `t1 + t2` - and
//! its length is `ΔMD / 2 · RF(β) · 2 cos(β / 2) = ΔMD · sin(β/2) / (β/2)`. Two
//! consequences, and they are the whole module:
//!
//! - **given the direction at one station, the next is its mirror image across
//!   the chord**: `t2 = 2 (t1 · ĉ) ĉ − t1`, and `β` is twice the angle between
//!   `t1` and the chord;
//! - **that is an arc only if its length says so**: `|Δp| = ΔMD · sin(β/2) /
//!   (β/2)`, which the positions and the depths **check**, interval by
//!   interval. A kinked line - straight segments meeting at an angle, the way a
//!   polyline drawn by hand is - fails it at its first corner, by name.
//!
//! # Where the first direction comes from
//!
//! **From the path itself, never assumed.** An interval whose chord is as long
//! as its hole, to [`SLACK`], is straight, and its direction is its chord's:
//! the straightest such interval anchors the survey, and the directions run
//! forward and backward from it by the mirror above. Every later straight
//! interval reads its direction again from its own chord, so what a long path
//! accumulates in rounding starts over at each hold. A path with **no**
//! straight interval is refused: the dogleg of each arc is known from its
//! length, the plane it turns in is not, and *« a well starts vertical »* would
//! be the guess.
//!
//! # Conventions
//!
//! - **inclination** in degrees from vertical down - 0 down, 90 horizontal;
//! - **azimuth** in degrees **clockwise from north**, in [0, 360): *« 0.00 and
//!   360.00 represent North, 90.00 is East, 180.00 is South and 270.00 is
//!   West »* (WITSML TRG 2.1 §16.10 p. 357, `ClosureDirection` - the same
//!   north as `Azi`, the trajectory's `AziRef`). **None where the hole is
//!   vertical**, which has no azimuth: a zero there would say *north*;
//! - **dogleg severity** in degrees per 30 m of measured depth
//!   ([`RATE_LENGTH`]), over the interval that **ends** at the station - none
//!   at the first.
//!
//! # Refused by name
//!
//! Fewer than two stations, a number that is not finite, measured depths that
//! do not increase, a chord longer than its hole (no path between two points is
//! shorter than the straight line), two stations at one place, no straight
//! interval, and an interval that is not one arc from the direction the hole
//! has - a turn past 180°, or a chord whose length is not the arc's.

use std::f64::consts::FRAC_PI_2;

use crate::well_plan::{RATE_LENGTH, dogleg};

/// How close to its hole a chord must be for the interval to be **straight**,
/// and how close to the arc's a chord must be for the stations to be **on**
/// one: a ten-millionth of the hole, plus [`FLOOR`]. A dogleg under about
/// 0.09° over one interval reads as none; the doglegs of a plan - 2.5° per
/// 5 m station at 15°/30 m, 0.5° per metre - are far above it.
pub const SLACK: f64 = 1e-7;

/// What is allowed on any chord whatever its length, in metres: the rounding
/// of two coordinates of the order of a Lambert-93 northing (6.86e6 m, whose
/// last digit is about 1e-9 m) subtracted, with room to spare.
pub const FLOOR: f64 = 1e-8;

/// Below this horizontal part, a unit direction is vertical and has no
/// azimuth - a ten-millionth, 6e-6 degree of inclination. What a chain of
/// mirrors leaves in a vertical direction read back across map coordinates is
/// of the order of 1e-8: twenty mirrors over 5 m chords whose ends each carry
/// 1e-9 m of rounding.
const VERTICAL: f64 = 1e-7;

/// One station: its measured depth and where it is - **north, east and down**,
/// from any origin, in the unit of the measured depth.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    /// The measured depth.
    pub md: f64,
    /// North, east, down.
    pub at: [f64; 3],
}

/// Which way the hole points at a station.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Direction {
    /// Degrees from vertical down.
    pub inclination: f64,
    /// Degrees clockwise from north, in [0, 360) - `None` where the hole is
    /// vertical.
    pub azimuth: Option<f64>,
    /// Degrees per 30 m of measured depth over the interval that ends here -
    /// `None` at the first station.
    pub dogleg_severity: Option<f64>,
}

/// Where a station is from the datum.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Offset {
    /// The vertical depth below the datum - positive down.
    pub tvd: f64,
    /// North of the datum.
    pub north: f64,
    /// East of the datum.
    pub east: f64,
    /// The horizontal distance from the datum.
    pub closure: f64,
    /// Its direction, clockwise from north - `None` under the datum.
    pub closure_direction: Option<f64>,
}

/// What stations cannot be read as, by name. Stations are counted from 1.
#[derive(Debug, Clone, thiserror::Error, PartialEq)]
pub enum SurveyError {
    /// Fewer than two stations.
    #[error("{given} station(s): a direction is read between two")]
    TooFew {
        /// How many were given.
        given: usize,
    },
    /// A measured depth or a coordinate that is not a number.
    #[error("station {station}: its measured depth or its position is not a finite number")]
    NotFinite {
        /// The station, from 1.
        station: usize,
    },
    /// Measured depths that do not increase.
    #[error("the measured depths do not increase at station {station}: MD {md} after MD {before}")]
    NotIncreasing {
        /// The station, from 1.
        station: usize,
        /// Its measured depth.
        md: f64,
        /// The one before.
        before: f64,
    },
    /// Two stations further apart than the hole between them.
    #[error(
        "between MD {from} and MD {to} the stations are {chord} apart for {hole} of hole - a hole is never shorter than the straight line between two of its points"
    )]
    ChordLongerThanHole {
        /// The upper station's measured depth.
        from: f64,
        /// The lower one's.
        to: f64,
        /// The distance between them.
        chord: f64,
        /// The hole between them.
        hole: f64,
    },
    /// Two stations at one place, with hole between them.
    #[error(
        "the stations at MD {from} and MD {to} are at one place for {hole} of hole: no arc of a minimum-curvature path comes back to where it started"
    )]
    NoChord {
        /// The upper station's measured depth.
        from: f64,
        /// The lower one's.
        to: f64,
        /// The hole between them.
        hole: f64,
    },
    /// No interval is straight, so no direction is known anywhere.
    #[error(
        "none of the {intervals} interval(s) is straight, so the direction of the hole is known at no station: the dogleg of each arc is its length's, the plane it turns in is not"
    )]
    NoStraightInterval {
        /// How many intervals there are.
        intervals: usize,
    },
    /// An interval that is not one arc from the direction the hole has.
    #[error(
        "between MD {from} and MD {to} the stations are not on one minimum-curvature arc from the direction the hole has at MD {known_at}: a turn of {turn}° would make {expected} of chord, and they are {chord} apart"
    )]
    NotAnArc {
        /// The upper station's measured depth.
        from: f64,
        /// The lower one's.
        to: f64,
        /// Where the direction it was read from is.
        known_at: f64,
        /// The dogleg that direction and the chord make, in degrees.
        turn: f64,
        /// The chord an arc of that dogleg would have.
        expected: f64,
        /// The chord the stations have.
        chord: f64,
    },
}

type Vector = [f64; 3];

fn sub(a: Vector, b: Vector) -> Vector {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn mul(a: Vector, k: f64) -> Vector {
    [a[0] * k, a[1] * k, a[2] * k]
}

fn dot(a: Vector, b: Vector) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn norm(a: Vector) -> f64 {
    dot(a, a).sqrt()
}

/// What a chord of `hole` may be off by - see [`SLACK`] and [`FLOOR`].
fn allowed(hole: f64) -> f64 {
    SLACK * hole + FLOOR
}

/// `sin x / x`, its series where the quotient loses its digits.
fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-4 {
        1.0 - x * x / 6.0 + x.powi(4) / 120.0
    } else {
        x.sin() / x
    }
}

/// A direction in the horizontal plane, in degrees clockwise from north, in
/// [0, 360) - a remainder of −1e-15 is 360 once rounded, and 360 is north.
fn bearing(north: f64, east: f64) -> f64 {
    let degrees = east.atan2(north).to_degrees().rem_euclid(360.0);
    if degrees >= 360.0 {
        degrees - 360.0
    } else {
        degrees
    }
}

/// One interval: its two depths and its chord.
struct Chord {
    from: f64,
    to: f64,
    /// The chord's unit direction.
    unit: Vector,
    length: f64,
    hole: f64,
}

impl Chord {
    /// How far from straight - what the chord lacks of the hole, per unit of
    /// hole.
    fn deficit(&self) -> f64 {
        (self.hole - self.length) / self.hole
    }

    fn straight(&self) -> bool {
        (self.hole - self.length).abs() <= allowed(self.hole)
    }

    /// The dogleg of the arc from direction `t` along this chord - or the
    /// refusal that says the stations are on none.
    fn arc(&self, t: Vector, known_at: f64) -> Result<f64, SurveyError> {
        // The angle between the direction and the chord is half the dogleg:
        // the chord bisects the two directions.
        let half = dogleg(t, self.unit);
        let expected = self.hole * sinc(half);
        if half > FRAC_PI_2 + 1e-12 || (expected - self.length).abs() > allowed(self.hole) {
            return Err(SurveyError::NotAnArc {
                from: self.from,
                to: self.to,
                known_at,
                turn: (2.0 * half).to_degrees(),
                expected,
                chord: self.length,
            });
        }
        Ok(2.0 * half)
    }
}

/// The direction at the other end of an arc from `t` along the chord `unit`:
/// its mirror image across the chord, kept of unit length.
fn mirrored(t: Vector, unit: Vector) -> Vector {
    let image = sub(mul(unit, 2.0 * dot(t, unit)), t);
    mul(image, 1.0 / norm(image))
}

/// The direction of the hole at every station - see the module.
// Stations and intervals are two arrays walked together, one a step ahead of
// the other: an index says it more plainly than two zipped iterators.
#[allow(clippy::needless_range_loop)]
pub fn directions(points: &[Point]) -> Result<Vec<Direction>, SurveyError> {
    if points.len() < 2 {
        return Err(SurveyError::TooFew { given: points.len() });
    }
    if let Some(at) = points
        .iter()
        .position(|point| !point.md.is_finite() || point.at.iter().any(|v| !v.is_finite()))
    {
        return Err(SurveyError::NotFinite { station: at + 1 });
    }
    if let Some(at) = points.windows(2).position(|pair| pair[1].md <= pair[0].md) {
        return Err(SurveyError::NotIncreasing {
            station: at + 2,
            md: points[at + 1].md,
            before: points[at].md,
        });
    }

    // -- every chord, checked against its hole ------------------------------
    let mut chords = Vec::with_capacity(points.len() - 1);
    for pair in points.windows(2) {
        let (from, to) = (pair[0].md, pair[1].md);
        let hole = to - from;
        let d = sub(pair[1].at, pair[0].at);
        let length = norm(d);
        if length > hole + allowed(hole) {
            return Err(SurveyError::ChordLongerThanHole {
                from,
                to,
                chord: length,
                hole,
            });
        }
        if length <= FLOOR {
            return Err(SurveyError::NoChord { from, to, hole });
        }
        chords.push(Chord {
            from,
            to,
            unit: mul(d, 1.0 / length),
            length,
            hole,
        });
    }

    // -- the anchor: the straightest straight interval ----------------------
    let anchor = chords
        .iter()
        .enumerate()
        .filter(|(_, chord)| chord.straight())
        .min_by(|(_, a), (_, b)| a.deficit().abs().total_cmp(&b.deficit().abs()))
        .map(|(at, _)| at)
        .ok_or(SurveyError::NoStraightInterval {
            intervals: chords.len(),
        })?;

    let count = points.len();
    let mut t: Vec<Vector> = vec![[0.0; 3]; count];
    let mut turn = vec![0.0; count - 1];
    t[anchor] = chords[anchor].unit;
    t[anchor + 1] = chords[anchor].unit;

    // -- forward from the anchor, then backward ------------------------------
    // Station `at` and `at + 1` bound interval `at`: two arrays walked
    // together, one a step ahead of the other.
    for at in anchor + 1..count - 1 {
        let chord = &chords[at];
        turn[at] = chord.arc(t[at], chord.from)?;
        // A straight interval reads its direction again from its chord: what
        // the mirrors accumulated in rounding starts over at each hold.
        t[at + 1] = if chord.straight() && turn[at] < 1e-6 {
            chord.unit
        } else {
            mirrored(t[at], chord.unit)
        };
    }
    for at in (0..anchor).rev() {
        let chord = &chords[at];
        turn[at] = chord.arc(t[at + 1], chord.to)?;
        t[at] = if chord.straight() && turn[at] < 1e-6 {
            chord.unit
        } else {
            mirrored(t[at + 1], chord.unit)
        };
    }

    Ok(t.iter()
        .enumerate()
        .map(|(at, &[north, east, down])| {
            let horizontal = north.hypot(east);
            Direction {
                // `atan2` rather than `acos(down)`: near the vertical, `acos`
                // turns one unit of rounding into 1e-6 degrees.
                inclination: horizontal.atan2(down).to_degrees(),
                azimuth: (horizontal > VERTICAL).then(|| bearing(north, east)),
                dogleg_severity: (at > 0)
                    .then(|| turn[at - 1].to_degrees() / chords[at - 1].hole * RATE_LENGTH),
            }
        })
        .collect())
}

/// Where each station is from `datum` - north, east and down, like the
/// stations: its vertical depth below the datum, its displacement, and the
/// closure and its direction.
pub fn offsets(points: &[Point], datum: [f64; 3]) -> Vec<Offset> {
    points
        .iter()
        .map(|point| {
            let north = point.at[0] - datum[0];
            let east = point.at[1] - datum[1];
            let closure = north.hypot(east);
            Offset {
                tvd: point.at[2] - datum[2],
                north,
                east,
                closure,
                closure_direction: (closure > 0.0).then(|| bearing(north, east)),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::well_plan::{self, Section, Station, Tie, Turn};

    fn near(a: f64, b: f64, tolerance: f64) -> bool {
        (a - b).abs() <= tolerance
    }

    /// Two bearings are near when they are near **on the circle**: 359.9999 is
    /// next to 0.
    fn near_bearing(a: f64, b: f64, tolerance: f64) -> bool {
        let d = (a - b).rem_euclid(360.0);
        d <= tolerance || 360.0 - d <= tolerance
    }

    fn curve(rate: f64, inclination: f64, azimuth: f64, turn: Turn) -> Section {
        Section::Curve {
            rate,
            inclination,
            azimuth,
            turn,
        }
    }

    /// The stations of a plan as points, moved by `shift` (north, east, down).
    fn points_of(stations: &[Station], shift: [f64; 3]) -> Vec<Point> {
        stations
            .iter()
            .map(|s| Point {
                md: s.md,
                at: [shift[0] + s.north, shift[1] + s.east, shift[2] + s.tvd],
            })
            .collect()
    }

    /// GTH-1 of the recipe: vertical to 15, a build to 60° towards the NW, a
    /// hold, a drop to 40°, a hold.
    fn gth1() -> Vec<Station> {
        well_plan::plan(
            Tie::vertical(),
            &[
                Section::HoldTo { md: 15.0 },
                curve(15.0, 60.0, 315.0, Turn::Build),
                Section::Hold { length: 80.0 },
                curve(10.0, 40.0, 315.0, Turn::Drop),
                Section::Hold { length: 10.0 },
            ],
            5.0,
        )
        .expect("a plan")
        .stations
    }

    /// Every station's direction against the one the forward survey gave it.
    fn agrees(stations: &[Station], read: &[Direction], tolerance: f64) {
        assert_eq!(stations.len(), read.len());
        for (station, direction) in stations.iter().zip(read) {
            assert!(
                near(direction.inclination, station.inclination, tolerance),
                "MD {}: {direction:?} for {station:?}",
                station.md
            );
            if station.inclination > 0.0 {
                let azimuth = direction.azimuth.unwrap_or_else(|| {
                    panic!(
                        "MD {}: no azimuth at inclination {}",
                        station.md, station.inclination
                    )
                });
                assert!(
                    near_bearing(azimuth, station.azimuth, tolerance),
                    "MD {}: {direction:?} for {station:?}",
                    station.md
                );
            } else {
                assert_eq!(
                    direction.azimuth, None,
                    "MD {}: a vertical hole has no azimuth",
                    station.md
                );
            }
        }
    }

    #[test]
    fn the_inverse_gives_back_the_directions_the_forward_survey_put() {
        // Two independent computations of one fact: `well_plan` places the
        // stations from their directions (the textbook formula), this module
        // reads the directions from the stations. To break: a mirror across
        // the wrong line, azimuths counter-clockwise, a dogleg read as the
        // whole angle between direction and chord rather than its double.
        let stations = gth1();
        let read = directions(&points_of(&stations, [0.0; 3])).expect("stations on arcs");
        agrees(&stations, &read, 1e-8);
    }

    #[test]
    fn far_from_the_origin_the_directions_hold() {
        // The same well at its Lambert-93 place (651 620, 6 861 170), its
        // datum at 48 m of elevation - the scale at which `heights` once
        // answered "outside" everywhere. A coordinate of 6.86e6 m carries
        // about 1e-9 m of rounding; over 5 m chords and some sixty mirrors it
        // stays well under 1e-5 degree.
        let stations = gth1();
        let points = points_of(&stations, [6_861_170.0, 651_620.0, -48.0]);
        assert!(
            points[10].at[0] > 6.8e6,
            "the test must be far from the origin to test anything"
        );
        let read = directions(&points).expect("stations on arcs");
        agrees(&stations, &read, 1e-5);
    }

    #[test]
    fn the_dogleg_severity_is_the_rate_of_the_arc_and_zero_on_a_hold() {
        let stations = gth1();
        let read = directions(&points_of(&stations, [0.0; 3])).expect("stations on arcs");
        assert_eq!(
            read[0].dogleg_severity, None,
            "no interval ends at the first station"
        );
        // MD 20 ends the first 5 m of the build at 15°/30 m; MD 10 is on the
        // vertical hold.
        let at = |md: f64| stations.iter().position(|s| s.md == md).expect("a station");
        assert!(
            near(read[at(20.0)].dogleg_severity.unwrap(), 15.0, 1e-8),
            "{:?}",
            read[at(20.0)]
        );
        assert!(
            near(read[at(10.0)].dogleg_severity.unwrap(), 0.0, 1e-9),
            "{:?}",
            read[at(10.0)]
        );
    }

    #[test]
    fn a_drain_is_horizontal_and_points_where_it_goes() {
        // GTH-2: vertical to 30, a build at 15°/30 m to horizontal towards the
        // west, a drain of 250 m. West is 270, clockwise from north - a
        // counter-clockwise reading would say 90.
        let stations = well_plan::plan(
            Tie::vertical(),
            &[
                Section::HoldTo { md: 30.0 },
                curve(15.0, 90.0, 270.0, Turn::Build),
                Section::Hold { length: 250.0 },
            ],
            10.0,
        )
        .expect("a plan")
        .stations;
        let read =
            directions(&points_of(&stations, [6_861_145.0, 651_595.0, -47.0])).expect("stations on arcs");
        let drain: Vec<&Direction> = stations
            .iter()
            .zip(&read)
            .filter(|(s, _)| s.md >= 210.0)
            .map(|(_, d)| d)
            .collect();
        assert!(drain.len() > 20, "the drain has its stations");
        for direction in drain {
            assert!(near(direction.inclination, 90.0, 1e-6), "{direction:?}");
            assert!(
                near_bearing(
                    direction.azimuth.expect("a horizontal hole has an azimuth"),
                    270.0,
                    1e-6
                ),
                "{direction:?}"
            );
        }
    }

    #[test]
    fn azimuths_turn_clockwise_from_north() {
        for azimuth in [0.0, 90.0, 180.0, 270.0, 45.0] {
            let stations = well_plan::plan(
                Tie::vertical(),
                &[
                    Section::HoldTo { md: 20.0 },
                    curve(10.0, 30.0, azimuth, Turn::Build),
                    Section::Hold { length: 50.0 },
                ],
                10.0,
            )
            .expect("a plan")
            .stations;
            let read = directions(&points_of(&stations, [0.0; 3])).expect("stations on arcs");
            let last = read.last().expect("a station");
            assert!(near(last.inclination, 30.0, 1e-8), "{azimuth}: {last:?}");
            assert!(
                near_bearing(last.azimuth.expect("an azimuth"), azimuth, 1e-8),
                "{azimuth}: {last:?}"
            );
        }
    }

    #[test]
    fn a_vertical_hole_has_no_azimuth_and_no_dogleg() {
        let points: Vec<Point> = (0..=20)
            .map(|k| Point {
                md: k as f64 * 10.0,
                at: [6_861_200.0, 651_800.0, -48.0 + k as f64 * 10.0],
            })
            .collect();
        let read = directions(&points).expect("a straight hole");
        for direction in &read {
            assert_eq!(direction.inclination, 0.0, "{direction:?}");
            assert_eq!(direction.azimuth, None, "{direction:?}");
        }
        assert!(
            read[1..].iter().all(|d| d.dogleg_severity == Some(0.0)),
            "{read:?}"
        );
    }

    #[test]
    fn a_sidetrack_that_starts_on_an_arc_reads_its_first_direction_backward_from_its_hold() {
        // GTH-3 ST1: from MD 43 on a vertical parent, a build at once to 45°
        // towards the north, then a hold. The first interval is an arc, so the
        // direction at the kick-off comes back from the hold - and must be the
        // parent's, vertical.
        let tie = Tie {
            md: 43.0,
            inclination: 0.0,
            azimuth: 0.0,
        };
        let stations = well_plan::plan(
            tie,
            &[
                curve(15.0, 45.0, 0.0, Turn::Build),
                Section::Hold { length: 40.0 },
            ],
            5.0,
        )
        .expect("a plan")
        .stations;
        let read =
            directions(&points_of(&stations, [6_861_200.0, 651_800.0, -5.0])).expect("stations on arcs");
        assert!(near(read[0].inclination, 0.0, 1e-5), "{:?}", read[0]);
        agrees(&stations, &read, 1e-5);
    }

    #[test]
    fn a_kinked_line_is_no_arc_and_is_refused_at_its_corner() {
        // Vertical for 10 m, then straight on at 30° for 10 m, the depths the
        // lengths of the segments: each interval is straight by its length,
        // and the corner is where no arc turns 30° in no hole.
        let (s, c) = (30f64.to_radians().sin(), 30f64.to_radians().cos());
        let points = vec![
            Point {
                md: 0.0,
                at: [0.0, 0.0, 0.0],
            },
            Point {
                md: 10.0,
                at: [0.0, 0.0, 10.0],
            },
            Point {
                md: 20.0,
                at: [10.0 * s, 0.0, 10.0 + 10.0 * c],
            },
        ];
        match directions(&points) {
            Err(SurveyError::NotAnArc {
                from,
                to,
                known_at,
                turn,
                ..
            }) => {
                assert_eq!((from, to, known_at), (10.0, 20.0, 10.0));
                assert!(
                    near(turn, 60.0, 1e-9),
                    "the direction and the chord part by 30°, an arc would turn twice that: {turn}"
                );
            }
            other => panic!("a kink is no arc: {other:?}"),
        }
    }

    #[test]
    fn what_no_path_can_be_is_refused_by_name() {
        let p = |md: f64, down: f64| Point {
            md,
            at: [0.0, 0.0, down],
        };
        assert_eq!(directions(&[p(0.0, 0.0)]), Err(SurveyError::TooFew { given: 1 }));
        assert_eq!(
            directions(&[p(0.0, 0.0), p(10.0, 10.0), p(10.0, 20.0)]),
            Err(SurveyError::NotIncreasing {
                station: 3,
                md: 10.0,
                before: 10.0
            })
        );
        assert_eq!(
            directions(&[p(0.0, 0.0), p(f64::NAN, 10.0)]),
            Err(SurveyError::NotFinite { station: 2 })
        );
        assert!(matches!(
            directions(&[p(0.0, 0.0), p(10.0, 12.0)]),
            Err(SurveyError::ChordLongerThanHole { from, to, .. }) if from == 0.0 && to == 10.0
        ));
        assert_eq!(
            directions(&[p(0.0, 0.0), p(10.0, 0.0)]),
            Err(SurveyError::NoChord {
                from: 0.0,
                to: 10.0,
                hole: 10.0
            })
        );
    }

    #[test]
    fn a_path_that_is_all_arcs_has_no_first_direction_and_says_so() {
        // A build from the surface with no hold anywhere: each arc's dogleg is
        // known, the plane it turns in is not - refused rather than assumed to
        // start vertical.
        let stations = well_plan::plan(Tie::vertical(), &[curve(10.0, 60.0, 90.0, Turn::Build)], 10.0)
            .expect("a plan")
            .stations;
        assert_eq!(
            directions(&points_of(&stations, [0.0; 3])),
            Err(SurveyError::NoStraightInterval {
                intervals: stations.len() - 1
            })
        );
    }

    #[test]
    fn offsets_are_from_the_datum_down_and_clockwise() {
        let datum = [6_861_200.0, 651_800.0, -48.0];
        let points = [
            Point { md: 0.0, at: datum },
            Point {
                md: 150.0,
                at: [6_861_210.0, 651_790.0, 52.0],
            },
        ];
        let read = offsets(&points, datum);
        assert_eq!(read[0].closure, 0.0);
        assert_eq!(read[0].closure_direction, None, "no direction under the datum");
        assert_eq!((read[1].tvd, read[1].north, read[1].east), (100.0, 10.0, -10.0));
        assert!(near(read[1].closure, 200f64.sqrt(), 1e-9));
        assert!(
            near(read[1].closure_direction.unwrap(), 315.0, 1e-9),
            "north-west is 315: {:?}",
            read[1]
        );
    }
}
