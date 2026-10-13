//! **Whether a water analysis is electrically neutral** — the check a stated
//! analysis of dissolved ions has to pass before it is written, for the water
//! of Lutetia's chalk aquifer (fresh water, calcium bicarbonate, possible sulfates from Lutetian gypsum,
//! mineralisation of the order of g/L).
//!
//! Nothing here reads a package or knows PRODML: the inputs are ion formulas
//! and concentrations in mg/L, so what can be wrong in the arithmetic is tested
//! without a file, and the operation that writes the analysis only sews.
//!
//! # Why a balance, and not a plausibility range
//!
//! A water is electrically neutral: the charges its cations carry equal the
//! charges its anions carry. A list of concentrations that does not balance is
//! not a water — it is a typing error, or an ion forgotten, and nothing in the
//! written analysis would say so. The balance is the one property every
//! analysis must have whatever its origin, so it is the one this module
//! checks; it says nothing of whether the water is *this* aquifer's.
//!
//! The balance error is the conventional
//! `100 · (Σ cations − Σ anions) / (Σ cations + Σ anions)`, each sum in
//! milliequivalents per litre (`meq/L = mg/L ÷ molar mass × |charge|`). What
//! error is acceptable is the caller's to state — this module computes, it
//! does not judge.
//!
//! # The ions, and where their numbers come from
//!
//! The major ions of a fresh groundwater and a few minor ones. Molar masses are
//! the sums of the IUPAC standard atomic weights (abridged, CIAAW 2021), in
//! g/mol: H 1.008, Li 6.94, C 12.011, N 14.007, O 15.999, F 18.998, Na 22.990,
//! Mg 24.305, S 32.06, Cl 35.45, K 39.098, Ca 40.078, Fe 55.845, Br 79.904,
//! Sr 87.62, Ba 137.33. The charges are those of the formulas.
//!
//! **Nitrate is not in the list, on purpose.** Its formula is `NO3-`, one
//! negative charge; PRODML 2.3 spells it `NO3-2` (`AnionKind`, FluidAnalysis.xsd),
//! which no chemistry gives it. An ion whose charge depends on which of the two
//! is believed cannot enter a balance, so it is refused by its formula rather
//! than counted with either.

/// One ion: its formula as written, its molar mass in g/mol, its charge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ion {
    /// `Ca+2`, `HCO3-` — the formula, its charge after the sign.
    pub formula: &'static str,
    /// g/mol, from the standard atomic weights listed in the module doc.
    pub molar_mass: f64,
    /// Positive for a cation, negative for an anion.
    pub charge: i32,
}

/// The ions this module can balance.
pub const IONS: &[Ion] = &[
    // Cations.
    Ion { formula: "Ca+2", molar_mass: 40.078, charge: 2 },
    Ion { formula: "Mg+2", molar_mass: 24.305, charge: 2 },
    Ion { formula: "Na+", molar_mass: 22.990, charge: 1 },
    Ion { formula: "K+", molar_mass: 39.098, charge: 1 },
    Ion { formula: "Li+", molar_mass: 6.94, charge: 1 },
    // N + 4 H.
    Ion { formula: "NH4+", molar_mass: 18.039, charge: 1 },
    Ion { formula: "Fe+2", molar_mass: 55.845, charge: 2 },
    Ion { formula: "Sr+2", molar_mass: 87.62, charge: 2 },
    Ion { formula: "Ba+2", molar_mass: 137.33, charge: 2 },
    // Anions.
    // H + C + 3 O.
    Ion { formula: "HCO3-", molar_mass: 61.016, charge: -1 },
    // C + 3 O.
    Ion { formula: "CO3-2", molar_mass: 60.008, charge: -2 },
    // S + 4 O.
    Ion { formula: "SO4-2", molar_mass: 96.056, charge: -2 },
    Ion { formula: "Cl-", molar_mass: 35.45, charge: -1 },
    Ion { formula: "F-", molar_mass: 18.998, charge: -1 },
    Ion { formula: "Br-", molar_mass: 79.904, charge: -1 },
];

/// The ion of a formula, written exactly as in [`IONS`].
pub fn ion(formula: &str) -> Option<&'static Ion> {
    IONS.iter().find(|one| one.formula == formula)
}

/// Milliequivalents per litre of a concentration in mg/L.
pub fn milliequivalents(ion: &Ion, mg_per_l: f64) -> f64 {
    mg_per_l / ion.molar_mass * f64::from(ion.charge.abs())
}

/// The charges of an analysis, and how far apart they are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Balance {
    /// Σ cations, meq/L.
    pub cations: f64,
    /// Σ anions, meq/L.
    pub anions: f64,
    /// `100 · (cations − anions) / (cations + anions)`, in percent — positive
    /// when the cations carry more.
    pub error: f64,
}

/// What an analysis cannot be, by name.
#[derive(Debug, Clone, thiserror::Error, PartialEq)]
pub enum BalanceError {
    #[error("`{formula}` is no ion this balance knows — one of {known}")]
    Unknown { formula: String, known: String },
    #[error("`{formula}` has a concentration of {given} mg/L: a concentration is a finite number ≥ 0")]
    BadConcentration { formula: String, given: f64 },
    #[error("`{formula}` is given twice: an analysis states each ion once")]
    Twice { formula: String },
    #[error("no cation carries a charge: a water with anions alone is not neutral, and the balance would divide nothing")]
    NoCation,
    #[error("no anion carries a charge: a water with cations alone is not neutral, and the balance would divide nothing")]
    NoAnion,
}

/// The balance of an analysis — formulas and concentrations in mg/L.
pub fn balance(analysis: &[(&str, f64)]) -> Result<Balance, BalanceError> {
    let mut seen: Vec<&str> = Vec::with_capacity(analysis.len());
    let (mut cations, mut anions) = (0.0, 0.0);
    for (formula, mg_per_l) in analysis {
        let Some(known) = ion(formula) else {
            return Err(BalanceError::Unknown {
                formula: (*formula).to_string(),
                known: IONS.iter().map(|one| one.formula).collect::<Vec<_>>().join(", "),
            });
        };
        if !(mg_per_l.is_finite() && *mg_per_l >= 0.0) {
            return Err(BalanceError::BadConcentration { formula: (*formula).to_string(), given: *mg_per_l });
        }
        if seen.contains(formula) {
            return Err(BalanceError::Twice { formula: (*formula).to_string() });
        }
        seen.push(formula);
        let charges = milliequivalents(known, *mg_per_l);
        if known.charge > 0 {
            cations += charges;
        } else {
            anions += charges;
        }
    }
    if cations <= 0.0 {
        return Err(BalanceError::NoCation);
    }
    if anions <= 0.0 {
        return Err(BalanceError::NoAnion);
    }
    Ok(Balance { cations, anions, error: 100.0 * (cations - anions) / (cations + anions) })
}

/// The molar mass of calcium carbonate, g/mol — Ca + C + 3 O from the weights
/// of the module doc.
pub const CACO3_MOLAR_MASS: f64 = 100.086;

/// The total hardness, in mg/L **as CaCO3**: the calcium and the magnesium
/// stated, as milliequivalents per litre, times the equivalent mass of CaCO3
/// (half its molar mass, two charges). The conventional definition — *the sum
/// of the calcium and magnesium concentrations, both expressed as calcium
/// carbonate* (APHA *Standard Methods*, 2340 B). `None` when neither ion is stated: a hardness of
/// zero would be a measurement nobody made.
///
/// TODO(reference): verify — cited from memory; no copy was available.
pub fn total_hardness(analysis: &[(&str, f64)]) -> Option<f64> {
    let mut charges = None;
    for (formula, mg_per_l) in analysis {
        if matches!(*formula, "Ca+2" | "Mg+2")
            && let Some(known) = ion(formula)
        {
            *charges.get_or_insert(0.0) += milliequivalents(known, *mg_per_l);
        }
    }
    charges.map(|meq| meq * CACO3_MOLAR_MASS / 2.0)
}

/// The sum of the concentrations stated, mg/L — what the ions listed add up
/// to, which is not a total dissolved solids (that one is weighed after
/// evaporation, and bicarbonate does not survive it whole).
pub fn sum_of_ions(analysis: &[(&str, f64)]) -> f64 {
    analysis.iter().map(|(_, mg_per_l)| mg_per_l).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hardness_counts_calcium_and_magnesium_as_calcium_carbonate() {
        // 40.078 mg/L of Ca+2 is one millimole, as much calcium as 100.086
        // mg/L of CaCO3; 24.305 mg/L of Mg+2 is one millimole more.
        let one = total_hardness(&[("Ca+2", 40.078), ("HCO3-", 100.0)]).unwrap();
        assert!((one - 100.086).abs() < 1e-9, "{one}");
        let two = total_hardness(&[("Ca+2", 40.078), ("Mg+2", 24.305), ("Na+", 50.0)]).unwrap();
        assert!((two - 2.0 * 100.086).abs() < 1e-9, "{two}");
        // Neither ion stated: no hardness, rather than a hardness of zero.
        assert_eq!(total_hardness(&[("Na+", 50.0), ("Cl-", 70.0)]), None);
        assert!((sum_of_ions(&[("Na+", 50.0), ("Cl-", 70.0)]) - 120.0).abs() < 1e-12);
    }

    #[test]
    fn a_salt_dissolved_alone_balances_exactly() {
        // One mole of NaCl per litre: 22 990 mg/L of Na+ and 35 450 mg/L of
        // Cl-, one equivalent each — the balance is zero by construction, and
        // any error in a molar mass or a charge shows here.
        let found = balance(&[("Na+", 22_990.0), ("Cl-", 35_450.0)]).unwrap();
        assert!((found.cations - 1000.0).abs() < 1e-9, "{found:?}");
        assert!((found.anions - 1000.0).abs() < 1e-9, "{found:?}");
        assert!(found.error.abs() < 1e-9, "{found:?}");
    }

    #[test]
    fn a_divalent_ion_carries_two_charges_per_mole() {
        // CaSO4, one millimole per litre: 40.078 mg of Ca+2 and 96.056 mg of
        // SO4-2, two milliequivalents each.
        let found = balance(&[("Ca+2", 40.078), ("SO4-2", 96.056)]).unwrap();
        assert!((found.cations - 2.0).abs() < 1e-12);
        assert!((found.anions - 2.0).abs() < 1e-12);
        // Calcium bicarbonate, Ca(HCO3)2: one Ca+2 for two HCO3-.
        let found = balance(&[("Ca+2", 40.078), ("HCO3-", 2.0 * 61.016)]).unwrap();
        assert!(found.error.abs() < 1e-12, "{found:?}");
    }

    #[test]
    fn the_error_is_signed_and_relative_to_the_total_charge() {
        // 3 meq/L of cations against 1 of anions: 100 · 2 / 4 = +50 %.
        let found = balance(&[("Na+", 3.0 * 22.990), ("Cl-", 35.45)]).unwrap();
        assert!((found.error - 50.0).abs() < 1e-9, "{found:?}");
        let found = balance(&[("Na+", 22.990), ("Cl-", 3.0 * 35.45)]).unwrap();
        assert!((found.error + 50.0).abs() < 1e-9, "{found:?}");
    }

    #[test]
    fn what_cannot_be_balanced_is_refused_by_name() {
        assert!(matches!(balance(&[("NO3-2", 10.0), ("Na+", 5.0)]), Err(BalanceError::Unknown { .. })));
        assert!(matches!(balance(&[("Na+", -1.0), ("Cl-", 5.0)]), Err(BalanceError::BadConcentration { .. })));
        assert!(matches!(balance(&[("Na+", f64::NAN), ("Cl-", 5.0)]), Err(BalanceError::BadConcentration { .. })));
        assert!(matches!(balance(&[("Na+", 1.0), ("Na+", 1.0), ("Cl-", 5.0)]), Err(BalanceError::Twice { .. })));
        assert_eq!(balance(&[("Cl-", 5.0)]), Err(BalanceError::NoCation));
        assert_eq!(balance(&[("Na+", 5.0), ("Cl-", 0.0)]), Err(BalanceError::NoAnion));
    }
}
