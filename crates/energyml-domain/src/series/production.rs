//! PRODML 2.3 `WellProductionParameters`: period by period, a start, a
//! duration, and a rate per product — the quantity is the rate times the
//! period's **own** duration.
//!
//! `ProductionWellPeriod` (SimpleProductVolume.xsd l. 465–491): `StartTime`
//! 1..1, `Duration` 1..1 (an `eml:TimeMeasure`), `ProductRate` 1..n, each with
//! a `VolumeFlowRate` and/or a `MassFlowRate`. A period is not a month: the
//! duration the file states is the one multiplied, never an assumed calendar step.
//!
//! # Units are checked, never converted blindly
//!
//! The duration's unit must be a time unit `energyml-domain::units` knows; a
//! rate's unit must read `<quantity>/<time>` with the time a unit it knows —
//! `m3/h`, `bbl/d`, `kg/s`. The quantity is then
//! `rate × duration / (one time unit of the rate)`, both times in seconds
//! through [`crate::units::seconds`], and its unit is the rate's numerator
//! **as written**: this build has no table of volumes or masses, so it
//! converts nothing it cannot check. Any other unit is [`Refused::Unit`] —
//! a quantity computed from a unit read wrong is a number with nothing to show
//! it is wrong.
//!
//! A product with no rate at all is kept with none: the schema allows it, and
//! a zero would be a measurement nobody made.

use serde_json::{Value, json};

use energyml_core::value::Reflect;

use super::{Refused, instant, number_of, objects, reference, text};

/// One production period.
#[derive(Clone, Debug, PartialEq)]
pub struct Period {
    /// Its start, as written.
    pub start: String,
    /// Its start, in Unix seconds.
    pub start_seconds: f64,
    /// Its duration, in its own unit.
    pub duration: f64,
    /// That unit.
    pub duration_uom: String,
    /// Its duration in seconds.
    pub duration_seconds: f64,
    /// The well's status over it, as written.
    pub status: Option<String>,
    /// The reporting entity it reports for.
    pub reporting_entity: Option<String>,
    /// A rate per product and basis.
    pub products: Vec<Product>,
}

/// What a product did over a period.
#[derive(Clone, Debug, PartialEq)]
pub struct Product {
    /// The product, as written (`water - processed`, `oil`, …).
    pub product: String,
    /// `volume` or `mass` — which rate of `ProductRate` this is; `None` for a
    /// product the period gives no rate for.
    pub basis: Option<&'static str>,
    /// The rate.
    pub rate: Option<f64>,
    /// Its unit, as written.
    pub rate_uom: Option<String>,
    /// Rate × duration.
    pub quantity: Option<f64>,
    /// The rate's numerator, as written.
    pub quantity_uom: Option<String>,
}

impl Period {
    /// As the bridges hand it to a front end.
    pub fn to_json(&self) -> Value {
        json!({
            "start": self.start,
            "startSeconds": self.start_seconds,
            "duration": self.duration,
            "durationUom": self.duration_uom,
            "durationSeconds": self.duration_seconds,
            "status": self.status,
            "reportingEntity": self.reporting_entity,
            "products": self.products.iter().map(|one| json!({
                "product": one.product,
                "basis": one.basis,
                "rate": one.rate,
                "rateUom": one.rate_uom,
                "quantity": one.quantity,
                "quantityUom": one.quantity_uom,
            })).collect::<Vec<_>>(),
        })
    }
}

/// Whether this reads the object — a `WellProductionParameters`, by its
/// periods.
pub fn reads(object: &dyn Reflect) -> bool {
    object.type_info().field_ignore_case("ProductionPeriod").is_some()
        && object.type_info().type_key == "WellProductionParameters"
}

/// Read every period.
pub fn read(object: &dyn Reflect) -> Result<Vec<Period>, Refused> {
    if !reads(object) {
        return Err(Refused::NotThisReader {
            expected: "a PRODML WellProductionParameters",
            found: object.type_info().type_key.to_string(),
        });
    }
    let mut out = Vec::new();
    for (rank, period) in objects(object, "ProductionPeriod").into_iter().enumerate() {
        let name = format!("period {}", rank + 1);
        let start = text(period, "StartTime").ok_or_else(|| Refused::Missing {
            what: format!("the StartTime of {name}"),
        })?;
        let start_seconds = instant::unix_seconds(&start).map_err(|why| Refused::Instant {
            at: name.clone(),
            text: start.clone(),
            why,
        })?;
        let duration_object =
            objects(period, "Duration")
                .into_iter()
                .next()
                .ok_or_else(|| Refused::Missing {
                    what: format!("the Duration of {name}"),
                })?;
        let duration = number_of(duration_object, "value").ok_or_else(|| Refused::Missing {
            what: format!("the Duration of {name}"),
        })?;
        let duration_uom = text(duration_object, "uom").unwrap_or_default();
        let duration_seconds =
            crate::units::seconds(duration, &duration_uom).map_err(|why| Refused::Unit {
                what: format!("the duration of {name}"),
                uom: duration_uom.clone(),
                why: why.to_string(),
            })?;

        let mut products = Vec::new();
        for rate in objects(period, "ProductRate") {
            let product = text(rate, "ProductFluidKind").unwrap_or_default();
            // `VolumeFlowRate` is a `FlowRateValue` whose `FlowRate` carries
            // the measure; `MassFlowRate` is the measure itself.
            let volume = objects(rate, "VolumeFlowRate")
                .into_iter()
                .next()
                .and_then(|value| objects(value, "FlowRate").into_iter().next());
            let mass = objects(rate, "MassFlowRate").into_iter().next();
            let mut any = false;
            for (basis, measure) in [("volume", volume), ("mass", mass)] {
                let Some(measure) = measure else { continue };
                any = true;
                let value = number_of(measure, "value").ok_or_else(|| Refused::Missing {
                    what: format!("the {basis} rate of `{product}` in {name}"),
                })?;
                let uom = text(measure, "uom").unwrap_or_default();
                let what = format!("the {basis} rate of `{product}` in {name}");
                let (numerator, denominator) = uom.rsplit_once('/').ok_or_else(|| Refused::Unit {
                    what: what.clone(),
                    uom: uom.clone(),
                    why: "a rate reads `<quantity>/<time>`, and this one has no `/`".into(),
                })?;
                let per = crate::units::seconds(1.0, denominator).map_err(|why| Refused::Unit {
                    what: what.clone(),
                    uom: uom.clone(),
                    why: format!("`{denominator}` is not a time this build converts — {why}"),
                })?;
                if numerator.trim().is_empty() {
                    return Err(Refused::Unit {
                        what,
                        uom,
                        why: "the rate names no quantity before its `/`".into(),
                    });
                }
                products.push(Product {
                    product: product.clone(),
                    basis: Some(basis),
                    rate: Some(value),
                    rate_uom: Some(uom.clone()),
                    quantity: Some(value * duration_seconds / per),
                    quantity_uom: Some(numerator.to_string()),
                });
            }
            if !any {
                products.push(Product {
                    product,
                    basis: None,
                    rate: None,
                    rate_uom: None,
                    quantity: None,
                    quantity_uom: None,
                });
            }
        }
        out.push(Period {
            start,
            start_seconds,
            duration,
            duration_uom,
            duration_seconds,
            status: text(period, "WellStatus"),
            reporting_entity: reference(period, "ReportingEntity").map(|(uuid, _)| uuid),
            products,
        });
    }
    Ok(out)
}
