//! **A flow rate over time, and what it delivers between two instants** - the
//! synthetic production law of Lutetia's geothermal doublet
//! (`prodml-doublet`: the rates of the producer and the injector period by period,
//! and the injector's rate sampled at the minute).
//!
//! Nothing here reads a package or knows PRODML: the inputs are a rate, a
//! decline and hours, so that what can be wrong in the arithmetic is tested
//! without a file, and the operation that writes the objects only sews.
//!
//! # The law
//!
//! A rate `q0` at the anchor - hour 0 - and an exponential decline `D`, a
//! fraction per year: `q(t) = q0 · exp(−D · t / Y)`, `t` in hours since the
//! anchor, `Y` the Julian year of 8 766 hours (365.25 days). `D = 0` is a
//! constant rate. The year is a convention, and it is said: a decline *per
//! year* read over months of 28 to 31 days is only exact against one stated
//! length.
//!
//! The unit of the rate is the caller's - `m3/h` for the doublet - and a
//! volume is in the matching volume unit: a rate times hours.
//!
//! # A period's volume is the rate integrated over it
//!
//! `V = ∫ q dt` from `a` to `b`, in closed form: `q0 · (b − a)` for a constant
//! rate, `q0 / k · e^(−k·a) · (1 − e^(−k·(b − a)))` with `k = D / Y` otherwise -
//! written with `exp_m1`, so that a small decline does not lose its digits
//! in the difference of two exponentials close to one.
//!
//! The rate a period reports is its **mean**, `V / (b − a)`. That is what keeps
//! a monthly report honest: the rate at the start of the month times the month
//! would overstate a declining well's volume, and nothing in the report would
//! say so.
//!
//! # n instants make n − 1 periods
//!
//! A period runs from one instant to the next. Under `UseInterval`, do 13 dates give 12 values? The recommended answer is
//! yes, the only arithmetic that makes sense; it is not yet confirmed. Fewer than two
//! instants make no period and are refused rather than answered with nothing;
//! instants that do not increase are refused by their rank - an empty or a
//! reversed period would carry a volume of zero or less, which reads exactly
//! like a well that stopped.

/// Hours in the Julian year, 365.25 days - the year a decline is counted per.
pub const HOURS_PER_YEAR: f64 = 8_766.0;

/// What a law or a list of instants cannot be, by name. Ranks count from 1.
#[derive(Debug, Clone, thiserror::Error, PartialEq)]
pub enum LawError {
    #[error("a rate is a finite number > 0, not {given}")]
    BadRate { given: f64 },
    #[error("a decline is a finite fraction per year ≥ 0, not {given} - a rate that grows is no decline")]
    BadDecline { given: f64 },
    #[error("{given} instant(s): a period runs from one instant to the next, so two are needed at least")]
    TooFewInstants { given: usize },
    #[error("instant {rank} is not a finite number of hours")]
    NotFinite { rank: usize },
    #[error(
        "instant {rank} ({hours} h) does not come after the one before it ({before} h): periods run forward, and none is empty"
    )]
    NotIncreasing { rank: usize, hours: f64, before: f64 },
}

/// A rate at the anchor, and its exponential decline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Law {
    /// The rate at hour 0, in the caller's unit - `m3/h`.
    pub rate: f64,
    /// The decline, a fraction per year of [`HOURS_PER_YEAR`] hours - 0 for a
    /// constant rate.
    pub decline: f64,
}

impl Law {
    /// A law, or why not.
    pub fn new(rate: f64, decline: f64) -> Result<Law, LawError> {
        if !(rate.is_finite() && rate > 0.0) {
            return Err(LawError::BadRate { given: rate });
        }
        if !(decline.is_finite() && decline >= 0.0) {
            return Err(LawError::BadDecline { given: decline });
        }
        Ok(Law { rate, decline })
    }

    /// The same law times `share` - what an injector returns of what a
    /// producer pumps. The decline is the producer's: the loop carries the
    /// same water.
    pub fn scaled(&self, share: f64) -> Law {
        Law {
            rate: self.rate * share,
            decline: self.decline,
        }
    }

    /// The decline per hour.
    fn per_hour(&self) -> f64 {
        self.decline / HOURS_PER_YEAR
    }

    /// The rate at `hours` since the anchor.
    pub fn rate_at(&self, hours: f64) -> f64 {
        self.rate * (-self.per_hour() * hours).exp()
    }

    /// What the law delivers from `from` to `to`, hours since the anchor -
    /// the rate integrated, in closed form.
    pub fn volume(&self, from: f64, to: f64) -> f64 {
        let k = self.per_hour();
        if k == 0.0 {
            return self.rate * (to - from);
        }
        // q0 / k · e^(−k·from) · (1 − e^(−k·(to − from))), the last factor
        // through `exp_m1` so that a small k keeps its digits.
        let start = (-k * from).exp();
        let fraction = -(-k * (to - from)).exp_m1();
        self.rate * start * fraction / k
    }
}

/// One period: from an instant to the next.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Period {
    /// Its start, hours since the anchor.
    pub from: f64,
    /// Its end, hours since the anchor.
    pub to: f64,
    /// What the law delivers over it - the rate integrated.
    pub volume: f64,
    /// Its mean rate, `volume / (to − from)` - what a report states for it.
    pub mean_rate: f64,
}

impl Period {
    /// How long it lasts, in hours.
    pub fn hours(&self) -> f64 {
        self.to - self.from
    }
}

/// The periods between consecutive instants - hours since the anchor, in
/// increasing order: `n` instants make `n − 1` periods.
pub fn periods(law: &Law, instants: &[f64]) -> Result<Vec<Period>, LawError> {
    if instants.len() < 2 {
        return Err(LawError::TooFewInstants {
            given: instants.len(),
        });
    }
    for (at, hours) in instants.iter().enumerate() {
        if !hours.is_finite() {
            return Err(LawError::NotFinite { rank: at + 1 });
        }
        if at > 0 && *hours <= instants[at - 1] {
            return Err(LawError::NotIncreasing {
                rank: at + 1,
                hours: *hours,
                before: instants[at - 1],
            });
        }
    }
    Ok(instants
        .windows(2)
        .map(|pair| {
            let (from, to) = (pair[0], pair[1]);
            let volume = law.volume(from, to);
            Period {
                from,
                to,
                volume,
                mean_rate: volume / (to - from),
            }
        })
        .collect())
}

/// The least, the greatest and the mean of a series' values - what a daily
/// statistic states.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Summary {
    /// The values taken.
    pub count: usize,
    /// The values set aside because they were not finite - counted, never
    /// averaged.
    pub set_aside: usize,
    /// The least.
    pub minimum: f64,
    /// The greatest.
    pub maximum: f64,
    /// The arithmetic mean.
    pub mean: f64,
}

/// The summary of `values` - `None` when no finite value is left: a statistic
/// of nothing is no statistic, and a mean of zero values would be a `NaN`
/// written as a number.
pub fn summary(values: &[f64]) -> Option<Summary> {
    let kept: Vec<f64> = values.iter().copied().filter(|value| value.is_finite()).collect();
    if kept.is_empty() {
        return None;
    }
    let minimum = kept.iter().copied().fold(f64::INFINITY, f64::min);
    let maximum = kept.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let mean = kept.iter().sum::<f64>() / kept.len() as f64;
    Some(Summary {
        count: kept.len(),
        set_aside: values.len() - kept.len(),
        minimum,
        maximum,
        mean,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The hours of the twelve months of 2025, from the calendar - not from
    /// any date parser: 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31 days.
    fn months_of_2025() -> Vec<f64> {
        let days = [
            31.0, 28.0, 31.0, 30.0, 31.0, 30.0, 31.0, 31.0, 30.0, 31.0, 30.0, 31.0,
        ];
        let mut instants: Vec<f64> = vec![0.0];
        for one in days {
            let last = *instants.last().unwrap();
            instants.push(last + one * 24.0);
        }
        instants
    }

    /// Composite Simpson over `n` (even) intervals - a quadrature that knows
    /// nothing of the closed form it checks.
    fn simpson(f: impl Fn(f64) -> f64, a: f64, b: f64, n: usize) -> f64 {
        let h = (b - a) / n as f64;
        let mut sum = f(a) + f(b);
        for i in 1..n {
            let weight = if i % 2 == 1 { 4.0 } else { 2.0 };
            sum += weight * f(a + i as f64 * h);
        }
        sum * h / 3.0
    }

    fn close(a: f64, b: f64, relative: f64) -> bool {
        (a - b).abs() <= relative * a.abs().max(b.abs()).max(1.0)
    }

    #[test]
    fn thirteen_instants_make_twelve_periods_and_a_constant_rate_delivers_rate_times_hours() {
        // 13 dates, 12 values. To break: `windows(2)` becomes a map over
        // every instant, or the volume forgets the length of the month.
        let law = Law::new(40.0, 0.0).unwrap();
        let found = periods(&law, &months_of_2025()).unwrap();
        assert_eq!(found.len(), 12);
        let february = found[1];
        assert_eq!(february.hours(), 28.0 * 24.0);
        assert_eq!(february.volume, 40.0 * 672.0);
        assert_eq!(february.mean_rate, 40.0);
        let year: f64 = found.iter().map(|one| one.volume).sum();
        assert_eq!(year, 40.0 * 8_760.0, "2025 has 365 days");
    }

    #[test]
    fn a_declining_rate_integrates_as_a_quadrature_does() {
        // Two readings of one fact: the closed form and Simpson's rule over
        // the rate itself. To break: a decline per day read as per year, or
        // the rate at the start of a period taken for its volume.
        let law = Law::new(40.0, 0.25).unwrap();
        let instants = months_of_2025();
        for one in periods(&law, &instants).unwrap() {
            let numeric = simpson(|t| law.rate_at(t), one.from, one.to, 2_000);
            assert!(close(one.volume, numeric, 1e-10), "{one:?} against {numeric}");
            // The mean, and not the first rate: under a decline they differ.
            assert!(one.mean_rate < law.rate_at(one.from), "{one:?}");
            assert!(one.mean_rate > law.rate_at(one.to), "{one:?}");
            assert!(close(one.mean_rate * one.hours(), one.volume, 1e-14), "{one:?}");
        }
    }

    #[test]
    fn a_volume_adds_up_over_consecutive_periods() {
        let law = Law::new(12.5, 0.8).unwrap();
        let whole = law.volume(100.0, 9_000.0);
        let parts = law.volume(100.0, 4_321.0) + law.volume(4_321.0, 9_000.0);
        assert!(close(whole, parts, 1e-12), "{whole} against {parts}");
    }

    #[test]
    fn a_tiny_decline_keeps_its_digits() {
        // Without `exp_m1`, 1 − e^(−x) for x ≈ 1e-12 is mostly rounding.
        let law = Law::new(40.0, 1e-9).unwrap();
        let volume = law.volume(0.0, 744.0);
        let constant = 40.0 * 744.0;
        assert!(volume < constant, "a decline delivers less: {volume}");
        assert!(close(volume, constant, 1e-9), "{volume} against {constant}");
    }

    #[test]
    fn an_injector_scaled_by_one_returns_what_the_producer_pumps() {
        let producer = Law::new(40.0, 0.1).unwrap();
        let injector = producer.scaled(1.0);
        for (p, i) in periods(&producer, &months_of_2025())
            .unwrap()
            .iter()
            .zip(periods(&injector, &months_of_2025()).unwrap())
        {
            assert_eq!(p.volume, i.volume);
        }
        let half = producer.scaled(0.5);
        assert!(close(
            half.volume(0.0, 744.0),
            producer.volume(0.0, 744.0) / 2.0,
            1e-14
        ));
    }

    #[test]
    fn too_few_instants_and_instants_out_of_order_are_refused_by_rank() {
        let law = Law::new(40.0, 0.0).unwrap();
        assert_eq!(periods(&law, &[0.0]), Err(LawError::TooFewInstants { given: 1 }));
        assert_eq!(periods(&law, &[]), Err(LawError::TooFewInstants { given: 0 }));
        assert_eq!(
            periods(&law, &[0.0, 744.0, 744.0]),
            Err(LawError::NotIncreasing {
                rank: 3,
                hours: 744.0,
                before: 744.0
            })
        );
        assert_eq!(
            periods(&law, &[0.0, f64::NAN]),
            Err(LawError::NotFinite { rank: 2 })
        );
        // And two instants are one period - the least a report can hold.
        assert_eq!(periods(&law, &[0.0, 24.0]).unwrap().len(), 1);
    }

    #[test]
    fn a_rate_or_a_decline_that_is_no_number_is_refused() {
        assert_eq!(Law::new(0.0, 0.0), Err(LawError::BadRate { given: 0.0 }));
        assert!(matches!(Law::new(f64::NAN, 0.0), Err(LawError::BadRate { .. })));
        assert_eq!(Law::new(40.0, -0.1), Err(LawError::BadDecline { given: -0.1 }));
    }

    #[test]
    fn a_summary_sets_aside_what_is_no_number_and_refuses_nothing() {
        let found = summary(&[3.0, f64::NAN, 1.0, 2.0]).unwrap();
        assert_eq!((found.count, found.set_aside), (3, 1));
        assert_eq!((found.minimum, found.maximum, found.mean), (1.0, 3.0, 2.0));
        assert_eq!(summary(&[]), None);
        assert_eq!(summary(&[f64::NAN]), None);
    }
}
