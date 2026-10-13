//! **A wellbore from a drilling plan** - straight holds and circular arcs,
//! surveyed by the minimum curvature method: measured depth in, stations out
//! (north, east and vertical depth from where the plan starts).
//!
//! Written for `well-from-plan` in `energyml-build` (a synthetic model: a trajectory from a drilling plan - kick-off, build rate,
//! hold, resumption, drain, lateral). Nothing here reads a package or knows a frame: the
//! operation places the stations, this module only says where along the hole
//! they are.
//!
//! # The method, and where its form comes from
//!
//! The minimum curvature method joins two survey stations by **the circular
//! arc tangent to both directions**. It is one of the survey calculation
//! methods API Bulletin D20 (*Directional Drilling Survey Calculation Methods
//! and Terminology*, 1985) standardises, and the form written here is the one
//! of Sawaryn & Thorogood, *A Compendium of Directional Calculations Based on
//! the Minimum Curvature Method* (SPE 84246, SPE Drilling & Completion 20(1),
//! 2005). Between stations 1 and 2, ΔMD apart, with inclinations `I` and
//! azimuths `A`:
//!
//! ```text
//! cos β = cos(I2 − I1) − sin I1 · sin I2 · (1 − cos(A2 − A1))     the dogleg
//! RF    = (2 / β) · tan(β / 2)                                     1 when β = 0
//! ΔN    = ΔMD / 2 · (sin I1 cos A1 + sin I2 cos A2) · RF
//! ΔE    = ΔMD / 2 · (sin I1 sin A1 + sin I2 sin A2) · RF
//! ΔTVD  = ΔMD / 2 · (cos I1 + cos I2) · RF
//! ```
//!
//! In vectors - `t = (sin I cos A, sin I sin A, cos I)`, north, east, down -
//! the dogleg is the angle between `t1` and `t2`, and `Δp = ΔMD / 2 · RF ·
//! (t1 + t2)`: the chord of the arc, **exact** on it, not an approximation of
//! it. The direction part of the way along an arc is the one the same paper
//! interpolates, `t(θ) = [sin(β − θ) · t1 + sin θ · t2] / sin β`.
//!
//! # What a plan says, and what this decides
//!
//! - **a hold** goes straight on in the direction the hole has - for a new
//!   well, straight down: a well starts vertical;
//! - **a curve** turns the hole at a *dogleg severity* in degrees per 30 m of
//!   measured depth ([`RATE_LENGTH`]) until it points at the inclination and
//!   azimuth said. One arc, so its length is the dogleg over the rate - **the
//!   MD at which the direction is reached is computed, never asked**, and the
//!   station at the end of the arc carries the direction said, exactly;
//! - **a build raises the inclination and a drop lowers it** - the words are
//!   checked, because a *build* that lowers is a plan misread somewhere;
//! - **every station is on the arc**: each is computed from its section's
//!   start by the formula above, never by summing chords from station to
//!   station, so nothing drifts along a long section;
//! - **a measured depth given is kept**: `hold to 120` ends at `120.0`, the
//!   number written, not a sum that comes back `119.99999999999999` (the rule
//!   that a given MD is never recomputed).
//!
//! # Refused by name
//!
//! A length that is not a positive number, a `hold to` that does not come
//! after where the hole is, a rate that is not a positive number, an
//! inclination outside [0°, 180°], an azimuth outside [0°, 360°], a build that
//! lowers or a drop that raises, a curve to the direction the hole already has
//! (it has no length), a turn back on itself (a dogleg of 180° lies in no one
//! plane), a step that is not a positive number, and more stations than
//! [`MOST_STATIONS`].

use std::f64::consts::PI;

/// The length a rate is counted over: a dogleg severity is in **degrees per
/// 30 m** of measured depth - the metric convention used here (a
/// build of about 13-17°/30 m).
pub const RATE_LENGTH: f64 = 30.0;

/// The most stations one plan may place. A step of a millimetre along a
/// kilometre of hole is a mistake in the step, not a survey - refused with its
/// count rather than computed for a minute.
pub const MOST_STATIONS: usize = 1_000_000;

/// Below this dogleg, in radians, a curve has no length: the hole already
/// points where it is told to.
const NO_TURN: f64 = 1e-12;

/// What a curve section is called, and so what it must do to the inclination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Turn {
    /// Any change of direction - inclination, azimuth or both.
    Any,
    /// The inclination must rise.
    Build,
    /// The inclination must fall.
    Drop,
}

/// One section of a plan, down the hole.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Section {
    /// Straight on in the direction the hole has, for this length of MD.
    Hold {
        /// In metres of measured depth, > 0.
        length: f64,
    },
    /// Straight on in the direction the hole has, down to this MD.
    HoldTo {
        /// The measured depth the section ends at - kept as given.
        md: f64,
    },
    /// One circular arc, at `rate` degrees per 30 m, until the hole points at
    /// `inclination` and `azimuth`.
    Curve {
        /// The dogleg severity, degrees per [`RATE_LENGTH`] metres, > 0.
        rate: f64,
        /// The inclination reached, degrees from vertical down: 0 down, 90
        /// horizontal, 180 up.
        inclination: f64,
        /// The azimuth reached, degrees clockwise from the frame's north.
        azimuth: f64,
        /// What the section was called - checked against what it does.
        turn: Turn,
    },
}

/// Where a plan starts: its measured depth and the direction of the hole
/// there. A new well starts at MD 0, vertical; a sidetrack at its kick-off,
/// in the direction its parent has there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tie {
    /// The measured depth of the first station.
    pub md: f64,
    /// Degrees from vertical down.
    pub inclination: f64,
    /// Degrees clockwise from north - meaningless, and kept, when vertical.
    pub azimuth: f64,
}

impl Tie {
    /// A new well: MD 0, straight down.
    pub fn vertical() -> Tie {
        Tie {
            md: 0.0,
            inclination: 0.0,
            azimuth: 0.0,
        }
    }
}

/// One station: where the hole is at a measured depth, and which way it points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Station {
    /// Measured depth.
    pub md: f64,
    /// Degrees from vertical down.
    pub inclination: f64,
    /// Degrees clockwise from north, in [0, 360). Carried from the station
    /// before where the hole is vertical, which has none.
    pub azimuth: f64,
    /// North of where the plan starts.
    pub north: f64,
    /// East of where the plan starts.
    pub east: f64,
    /// Vertical depth below where the plan starts - positive down.
    pub tvd: f64,
}

/// A plan laid out: its stations, and which of them ends each section.
#[derive(Debug, Clone, PartialEq)]
pub struct Planned {
    /// The stations, the tie first, in increasing measured depth.
    pub stations: Vec<Station>,
    /// For each section, the index of the station it ends at.
    pub ends: Vec<usize>,
    /// The radius of the tightest arc of the plan - `None` when it has none.
    pub tightest: Option<f64>,
}

/// What a plan cannot be, by name.
#[derive(Debug, Clone, thiserror::Error, PartialEq)]
pub enum PlanError {
    #[error("the station step must be a finite number of metres > 0, not {given}")]
    BadStep { given: f64 },
    #[error("a plan has one section at least")]
    NoSection,
    #[error(
        "the plan starts nowhere: MD {md}, inclination {inclination}°, azimuth {azimuth}° - an MD and an azimuth are finite numbers, an inclination is in [0°, 180°]"
    )]
    BadTie { md: f64, inclination: f64, azimuth: f64 },
    #[error("section {section}: a length is a finite number of metres > 0, not {given}")]
    BadLength { section: usize, given: f64 },
    #[error(
        "section {section}: `hold to {md}` does not come after MD {at}, where the hole already is - the sections run down the hole, in order"
    )]
    NotAfter { section: usize, md: f64, at: f64 },
    #[error("section {section}: a rate is a finite number of degrees per 30 m > 0, not {given}")]
    BadRate { section: usize, given: f64 },
    #[error(
        "section {section}: an inclination is in [0°, 180°] - 0 down, 90 horizontal, 180 up - not {given}"
    )]
    BadInclination { section: usize, given: f64 },
    #[error("section {section}: an azimuth is in [0°, 360°], not {given}")]
    BadAzimuth { section: usize, given: f64 },
    #[error(
        "section {section}: a build raises the inclination, and this one goes from {from}° to {to}° - call it a drop, or a curve"
    )]
    NotABuild { section: usize, from: f64, to: f64 },
    #[error(
        "section {section}: a drop lowers the inclination, and this one goes from {from}° to {to}° - call it a build, or a curve"
    )]
    NotADrop { section: usize, from: f64, to: f64 },
    #[error(
        "section {section}: the hole already points at inclination {inclination}°, azimuth {azimuth}° - a curve to where it points has no length"
    )]
    NoTurn {
        section: usize,
        inclination: f64,
        azimuth: f64,
    },
    #[error(
        "section {section}: the curve turns the hole back on itself, a dogleg of 180° - no single arc does that, the plane it would turn in is undefined"
    )]
    Reversal { section: usize },
    #[error(
        "the plan would place {count} stations, more than the {most} one plan may hold - take a larger step"
    )]
    TooManyStations { count: usize, most: usize },
}

impl PlanError {
    /// The section a refusal is about, counted from 1 - `None` when it is
    /// about the whole plan (its step, its start, its size).
    pub fn section(&self) -> Option<usize> {
        match self {
            PlanError::BadLength { section, .. }
            | PlanError::NotAfter { section, .. }
            | PlanError::BadRate { section, .. }
            | PlanError::BadInclination { section, .. }
            | PlanError::BadAzimuth { section, .. }
            | PlanError::NotABuild { section, .. }
            | PlanError::NotADrop { section, .. }
            | PlanError::NoTurn { section, .. }
            | PlanError::Reversal { section } => Some(*section),
            PlanError::BadStep { .. }
            | PlanError::NoSection
            | PlanError::BadTie { .. }
            | PlanError::TooManyStations { .. } => None,
        }
    }
}

type Vector = [f64; 3];

fn add(a: Vector, b: Vector) -> Vector {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn mul(a: Vector, k: f64) -> Vector {
    [a[0] * k, a[1] * k, a[2] * k]
}

/// The three pieces of the minimum curvature method, **from
/// `energyml_geometry::min_curvature`** - reading a surveyed hole is reading a
/// shape, and this crate sits above geometry, never inside it.
pub use energyml_geometry::min_curvature::{direction, dogleg, ratio_factor};

/// The inclination and azimuth of a unit direction, in degrees - `carried` as
/// the azimuth when the direction is vertical and has none.
pub fn angles(t: [f64; 3], carried: f64) -> (f64, f64) {
    let inclination = t[2].clamp(-1.0, 1.0).acos().to_degrees();
    let horizontal = t[0].hypot(t[1]);
    let azimuth = if horizontal > 1e-12 {
        t[1].atan2(t[0]).to_degrees().rem_euclid(360.0)
    } else {
        carried
    };
    (inclination, azimuth)
}

/// The radius of an arc at `rate` degrees per 30 m.
pub fn radius(rate: f64) -> f64 {
    RATE_LENGTH / rate.to_radians()
}

/// How far an arc of this radius strays from the chord joining two of its
/// points `spacing` apart along it - the sagitta, `R (1 − cos(s / 2R))`. What
/// a straight line between two stations misses of the arc they sit on.
pub fn sag(radius: f64, spacing: f64) -> f64 {
    radius * (1.0 - (spacing / (2.0 * radius)).cos())
}

/// One section once checked: where it starts and ends, and how it bends.
enum Laid {
    Straight {
        from: f64,
        to: f64,
        t: Vector,
        inclination: f64,
        azimuth: f64,
    },
    Arc {
        from: f64,
        to: f64,
        t1: Vector,
        t2: Vector,
        beta: f64,
        per_metre: f64,
        /// The azimuth the arc starts with - carried while it is vertical.
        start_azimuth: f64,
        /// The direction it was told to reach, as said.
        end: (f64, f64),
    },
}

impl Laid {
    fn span(&self) -> (f64, f64) {
        match self {
            Laid::Straight { from, to, .. } | Laid::Arc { from, to, .. } => (*from, *to),
        }
    }
}

/// The grid measured depths strictly between `from` and `to`: the multiples
/// of `step`, each computed as `k · step` - never accumulated, so the
/// thousandth is where the first would put it.
fn grid_between(from: f64, to: f64, step: f64) -> impl Iterator<Item = f64> {
    let slack = step * 1e-9;
    let first = ((from + slack) / step).floor() as i64 + 1;
    let last = ((to - slack) / step).ceil() as i64 - 1;
    (first..=last)
        .map(move |k| k as f64 * step)
        .filter(move |md| *md > from + slack && *md < to - slack)
}

/// How many grid depths `grid_between` would give, without giving them.
fn grid_count(from: f64, to: f64, step: f64) -> usize {
    let slack = step * 1e-9;
    let first = ((from + slack) / step).floor() + 1.0;
    let last = ((to - slack) / step).ceil() - 1.0;
    if last >= first {
        (last - first + 1.0) as usize
    } else {
        0
    }
}

fn station(md: f64, p: Vector, inclination: f64, azimuth: f64) -> Station {
    Station {
        md,
        inclination,
        azimuth,
        north: p[0],
        east: p[1],
        tvd: p[2],
    }
}

/// The plan laid out from `tie`, a station every `step` metres of MD - the
/// multiples of `step` - and one at each section's end.
pub fn plan(tie: Tie, sections: &[Section], step: f64) -> Result<Planned, PlanError> {
    if !(step.is_finite() && step > 0.0) {
        return Err(PlanError::BadStep { given: step });
    }
    if sections.is_empty() {
        return Err(PlanError::NoSection);
    }
    if !(tie.md.is_finite() && tie.azimuth.is_finite() && (0.0..=180.0).contains(&tie.inclination)) {
        return Err(PlanError::BadTie {
            md: tie.md,
            inclination: tie.inclination,
            azimuth: tie.azimuth,
        });
    }

    // -- every section checked and measured, before a station is placed -----
    let mut laid = Vec::with_capacity(sections.len());
    let mut md = tie.md;
    let (mut inclination, mut azimuth) = (tie.inclination, tie.azimuth.rem_euclid(360.0));
    let mut t = direction(inclination, azimuth);
    let mut tightest: Option<f64> = None;
    for (at, section) in sections.iter().enumerate() {
        let number = at + 1;
        match *section {
            Section::Hold { length } => {
                if !(length.is_finite() && length > 0.0) {
                    return Err(PlanError::BadLength {
                        section: number,
                        given: length,
                    });
                }
                laid.push(Laid::Straight {
                    from: md,
                    to: md + length,
                    t,
                    inclination,
                    azimuth,
                });
                md += length;
            }
            Section::HoldTo { md: to } => {
                if !to.is_finite() || to <= md {
                    return Err(PlanError::NotAfter {
                        section: number,
                        md: to,
                        at: md,
                    });
                }
                laid.push(Laid::Straight {
                    from: md,
                    to,
                    t,
                    inclination,
                    azimuth,
                });
                // **The depth said, not a sum**: the next section starts at
                // exactly the number written.
                md = to;
            }
            Section::Curve {
                rate,
                inclination: to_inclination,
                azimuth: to_azimuth,
                turn,
            } => {
                if !(rate.is_finite() && rate > 0.0) {
                    return Err(PlanError::BadRate {
                        section: number,
                        given: rate,
                    });
                }
                if !(to_inclination.is_finite() && (0.0..=180.0).contains(&to_inclination)) {
                    return Err(PlanError::BadInclination {
                        section: number,
                        given: to_inclination,
                    });
                }
                if !(to_azimuth.is_finite() && (0.0..=360.0).contains(&to_azimuth)) {
                    return Err(PlanError::BadAzimuth {
                        section: number,
                        given: to_azimuth,
                    });
                }
                match turn {
                    Turn::Build if to_inclination <= inclination => {
                        return Err(PlanError::NotABuild {
                            section: number,
                            from: inclination,
                            to: to_inclination,
                        });
                    }
                    Turn::Drop if to_inclination >= inclination => {
                        return Err(PlanError::NotADrop {
                            section: number,
                            from: inclination,
                            to: to_inclination,
                        });
                    }
                    _ => {}
                }
                let t2 = direction(to_inclination, to_azimuth);
                let beta = dogleg(t, t2);
                if beta < NO_TURN {
                    return Err(PlanError::NoTurn {
                        section: number,
                        inclination,
                        azimuth,
                    });
                }
                if PI - beta < 1e-9 {
                    return Err(PlanError::Reversal { section: number });
                }
                // Radians of turn per metre of MD; the arc's length is the
                // dogleg over it - the MD where the direction is reached.
                let per_metre = rate.to_radians() / RATE_LENGTH;
                let length = beta / per_metre;
                let r = radius(rate);
                tightest = Some(tightest.map_or(r, |seen: f64| seen.min(r)));
                let end = (to_inclination, to_azimuth.rem_euclid(360.0));
                laid.push(Laid::Arc {
                    from: md,
                    to: md + length,
                    t1: t,
                    t2,
                    beta,
                    per_metre,
                    start_azimuth: azimuth,
                    end,
                });
                md += length;
                t = t2;
                (inclination, azimuth) = end;
            }
        }
    }

    // -- how many, before any is made ----------------------------------------
    let count = 1 + laid
        .iter()
        .map(|one| {
            let (from, to) = one.span();
            grid_count(from, to, step) + 1
        })
        .sum::<usize>();
    if count > MOST_STATIONS {
        return Err(PlanError::TooManyStations {
            count,
            most: MOST_STATIONS,
        });
    }

    // -- the stations ---------------------------------------------------------
    let mut stations = Vec::with_capacity(count);
    let mut ends = Vec::with_capacity(laid.len());
    let mut at: Vector = [0.0, 0.0, 0.0];
    stations.push(station(
        tie.md,
        at,
        tie.inclination,
        tie.azimuth.rem_euclid(360.0),
    ));
    for one in &laid {
        match *one {
            Laid::Straight {
                from,
                to,
                t,
                inclination,
                azimuth,
            } => {
                let p0 = at;
                for md in grid_between(from, to, step) {
                    stations.push(station(md, add(p0, mul(t, md - from)), inclination, azimuth));
                }
                at = add(p0, mul(t, to - from));
                stations.push(station(to, at, inclination, azimuth));
            }
            Laid::Arc {
                from,
                to,
                t1,
                t2,
                beta,
                per_metre,
                start_azimuth,
                end,
            } => {
                let p0 = at;
                let sin_beta = beta.sin();
                let mut carried = start_azimuth;
                for md in grid_between(from, to, step) {
                    let s = md - from;
                    let theta = s * per_metre;
                    // The direction part of the way, then the minimum
                    // curvature chord from the section's start to here.
                    let t = add(
                        mul(t1, (beta - theta).sin() / sin_beta),
                        mul(t2, theta.sin() / sin_beta),
                    );
                    let p = add(p0, mul(add(t1, t), s / 2.0 * ratio_factor(theta)));
                    let (i, a) = angles(t, carried);
                    carried = a;
                    stations.push(station(md, p, i, a));
                }
                // The end: the textbook formula over the whole arc, and the
                // direction as it was said.
                at = add(p0, mul(add(t1, t2), (to - from) / 2.0 * ratio_factor(beta)));
                stations.push(station(to, at, end.0, end.1));
            }
        }
        ends.push(stations.len() - 1);
    }
    Ok(Planned {
        stations,
        ends,
        tightest,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Close enough for a hand computation carried to eight digits.
    fn near(a: f64, b: f64, tolerance: f64) -> bool {
        (a - b).abs() <= tolerance
    }

    fn last(planned: &Planned) -> Station {
        *planned.stations.last().expect("a station")
    }

    #[test]
    fn a_hold_from_a_new_well_goes_straight_down_and_keeps_its_stations_on_the_grid() {
        // To break: a first hold that is not vertical, or stations
        // accumulated rather than put on the multiples of the step.
        let planned = plan(Tie::vertical(), &[Section::HoldTo { md: 100.0 }], 10.0).unwrap();
        let mds: Vec<f64> = planned.stations.iter().map(|s| s.md).collect();
        assert_eq!(
            mds,
            vec![0.0, 10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0]
        );
        for station in &planned.stations {
            assert_eq!(station.north, 0.0);
            assert_eq!(station.east, 0.0);
            assert!(near(station.tvd, station.md, 1e-12), "{station:?}");
            assert_eq!(station.inclination, 0.0);
        }
        assert_eq!(planned.ends, vec![10]);
        assert_eq!(planned.tightest, None);
    }

    #[test]
    fn a_depth_given_is_the_depth_written_and_the_ends_are_stations() {
        // `hold to 43.7` ends at the number written - the grid goes on at the
        // multiples of the step on the other side of it.
        let planned = plan(
            Tie::vertical(),
            &[Section::HoldTo { md: 43.7 }, Section::HoldTo { md: 61.3 }],
            10.0,
        )
        .unwrap();
        let mds: Vec<f64> = planned.stations.iter().map(|s| s.md).collect();
        assert_eq!(mds, vec![0.0, 10.0, 20.0, 30.0, 40.0, 43.7, 50.0, 60.0, 61.3]);
        assert_eq!(planned.ends, vec![5, 8]);
    }

    #[test]
    fn a_quarter_turn_build_lands_where_the_textbook_formula_puts_it() {
        // Build from vertical to 90° toward north at 3°/30 m: 90° at 0.1°/m is
        // 900 m of hole. By hand, with the formula of the module (Sawaryn &
        // Thorogood, SPE 84246): I1 = 0, I2 = 90°, A1 = A2 = 0, so
        // cos β = cos 90° − 0 = 0 and β = π/2; RF = (2 / (π/2)) · tan(π/4)
        // = 4/π = 1.2732395447;
        // ΔN = 900/2 · (0 + 1·1) · RF = 450 · 1.2732395447 = 572.9577951;
        // ΔTVD = 900/2 · (1 + 0) · RF = 572.9577951; ΔE = 0.
        // The circle says the same: R = 30 / (3° in radians) = 572.9577951,
        // and a quarter turn from vertical ends R north of and R below its start.
        let planned = plan(
            Tie::vertical(),
            &[Section::Curve {
                rate: 3.0,
                inclination: 90.0,
                azimuth: 0.0,
                turn: Turn::Build,
            }],
            100.0,
        )
        .unwrap();
        let end = last(&planned);
        assert!(near(end.md, 900.0, 1e-9), "{end:?}");
        assert!(near(end.north, 572.957_795_1, 1e-6), "{end:?}");
        assert!(near(end.tvd, 572.957_795_1, 1e-6), "{end:?}");
        assert!(near(end.east, 0.0, 1e-9), "{end:?}");
        assert_eq!((end.inclination, end.azimuth), (90.0, 0.0));
        assert!(near(planned.tightest.unwrap(), 572.957_795_1, 1e-6));
        // A station inside the arc sits on the same circle: at MD 300 the
        // hole has turned 30°, so N = R (1 − cos 30°) = 572.9577951 ·
        // 0.1339745962 = 76.7617893, and TVD = R sin 30° = 286.4788976.
        let inside = planned
            .stations
            .iter()
            .find(|s| s.md == 300.0)
            .expect("a station at 300");
        assert!(near(inside.north, 76.761_789_3, 1e-6), "{inside:?}");
        assert!(near(inside.tvd, 286.478_897_6, 1e-6), "{inside:?}");
        assert!(near(inside.inclination, 30.0, 1e-9), "{inside:?}");
    }

    #[test]
    fn a_forty_five_degree_build_to_the_east_reaches_its_inclination_at_its_md() {
        // Kick-off at 40, build 15°/30 m to 45° azimuth 90: β = 45° = π/4,
        // the arc is 45 / 15 · 30 = 90 m long, so the direction is reached at
        // MD 130. By hand: RF = (2 / (π/4)) tan(π/8) = (8/π) · 0.4142135624
        // = 1.0547862; ΔE = 90/2 · (0 + sin 45° · sin 90°) · RF = 45 ·
        // 0.7071067812 · 1.0547862 = 33.5630906; ΔTVD = 45 · (1 + cos 45°) ·
        // RF = 45 · 1.7071067812 · 1.0547862 = 81.0284685.
        let planned = plan(
            Tie::vertical(),
            &[
                Section::HoldTo { md: 40.0 },
                Section::Curve {
                    rate: 15.0,
                    inclination: 45.0,
                    azimuth: 90.0,
                    turn: Turn::Build,
                },
                Section::Hold { length: 50.0 },
            ],
            10.0,
        )
        .unwrap();
        let end_of_build = planned.stations[planned.ends[1]];
        assert!(near(end_of_build.md, 130.0, 1e-9), "{end_of_build:?}");
        assert_eq!(end_of_build.inclination, 45.0);
        assert!(near(end_of_build.east, 33.563_090_6, 1e-6), "{end_of_build:?}");
        assert!(
            near(end_of_build.tvd, 40.0 + 81.028_468_5, 1e-6),
            "{end_of_build:?}"
        );
        assert!(near(end_of_build.north, 0.0, 1e-9), "{end_of_build:?}");
        // The hold after it goes on at 45°: as far east as down.
        let end = last(&planned);
        assert!(near(end.md, 180.0, 1e-9));
        assert!(
            near(end.east - end_of_build.east, 50.0 * 0.5f64.sqrt(), 1e-9),
            "{end:?}"
        );
        assert!(
            near(end.tvd - end_of_build.tvd, 50.0 * 0.5f64.sqrt(), 1e-9),
            "{end:?}"
        );
    }

    #[test]
    fn a_turn_in_the_horizontal_keeps_its_depth() {
        // Horizontal, azimuth 0 → 90 at 3°/30 m: the dogleg formula gives
        // cos β = cos 0 − sin 90° sin 90° (1 − cos 90°) = 1 − 1 = 0, β = 90°;
        // RF = 4/π; ΔN = 450 · (1 + 0) · RF = R, ΔE = 450 · (0 + 1) · RF = R,
        // ΔTVD = 450 · (0 + 0) · RF = 0.
        let tie = Tie {
            md: 1000.0,
            inclination: 90.0,
            azimuth: 0.0,
        };
        let planned = plan(
            tie,
            &[Section::Curve {
                rate: 3.0,
                inclination: 90.0,
                azimuth: 90.0,
                turn: Turn::Any,
            }],
            50.0,
        )
        .unwrap();
        let end = last(&planned);
        assert!(near(end.md, 1900.0, 1e-9), "{end:?}");
        assert!(near(end.north, 572.957_795_1, 1e-6), "{end:?}");
        assert!(near(end.east, 572.957_795_1, 1e-6), "{end:?}");
        assert!(near(end.tvd, 0.0, 1e-9), "{end:?}");
        for station in &planned.stations {
            assert!(near(station.tvd, 0.0, 1e-9), "{station:?}");
        }
    }

    #[test]
    fn a_build_then_a_drop_back_to_vertical_is_an_s() {
        // Build 0 → 30° then drop 30° → 0 at the same rate: two arcs of the
        // same radius, so the hole ends vertical, 2 R (1 − cos 30°) away and
        // 2 R sin 30° down.
        let rate = 10.0;
        let r = radius(rate);
        let planned = plan(
            Tie::vertical(),
            &[
                Section::Curve {
                    rate,
                    inclination: 30.0,
                    azimuth: 0.0,
                    turn: Turn::Build,
                },
                Section::Curve {
                    rate,
                    inclination: 0.0,
                    azimuth: 0.0,
                    turn: Turn::Drop,
                },
            ],
            5.0,
        )
        .unwrap();
        let end = last(&planned);
        assert!(
            near(end.md, 180.0, 1e-9),
            "30° at 10°/30 m is 90 m, twice: {end:?}"
        );
        assert!(
            near(end.north, 2.0 * r * (1.0 - 30f64.to_radians().cos()), 1e-9),
            "{end:?}"
        );
        assert!(near(end.tvd, 2.0 * r * 30f64.to_radians().sin(), 1e-9), "{end:?}");
        assert_eq!(end.inclination, 0.0);
    }

    #[test]
    fn the_ratio_factor_and_the_sag_are_the_textbook_ones() {
        // RF(π/2) = 4/π; RF → 1 as β → 0, the series and the quotient meeting.
        assert!(near(ratio_factor(PI / 2.0), 4.0 / PI, 1e-15));
        assert!(near(ratio_factor(1e-7), 1.0, 1e-13));
        assert!(near(ratio_factor(2e-6), 2.0 / 2e-6 * (1e-6f64).tan(), 1e-12));
        // A 10 m chord on the 114.59 m radius of 15°/30 m sags
        // R (1 − cos(5 / 114.59)) = 0.1090658 m.
        assert!(
            near(sag(radius(15.0), 10.0), 0.109_065_8, 1e-6),
            "{}",
            sag(radius(15.0), 10.0)
        );
    }

    #[test]
    fn impossible_plans_are_refused_by_name() {
        let v = Tie::vertical();
        assert_eq!(
            plan(v, &[Section::Hold { length: -5.0 }], 10.0),
            Err(PlanError::BadLength {
                section: 1,
                given: -5.0
            })
        );
        assert_eq!(
            plan(
                v,
                &[Section::HoldTo { md: 50.0 }, Section::HoldTo { md: 40.0 }],
                10.0
            ),
            Err(PlanError::NotAfter {
                section: 2,
                md: 40.0,
                at: 50.0
            })
        );
        let curve = |rate, inclination, azimuth, turn| Section::Curve {
            rate,
            inclination,
            azimuth,
            turn,
        };
        assert_eq!(
            plan(
                v,
                &[Section::HoldTo { md: 30.0 }, curve(0.0, 30.0, 0.0, Turn::Build)],
                10.0
            ),
            Err(PlanError::BadRate {
                section: 2,
                given: 0.0
            })
        );
        assert_eq!(
            plan(v, &[curve(10.0, 190.0, 0.0, Turn::Any)], 10.0),
            Err(PlanError::BadInclination {
                section: 1,
                given: 190.0
            })
        );
        assert_eq!(
            plan(v, &[curve(10.0, 30.0, 400.0, Turn::Any)], 10.0),
            Err(PlanError::BadAzimuth {
                section: 1,
                given: 400.0
            })
        );
        assert_eq!(
            plan(
                v,
                &[
                    curve(10.0, 40.0, 0.0, Turn::Build),
                    curve(10.0, 50.0, 0.0, Turn::Drop)
                ],
                10.0
            ),
            Err(PlanError::NotADrop {
                section: 2,
                from: 40.0,
                to: 50.0
            })
        );
        assert_eq!(
            plan(
                v,
                &[
                    curve(10.0, 40.0, 0.0, Turn::Build),
                    curve(10.0, 20.0, 0.0, Turn::Build)
                ],
                10.0
            ),
            Err(PlanError::NotABuild {
                section: 2,
                from: 40.0,
                to: 20.0
            })
        );
        assert_eq!(
            plan(v, &[curve(10.0, 0.0, 90.0, Turn::Any)], 10.0),
            Err(PlanError::NoTurn {
                section: 1,
                inclination: 0.0,
                azimuth: 0.0
            })
        );
        assert_eq!(
            plan(v, &[curve(10.0, 180.0, 0.0, Turn::Any)], 10.0),
            Err(PlanError::Reversal { section: 1 })
        );
        assert_eq!(
            plan(v, &[Section::Hold { length: 10.0 }], 0.0),
            Err(PlanError::BadStep { given: 0.0 })
        );
        assert_eq!(plan(v, &[], 10.0), Err(PlanError::NoSection));
        assert!(matches!(
            plan(v, &[Section::HoldTo { md: 2000.0 }], 1e-3),
            Err(PlanError::TooManyStations { count, .. }) if count > MOST_STATIONS
        ));
    }

    #[test]
    fn a_sidetrack_starts_at_its_tie_and_turns_from_its_direction() {
        // From MD 60, inclination 0 (a vertical parent), build to 45° west at
        // 15°/30 m: the first station is the tie, at the origin, and the arc
        // ends 90 m of MD later, R (1 − cos 45°) = 33.5630906 west and
        // R sin 45° = 81.0284685 down.
        let tie = Tie {
            md: 60.0,
            inclination: 0.0,
            azimuth: 0.0,
        };
        let planned = plan(
            tie,
            &[Section::Curve {
                rate: 15.0,
                inclination: 45.0,
                azimuth: 270.0,
                turn: Turn::Build,
            }],
            10.0,
        )
        .unwrap();
        assert_eq!(planned.stations[0].md, 60.0);
        assert_eq!(
            (
                planned.stations[0].north,
                planned.stations[0].east,
                planned.stations[0].tvd
            ),
            (0.0, 0.0, 0.0)
        );
        assert_eq!(
            planned.stations[1].md, 70.0,
            "the grid is the multiples of the step, not the tie plus the step"
        );
        let end = last(&planned);
        assert!(near(end.md, 150.0, 1e-9), "{end:?}");
        assert!(near(end.east, -33.563_090_6, 1e-6), "{end:?}");
        assert!(near(end.tvd, 81.028_468_5, 1e-6), "{end:?}");
    }
}
