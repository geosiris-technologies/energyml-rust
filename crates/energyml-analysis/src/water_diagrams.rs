//! **Piper and Stiff diagrams of a water analysis** - where a water falls
//! among the facies, and its fingerprint
//! (a PRODML/WITSML business view).
//!
//! Nothing here reads a package or knows PRODML: the input is the analysis as
//! [`crate::ion_balance`] takes it - formulas and concentrations in mg/L - and
//! the milliequivalents come from **its** table of molar masses and charges,
//! never from a second one.
//!
//! # Sources - textbook methods, not Energistics ones
//!
//! - Piper, A.M. (1944), *A graphic procedure in the geochemical
//!   interpretation of water-analyses*, Transactions, American Geophysical
//!   Union 25;
//! - Stiff, H.A. Jr. (1951), *The interpretation of chemical water analysis by
//!   means of patterns*, Journal of Petroleum Technology 3;
//! - Hem, J.D. (1985), *Study and Interpretation of the Chemical
//!   Characteristics of Natural Water*, USGS Water-Supply Paper 2254 - for the
//!   three-axis form of the Stiff pattern.
//!
//! All three are cited from memory: no page is given. The PRODML Technical Usage
//! Guide v2.3 names none of them.
//!
//! TODO(reference): verify - all three citations (Piper 1944, Stiff 1951, Hem 1985) were written from memory and not checked against the sources.
//!
//! # What is refused, and why it is refused before anything is drawn
//!
//! - **An analysis `ion_balance` refuses** - an unknown formula, a negative
//!   concentration, an ion given twice - is refused with its reason
//!   ([`WaterError::Balance`]).
//! - **An analysis that does not balance** within the tolerance the caller
//!   declares, in percent ([`WaterError::Unbalanced`]). A facies drawn from a
//!   typing error is a false facies, and it looks exactly like a true one.
//! - **An ion an axis needs and the analysis does not state**
//!   ([`WaterError::Missing`]). An absent ion is never drawn as zero: a zero
//!   is a measurement (below detection, say), an absence is nobody having
//!   measured. The caller who knows an ion is negligible - carbonate in a
//!   water below pH 8.3 is the usual case - **declares** it absent, and the
//!   result carries the declaration ([`MajorIons::declared_absent`]) so the
//!   front end can say it. Declaring an ion that is also stated, or one no axis
//!   reads, is refused: one of the two statements is wrong.
//!
//! # The axes
//!
//! Both diagrams read the same eight ions, grouped as Piper grouped them:
//! cations `Ca+2`, `Mg+2`, `Na+` with `K+`; anions `Cl-`, `SO4-2`, `HCO3-`
//! with `CO3-2`. The other ions an analysis states (barium, iron, strontium…)
//! enter the balance but no axis - a Piper percentage is a share of the
//! plotted ions, not of every charge in the water.
//!
//! **Piper.** Each ternary is the share, in percent of milliequivalents, of
//! its three axes. The layout is the one Piper's figure is reproduced in
//! (TODO(reference): verify, cited from memory): two equilateral triangles of side 1 on one base line,
//! `gap` apart - cations on the left, `Ca` bottom-left, `Na+K` bottom-right,
//! `Mg` at the apex; anions on the right, `HCO3+CO3` bottom-left, `Cl`
//! bottom-right, `SO4` at the apex - and the diamond above between them. A
//! water's point in the diamond is where the line of constant `Na+K` through
//! its cation point (parallel to the `Ca`-`Mg` side) meets the line of
//! constant `HCO3+CO3` through its anion point (parallel to the `Cl`-`SO4`
//! side). With `√3/2` written `h`, fractions (not percentages) and the anion
//! triangle shifted by `1 + gap`:
//!
//! - cation point `(na_k + mg / 2, h · mg)`;
//! - anion point `(1 + gap + cl + so4 / 2, h · so4)`;
//! - diamond point `((2 + gap + na_k − hco3_co3) / 2, h · (2 + gap − na_k − hco3_co3))`.
//!
//! The diamond's corners are then: bottom, alkali carbonate (`Na+K` and
//! `HCO3+CO3` at 100 %); left, calcium-magnesium bicarbonate; right, sodium
//! chloride-sulphate; top, calcium-magnesium chloride-sulphate. The
//! percentages are the method; the layout is a drawing, and `gap` is the
//! front end's to choose.
//!
//! **Stiff.** Three horizontal axes, milliequivalents per litre, cations to
//! the left and anions to the right, top to bottom: `Na+K` | `Cl`, `Ca` |
//! `HCO3+CO3`, `Mg` | `SO4` - the three-axis form Hem draws. Stiff's own
//! pattern had a fourth axis for iron and carbonate (TODO(reference): verify, cited from memory);
//! iron is seldom stated, and folding carbonate with bicarbonate keeps both
//! diagrams reading the same ions.

use crate::ion_balance::{self, Balance, BalanceError};

/// One axis: its label and the ions it adds up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Axis {
    /// `Na+K`, `HCO3+CO3` - how a diagram labels it.
    pub label: &'static str,
    /// The formulas it adds, as [`crate::ion_balance::IONS`] writes them.
    pub formulas: &'static [&'static str],
}

/// The cation axes, in Piper's order: `Ca`, `Mg`, `Na+K`.
pub const CATION_AXES: [Axis; 3] = [
    Axis {
        label: "Ca",
        formulas: &["Ca+2"],
    },
    Axis {
        label: "Mg",
        formulas: &["Mg+2"],
    },
    Axis {
        label: "Na+K",
        formulas: &["Na+", "K+"],
    },
];

/// The anion axes, in Piper's order: `Cl`, `SO4`, `HCO3+CO3`.
pub const ANION_AXES: [Axis; 3] = [
    Axis {
        label: "Cl",
        formulas: &["Cl-"],
    },
    Axis {
        label: "SO4",
        formulas: &["SO4-2"],
    },
    Axis {
        label: "HCO3+CO3",
        formulas: &["HCO3-", "CO3-2"],
    },
];

/// What a water diagram refuses, by name.
#[derive(Debug, Clone, thiserror::Error, PartialEq)]
pub enum WaterError {
    #[error("the analysis cannot be balanced: {0}")]
    Balance(#[from] BalanceError),
    #[error("the balance tolerance is a finite percentage ≥ 0, not {given}")]
    BadTolerance { given: f64 },
    #[error(
        "the analysis does not balance: {cations:.3} meq/L of cations against {anions:.3} meq/L of anions, an error of {error:.2} % beyond the {tolerance} % declared - a facies drawn from it could be a typing error's"
    )]
    Unbalanced {
        cations: f64,
        anions: f64,
        error: f64,
        tolerance: f64,
    },
    #[error(
        "{formulas:?} not stated, and an axis needs each: an absent ion is never drawn as zero - state it (0 when measured below detection) or declare it absent"
    )]
    Missing { formulas: Vec<&'static str> },
    #[error("`{formula}` is declared absent and stated at {given} mg/L: one of the two is wrong")]
    DeclaredAndStated { formula: String, given: f64 },
    #[error("`{formula}` is declared absent, but no axis reads it - the axes read {known}")]
    NotAnAxisIon { formula: String, known: String },
    #[error("the {which} axes add up to 0 meq/L: no share of nothing to plot")]
    EmptyTernary { which: &'static str },
    #[error("the gap between the triangles is a finite number ≥ 0, not {given}")]
    BadGap { given: f64 },
}

/// One ion stated, in both units.
#[derive(Debug, Clone, PartialEq)]
pub struct Equivalent {
    /// Its formula, as stated.
    pub formula: String,
    /// Its concentration as stated, mg/L.
    pub mg_per_l: f64,
    /// `mg/L ÷ molar mass × |charge|`, meq/L - from [`crate::ion_balance`].
    pub meq_per_l: f64,
}

/// An analysis checked for the diagrams: balanced within the tolerance
/// declared, every axis ion stated or declared absent.
#[derive(Debug, Clone, PartialEq)]
pub struct MajorIons {
    /// Its balance, from [`crate::ion_balance::balance`].
    pub balance: Balance,
    /// Every ion stated, in the order stated - the axis ions and the others.
    pub stated: Vec<Equivalent>,
    /// The axis ions the caller declared absent - counted as 0 by that
    /// declaration, and to be shown with the diagram.
    pub declared_absent: Vec<&'static str>,
    /// meq/L of [`CATION_AXES`], in their order.
    pub cations: [f64; 3],
    /// meq/L of [`ANION_AXES`], in their order.
    pub anions: [f64; 3],
}

/// Every formula an axis reads.
fn axis_formulas() -> impl Iterator<Item = &'static str> {
    CATION_AXES
        .iter()
        .chain(ANION_AXES.iter())
        .flat_map(|axis| axis.formulas.iter().copied())
}

/// The analysis checked for the diagrams - `tolerance` the largest balance
/// error accepted, in percent; `declared_absent` the axis ions the caller
/// states were not measured and are negligible.
pub fn major_ions(
    analysis: &[(&str, f64)],
    tolerance: f64,
    declared_absent: &[&str],
) -> Result<MajorIons, WaterError> {
    if !(tolerance.is_finite() && tolerance >= 0.0) {
        return Err(WaterError::BadTolerance { given: tolerance });
    }
    let balance = ion_balance::balance(analysis)?;
    if balance.error.abs() > tolerance {
        return Err(WaterError::Unbalanced {
            cations: balance.cations,
            anions: balance.anions,
            error: balance.error,
            tolerance,
        });
    }
    let mut declared: Vec<&'static str> = Vec::new();
    for formula in declared_absent {
        let Some(known) = axis_formulas().find(|one| one == formula) else {
            return Err(WaterError::NotAnAxisIon {
                formula: (*formula).to_string(),
                known: axis_formulas().collect::<Vec<_>>().join(", "),
            });
        };
        if let Some((_, given)) = analysis.iter().find(|(stated, _)| stated == formula) {
            return Err(WaterError::DeclaredAndStated {
                formula: (*formula).to_string(),
                given: *given,
            });
        }
        if !declared.contains(&known) {
            declared.push(known);
        }
    }
    let missing: Vec<&'static str> = axis_formulas()
        .filter(|formula| {
            !declared.contains(formula) && !analysis.iter().any(|(stated, _)| stated == formula)
        })
        .collect();
    if !missing.is_empty() {
        return Err(WaterError::Missing { formulas: missing });
    }
    // `balance` has accepted every formula, so each is in the table.
    let stated: Vec<Equivalent> = analysis
        .iter()
        .filter_map(|(formula, mg_per_l)| {
            ion_balance::ion(formula).map(|ion| Equivalent {
                formula: (*formula).to_string(),
                mg_per_l: *mg_per_l,
                meq_per_l: ion_balance::milliequivalents(ion, *mg_per_l),
            })
        })
        .collect();
    let of = |axis: &Axis| -> f64 {
        stated
            .iter()
            .filter(|one| axis.formulas.contains(&one.formula.as_str()))
            .map(|one| one.meq_per_l)
            .sum()
    };
    let cations = CATION_AXES.map(|axis| of(&axis));
    let anions = ANION_AXES.map(|axis| of(&axis));
    Ok(MajorIons {
        balance,
        stated,
        declared_absent: declared,
        cations,
        anions,
    })
}

/// A Piper diagram: the two ternaries in percent, and the analysis they come
/// from.
#[derive(Debug, Clone, PartialEq)]
pub struct Piper {
    /// % of `Ca`, `Mg`, `Na+K` - [`CATION_AXES`]' order; they add up to 100.
    pub cations: [f64; 3],
    /// % of `Cl`, `SO4`, `HCO3+CO3` - [`ANION_AXES`]' order; they add up to 100.
    pub anions: [f64; 3],
    /// What the percentages were computed from.
    pub ions: MajorIons,
}

/// A water's three points in the layout of the module doc.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PiperPoints {
    /// In the cation triangle.
    pub cation: [f64; 2],
    /// In the anion triangle.
    pub anion: [f64; 2],
    /// In the diamond.
    pub diamond: [f64; 2],
}

/// The outline of the diagram, for the front end to draw - the same layout as
/// [`Piper::points`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PiperFrame {
    /// `Ca`, `Na+K`, `Mg` corners.
    pub cation: [[f64; 2]; 3],
    /// `HCO3+CO3`, `Cl`, `SO4` corners.
    pub anion: [[f64; 2]; 3],
    /// Bottom, right, top, left corners.
    pub diamond: [[f64; 2]; 4],
}

/// `√3 / 2`, the height of a unit equilateral triangle.
const H: f64 = 0.866_025_403_784_438_6;

fn checked_gap(gap: f64) -> Result<f64, WaterError> {
    if gap.is_finite() && gap >= 0.0 {
        Ok(gap)
    } else {
        Err(WaterError::BadGap { given: gap })
    }
}

/// The diamond's point for fractions of `Na+K` and of `HCO3+CO3`.
fn diamond(na_k: f64, hco3_co3: f64, gap: f64) -> [f64; 2] {
    [
        (2.0 + gap + na_k - hco3_co3) / 2.0,
        H * (2.0 + gap - na_k - hco3_co3),
    ]
}

impl Piper {
    /// The water's points, triangles `gap` apart.
    pub fn points(&self, gap: f64) -> Result<PiperPoints, WaterError> {
        let gap = checked_gap(gap)?;
        let [_, mg, na_k] = self.cations.map(|one| one / 100.0);
        let [cl, so4, hco3_co3] = self.anions.map(|one| one / 100.0);
        Ok(PiperPoints {
            cation: [na_k + mg / 2.0, H * mg],
            anion: [1.0 + gap + cl + so4 / 2.0, H * so4],
            diamond: diamond(na_k, hco3_co3, gap),
        })
    }

    /// The outline, triangles `gap` apart.
    pub fn frame(gap: f64) -> Result<PiperFrame, WaterError> {
        let gap = checked_gap(gap)?;
        Ok(PiperFrame {
            cation: [[0.0, 0.0], [1.0, 0.0], [0.5, H]],
            anion: [[1.0 + gap, 0.0], [2.0 + gap, 0.0], [1.5 + gap, H]],
            diamond: [
                diamond(1.0, 1.0, gap),
                diamond(1.0, 0.0, gap),
                diamond(0.0, 0.0, gap),
                diamond(0.0, 1.0, gap),
            ],
        })
    }
}

/// The shares of three axes, in percent.
fn shares(meq: [f64; 3], which: &'static str) -> Result<[f64; 3], WaterError> {
    let total: f64 = meq.iter().sum();
    if total <= 0.0 {
        return Err(WaterError::EmptyTernary { which });
    }
    Ok(meq.map(|one| 100.0 * one / total))
}

/// The Piper diagram of an analysis - see [`major_ions`] for `tolerance` and
/// `declared_absent`.
pub fn piper(
    analysis: &[(&str, f64)],
    tolerance: f64,
    declared_absent: &[&str],
) -> Result<Piper, WaterError> {
    let ions = major_ions(analysis, tolerance, declared_absent)?;
    let cations = shares(ions.cations, "cation")?;
    let anions = shares(ions.anions, "anion")?;
    Ok(Piper {
        cations,
        anions,
        ions,
    })
}

/// One horizontal axis of a Stiff pattern.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StiffAxis {
    /// The cation label, drawn to the left.
    pub cation: &'static str,
    /// Its meq/L.
    pub cation_meq: f64,
    /// The anion label, drawn to the right.
    pub anion: &'static str,
    /// Its meq/L.
    pub anion_meq: f64,
}

/// A Stiff pattern: three axes top to bottom, and the analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct Stiff {
    /// `Na+K` | `Cl`, `Ca` | `HCO3+CO3`, `Mg` | `SO4`.
    pub axes: [StiffAxis; 3],
    /// What the values were computed from.
    pub ions: MajorIons,
}

/// The Stiff pattern of an analysis - see [`major_ions`] for `tolerance` and
/// `declared_absent`.
pub fn stiff(
    analysis: &[(&str, f64)],
    tolerance: f64,
    declared_absent: &[&str],
) -> Result<Stiff, WaterError> {
    let ions = major_ions(analysis, tolerance, declared_absent)?;
    // (cation axis, anion axis) as indices into CATION_AXES and ANION_AXES,
    // top to bottom.
    let rows = [(2, 0), (0, 2), (1, 1)];
    let axes = rows.map(|(cation, anion)| StiffAxis {
        cation: CATION_AXES[cation].label,
        cation_meq: ions.cations[cation],
        anion: ANION_AXES[anion].label,
        anion_meq: ions.anions[anion],
    });
    Ok(Stiff { axes, ions })
}
