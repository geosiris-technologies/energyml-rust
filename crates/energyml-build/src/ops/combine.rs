//! `combine` - a property derived from two others, value by value: a ratio, a
//! difference, the log of a ratio.
//!
//! The arithmetic is [`energyml_analysis::combine`] and the writing is
//! [`super::property::write_shaped`]: one arithmetic, one writer. What this
//! operation adds is the refusals between them - two properties on two
//! supports, a unit that disagrees with the computed one, nothing defined - and
//! the activity that records both parents.
//!
//! # The shape of the parents
//!
//! A property of an UDAR cube holds a matrix per node (`[81, 81]`); the derived
//! property keeps that shape. The caller states it (`valuesPerElement`, and the
//! `componentLabels` when the parents have some), as read from the parents'
//! `ValueCountPerIndexableElement` and `LabelPerComponent`; left empty, one
//! value per element.
//!
//! # References
//!
//! - RESQML Technical Reference Guide v2.2, section 7.1 p. 222:
//!   `ValueCountPerIndexableElement` and `LabelPerComponent`.

use energyml_analysis::combine::{self as derived, Combines};
use energyml_domain::property::shape::{ElementShape, ShapeRefusal};
use energyml_domain::provenance::Parameter;
use serde_json::json;

use crate::declare::{Choice, Field, Inputs, ObjectIn, Params, Suggestions, known_named};
use crate::operation::{About, Category, Context, Need, Note, Operation, Plan, Refusal, Slot};
use crate::pieces::{Pieces, Source};

use super::property::{PropertyParams, Timing};

/// The operation. (`Combine` is the analysis trait's name: the operation is
/// `CombineProperties`.)
pub struct CombineProperties;

/// How the two are combined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Choice)]
pub enum CombineChoice {
    /// a / b, unitless
    Ratio,
    /// a - b, in the unit of a
    Difference,
    /// log10 a - log10 b, unitless
    LogRatio,
}

impl CombineChoice {
    /// The id of [`energyml_analysis::combine::Combines`] it names.
    pub fn id(self) -> &'static str {
        match self {
            CombineChoice::Ratio => "ratio",
            CombineChoice::Difference => "difference",
            CombineChoice::LogRatio => "log-ratio",
        }
    }
}

/// What `combine` is told.
#[derive(Debug, Params)]
pub struct CombineParams {
    /// how the two properties are combined, value by value
    pub operation: CombineChoice,
    /// the values per element of both properties, slowest first - their `ValueCountPerIndexableElement`, as read from them; left empty, one value per element
    #[param(name = "valuesPerElement")]
    pub value_count: Vec<f64>,
    /// the labels of the components, in the same order - their `LabelPerComponent`, when they have some
    pub component_labels: Vec<String>,
    /// The property written: the representation, the kind, the unit, the title, the file.
    #[param(flatten)]
    pub written: PropertyParams,
}

/// What `combine` takes.
#[derive(Debug, Inputs)]
pub struct CombineInputs {
    /// the first property's values - the numerator, or what is subtracted from
    pub a: Field,
    /// the second property's values
    pub b: Field,
    /// the two properties, recorded as the activity's inputs
    #[input(types = "resqml22.ContinuousProperty, resqml20.obj_ContinuousProperty")]
    pub parents: Vec<ObjectIn>,
}

/// The counts of `valueCount`, as whole numbers of at least one.
fn counts_of(value_count: &[f64]) -> Result<Vec<u64>, Refusal> {
    value_count
        .iter()
        .map(|&count| {
            if count.is_finite() && count >= 1.0 && count.fract() == 0.0 && count < 9.0e15 {
                Ok(count as u64)
            } else {
                Err(Refusal::on(
                    "valuesPerElement",
                    format!("`{count}` is not a count: each entry is a whole number, 1 or more"),
                ))
            }
        })
        .collect()
}

impl Operation for CombineProperties {
    type Params = CombineParams;
    type Inputs = CombineInputs;

    fn about(&self) -> About {
        About {
            name: "combine",
            describe: "a property derived from two others value by value - a ratio, a difference, the log of a ratio - with the shape of its parents",
            category: Category::Properties,
            makes: &["resqml22.ContinuousProperty"],
            recipe: "combine",
            represents: None,
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        let Some(over) = bag.iter().find_map(|(name, source)| match source {
            Source::Field { over, .. } if name == "a" => Some(over.as_str()),
            _ => None,
        }) else {
            return;
        };
        if let Some(reference) = known_named(bag, over).and_then(|known| known.reference.as_ref()) {
            slots.inferred("representation", reference.dor(), reference.clone());
            slots.set("title", json!(format!("{} - property", reference.title)));
        }
    }

    fn check(&self, inputs: &CombineInputs) -> Result<(), Refusal> {
        let (a, b) = (&inputs.a, &inputs.b);
        if a.over != b.over {
            return Err(Refusal::on(
                "b",
                format!(
                    "lies over `{}`, and `a` over `{}` - two properties combine on one support",
                    b.over, a.over
                ),
            ));
        }
        if a.on != b.on {
            return Err(Refusal::on(
                "b",
                format!(
                    "lies on `{}`, and `a` on `{}` - two properties combine on one indexable element",
                    b.on, a.on
                ),
            ));
        }
        if a.values.len() != b.values.len() {
            return Err(Refusal::on(
                "b",
                format!(
                    "holds {} value(s), and `a` {} - two properties combine value by value",
                    b.values.len(),
                    a.values.len()
                ),
            ));
        }
        Ok(())
    }

    fn plan(&self, inputs: CombineInputs, params: CombineParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
        let (a, b) = (&inputs.a, &inputs.b);
        let counts = counts_of(&params.value_count)?;
        let shape = ElementShape::of(&counts, &params.component_labels).map_err(|why| match why {
            ShapeRefusal::LabelCount { .. } => Refusal::on("componentLabels", why.to_string()),
            other => Refusal::on("valuesPerElement", other.to_string()),
        })?;
        let combines = Combines::builtin();
        let op = combines
            .get(params.operation.id())
            .ok_or_else(|| Refusal::on("operation", format!("no combination `{}`", params.operation.id())))?;
        let combined = derived::combine(&a.values, &b.values, a.uom.as_deref(), b.uom.as_deref(), op)
            .map_err(|why| Refusal::on("b", why.to_string()))?;
        if combined.values.iter().all(|value| value.is_nan()) {
            return Err(Refusal::on(
                "a",
                format!(
                    "every pair is absent or undefined ({} absent, {} undefined): there is nothing to write",
                    combined.absent, combined.undefined
                ),
            ));
        }
        // The unit the combination is in: never a silent replacement of the
        // one that was asked for.
        let computed = combined.uom.clone().unwrap_or_else(|| "Euc".to_string());
        if params.written.uom != computed {
            return Err(Refusal::on(
                "uom",
                format!(
                    "`{}` is given, and a {} of `{}` and `{}` is in `{computed}`",
                    params.written.uom,
                    op.id(),
                    a.name,
                    b.name
                ),
            ));
        }
        let field = Field {
            name: format!("{} of {} and {}", op.id(), a.name, b.name),
            over: a.over.clone(),
            on: a.on.clone(),
            values: combined.values.clone(),
            uom: combined.uom.clone(),
        };
        let seed = cx.seed(&params.written.title.0);
        let mut plan = super::property::write_shaped(
            &field,
            known_named(cx.bag, &a.over),
            &params.written,
            &seed,
            cx,
            &Timing::Timeless,
            &shape,
        )?;
        plan.activity.recipe = "combine";
        plan.activity.title = format!("combine → {}", params.written.title.0);
        for parent in &inputs.parents {
            if let Some(reference) = &parent.known.reference {
                plan.activity.parameters.push(Parameter::Object {
                    title: "input".to_string(),
                    reference: reference.dor(),
                });
            }
        }
        plan.activity.parameters.push(Parameter::Text {
            title: "operation".to_string(),
            value: op.id().to_string(),
        });
        plan.activity.parameters.push(Parameter::Integer {
            title: "absent".to_string(),
            value: combined.absent as i64,
        });
        plan.activity.parameters.push(Parameter::Integer {
            title: "undefined".to_string(),
            value: combined.undefined as i64,
        });
        plan.activity.parameters.push(Parameter::Double {
            title: "convertedBy".to_string(),
            value: combined.converted_by,
            uom: None,
        });
        plan.says.push(Note {
            key: "combined",
            detail: format!(
                "{} value(s): {} absent, {} undefined",
                combined.values.len(),
                combined.absent,
                combined.undefined
            ),
        });
        if combined.converted_by != 1.0 {
            plan.says.push(Note {
                key: "converted",
                detail: format!(
                    "`{}` was converted to the unit of `{}` by a factor of {}",
                    b.name, a.name, combined.converted_by
                ),
            });
        }
        Ok(plan)
    }

    fn reports(&self) -> Vec<Slot> {
        vec![
            Slot::required("indexable", Need::Text, "the indexable element the values lie on"),
            Slot::required(
                "valueCount",
                Need::Number {
                    uom: None,
                    least: None,
                    most: None,
                },
                "values written",
            ),
            Slot::required(
                "absent",
                Need::Integer {
                    least: Some(0),
                    most: None,
                },
                "pairs where an input was absent",
            ),
            Slot::required(
                "undefined",
                Need::Integer {
                    least: Some(0),
                    most: None,
                },
                "pairs of two defined values whose combination is undefined",
            ),
            Slot::optional(
                "convertedBy",
                Need::Number {
                    uom: None,
                    least: None,
                    most: None,
                },
                "the factor the second property was multiplied by, to be in the unit of the first",
            ),
        ]
    }
}
