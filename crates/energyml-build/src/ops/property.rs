//! `property` — a `ContinuousProperty` from a field of values laid over a
//! representation of the package.
//!
//! The values come as a [`Source::Field`] — one number per indexable element
//! of a named piece — and the representation they sit on as the
//! [`crate::pieces::Source::Object`] that piece names, or the object the piece
//! is paired with. What the old promoter's `property`
//! arm did, with the one check it made — *the representation holds that many
//! nodes, the values that many* — kept, and made from what the input knows
//! rather than from a store: `Known::node_count` is filled by the reader of
//! objects, and a field over `nodes` that does not add up is refused by name.
//! A field over anything else cannot be counted here and the plan says so.

use serde_json::json;

use energyml_domain::property::IndexableElement;
use energyml_domain::provenance::Parameter;

use crate::declare::{Field, Inputs, ObjectRef, Params, PropertyKindRef, Suggestions, Title, known_named};
use crate::operation::{
    About, ActivityDraft, Category, Context, Dataset, Need, NewObject, Note, Numbers, Operation, Plan,
    Refusal, Slot,
};
use crate::pieces::{Known, Pieces, Source};

use super::support;

/// The operation.
pub struct Property;

/// What `property` is told — and what `thickness` and `surface-map` hand it,
/// typed, when they delegate the writing.
#[derive(Debug, Clone, Params)]
pub struct PropertyParams {
    /// the representation the values are laid on
    pub representation: ObjectRef,
    /// what is measured — a property kind of the package or of the standard dictionary
    pub property_kind: PropertyKindRef,
    /// the unit of the values — an Energistics uom code, `Euc` for none
    pub uom: String,
    /// the property's title
    pub title: Title,
    /// the external file the values go into
    #[param(default = String::from("data.h5"))]
    pub data_file: String,
}

/// What `property` takes.
#[derive(Debug, Inputs)]
pub struct PropertyInputs {
    /// the values to write, and what they lie on
    pub field: Field,
}

impl Operation for Property {
    type Params = PropertyParams;
    type Inputs = PropertyInputs;

    fn about(&self) -> About {
        About {
            name: "property",
            describe: "a continuous property from a field of values laid over a representation — one value per node or cell",
            category: Category::Properties,
            makes: &["resqml22.ContinuousProperty"],
            recipe: "property",
            represents: None,
        }
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        let Some((over, uom)) = bag.iter().find_map(|(_, source)| match source {
            Source::Field { over, uom, .. } => Some((over.as_str(), uom.clone())),
            _ => None,
        }) else {
            return;
        };
        if let Some(unit) = uom {
            slots.set("uom", json!(unit));
        }
        if let Some(reference) = known_named(bag, over).and_then(|known| known.reference.as_ref()) {
            slots.inferred("representation", reference.dor(), reference.clone());
            slots.set("title", json!(format!("{} — property", reference.title)));
        }
    }

    fn check(&self, inputs: &PropertyInputs) -> Result<(), Refusal> {
        if inputs.field.values.is_empty() {
            return Err(Refusal::on("field", "holds no value"));
        }
        if IndexableElement::parse(&inputs.field.on).is_none() {
            return Err(Refusal::on(
                "field",
                format!(
                    "`{}` is not an indexable element (nodes, cells, …)",
                    inputs.field.on
                ),
            ));
        }
        Ok(())
    }

    fn plan(
        &self,
        inputs: PropertyInputs,
        params: PropertyParams,
        cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        let known = known_named(cx.bag, &inputs.field.over);
        let seed = cx.seed(&params.title.0);
        write(&inputs.field, known, &params, &seed, cx)
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
        ]
    }
}

/// **The property, written** — what the operation does, and what `thickness`
/// and `surface-map` call directly with the values they computed: typed in, no
/// bag to derive. `known` is what is known of the representation the values
/// lie on — the one count this can check, its nodes; `seed` is the caller's.
pub fn write(
    field: &Field,
    known: Option<&Known>,
    params: &PropertyParams,
    seed: &str,
    cx: &Context<'_>,
) -> Result<Plan, Refusal> {
    write_timed(field, known, params, seed, cx, &Timing::Timeless)
}

/// When the values of a property hold — the two forms RESQML 2.2 has for a
/// property through time, and neither.
///
/// `AbstractProperty` states `Time` (`GeologicTime`, 0..1) and
/// `TimeOrIntervalSeries` (0..1) — RESQML Technical Reference Guide v2.2 §7.1
/// p. 222 — and **no time index**: the one a draft of the usage guide showed
/// is absent from the final XSD.
#[derive(Debug, Clone, PartialEq)]
pub enum Timing {
    /// No time is said.
    Timeless,
    /// The values hold at one instant: `Time/DateTime`, a W3CDTF date-time as
    /// written — one property per step of a series.
    At(String),
    /// The values hold at every step of a series, **time the slowest
    /// dimension** (TRG v2.2 §7, the note of the diagram p. 221:
    /// *“\[TimeCount] … \[IndexableElementCount] …”*): `steps` × the elements,
    /// the array `[steps, elements]`.
    Series {
        /// The `eml23.TimeSeries`, as a `DataObjectReference`.
        series: serde_json::Value,
        /// `UseInterval`: one step per interval from an instant to the next.
        use_interval: bool,
        /// How many steps the values hold — the series' instants, one fewer
        /// over intervals.
        steps: usize,
    },
}

/// [`write()`], with the time the values hold at — the one writer of a
/// `ContinuousProperty`, timed or not (`property-time-series` calls it).
pub fn write_timed(
    field: &Field,
    known: Option<&Known>,
    params: &PropertyParams,
    seed: &str,
    cx: &Context<'_>,
    timing: &Timing,
) -> Result<Plan, Refusal> {
    let Field { over, on, values, .. } = field;
    // How many steps the values hold, and how many values each step has.
    let steps = match timing {
        Timing::Series { steps: 0, .. } => {
            return Err(Refusal::on("field", "a series of no step holds no value"));
        }
        Timing::Series { steps, .. } => *steps,
        _ => 1,
    };
    if values.len() % steps != 0 {
        return Err(Refusal::on(
            "field",
            format!(
                "{} value(s) do not make {steps} step(s) of equal length — each step holds one value per element",
                values.len()
            ),
        ));
    }
    let per_step = values.len() / steps;
    let element = IndexableElement::parse(on)
        .ok_or_else(|| Refusal::on("field", format!("`{on}` is not an indexable element")))?;
    let representation = match &params.representation.0 {
        value if value.is_object() && value["Uuid"].is_string() => value.clone(),
        other => {
            return Err(Refusal::on(
                "representation",
                format!("expected a DataObjectReference, got {other}"),
            ));
        }
    };
    let kind = match &params.property_kind.0 {
        value if value.is_object() && value["Uuid"].is_string() => value.clone(),
        other => {
            return Err(Refusal::on(
                "propertyKind",
                format!("expected a DataObjectReference, got {other}"),
            ));
        }
    };
    let uom = params.uom.clone();
    let title = params.title.0.clone();
    let seed = seed.to_string();
    let mut says = Vec::new();
    // The one count this operation can check: the nodes of the piece the
    // field lies on, when the reader counted them.
    match (element, known.and_then(|k| k.node_count)) {
        (IndexableElement::Nodes, Some(nodes)) if nodes != per_step => {
            return Err(Refusal::on(
                "field",
                format!(
                    "`{over}` holds {nodes} node(s) and the field {per_step} value(s){} — values would land on the wrong elements",
                    if steps > 1 { format!(" per step, over {steps} steps") } else { String::new() }
                ),
            ));
        }
        (IndexableElement::Nodes, Some(_)) => {}
        _ => says.push(Note {
            key: "count-unchecked",
            detail: format!(
                "{} value(s) over `{on}`: the input does not say how many the representation holds, so the count is not checked here",
                values.len()
            ),
        }),
    }

    let mut rank = 0usize;
    let uuid = support::next_uuid(cx.identity, &format!("property:{seed}"), &mut rank)?;
    let file = params.data_file.clone();
    let path = format!("/resqml22/{uuid}/values");
    // Time slowest: a series is `[steps, elements]`, anything else `[values]`.
    let dimensions: Vec<i64> = match timing {
        Timing::Series { .. } => vec![steps as i64, per_step as i64],
        _ => vec![values.len() as i64],
    };
    let mut object = json!({
        "$type": "resqml22.ContinuousProperty",
        "uuid": uuid,
        "schemaVersion": "2.2",
        "Citation": support::citation(cx.when, &title),
        "IndexableElement": on,
        "ValueCountPerIndexableElement": [1],
        "PropertyKind": kind,
        "SupportingRepresentation": representation,
        "Uom": uom,
        "ValuesForPatch": [{
            "$type": "eml23.FloatingPointExternalArray",
            "ArrayFloatingPointType": "arrayOfDouble64LE",
            "CountPerValue": 1,
            "Values": support::external(&file, &path, &dimensions),
        }],
    });
    match timing {
        Timing::Timeless => {}
        Timing::At(instant) => {
            object["Time"] = energyml_core::json::write_value(&energyml_eml_2_3::GeologicTime {
                date_time: instant.clone(),
                ..Default::default()
            });
        }
        Timing::Series {
            series, use_interval, ..
        } => {
            object["TimeOrIntervalSeries"] = json!({
                "$type": "eml23.TimeOrIntervalSeries",
                "UseInterval": use_interval,
                "TimeSeries": series,
            });
        }
    }
    Ok(Plan {
        media: Vec::new(),
        blocked: None,
        objects: vec![NewObject {
            uuid,
            qualified_type: "resqml22.ContinuousProperty".to_string(),
            role: "property".to_string(),
            title: title.clone(),
            json: object,
        }],
        datasets: vec![Dataset {
            file,
            path,
            dimensions,
            values: Numbers::F64(values.to_vec()),
        }],
        // **No shape**: a property is laid *on* a geometry, it
        // makes none.
        shapes: Vec::new(),
        activity: ActivityDraft {
            recipe: "property",
            title: format!("property → {title}"),
            seed,
            parameters: vec![
                Parameter::Object {
                    title: "input".to_string(),
                    reference: representation,
                },
                Parameter::Text {
                    title: "indexable".to_string(),
                    value: on.to_string(),
                },
                Parameter::Text {
                    title: "uom".to_string(),
                    value: uom,
                },
                Parameter::Double {
                    title: "valueCount".to_string(),
                    value: values.len() as f64,
                    uom: None,
                },
            ],
        },
        says,
        dropped: Vec::new(),
    })
}
