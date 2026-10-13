//! **The minimum curvature method** - the arc tangent to two directions that
//! joins two stations of a well path - and, run **forward**, where a surveyed
//! hole runs: positions from the stations a directional survey measured, a
//! measured depth, an inclination and an azimuth each, out north, east and
//! vertical depth from the first station.
//!
//! # Why this is in `energyml-geometry`
//!
//! Reading a WITSML 2.1 `Trajectory` is reading a **shape**, as evaluating a
//! RESQML parametric line is ([`crate::parametric`]): the stations are what the
//! file says, the positions are the hole. `energyml-analysis` sits **above**
//! geometry and never inside it; when this method first
//! lived there, geometry had to depend on analysis to draw a trajectory - the
//! layering upside down. So the method comes down here, and
//! `energyml_analysis::well_plan` (the plan) and `survey` (the inverse)
//! import its pieces from this module.
//!
//! Written for the reader of a WITSML 2.1 `Trajectory` ([`crate::stations`]): a
//! `TrajectoryStation` carries `Md`, `Incl` - *« Hole inclination, measured
//! from vertical »* - and `Azi` - *« Hole azimuth. Corrected to wells azimuth
//! reference »* (WITSML TRG 2.1 §16.10 p. 356-358). Nothing here reads a
//! package or knows a frame: the caller says which north the azimuths are from,
//! and places the first station.
//!
//! **Not `energyml_analysis::survey`.** That module is the inverse - positions in,
//! angles out - and is what `witsml-wells` writes stations with. The two are
//! independent code paths over one method, which is what lets a round trip
//! through both confront them (`energyml-build/tests/witsml_trajectory.rs`).
//!
//! # The method, and where its form comes from
//!
//! Two stations are joined by **the circular arc tangent to both
//! directions** - API Bulletin D20 (*Directional Drilling Survey Calculation
//! Methods and Terminology*, 1985), in the form of Sawaryn & Thorogood, *A
//! Compendium of Directional Calculations Based on the Minimum Curvature
//! Method* (SPE 84246, SPE Drilling & Completion 20(1), 2005) - the same
//! source `energyml_analysis::well_plan` cites - and the pieces below are the
//! ones it uses, never a second copy of them:
//!
//! ```text
//! t     = (sin I cos A, sin I sin A, cos I)          north, east, down - [`direction`]
//! β     = the angle between t1 and t2                the dogleg - [`dogleg`]
//! RF    = (2 / β) · tan(β / 2), 1 when β = 0          [`ratio_factor`]
//! Δp    = ΔMD / 2 · RF · (t1 + t2)
//! ```
//!
//! `Δp` is the **chord** of the arc, exact on it. Each station is the previous
//! one plus its interval's chord: a survey states nothing between two stations
//! but that they are joined by one arc, so there is no section start to compute
//! from the way a plan does - the sum is the method.
//!
//! # Conventions
//!
//! - **angles in degrees**: an inclination from vertical down (0 down, 90
//!   horizontal, 180 up), an azimuth clockwise from the north the caller names -
//!   *« 0.00 and 360.00 represent North, 90.00 is East, 180.00 is South and
//!   270.00 is West »* (§16.10 p. 357, `ClosureDirection`). The unit of the
//!   file is the caller's business: this module holds no unit table, and a
//!   second one beside `energyml-domain`'s would be a second answer to *what is
//!   a grad*. The reader converts, and refuses a unit that is not an angle;
//! - **measured depths in one length unit**, and the positions come out in it;
//! - **the first station is the origin**: `(0, 0, 0)` - where it is in the
//!   world is the caller's, who knows the datum.
//!
//! # A station with no azimuth
//!
//! Is read only where the hole is **vertical** - its inclination's sine at most
//! [`VERTICAL`] - and then points straight down (or up): a vertical hole has no
//! azimuth, and `witsml-wells` writes none there (*« Where the hole is
//! vertical, no azimuth is written: there is none »*). Anywhere else the
//! azimuth decides where the hole goes, and its absence is refused rather than
//! read as north.
//!
//! # Refused by name
//!
//! Fewer than two stations, a number that is not finite, measured depths that
//! do not increase, a station with no inclination, a station off vertical with
//! no azimuth, an inclination outside [0°, 180°], an azimuth outside
//! [0°, 360°], and two stations whose directions are opposite - a dogleg of
//! 180° lies in no one plane, the arc is undefined.

use std::f64::consts::PI;

type Vector = [f64; 3];

fn sub(a: Vector, b: Vector) -> Vector {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn norm(a: Vector) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

/// The unit direction of an inclination and an azimuth, in degrees: north,
/// east, down.
pub fn direction(inclination: f64, azimuth: f64) -> [f64; 3] {
    let (i, a) = (inclination.to_radians(), azimuth.to_radians());
    [i.sin() * a.cos(), i.sin() * a.sin(), i.cos()]
}

/// The dogleg between two unit directions, in radians - as `2 asin(|t2 − t1| /
/// 2)`, the same angle as the `acos` of the textbook form but not losing its
/// digits when it is small.
pub fn dogleg(t1: [f64; 3], t2: [f64; 3]) -> f64 {
    2.0 * (norm(sub(t2, t1)) / 2.0).clamp(0.0, 1.0).asin()
}

/// The ratio factor of the minimum curvature method, `(2 / β) tan(β / 2)` -
/// its series `1 + β² / 12` below a microradian, where the quotient loses its
/// digits.
pub fn ratio_factor(beta: f64) -> f64 {
    if beta.abs() < 1e-6 {
        1.0 + beta * beta / 12.0
    } else {
        2.0 / beta * (beta / 2.0).tan()
    }
}

/// Below this, the sine of an inclination makes the hole **vertical**: a
/// station there may state no azimuth. A ten-millionth - 6e-6° of inclination -
/// the threshold under which `energyml_analysis::survey` writes none; what an azimuth
/// could still move there is a ten-millionth of the hole's length sideways.
pub const VERTICAL: f64 = 1e-7;

/// Below this distance from 180°, in radians, two directions are opposite.
const REVERSAL: f64 = 1e-9;

/// One station as measured.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Measured {
    /// The measured depth.
    pub md: f64,
    /// Degrees from vertical down - `None` when the station states none.
    pub inclination: Option<f64>,
    /// Degrees clockwise from north - `None` when the station states none,
    /// which is read only where the hole is vertical.
    pub azimuth: Option<f64>,
}

/// One station placed: where the hole is at its measured depth, from the first
/// station.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Located {
    /// The measured depth, as given.
    pub md: f64,
    /// North of the first station, in the unit of the measured depths.
    pub north: f64,
    /// East of the first station.
    pub east: f64,
    /// Vertical depth below the first station - positive down.
    pub tvd: f64,
}

/// What a survey cannot be, by name. Stations are counted **from 1**, in the
/// order given.
#[derive(Debug, Clone, thiserror::Error, PartialEq)]
pub enum StationsError {
    #[error("{given} station(s): a path is two stations at least")]
    /// Fewer than two stations: no interval to join.
    TooFewStations {
        /// How many were given.
        given: usize,
    },
    #[error("station {station}: its {what} is not a number ({given})")]
    /// A measured depth or an angle that is not a finite number.
    NotANumber {
        /// The station, from 1.
        station: usize,
        /// Which value: `md`, `inclination` or `azimuth`.
        what: &'static str,
        /// What was given.
        given: f64,
    },
    #[error(
        "station {station}: MD {md} does not come after MD {previous} - the stations of a survey run down the hole, in order"
    )]
    /// A measured depth that does not come after the previous one.
    NotIncreasing {
        /// The station, from 1.
        station: usize,
        /// Its measured depth.
        md: f64,
        /// The previous station's.
        previous: f64,
    },
    #[error("station {station}: no inclination - the direction of the hole there is not stated")]
    /// A station with no inclination.
    NoInclination {
        /// The station, from 1.
        station: usize,
    },
    #[error(
        "station {station}: no azimuth, and the hole is inclined {inclination}° there - which way it goes is not stated, and reading it as north would be a guess"
    )]
    /// An inclined station with no azimuth.
    NoAzimuth {
        /// The station, from 1.
        station: usize,
        /// Its inclination, in degrees.
        inclination: f64,
    },
    #[error(
        "station {station}: an inclination is in [0°, 180°] - 0 down, 90 horizontal, 180 up - not {given}°"
    )]
    /// An inclination outside [0°, 180°].
    BadInclination {
        /// The station, from 1.
        station: usize,
        /// What was given, in degrees.
        given: f64,
    },
    #[error("station {station}: an azimuth is in [0°, 360°], not {given}°")]
    /// An azimuth outside [0°, 360°].
    BadAzimuth {
        /// The station, from 1.
        station: usize,
        /// What was given, in degrees.
        given: f64,
    },
    #[error(
        "station {station}: the hole turns back on itself since the station before, a dogleg of 180° - no single arc does that, the plane it would turn in is undefined"
    )]
    /// A dogleg of 180°: the arc's plane is undefined.
    Reversal {
        /// The station, from 1.
        station: usize,
    },
}

/// The unit direction of one station - checked, north, east, down.
fn direction_of(station: usize, measured: &Measured) -> Result<[f64; 3], StationsError> {
    let inclination = measured
        .inclination
        .ok_or(StationsError::NoInclination { station })?;
    if !inclination.is_finite() {
        return Err(StationsError::NotANumber {
            station,
            what: "inclination",
            given: inclination,
        });
    }
    if !(0.0..=180.0).contains(&inclination) {
        return Err(StationsError::BadInclination {
            station,
            given: inclination,
        });
    }
    match measured.azimuth {
        Some(azimuth) => {
            if !azimuth.is_finite() {
                return Err(StationsError::NotANumber {
                    station,
                    what: "azimuth",
                    given: azimuth,
                });
            }
            if !(0.0..=360.0).contains(&azimuth) {
                return Err(StationsError::BadAzimuth {
                    station,
                    given: azimuth,
                });
            }
            Ok(direction(inclination, azimuth))
        }
        None if inclination.to_radians().sin().abs() <= VERTICAL => {
            // Vertical: straight down, or straight up past 90°.
            Ok([0.0, 0.0, if inclination <= 90.0 { 1.0 } else { -1.0 }])
        }
        None => Err(StationsError::NoAzimuth { station, inclination }),
    }
}

/// Every station placed from the first, by the minimum curvature method.
pub fn positions(stations: &[Measured]) -> Result<Vec<Located>, StationsError> {
    if stations.len() < 2 {
        return Err(StationsError::TooFewStations {
            given: stations.len(),
        });
    }
    // -- every station checked before any is placed ----------------------------
    let mut directions = Vec::with_capacity(stations.len());
    for (at, measured) in stations.iter().enumerate() {
        let station = at + 1;
        if !measured.md.is_finite() {
            return Err(StationsError::NotANumber {
                station,
                what: "measured depth",
                given: measured.md,
            });
        }
        if at > 0 && measured.md <= stations[at - 1].md {
            return Err(StationsError::NotIncreasing {
                station,
                md: measured.md,
                previous: stations[at - 1].md,
            });
        }
        directions.push(direction_of(station, measured)?);
    }

    // -- the chords, one per interval ------------------------------------------
    let mut out = Vec::with_capacity(stations.len());
    let mut at = [0.0f64; 3];
    out.push(Located {
        md: stations[0].md,
        north: 0.0,
        east: 0.0,
        tvd: 0.0,
    });
    for k in 1..stations.len() {
        let (t1, t2) = (directions[k - 1], directions[k]);
        let beta = dogleg(t1, t2);
        if PI - beta < REVERSAL {
            return Err(StationsError::Reversal { station: k + 1 });
        }
        let scale = (stations[k].md - stations[k - 1].md) / 2.0 * ratio_factor(beta);
        for axis in 0..3 {
            at[axis] += scale * (t1[axis] + t2[axis]);
        }
        out.push(Located {
            md: stations[k].md,
            north: at[0],
            east: at[1],
            tvd: at[2],
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: f64, b: f64, tolerance: f64) -> bool {
        (a - b).abs() <= tolerance
    }

    fn station(md: f64, inclination: f64, azimuth: f64) -> Measured {
        Measured {
            md,
            inclination: Some(inclination),
            azimuth: Some(azimuth),
        }
    }

    #[test]
    fn a_vertical_hole_is_as_deep_as_it_is_long() {
        // To break: a vertical station read with a made-up azimuth that is not
        // ignored, or a ratio factor that is not 1 on a straight interval.
        let stations: Vec<Measured> = (0..=10)
            .map(|k| Measured {
                md: 10.0 * k as f64,
                inclination: Some(0.0),
                azimuth: None,
            })
            .collect();
        let placed = positions(&stations).unwrap();
        for (one, measured) in placed.iter().zip(&stations) {
            assert_eq!((one.north, one.east), (0.0, 0.0), "{one:?}");
            assert!(
                near(one.tvd, measured.md, 1e-12),
                "TVD = MD on a vertical hole: {one:?}"
            );
        }
    }

    #[test]
    fn a_ninety_degree_build_ends_one_radius_north_and_one_radius_down() {
        // By hand: 90° of build over a course length of 900 m is a circle of
        // radius R = 180/π × (900 / 90) = 57.29577951 × 10 = 572.9577951 m.
        // A quarter turn from vertical, toward north, ends R north of its
        // start and R below it. Two stations, one interval: the whole arc.
        let placed = positions(&[station(0.0, 0.0, 0.0), station(900.0, 90.0, 0.0)]).unwrap();
        let end = placed[1];
        assert!(near(end.north, 572.957_795_1, 1e-6), "{end:?}");
        assert!(near(end.tvd, 572.957_795_1, 1e-6), "{end:?}");
        assert!(near(end.east, 0.0, 1e-12), "{end:?}");
    }

    #[test]
    fn a_constant_build_lands_on_the_circle_at_every_station() {
        // 3°/30 m from vertical, toward east, a station every 30 m to 90°: the
        // circle of radius R = 30 / (3° in radians) = 572.9577951 m. After a
        // turn θ the hole is R (1 − cos θ) east and R sin θ down of its start
        // (the circle's own equation, not the survey formula). At θ = 30°,
        // MD 300: R (1 − cos 30°) = 76.7617893, R sin 30° = 286.4788976.
        let r = 30.0 / 3f64.to_radians();
        let stations: Vec<Measured> = (0..=30)
            .map(|k| station(30.0 * k as f64, 3.0 * k as f64, 90.0))
            .collect();
        let placed = positions(&stations).unwrap();
        for one in &placed {
            let theta = (one.md / r).min(std::f64::consts::FRAC_PI_2);
            assert!(near(one.east, r * (1.0 - theta.cos()), 1e-9), "{one:?}");
            assert!(near(one.tvd, r * theta.sin(), 1e-9), "{one:?}");
            assert!(
                near(one.north, 0.0, 1e-9),
                "an azimuth of 90 is east, not north: {one:?}"
            );
        }
        let at_300 = placed[10];
        assert_eq!(at_300.md, 300.0);
        assert!(near(at_300.east, 76.761_789_3, 1e-6), "{at_300:?}");
        assert!(near(at_300.tvd, 286.478_897_6, 1e-6), "{at_300:?}");
    }

    #[test]
    fn azimuth_runs_clockwise_from_north() {
        // Horizontal holds at 0, 90, 180 and 270: north, east, south, west.
        for (azimuth, north, east) in [
            (0.0, 1.0, 0.0),
            (90.0, 0.0, 1.0),
            (180.0, -1.0, 0.0),
            (270.0, 0.0, -1.0),
        ] {
            let placed = positions(&[station(0.0, 90.0, azimuth), station(10.0, 90.0, azimuth)]).unwrap();
            let end = placed[1];
            assert!(
                near(end.north, 10.0 * north, 1e-12) && near(end.east, 10.0 * east, 1e-12),
                "{azimuth}: {end:?}"
            );
            assert!(near(end.tvd, 0.0, 1e-12), "{end:?}");
        }
    }

    #[test]
    fn what_is_not_a_survey_is_refused_by_name() {
        let vertical = |md| Measured {
            md,
            inclination: Some(0.0),
            azimuth: None,
        };
        assert_eq!(
            positions(&[vertical(0.0)]),
            Err(StationsError::TooFewStations { given: 1 })
        );
        assert_eq!(
            positions(&[vertical(0.0), vertical(10.0), vertical(10.0)]),
            Err(StationsError::NotIncreasing {
                station: 3,
                md: 10.0,
                previous: 10.0
            })
        );
        assert_eq!(
            positions(&[
                vertical(0.0),
                Measured {
                    md: 10.0,
                    inclination: None,
                    azimuth: Some(0.0)
                }
            ]),
            Err(StationsError::NoInclination { station: 2 })
        );
        assert_eq!(
            positions(&[
                vertical(0.0),
                Measured {
                    md: 10.0,
                    inclination: Some(5.0),
                    azimuth: None
                }
            ]),
            Err(StationsError::NoAzimuth {
                station: 2,
                inclination: 5.0
            })
        );
        assert_eq!(
            positions(&[vertical(0.0), station(10.0, 190.0, 0.0)]),
            Err(StationsError::BadInclination {
                station: 2,
                given: 190.0
            })
        );
        assert_eq!(
            positions(&[vertical(0.0), station(10.0, 10.0, -5.0)]),
            Err(StationsError::BadAzimuth {
                station: 2,
                given: -5.0
            })
        );
        assert!(matches!(
            positions(&[vertical(0.0), vertical(f64::NAN)]),
            Err(StationsError::NotANumber {
                station: 2,
                what: "measured depth",
                ..
            })
        ));
        assert_eq!(
            positions(&[station(0.0, 90.0, 0.0), station(10.0, 90.0, 180.0)]),
            Err(StationsError::Reversal { station: 2 })
        );
        // Straight up is vertical too, and needs no azimuth: a quarter turn
        // from horizontal toward north to straight up goes north and rises.
        let up = positions(&[
            station(0.0, 90.0, 0.0),
            Measured {
                md: 10.0,
                inclination: Some(180.0),
                azimuth: None,
            },
        ])
        .unwrap();
        assert!(up[1].tvd < 0.0 && up[1].north > 0.0, "{:?}", up[1]);
    }
}
