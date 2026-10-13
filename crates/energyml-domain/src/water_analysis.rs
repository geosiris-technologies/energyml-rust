//! The ions a PRODML `WaterAnalysis` states, as `energyml_analysis::ion_balance`
//! and `energyml_analysis::water_diagrams` take them: a formula and a
//! concentration in **mg/L**, one per component.
//!
//! # The shape
//!
//! `WaterAnalysis.WaterSampleComponent` 0..n (FluidAnalysis.xsd), each naming
//! its ion by `Anion` (`AnionKind`) or `Cation` (`CationKind`) and giving it as
//! `VolumeConcentration` — a `MassPerVolumeMeasure`. That is how
//! `energyml-build`'s `fluid-sample` writes them (`fluid_sample.rs`,
//! `component`), and read here **by shape**: the fields are looked up by name,
//! never through a PRODML release's types.
//!
//! The formulas are kept as the file spells them. `AnionKind` and `CationKind`
//! write `Ca+2`, `HCO3-`, `SO4-2` — the spelling `ion_balance::IONS` uses — so
//! an ion the balance does not know (nitrate, spelled `NO3-2` by PRODML 2.3) is
//! refused **there**, by its formula, with the reason `ion_balance` gives.
//!
//! # What is refused, and why
//!
//! - **No component.** An analysis stating no ion is not a water with nothing
//!   dissolved in it, and a diagram of nothing would be drawn at a corner.
//! - **A component that is not an ion** — an `OrganicAcid`, or a component
//!   naming neither anion nor cation. An organic acid carries a charge the
//!   balance does not count; leaving it out would unbalance an analysis that
//!   balances, or balance one that does not.
//! - **A concentration that is not a mass per volume this reader converts.**
//!   Only `VolumeConcentration` is read (a `MassConcentration` is a mass per
//!   *mass*, which needs a density to become mg/L), and only in the four units
//!   of [`TO_MG_PER_L`], whose factors are exact. `energyml-domain::units` has
//!   no mass-per-volume table; a unit converted by guess is a concentration off
//!   by a thousand with nothing to show it.

use energyml_core::value::Reflect;

use crate::series::{number_of, objects, text};

/// The mass-per-volume units read, and what one of each is in mg/L — exact by
/// definition of the prefixes (1 L = 10⁻³ m³).
pub const TO_MG_PER_L: &[(&str, f64)] = &[("mg/L", 1.0), ("g/L", 1_000.0), ("g/m3", 1.0), ("kg/m3", 1_000.0)];

/// What reading a water analysis refuses, by name. Ranks count from 1.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum WaterRead {
    /// Not a water analysis.
    #[error("`{found}` is no water analysis: it holds no WaterSampleComponent")]
    NotAnAnalysis {
        /// The type met.
        found: String,
    },
    /// An analysis stating no ion.
    #[error("the analysis states no WaterSampleComponent: no ion to place on a diagram")]
    NoComponent,
    /// An organic acid, whose charge the balance does not count.
    #[error("component {rank} is the organic acid `{acid}`: its charge is not in the ion balance, and leaving it out would change the balance")]
    OrganicAcid {
        /// Its rank.
        rank: usize,
        /// As written.
        acid: String,
    },
    /// A component naming no ion.
    #[error("component {rank} names neither an anion nor a cation")]
    NoIon {
        /// Its rank.
        rank: usize,
    },
    /// A component with no mass-per-volume concentration.
    #[error("component {rank} (`{formula}`) states no VolumeConcentration — {why}")]
    NoConcentration {
        /// Its rank.
        rank: usize,
        /// Its formula.
        formula: String,
        /// What it states instead.
        why: String,
    },
    /// A unit this reader does not convert.
    #[error("component {rank} (`{formula}`) is in `{uom}`, which this reader does not convert to mg/L (it reads {known})")]
    Unit {
        /// Its rank.
        rank: usize,
        /// Its formula.
        formula: String,
        /// The unit met.
        uom: String,
        /// The units read.
        known: String,
    },
}

/// Whether this reads the object — a water analysis, by the field holding its
/// components.
pub fn reads(object: &dyn Reflect) -> bool {
    object.type_info().field_ignore_case("WaterSampleComponent").is_some()
}

/// The ions a water analysis states, in the order written: formula, mg/L.
pub fn ions(analysis: &dyn Reflect) -> Result<Vec<(String, f64)>, WaterRead> {
    if !reads(analysis) {
        return Err(WaterRead::NotAnAnalysis { found: analysis.type_info().type_key.to_string() });
    }
    let components = objects(analysis, "WaterSampleComponent");
    if components.is_empty() {
        return Err(WaterRead::NoComponent);
    }
    let mut out = Vec::with_capacity(components.len());
    for (at, component) in components.into_iter().enumerate() {
        let rank = at + 1;
        if let Some(acid) = text(component, "OrganicAcid") {
            return Err(WaterRead::OrganicAcid { rank, acid });
        }
        let formula = text(component, "Anion").or_else(|| text(component, "Cation")).ok_or(WaterRead::NoIon { rank })?;
        let Some(measure) = objects(component, "VolumeConcentration").into_iter().next() else {
            let why = if objects(component, "MassConcentration").into_iter().next().is_some() {
                "a MassConcentration is a mass per mass, which needs a density to become mg/L"
            } else if objects(component, "MolarConcentration").into_iter().next().is_some() {
                "a MolarConcentration is an amount of substance, not a mass per volume"
            } else {
                "it states no concentration at all"
            };
            return Err(WaterRead::NoConcentration { rank, formula, why: why.to_string() });
        };
        let uom = text(measure, "uom").unwrap_or_default();
        let value = number_of(measure, "value").unwrap_or(f64::NAN);
        let Some((_, factor)) = TO_MG_PER_L.iter().find(|(known, _)| *known == uom) else {
            return Err(WaterRead::Unit {
                rank,
                formula,
                uom,
                known: TO_MG_PER_L.iter().map(|(known, _)| *known).collect::<Vec<_>>().join(", "),
            });
        };
        // A value that is not a number reaches `ion_balance`, which refuses it
        // by its formula (`BadConcentration`).
        out.push((formula, value * factor));
    }
    Ok(out)
}
