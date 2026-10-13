//! What a **law of depth** gives along a wellbore: a temperature rising with a
//! geothermal gradient, a hydrostatic pressure below a water table - one value
//! per node, from the node's vertical depth below the ground.
//!
//! A step of the build plan of the synthetic model (`well-logs`: pressure,
//! temperature, and logs derived from lithology), and the two requirements
//! that a law can give:
//!
//! > pressure - hydrostatic below the top of the water table (~1 bar per 10 m
//! > of water), marked synthetic
//! >
//! > temperature - ~12-13 °C at about 20 m, then a gradient of a few °C per
//! > 100 m - order of magnitude only, **source to be found**
//!
//! TODO(reference): verify - the geothermal gradient and the hydrostatic law carry no published source (the requirement itself says the source is still to be found).
//!
//! # One depth, and only one
//!
//! Every law here is a function of **the vertical depth below the ground**, in
//! metres - not the measured depth, which runs along the hole and is longer as
//! soon as the hole deviates, and not the depth below the datum, which stands
//! above the ground on a rig. The caller computes it from what the package
//! states (a node's elevation, the ground under the head); this module never
//! sees a position.
//!
//! # Where a law holds, and where it says nothing
//!
//! - **above the ground** no law of the subsurface holds: a node there - the
//!   datum on a kelly bushing, the first metres of a hole measured from it -
//!   gets NaN, and is counted (`above_ground`);
//! - **above where the law starts** - the depth its temperature is stated at,
//!   the water table - the law says nothing either: NaN, counted apart
//!   (`shallow`). Extrapolating a gradient up into the zone of seasonal swings,
//!   or a column of water above the water, would write values nobody stated.
//!
//! The two counts are two findings - *the hole starts in the air* and *the law
//! starts deeper* - and are never added up here.
//!
//! # A node cannot lie farther from its datum than its measured depth
//!
//! [`beyond_reach`] is the one check this module offers a caller about the
//! positions it computed its depths from: a measured depth is a length along
//! the hole from the datum, and a straight line is the shortest path, so a node
//! farther from the datum than its MD is not where the datum and the depths put
//! it. It is a **necessary** condition, not a proof - a frame offset by less
//! than its depths passes - and its caller says so.

use std::fmt;

/// A node within this of the ground, or of where a law starts, is taken on it:
/// a micrometre - what a difference of two elevations drifts by, nothing a
/// producer means.
const SLACK: f64 = 1e-6;

/// A coefficient of a law, for a refusal to name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Coefficient {
    /// The temperature at the reference depth.
    Temperature,
    /// The reference depth of a temperature.
    From,
    /// The geothermal gradient.
    Gradient,
    /// The depth of the water table below the ground.
    Table,
    /// The density of the water.
    Density,
    /// The acceleration of gravity.
    Gravity,
    /// The pressure at the water table.
    AtTable,
}

impl Coefficient {
    /// The coefficient in words.
    pub fn words(self) -> &'static str {
        match self {
            Coefficient::Temperature => "temperature at the reference depth",
            Coefficient::From => "reference depth",
            Coefficient::Gradient => "gradient",
            Coefficient::Table => "depth of the water table",
            Coefficient::Density => "density of the water",
            Coefficient::Gravity => "acceleration of gravity",
            Coefficient::AtTable => "pressure at the water table",
        }
    }
}

impl fmt::Display for Coefficient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.words())
    }
}

/// What this module refuses, by name.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum LogError {
    #[error("the {which} of the law must be a finite number - {given} given")]
    NotFinite { which: Coefficient, given: f64 },
    #[error("the {which} must be greater than 0 - {given} given: a column of water would weigh nothing")]
    NotPositive { which: Coefficient, given: f64 },
    #[error(
        "the {which} must be 0 or more - {given} given: a law of the subsurface starts at the ground or below it"
    )]
    Negative { which: Coefficient, given: f64 },
    #[error(
        "the depth of node {at} is not a finite number ({given}): a node with no depth has no value to give, and is no hole of the log"
    )]
    BadDepth { at: usize, given: f64 },
}

/// A law of the vertical depth below the ground, in metres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Law {
    /// `at` °C at `from` metres below the ground, and `gradient` °C more per
    /// metre deeper - a geothermal gradient, in °C/m.
    Geothermal {
        /// The temperature at `from`, in °C.
        at: f64,
        /// Where it is stated, in metres below the ground - 0 or more.
        from: f64,
        /// °C per metre of vertical depth.
        gradient: f64,
    },
    /// `at_table` pascals at the water table, `table` metres below the ground -
    /// negative when the water stands above it - and `density × gravity`
    /// pascals more per metre deeper.
    Hydrostatic {
        /// The water table, in metres below the ground.
        table: f64,
        /// kg/m³.
        density: f64,
        /// m/s².
        gravity: f64,
        /// The pressure at the water table, in pascals - 0 for a gauge pressure.
        at_table: f64,
    },
}

impl Law {
    /// Whether the law can be laid - every coefficient finite, a reference
    /// depth at or below the ground, a column of water that weighs something.
    pub fn check(&self) -> Result<(), LogError> {
        let finite = |which: Coefficient, given: f64| {
            if given.is_finite() {
                Ok(())
            } else {
                Err(LogError::NotFinite { which, given })
            }
        };
        match *self {
            Law::Geothermal { at, from, gradient } => {
                finite(Coefficient::Temperature, at)?;
                finite(Coefficient::From, from)?;
                finite(Coefficient::Gradient, gradient)?;
                if from < 0.0 {
                    return Err(LogError::Negative {
                        which: Coefficient::From,
                        given: from,
                    });
                }
            }
            Law::Hydrostatic {
                table,
                density,
                gravity,
                at_table,
            } => {
                finite(Coefficient::Table, table)?;
                finite(Coefficient::Density, density)?;
                finite(Coefficient::Gravity, gravity)?;
                finite(Coefficient::AtTable, at_table)?;
                for (which, given) in [(Coefficient::Density, density), (Coefficient::Gravity, gravity)] {
                    if given <= 0.0 {
                        return Err(LogError::NotPositive { which, given });
                    }
                }
            }
        }
        Ok(())
    }

    /// Where the law starts, in metres below the ground - never above it: a
    /// water table above the ground (artesian water) starts the law at the
    /// ground, since the air above it is no subsurface.
    pub fn start(&self) -> f64 {
        match *self {
            Law::Geothermal { from, .. } => from,
            Law::Hydrostatic { table, .. } => table.max(0.0),
        }
    }

    /// The value at `depth` metres below the ground, **without** asking whether
    /// the law holds there - [`lay`] asks. A depth a micrometre short of the
    /// start is read on it.
    fn value(&self, depth: f64) -> f64 {
        match *self {
            Law::Geothermal { at, from, gradient } => at + gradient * (depth - from).max(0.0),
            Law::Hydrostatic {
                table,
                density,
                gravity,
                at_table,
            } => at_table + density * gravity * (depth - table).max(0.0),
        }
    }
}

/// What a law gives along a hole: a value per depth, and what it left empty.
#[derive(Clone, Debug, PartialEq)]
pub struct Logged {
    /// One per depth, in their order; NaN where the law says nothing.
    pub values: Vec<f64>,
    /// Depths above the ground - no law of the subsurface holds there.
    pub above_ground: usize,
    /// Depths below the ground and above where the law starts.
    pub shallow: usize,
    /// The smallest and the largest value laid, `None` when none was.
    pub range: Option<[f64; 2]>,
}

impl Logged {
    /// How many values were laid - the depths the law holds at.
    pub fn laid(&self) -> usize {
        self.values.len() - self.above_ground - self.shallow
    }
}

/// The law at each depth, in metres below the ground - **exactly one value per
/// depth**, NaN where the law says nothing, and the two counts of why.
///
/// A depth that is not a finite number is refused by its rank: it would be a
/// NaN written as if the law had been asked and had said nothing.
pub fn lay(law: &Law, depths: &[f64]) -> Result<Logged, LogError> {
    law.check()?;
    if let Some(at) = depths.iter().position(|depth| !depth.is_finite()) {
        return Err(LogError::BadDepth {
            at,
            given: depths[at],
        });
    }
    let start = law.start();
    let mut values = Vec::with_capacity(depths.len());
    let (mut above_ground, mut shallow) = (0usize, 0usize);
    let mut range: Option<[f64; 2]> = None;
    for &depth in depths {
        if depth < -SLACK {
            above_ground += 1;
            values.push(f64::NAN);
        } else if depth < start - SLACK {
            shallow += 1;
            values.push(f64::NAN);
        } else {
            let value = law.value(depth);
            range = Some(match range {
                None => [value, value],
                Some([low, high]) => [low.min(value), high.max(value)],
            });
            values.push(value);
        }
    }
    Ok(Logged {
        values,
        above_ground,
        shallow,
        range,
    })
}

/// A node farther from the datum than its measured depth allows - see
/// [`beyond_reach`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Beyond {
    /// Its rank among the nodes.
    pub node: usize,
    /// How far it is from the datum, in the unit of the coordinates.
    pub distance: f64,
    /// Its measured depth.
    pub md: f64,
}

/// The first node that lies farther from `datum` than a hole of its measured
/// depth can reach - or `None` when every node is within reach.
///
/// `datum`, `nodes` and `mds` are in **one** length unit, and `nodes` pairs
/// with `mds` rank for rank (the caller checks both counts first; extra
/// entries of the longer list are not looked at). The slack is a millimetre
/// and one part per million of the depth: a frame another producer rounded to
/// the millimetre passes, a frame placed in another frame of reference - its
/// local numbers read as placed ones, hundreds of metres or kilometres off -
/// does not. A distance that is not a number is beyond reach too.
pub fn beyond_reach(datum: [f64; 3], nodes: &[[f64; 3]], mds: &[f64]) -> Option<Beyond> {
    nodes
        .iter()
        .zip(mds)
        .enumerate()
        .find_map(|(node, (point, &md))| {
            let distance = ((point[0] - datum[0]).powi(2)
                + (point[1] - datum[1]).powi(2)
                + (point[2] - datum[2]).powi(2))
            .sqrt();
            let reach = md.abs() * (1.0 + 1e-6) + 1e-3;
            // Written so that a NaN distance falls on the far side.
            if distance <= reach {
                None
            } else {
                Some(Beyond { node, distance, md })
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The requirement's own order of magnitude, stated in the test: 12.5 °C
    /// at 20 m, 3 °C per 100 m.
    fn geothermal() -> Law {
        Law::Geothermal {
            at: 12.5,
            from: 20.0,
            gradient: 0.03,
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-9 * (1.0 + b.abs())
    }

    #[test]
    fn a_gradient_rises_from_its_reference_depth_and_says_nothing_above_it() {
        let laid = lay(&geothermal(), &[-3.0, 0.0, 10.0, 20.0, 120.0, 220.0]).unwrap();
        assert!(laid.values[0].is_nan(), "in the air");
        assert!(
            laid.values[1].is_nan() && laid.values[2].is_nan(),
            "above 20 m the law is not stated"
        );
        assert!(close(laid.values[3], 12.5));
        assert!(
            close(laid.values[4], 15.5),
            "100 m below 20 m at 3 °C per 100 m: {}",
            laid.values[4]
        );
        assert!(close(laid.values[5], 18.5));
        assert_eq!((laid.above_ground, laid.shallow, laid.laid()), (1, 2, 3));
        let [low, high] = laid.range.unwrap();
        assert!(close(low, 12.5) && close(high, 18.5));
    }

    #[test]
    fn a_node_on_the_ground_or_on_the_start_is_read_there_and_a_hair_above_is_not() {
        // From the ground: a node exactly on it takes the surface value.
        let surface = Law::Geothermal {
            at: 11.0,
            from: 0.0,
            gradient: 0.03,
        };
        let laid = lay(&surface, &[0.0, -1e-9, -1e-3]).unwrap();
        assert!(close(laid.values[0], 11.0) && close(laid.values[1], 11.0));
        assert!(
            laid.values[2].is_nan(),
            "a millimetre above the ground is above it"
        );
        assert_eq!(laid.above_ground, 1);
    }

    #[test]
    fn a_column_of_water_weighs_density_times_gravity_per_metre() {
        // 1000 kg/m³ and the standard gravity: 98 066.5 Pa under 10 m of water,
        // the "~1 bar per 10 m of water" requirement.
        let law = Law::Hydrostatic {
            table: 5.0,
            density: 1000.0,
            gravity: 9.80665,
            at_table: 0.0,
        };
        let laid = lay(&law, &[2.0, 5.0, 15.0]).unwrap();
        assert!(laid.values[0].is_nan(), "above the water table");
        assert!(close(laid.values[1], 0.0));
        assert!(close(laid.values[2], 98_066.5), "{}", laid.values[2]);
        assert!(
            (laid.values[2] / 1e5 - 1.0).abs() < 0.05,
            "about a bar per ten metres"
        );
        assert_eq!((laid.above_ground, laid.shallow), (0, 1));

        // The pressure stated at the table is added everywhere below it.
        let absolute = Law::Hydrostatic {
            table: 5.0,
            density: 1000.0,
            gravity: 9.80665,
            at_table: 101_325.0,
        };
        assert!(close(
            lay(&absolute, &[15.0]).unwrap().values[0],
            98_066.5 + 101_325.0
        ));
    }

    #[test]
    fn water_standing_above_the_ground_fills_the_hole_from_the_ground_down() {
        // A water table 4 m above the ground: nothing is shallow, and the ground
        // is already under 4 m of water; the air above stays empty.
        let law = Law::Hydrostatic {
            table: -4.0,
            density: 1000.0,
            gravity: 10.0,
            at_table: 0.0,
        };
        let laid = lay(&law, &[-1.0, 0.0, 6.0]).unwrap();
        assert!(laid.values[0].is_nan());
        assert!(close(laid.values[1], 40_000.0));
        assert!(close(laid.values[2], 100_000.0));
        assert_eq!((laid.above_ground, laid.shallow), (1, 0));
    }

    #[test]
    fn a_law_that_cannot_be_laid_is_refused_by_its_coefficient() {
        let nan = Law::Geothermal {
            at: 12.0,
            from: 0.0,
            gradient: f64::NAN,
        };
        assert!(matches!(
            lay(&nan, &[1.0]),
            Err(LogError::NotFinite {
                which: Coefficient::Gradient,
                ..
            })
        ));
        let above = Law::Geothermal {
            at: 12.0,
            from: -1.0,
            gradient: 0.03,
        };
        assert!(matches!(
            lay(&above, &[1.0]),
            Err(LogError::Negative {
                which: Coefficient::From,
                ..
            })
        ));
        let weightless = Law::Hydrostatic {
            table: 0.0,
            density: 0.0,
            gravity: 9.8,
            at_table: 0.0,
        };
        assert!(matches!(
            lay(&weightless, &[1.0]),
            Err(LogError::NotPositive {
                which: Coefficient::Density,
                ..
            })
        ));
        let no_gravity = Law::Hydrostatic {
            table: 0.0,
            density: 1000.0,
            gravity: -9.8,
            at_table: 0.0,
        };
        assert!(matches!(
            lay(&no_gravity, &[1.0]),
            Err(LogError::NotPositive {
                which: Coefficient::Gravity,
                ..
            })
        ));
    }

    #[test]
    fn a_depth_that_is_no_number_is_refused_by_its_rank() {
        assert!(matches!(
            lay(&geothermal(), &[30.0, f64::NAN]),
            Err(LogError::BadDepth { at: 1, .. })
        ));
        assert!(matches!(
            lay(&geothermal(), &[f64::INFINITY]),
            Err(LogError::BadDepth { at: 0, .. })
        ));
    }

    #[test]
    fn exactly_one_value_per_depth() {
        let depths: Vec<f64> = (0..57).map(|at| at as f64 * 3.7 - 5.0).collect();
        let laid = lay(&geothermal(), &depths).unwrap();
        assert_eq!(laid.values.len(), depths.len());
        assert_eq!(
            laid.values.iter().filter(|v| v.is_nan()).count(),
            laid.above_ground + laid.shallow
        );
    }

    #[test]
    fn a_node_farther_from_its_datum_than_its_depth_is_found() {
        // Far from the origin, as a Lambert-93 frame is.
        let datum = [651_620.0, 6_861_170.0, 48.0];
        let vertical: Vec<[f64; 3]> = (0..5)
            .map(|k| [datum[0], datum[1], datum[2] - 10.0 * k as f64])
            .collect();
        let mds: Vec<f64> = (0..5).map(|k| 10.0 * k as f64).collect();
        assert_eq!(
            beyond_reach(datum, &vertical, &mds),
            None,
            "a vertical hole is exactly as far as its depth"
        );

        // The same nodes read in another frame: every one is kilometres off.
        let local: Vec<[f64; 3]> = vertical
            .iter()
            .map(|p| [p[0] - datum[0], p[1] - datum[1], p[2]])
            .collect();
        let found = beyond_reach(datum, &local, &mds).expect("off by the whole origin");
        assert_eq!(found.node, 0);
        assert!(found.distance > 6_000_000.0);

        // A deviated node, 15.8 m from the datum for 20 m of hole: within reach.
        // Then one pushed 5 m aside at 30 m straight down: 30.4 m from the datum
        // for 30 m of hole, which no hole reaches.
        let mut bent = vertical.clone();
        bent[2] = [datum[0] + 5.0, datum[1], datum[2] - 15.0];
        assert_eq!(beyond_reach(datum, &bent, &mds), None);
        bent[3][0] += 5.0;
        assert_eq!(beyond_reach(datum, &bent, &mds).map(|b| b.node), Some(3));

        let broken = vec![[f64::NAN, 0.0, 0.0]];
        assert_eq!(beyond_reach(datum, &broken, &[0.0]).map(|b| b.node), Some(0));
    }
}
