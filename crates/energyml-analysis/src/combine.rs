//! Derived values from two sets of values, cell by cell: a ratio, a difference,
//! the log of a ratio - in a registry, the caller's own tried first.
//!
//! Pure and store-free, like the rest of the crate: the inputs are two plain
//! sequences (a curve is a matrix of one column), the caller has already turned
//! each `NullValue` into `NaN`, and the answer says how many pairs were absent
//! and how many were undefined, apart. Writing the result as a property is the
//! `combine` operation of `energyml-build`, not this module.

use energyml_domain::stats::{CellRule, RuleTally, TallyRefusal, tally};
use energyml_domain::units;

/// One way of combining two values.
pub trait Combine: Send + Sync {
    /// Its stable id: `ratio`, `difference`, `log-ratio`.
    fn id(&self) -> &'static str;
    /// The unit of the result, given the common unit of the two inputs
    /// (`None`: they have none).
    fn unit(&self, common: Option<&str>) -> Option<String>;
    /// One cell. NaN where the result is undefined (a division by zero, the
    /// logarithm of a value <= 0); never an infinity.
    fn cell(&self, a: f64, b: f64) -> f64;
}

/// `a / b`, unitless (`Euc`).
pub struct Ratio;
/// `a - b`, in the unit of `a`.
pub struct Difference;
/// `log10(a) - log10(b)`, unitless (`Euc`).
pub struct LogRatio;

impl Combine for Ratio {
    fn id(&self) -> &'static str {
        "ratio"
    }
    fn unit(&self, _common: Option<&str>) -> Option<String> {
        Some("Euc".to_string())
    }
    fn cell(&self, a: f64, b: f64) -> f64 {
        if b == 0.0 { f64::NAN } else { a / b }
    }
}

impl Combine for Difference {
    fn id(&self) -> &'static str {
        "difference"
    }
    fn unit(&self, common: Option<&str>) -> Option<String> {
        common.map(String::from)
    }
    fn cell(&self, a: f64, b: f64) -> f64 {
        a - b
    }
}

impl Combine for LogRatio {
    fn id(&self) -> &'static str {
        "log-ratio"
    }
    fn unit(&self, _common: Option<&str>) -> Option<String> {
        Some("Euc".to_string())
    }
    fn cell(&self, a: f64, b: f64) -> f64 {
        if a > 0.0 && b > 0.0 {
            a.log10() - b.log10()
        } else {
            f64::NAN
        }
    }
}

/// The combinations known, the caller's first.
pub struct Combines {
    entries: Vec<Box<dyn Combine>>,
}

impl Combines {
    /// `ratio`, `difference`, `log-ratio`.
    pub fn builtin() -> Combines {
        Combines {
            entries: vec![Box::new(Ratio), Box::new(Difference), Box::new(LogRatio)],
        }
    }

    /// The same, with `extra` tried **before** every built-in (the house rule of
    /// every registry, ADR-0006).
    pub fn with(mut self, extra: Box<dyn Combine>) -> Combines {
        self.entries.insert(0, extra);
        self
    }

    /// The first entry of that id.
    pub fn get(&self, id: &str) -> Option<&dyn Combine> {
        self.entries.iter().find(|entry| entry.id() == id).map(|entry| entry.as_ref())
    }

    /// The first entry of that id, or a refusal that lists what is known.
    pub fn require(&self, id: &str) -> Result<&dyn Combine, CombineRefusal> {
        self.get(id).ok_or_else(|| CombineRefusal::UnknownOperation {
            id: id.to_string(),
            known: self.ids(),
        })
    }

    /// Every id, in the order they are tried.
    pub fn ids(&self) -> Vec<&'static str> {
        self.entries.iter().map(|entry| entry.id()).collect()
    }
}

/// What [`combine`] gives.
#[derive(Clone, Debug, PartialEq)]
pub struct Combined {
    /// One value per input pair, NaN where absent or undefined.
    pub values: Vec<f64>,
    /// Pairs where an input was not a finite number (NaN: the caller has
    /// already turned each `NullValue` into NaN).
    pub absent: usize,
    /// Pairs of two finite inputs whose result is undefined.
    pub undefined: usize,
    /// The unit of the result.
    pub uom: Option<String>,
    /// The factor the second input was multiplied by to be in the unit of the
    /// first (1 when the units were the same).
    pub converted_by: f64,
}

impl Combined {
    /// The cells of the result that a rule matches, `cols` values per row:
    /// `energyml_domain::stats::tally`, not a second count.
    pub fn tally(&self, cols: usize, rule: &CellRule) -> Result<RuleTally, TallyRefusal> {
        tally(&self.values, cols, rule)
    }
}

/// What [`combine`] refuses, by name.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CombineRefusal {
    /// The two inputs do not have the same length.
    #[error(
        "{a} value(s) against {b} - two properties combine value by value, on the same elements and the same shape"
    )]
    LengthsDiffer {
        /// The length of the first input.
        a: usize,
        /// The length of the second input.
        b: usize,
    },
    /// The two units cannot be brought to one.
    #[error("the units {a:?} and {b:?} do not combine: neither the same unit nor two units of one family")]
    UnitsDiffer {
        /// The unit of the first input.
        a: Option<String>,
        /// The unit of the second input.
        b: Option<String>,
    },
    /// No combination of that id.
    #[error("no combination `{id}` - known: {known:?}")]
    UnknownOperation {
        /// The id asked for.
        id: String,
        /// The ids known, in the order they are tried.
        known: Vec<&'static str>,
    },
}

/// Combine `a` and `b`, value by value.
pub fn combine(
    a: &[f64],
    b: &[f64],
    a_uom: Option<&str>,
    b_uom: Option<&str>,
    op: &dyn Combine,
) -> Result<Combined, CombineRefusal> {
    if a.len() != b.len() {
        return Err(CombineRefusal::LengthsDiffer { a: a.len(), b: b.len() });
    }
    let differ = || CombineRefusal::UnitsDiffer {
        a: a_uom.map(String::from),
        b: b_uom.map(String::from),
    };
    let (common, factor) = match (a_uom, b_uom) {
        (None, None) => (None, 1.0),
        (Some(x), Some(y)) if x == y => (Some(x), 1.0),
        (Some(x), Some(y)) => {
            let factor = units::convert(1.0, y, x).map_err(|_| differ())?;
            (Some(x), factor)
        }
        _ => return Err(differ()),
    };
    let mut values = Vec::with_capacity(a.len());
    let (mut absent, mut undefined) = (0usize, 0usize);
    for (&x, &y) in a.iter().zip(b) {
        let y = y * factor;
        if !x.is_finite() || !y.is_finite() {
            absent += 1;
            values.push(f64::NAN);
            continue;
        }
        let r = op.cell(x, y);
        if r.is_finite() {
            values.push(r);
        } else {
            undefined += 1;
            values.push(f64::NAN);
        }
    }
    Ok(Combined {
        values,
        absent,
        undefined,
        uom: op.unit(common),
        converted_by: factor,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ratio_of_one_unit_is_unitless() {
        let r = combine(&[2.0, 9.0], &[4.0, 3.0], Some("ohm.m"), Some("ohm.m"), &Ratio).unwrap();
        assert_eq!(r.values, vec![0.5, 3.0]);
        assert_eq!(r.uom.as_deref(), Some("Euc"));
    }

    #[test]
    fn the_second_unit_is_converted_first() {
        let r = combine(&[1.0], &[1.0], Some("m"), Some("ft"), &Ratio).unwrap();
        assert!((r.values[0] - 1.0 / 0.3048).abs() < 1e-12);
        assert!((r.converted_by - 0.3048).abs() < 1e-12);
    }

    #[test]
    fn units_of_two_families_are_refused() {
        let e = combine(&[1.0], &[1.0], Some("m"), Some("s"), &Ratio).unwrap_err();
        assert!(matches!(e, CombineRefusal::UnitsDiffer { .. }));
        let e = combine(&[1.0], &[1.0], Some("m"), None, &Ratio).unwrap_err();
        assert!(matches!(e, CombineRefusal::UnitsDiffer { .. }));
    }

    #[test]
    fn a_division_by_zero_is_undefined_not_infinite() {
        let r = combine(&[1.0], &[0.0], None, None, &Ratio).unwrap();
        assert!(r.values[0].is_nan());
        assert_eq!(r.undefined, 1);
        assert_eq!(r.absent, 0);
    }

    #[test]
    fn the_log_of_a_non_positive_value_is_undefined() {
        let r = combine(&[-1.0, 10.0], &[1.0, 1.0], None, None, &LogRatio).unwrap();
        assert!(r.values[0].is_nan());
        assert!((r.values[1] - 1.0).abs() < 1e-12);
        assert_eq!(r.undefined, 1);
    }

    #[test]
    fn absent_inputs_are_counted_apart() {
        let r = combine(&[f64::NAN, 1.0], &[1.0, 1.0], None, None, &Ratio).unwrap();
        assert_eq!(r.absent, 1);
        assert_eq!(r.undefined, 0);
    }

    #[test]
    fn lengths_that_differ_are_refused() {
        let e = combine(&[1.0, 2.0], &[1.0, 2.0, 3.0], None, None, &Ratio).unwrap_err();
        assert_eq!(e, CombineRefusal::LengthsDiffer { a: 2, b: 3 });
    }

    #[test]
    fn the_callers_combination_is_tried_first() {
        struct Zero;
        impl Combine for Zero {
            fn id(&self) -> &'static str {
                "ratio"
            }
            fn unit(&self, _: Option<&str>) -> Option<String> {
                None
            }
            fn cell(&self, _: f64, _: f64) -> f64 {
                0.0
            }
        }
        let combines = Combines::builtin().with(Box::new(Zero));
        assert_eq!(combines.get("ratio").unwrap().cell(4.0, 2.0), 0.0);
        assert_eq!(combines.ids()[0], "ratio");
        assert_eq!(combines.ids().len(), 4);
        assert!(matches!(
            combines.require("nope"),
            Err(CombineRefusal::UnknownOperation { .. })
        ));
    }

    #[test]
    fn the_tally_of_a_ratio_counts_the_cells_below_the_rule() {
        let r = Combined {
            values: vec![0.9, 1.0, 1.2, 0.5],
            absent: 0,
            undefined: 0,
            uom: Some("Euc".to_string()),
            converted_by: 1.0,
        };
        let t = r.tally(2, &CellRule::Below(0.95)).unwrap();
        assert_eq!(t.matched, 2);
        assert_eq!(t.matched_per_row, vec![1, 1]);
        assert_eq!(t.present_per_row, vec![2, 2]);
    }
}
