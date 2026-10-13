//! Units of measure, keyed on what the file actually says.
//!
//! # The rule this module exists to enforce
//!
//! **Convert on the unit string, never on the Rust enum variant.**
//!
//! The generated `*Uom` enums are not stable across versions. When the
//! generator's case-disambiguation changed between eml 2.0 and eml 2.1, six
//! pairs of units swapped identifiers:
//!
//! | | eml 2.0 | eml 2.1 / 2.2 / 2.3 |
//! |---|---|---|
//! | `LengthUom::Mm` | megametre | **millimetre** |
//! | `LengthUom::Mm2` | millimetre | **megametre** |
//!
//! and likewise `MA`/`mA`, `MW`/`mW`, `MWb`/`mWb`, `Mcal[th]`/`mcal[th]`,
//! `dB.MW`/`dB.mW`. A `match` on the variant is a factor of 10⁹ error on one
//! version and correct on the other, with nothing to notice it. The wire
//! strings never moved: `m` is `m` in all four versions.
//!
//! # The second trap: one unit, two spellings
//!
//! The *same physical unit* is spelled differently depending on which enum the
//! field happens to be typed with — in every version:
//!
//! | unit | `LengthUom` | `VerticalCoordinateUom` |
//! |---|---|---|
//! | US survey foot | `ft[US]` | `ftUS` |
//! | British foot 1865 | `ft[Br65]` | `ftBr(65)` |
//!
//! This matters directly here: RESQML 2.0.1 types `ProjectedUom` and
//! `VerticalUom` as `LengthUom`, while other parts of the schema use
//! `VerticalCoordinateUom` for the same quantity. Both spellings are accepted.
//!
//! # What is deliberately not here
//!
//! A complete unit system. The Energistics *Unit of Measure Dictionary* —
//! which carries the dimension and conversion factor of all ~1540 unit strings
//! — is **not vendored in this repository** (`rc/wellKnownData/` holds only the
//! PropertyKind dictionary). It is published at
//! <http://w3.energistics.org/uom/poscUnits22.xml>, and vendoring it is a
//! codegen job like PropertyKind, not hand-written code.
//!
//! Until then this table covers the units whose factors are exact and
//! well-established, and **every other unit is a named error**. A unit that
//! cannot be converted must not be assumed to be the base one: that is how a
//! model in feet gets drawn as if it were in metres.

use crate::error::{Error, Result};

/// A family of units that can be converted among themselves.
///
/// Conversion across families is refused: a length is not a pressure, and
/// asking is a bug in the caller rather than a value to compute.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dimension {
    /// Base unit: metre.
    Length,
    /// Base unit: radian.
    PlaneAngle,
    /// Base unit: pascal.
    Pressure,
    /// Base unit: second.
    Time,
}

impl Dimension {
    /// The unit every value of this family is converted through.
    pub fn base(self) -> &'static str {
        match self {
            Dimension::Length => "m",
            Dimension::PlaneAngle => "rad",
            Dimension::Pressure => "Pa",
            Dimension::Time => "s",
        }
    }

    /// The family a unit belongs to, or `None` if this table does not know it.
    pub fn of(unit: &str) -> Option<Dimension> {
        for (dimension, table) in TABLES {
            if table.iter().any(|(name, _)| *name == unit) {
                return Some(*dimension);
            }
        }
        None
    }
}

/// How many base units one of `unit` is worth.
///
/// `None` when the unit is outside this table — which is not the same as
/// "unitless", and callers must not treat it as 1.
pub fn factor(unit: &str) -> Option<f64> {
    TABLES
        .iter()
        .find_map(|(_, table)| table.iter().find(|(name, _)| *name == unit))
        .map(|(_, factor)| *factor)
}

/// Convert a value between two units of the same family.
///
/// ```
/// # use energyml_domain::units;
/// let metres = units::convert(1.0, "ft", "m").unwrap();
/// assert!((metres - 0.3048).abs() < 1e-12);
/// ```
///
/// Errors when either unit is unknown, or when the two belong to different
/// families.
pub fn convert(value: f64, from: &str, to: &str) -> Result<f64> {
    if from == to {
        return Ok(value);
    }
    let from_dimension = Dimension::of(from).ok_or_else(|| Error::unknown_unit(from))?;
    let to_dimension = Dimension::of(to).ok_or_else(|| Error::unknown_unit(to))?;
    if from_dimension != to_dimension {
        return Err(Error::MismatchedUnits {
            from: from.to_string(),
            to: to.to_string(),
        });
    }
    // Both factors are known: `Dimension::of` found them in the same table.
    let from_factor = factor(from).unwrap_or(1.0);
    let to_factor = factor(to).unwrap_or(1.0);
    Ok(value * from_factor / to_factor)
}

/// Convert to the base unit of the value's own family.
pub fn to_base(value: f64, unit: &str) -> Result<f64> {
    let dimension = Dimension::of(unit).ok_or_else(|| Error::unknown_unit(unit))?;
    convert(value, unit, dimension.base())
}

/// A length in metres.
pub fn metres(value: f64, unit: &str) -> Result<f64> {
    in_family(value, unit, Dimension::Length)
}

/// An angle in radians.
pub fn radians(value: f64, unit: &str) -> Result<f64> {
    in_family(value, unit, Dimension::PlaneAngle)
}

/// A pressure in pascals.
pub fn pascals(value: f64, unit: &str) -> Result<f64> {
    in_family(value, unit, Dimension::Pressure)
}

/// A duration in seconds.
pub fn seconds(value: f64, unit: &str) -> Result<f64> {
    in_family(value, unit, Dimension::Time)
}

fn in_family(value: f64, unit: &str, expected: Dimension) -> Result<f64> {
    match Dimension::of(unit) {
        Some(found) if found == expected => convert(value, unit, expected.base()),
        Some(found) => Err(Error::WrongDimension {
            unit: unit.to_string(),
            found: format!("{found:?}"),
            expected: format!("{expected:?}"),
        }),
        None => Err(Error::unknown_unit(unit)),
    }
}

// ---------------------------------------------------------------------------
// The tables
//
// Only factors that are exact by definition, or standard to the last digit
// quoted. Anything whose value depends on which authority you ask — the
// historical chains and links, the survey feet of eight different countries,
// the several definitions of a year — is left out on purpose, so that it
// errors instead of being silently wrong.
// ---------------------------------------------------------------------------

const TABLES: &[(Dimension, &[(&str, f64)])] = &[
    (Dimension::Length, LENGTH),
    (Dimension::PlaneAngle, PLANE_ANGLE),
    (Dimension::Pressure, PRESSURE),
    (Dimension::Time, TIME),
];

/// Metres per unit.
const LENGTH: &[(&str, f64)] = &[
    ("m", 1.0),
    ("Em", 1e18),
    ("Tm", 1e12),
    ("Gm", 1e9),
    ("Mm", 1e6),
    ("km", 1e3),
    ("hm", 1e2),
    ("dam", 1e1),
    ("dm", 1e-1),
    ("cm", 1e-2),
    ("mm", 1e-3),
    ("um", 1e-6),
    ("nm", 1e-9),
    ("pm", 1e-12),
    ("fm", 1e-15),
    ("angstrom", 1e-10),
    // The international foot and its family: exact by the 1959 agreement.
    ("ft", 0.3048),
    ("in", 0.0254),
    ("yd", 0.9144),
    ("mi", 1609.344),
    ("fathom", 1.8288),
    ("mil", 2.54e-5),
    ("mi[naut]", 1852.0),
    // The US survey foot: 1200/3937 m exactly, and 2 ppm longer than `ft`.
    // Both spellings are the same unit — see the module documentation.
    ("ft[US]", 1200.0 / 3937.0),
    ("ftUS", 1200.0 / 3937.0),
    ("in[US]", 1200.0 / 3937.0 / 12.0),
    ("mi[US]", 1200.0 / 3937.0 * 5280.0),
    // Decimal multiples the schema spells out.
    ("0.1 ft", 0.03048),
    ("0.1 in", 0.00254),
    ("0.1 yd", 0.09144),
    ("1/2 ft", 0.1524),
    ("1/16 in", 0.0254 / 16.0),
    ("1/32 in", 0.0254 / 32.0),
    ("1/64 in", 0.0254 / 64.0),
    ("10 ft", 3.048),
    ("10 in", 0.254),
    ("100 ft", 30.48),
    ("1000 ft", 304.8),
    ("30 ft", 9.144),
    ("30 m", 30.0),
    ("10 km", 1e4),
    ("100 km", 1e5),
];

/// Radians per unit.
const PLANE_ANGLE: &[(&str, f64)] = &[
    ("rad", 1.0),
    ("Mrad", 1e6),
    ("krad", 1e3),
    ("mrad", 1e-3),
    ("urad", 1e-6),
    ("dega", std::f64::consts::PI / 180.0),
    ("mina", std::f64::consts::PI / 180.0 / 60.0),
    ("seca", std::f64::consts::PI / 180.0 / 3600.0),
    ("0.001 seca", std::f64::consts::PI / 180.0 / 3600.0 * 1e-3),
    ("gon", std::f64::consts::PI / 200.0),
    ("rev", std::f64::consts::TAU),
];

/// Pascals per unit.
const PRESSURE: &[(&str, f64)] = &[
    ("Pa", 1.0),
    ("EPa", 1e18),
    ("TPa", 1e12),
    ("GPa", 1e9),
    ("MPa", 1e6),
    ("kPa", 1e3),
    ("dPa", 1e-1),
    ("cPa", 1e-2),
    ("mPa", 1e-3),
    ("uPa", 1e-6),
    ("nPa", 1e-9),
    ("pPa", 1e-12),
    ("fPa", 1e-15),
    ("N/m2", 1.0),
    ("kN/m2", 1e3),
    ("N/mm2", 1e6),
    ("bar", 1e5),
    ("hbar", 1e7),
    ("mbar", 1e2),
    ("ubar", 1e-1),
    ("atm", 101_325.0),
    ("at", 98_066.5),
    ("kgf/m2", 9.806_65),
    ("kgf/cm2", 98_066.5),
    ("kgf/mm2", 9_806_650.0),
    ("dyne/cm2", 0.1),
    ("torr", 101_325.0 / 760.0),
    ("psi", 4.448_221_615_260_5 / 0.000_645_16),
    ("kpsi", 4.448_221_615_260_5 / 0.000_645_16 * 1e3),
    ("Mpsi", 4.448_221_615_260_5 / 0.000_645_16 * 1e6),
    ("upsi", 4.448_221_615_260_5 / 0.000_645_16 * 1e-6),
];

/// Seconds per unit.
const TIME: &[(&str, f64)] = &[
    ("s", 1.0),
    ("ds", 1e-1),
    ("cs", 1e-2),
    ("ms", 1e-3),
    ("us", 1e-6),
    ("ns", 1e-9),
    ("ps", 1e-12),
    ("hs", 1e2),
    ("1/2 ms", 5e-4),
    ("min", 60.0),
    ("h", 3600.0),
    ("d", 86_400.0),
    ("wk", 604_800.0),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_international_foot_is_exact() {
        assert_eq!(convert(1.0, "ft", "m").unwrap(), 0.3048);
        assert_eq!(convert(1.0, "in", "m").unwrap(), 0.0254);
    }

    #[test]
    fn both_spellings_of_the_us_foot_agree() {
        // `LengthUom` says `ft[US]`, `VerticalCoordinateUom` says `ftUS`, in
        // every version. A table that knows only one silently rejects — or
        // worse, mis-handles — half the files.
        let bracketed = convert(1.0, "ft[US]", "m").unwrap();
        let bare = convert(1.0, "ftUS", "m").unwrap();
        assert_eq!(bracketed, bare);
        // And it is *not* the international foot: 2 ppm longer.
        assert!(bracketed > 0.3048);
        assert!((bracketed - 0.3048).abs() < 1e-6);
    }

    #[test]
    fn the_megametre_millimetre_trap_is_impossible_here() {
        // The whole reason this module keys on strings. `LengthUom::Mm` means
        // megametre in eml 2.0 and millimetre in eml 2.3; the *strings* `Mm`
        // and `mm` mean the same thing in both, and that is what is matched.
        assert_eq!(convert(1.0, "Mm", "m").unwrap(), 1e6);
        assert_eq!(convert(1.0, "mm", "m").unwrap(), 1e-3);
        assert_eq!(
            convert(1.0, "Mm", "mm").unwrap() / 1e9,
            1.0,
            "the two differ by exactly the factor a variant match would lose"
        );
    }

    #[test]
    fn a_quarter_turn_is_a_quarter_turn() {
        let quarter = std::f64::consts::FRAC_PI_2;
        assert!((radians(90.0, "dega").unwrap() - quarter).abs() < 1e-12);
        assert!((radians(100.0, "gon").unwrap() - quarter).abs() < 1e-12);
        assert!((radians(0.25, "rev").unwrap() - quarter).abs() < 1e-12);
    }

    #[test]
    fn standard_pressures_agree_with_their_definitions() {
        assert_eq!(pascals(1.0, "bar").unwrap(), 1e5);
        assert_eq!(pascals(1.0, "atm").unwrap(), 101_325.0);
        // 760 torr is one atmosphere, by definition.
        assert!((pascals(760.0, "torr").unwrap() - 101_325.0).abs() < 1e-9);
        // One psi to the published five decimals.
        assert!((pascals(1.0, "psi").unwrap() - 6_894.757_293_168).abs() < 1e-6);
    }

    #[test]
    fn an_unknown_unit_is_refused_not_assumed() {
        // `exoticMeter` is in the test corpus: the open `*UomExt` enums let a
        // producer write anything. Treating it as metres is exactly the bug
        // this module exists to prevent.
        let e = convert(1.0, "exoticMeter", "m").unwrap_err();
        assert!(format!("{e}").contains("exoticMeter"));
    }

    #[test]
    fn a_length_is_not_a_pressure() {
        assert!(convert(1.0, "m", "Pa").is_err());
        assert!(metres(1.0, "bar").is_err());
    }

    #[test]
    fn every_unit_is_listed_once_and_in_one_family() {
        let mut seen: Vec<&str> = Vec::new();
        for (_, table) in TABLES {
            for (name, factor) in *table {
                assert!(!seen.contains(name), "`{name}` is listed twice");
                assert!(*factor > 0.0, "`{name}` has a non-positive factor");
                seen.push(name);
            }
        }
    }
}
