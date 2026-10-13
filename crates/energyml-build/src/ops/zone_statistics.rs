//! `zone-statistics` — the statistics of a property per zone or per
//! stratigraphic unit, written as an `eml23.ColumnBasedTable`: one row per
//! group.
//!
//! `energyml-domain` has the statistics; what was missing is the slicing —
//! restricting to a sub-representation, to a stratigraphic unit. So:
//!
//! - **the slicing is [`energyml_analysis::groups`]** — a key per element,
//!   the elements without one counted apart;
//! - **the statistics are [`energyml_domain::stats::Summary`]**, one per group,
//!   never a second mean written here. Its rule is the one needed here: a `NaN` —
//!   a null — is counted apart and never averaged;
//! - **this is the seam**: what the values are, what the keys are, and whether
//!   the two lie on the same elements.
//!
//! # Two groupings
//!
//! - **`by = zone`**: the key of an element is a code — a discrete property on
//!   the same elements (a region, a facies, a zone index), given as a field or
//!   as a property of the package. A null code puts its element in no group;
//!   a code that is not a whole number is refused by its rank, never rounded;
//! - **`by = unit`**: the key of an interval of a wellbore frame is the
//!   stratigraphic unit the frame names for it (`IntervalStratigraphicUnits`,
//!   RESQML TRG 2.2 §5.34), so the values must lie on that frame's
//!   `intervals`. An interval the frame leaves without a unit is in no group.
//!
//! The third slicing named, **a sub-representation**, is not here: nothing
//! at the boundary reads its element indices yet, and guessing them from a
//! count would be the misalignment this module refuses.
//!
//! # One set of elements, or no statistics
//!
//! Values and keys pair by position, so they must be two lists over the same
//! elements of the same representation: `nodes` against `cells` is refused,
//! two representations are refused, and two lengths are refused with both
//! counts. A shorter key list would still make a table, with every row wrong.
//!
//! # The table
//!
//! One row per group, in key order: the key (the code, or the rank of the unit
//! in the frame's order), for a unit its title and uuid, the count of values
//! summarised and the count of nulls, then minimum, maximum, mean and the
//! **population** standard deviation — each of the last four under the kind
//! of the values, in their unit, with the facet eml 2.3 has for the first
//! three (`minimum`, `maximum`, `average`, kind `statistics`). There is no
//! facet for a deviation in the closed list, and an extension would be a
//! vocabulary invented here: the column says it by its title and description.
//! A group whose values are all null keeps its row, its statistics `NaN` — a
//! zone with no measure is news, not an omission.

use serde_json::{Value, json};

use energyml_analysis::groups;
use energyml_domain::provenance::Parameter;
use energyml_domain::stats::Summary;
use energyml_domain::values::Values;

use crate::declare::{
    Choice, Field, Inputs, ObjectIn, Params, PropertyKindRef, Suggestions, Title, known_named, known_of,
};
use crate::operation::{
    About, Category, Context, Dataset, Need, NewObject, Note, Operation, Plan, Refusal, Slot,
};
use crate::pieces::{Known, Pieces, Ref};

use super::support;

/// The operation.
pub struct ZoneStatistics;

/// What the values are grouped by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Choice)]
pub enum By {
    /// The zone code of each element — a discrete property on the same elements.
    Zone,
    /// The stratigraphic unit a wellbore frame names for each interval.
    Unit,
}

/// What `zone-statistics` is told.
#[derive(Debug, Params)]
pub struct ZoneStatisticsParams {
    /// group by the zone code of each element, or by the stratigraphic unit a wellbore frame names for each interval
    #[param(default = By::Zone)]
    pub by: By,
    /// what the values measure; left out, the property's kind
    pub property_kind: Option<PropertyKindRef>,
    /// the unit of the values; left out, the property's or the field's
    pub uom: Option<String>,
    /// the table's title; left out, the values' and the grouping's
    pub title: Option<Title>,
}

/// What `zone-statistics` takes.
#[derive(Debug, Inputs)]
pub struct ZoneStatisticsInputs {
    /// the values to summarise — a field laid over a piece, one value per element
    pub values: Option<Field>,
    /// or a property of the package whose values are read
    #[input(
        types = "resqml22.ContinuousProperty, resqml22.DiscreteProperty, resqml20.obj_ContinuousProperty, resqml20.obj_DiscreteProperty"
    )]
    pub property: Option<ObjectIn>,
    /// the zone code of each element — a discrete field over the same elements
    pub zones: Option<Field>,
    /// or a discrete property of the package over the same elements
    #[input(
        types = "resqml22.DiscreteProperty, resqml20.obj_DiscreteProperty, resqml20.obj_CategoricalProperty"
    )]
    pub zone_property: Option<ObjectIn>,
    /// the wellbore frame whose intervals name their stratigraphic unit — to group by unit
    #[input(types = "resqml22.WellboreFrameRepresentation, resqml20.obj_WellboreFrameRepresentation")]
    pub frame: Option<ObjectIn>,
}

/// A list of numbers read from a field or a property, and what it lies on.
struct Read {
    /// One per element; a null is `NaN`.
    values: Vec<f64>,
    /// `nodes`, `cells`, `intervals`, …
    on: String,
    /// The representation it lies on, when that is known.
    support: Option<Ref>,
    /// What to call it in a message and a title.
    name: String,
    uom: Option<String>,
    kind: Option<Ref>,
}

/// The values of a field, and what is known of the piece it lies on.
fn from_field(field: &Field, bag: &Pieces) -> Read {
    let support = known_named(bag, &field.over).and_then(|known| known.reference.clone());
    Read {
        values: field.values.clone(),
        on: field.on.clone(),
        name: support
            .as_ref()
            .map(|s| s.title.clone())
            .unwrap_or_else(|| field.name.clone()),
        support,
        uom: field.uom.clone(),
        kind: None,
    }
}

/// The values of a property of the package, as the boundary read them — or
/// why they cannot be summarised.
fn from_property(object: &ObjectIn, slot: &'static str) -> Result<Read, Refusal> {
    let Some(property) = &object.known.property else {
        return Err(Refusal::on(slot, format!("`{}` is no property", object.name)));
    };
    let values = property.values.as_ref().map_err(|why| {
        Refusal::on(
            slot,
            format!("the values of `{}` could not be read: {why}", object.name),
        )
    })?;
    if property.value_count.iter().product::<u64>() != 1 {
        return Err(Refusal::on(
            slot,
            format!(
                "`{}` holds {:?} values per element — a vector or a tensor has no single statistic",
                object.name, property.value_count
            ),
        ));
    }
    if property.timed || property.realizations {
        return Err(Refusal::on(
            slot,
            format!(
                "`{}` is indexed in time or by realization: its values are several arrays in one, and summarising them together would mix steps",
                object.name
            ),
        ));
    }
    let numbers = match values {
        Values::Floating(list) => list.clone(),
        // The null the array declares is a null, never a value: NaN, which the
        // summary counts apart.
        Values::Integer(list) => list
            .iter()
            .map(|&v| {
                if Some(v) == property.null_value {
                    f64::NAN
                } else {
                    v as f64
                }
            })
            .collect(),
        Values::Boolean(_) | Values::Text(_) => {
            return Err(Refusal::on(
                slot,
                format!("`{}` holds booleans or text, which have no mean", object.name),
            ));
        }
    };
    Ok(Read {
        values: numbers,
        on: property.indexable.clone(),
        support: property.supporting.clone(),
        name: object
            .known
            .reference
            .as_ref()
            .map(|r| r.title.clone())
            .unwrap_or_else(|| object.name.clone()),
        uom: property.uom.clone(),
        kind: property.kind.clone(),
    })
}

/// Zone codes out of numbers: a `NaN` is no zone, a fraction is refused.
fn codes(read: &Read, slot: &'static str) -> Result<Vec<Option<i64>>, Refusal> {
    read.values
        .iter()
        .enumerate()
        .map(|(rank, &v)| {
            if v.is_nan() {
                Ok(None)
            } else if v.is_finite() && v.fract() == 0.0 && v.abs() < 9.0e15 {
                Ok(Some(v as i64))
            } else {
                Err(Refusal::on(slot, format!("element {rank} holds {v}, which is no zone code — a code is a whole number, and rounding it would pick a zone")))
            }
        })
        .collect()
}

/// Whether two lists lie on the same elements — or the refusal that says how
/// they do not.
fn same_elements(values: &Read, keys: &Read, slot: &'static str) -> Result<(), Refusal> {
    if values.on != keys.on {
        return Err(Refusal::on(
            slot,
            format!(
                "lies on `{}` and the values on `{}`: a key pairs with the value of the same element",
                keys.on, values.on
            ),
        ));
    }
    if let (Some(a), Some(b)) = (&values.support, &keys.support)
        && a.uuid != b.uuid
    {
        return Err(Refusal::on(
            slot,
            format!(
                "lies on `{}` and the values on `{}`: two representations, two sets of elements",
                b.title, a.title
            ),
        ));
    }
    Ok(())
}

fn reference(uuid: &str, title: &str) -> Value {
    json!({ "$type": "eml23.DataObjectReference", "Uuid": uuid, "QualifiedType": "eml23.PropertyKind", "Title": title })
}

fn integers(title: &str, kind: Value, values: Vec<i64>, null_value: i64) -> Value {
    json!({
        "Title": title,
        "ValueCountPerRow": 1,
        "PropertyKind": kind,
        "Values": { "$type": "eml23.IntegerXmlArray", "CountPerValue": 1, "NullValue": null_value, "Values": values },
    })
}

fn texts(title: &str, kind: Value, values: Vec<String>) -> Value {
    json!({
        "Title": title,
        "ValueCountPerRow": 1,
        "PropertyKind": kind,
        "Values": { "$type": "eml23.StringXmlArray", "CountPerValue": 1, "Values": values },
    })
}

/// A column of doubles; a `NaN` is written as the `xsd:double` lexical form,
/// which JSON has no number for (the encoding of `energyml_core::json`).
fn reals(
    title: &str,
    kind: &Value,
    uom: Option<&str>,
    facet: Option<&str>,
    description: &str,
    values: &[f64],
) -> Value {
    let values: Vec<Value> = values
        .iter()
        .map(|v| if v.is_nan() { json!("NaN") } else { json!(v) })
        .collect();
    let mut column = json!({
        "Title": title,
        "Description": description,
        "ValueCountPerRow": 1,
        "PropertyKind": kind,
        "Values": { "$type": "eml23.FloatingPointXmlArray", "CountPerValue": 1, "Values": values },
    });
    if let Some(uom) = uom {
        column["Uom"] = json!(uom);
    }
    if let Some(facet) = facet {
        column["Facet"] =
            json!([{ "$type": "eml23.PropertyKindFacet", "Facet": facet, "Kind": "statistics" }]);
    }
    column
}

impl Operation for ZoneStatistics {
    type Params = ZoneStatisticsParams;
    type Inputs = ZoneStatisticsInputs;

    fn about(&self) -> About {
        About {
            name: "zone-statistics",
            describe: "the statistics of a property per zone or per stratigraphic unit — count, nulls, minimum, maximum, mean, deviation — as a table, one row per group",
            category: Category::Analysis,
            makes: &["eml23.ColumnBasedTable"],
            recipe: "zone-statistics",
            represents: None,
        }
    }

    fn also(&self) -> &'static [Category] {
        &[Category::Properties]
    }

    fn suggest(&self, bag: &Pieces, slots: &mut Suggestions<'_>) {
        if known_of(bag, "frame").is_some() && bag.get("frame").is_some() {
            slots.set("by", json!("unit"));
        }
        let property = bag
            .get("property")
            .and_then(|_| known_of(bag, "property"))
            .and_then(|known| known.property.as_ref());
        if let Some(property) = property {
            if let Some(kind) = &property.kind {
                slots.inferred("propertyKind", kind.dor(), kind.clone());
            }
            if let Some(uom) = &property.uom {
                slots.set("uom", json!(uom));
            }
        }
    }

    fn check(&self, inputs: &ZoneStatisticsInputs) -> Result<(), Refusal> {
        match (&inputs.values, &inputs.property) {
            (Some(_), Some(_)) => Err(Refusal::on(
                "property",
                "the values are given twice — a field or a property, not both",
            )),
            (None, None) => Err(Refusal::new(
                "values",
                "nothing to summarise: give `values` (a field) or `property` (a property of the package)",
            )),
            _ if inputs.zones.is_some() && inputs.zone_property.is_some() => Err(Refusal::on(
                "zoneProperty",
                "the zones are given twice — a field or a property, not both",
            )),
            _ => Ok(()),
        }
    }

    fn plan(
        &self,
        inputs: ZoneStatisticsInputs,
        params: ZoneStatisticsParams,
        cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        let values = match (&inputs.values, &inputs.property) {
            (Some(field), None) => from_field(field, cx.bag),
            (None, Some(object)) => from_property(object, "property")?,
            _ => {
                return Err(Refusal::new(
                    "values",
                    "give the values once: `values` (a field) or `property`",
                ));
            }
        };

        // The key of each element, and what each key is called.
        let mut says: Vec<Note> = Vec::new();
        let (keys, labels): (Vec<Option<i64>>, Option<Vec<Ref>>) = match params.by {
            By::Zone => {
                let zones = match (&inputs.zones, &inputs.zone_property) {
                    (Some(field), None) => from_field(field, cx.bag),
                    (None, Some(object)) => from_property(object, "zoneProperty")?,
                    (None, None) => {
                        return Err(Refusal::on(
                            "zones",
                            "is required to group by zone: the zone of each element, as a field or as `zoneProperty`",
                        ));
                    }
                    (Some(_), Some(_)) => {
                        return Err(Refusal::on("zoneProperty", "the zones are given twice"));
                    }
                };
                let slot = if inputs.zones.is_some() {
                    "zones"
                } else {
                    "zoneProperty"
                };
                same_elements(&values, &zones, slot)?;
                if values.support.is_none() || zones.support.is_none() {
                    says.push(Note {
                        key: "support-unchecked",
                        detail: "the values or the zones do not say which representation they lie on: that they share one is taken from their elements and counts only".to_string(),
                    });
                }
                (codes(&zones, slot)?, None)
            }
            By::Unit => {
                // The frame whose intervals name their units: given, or the
                // piece the values lie on.
                let frame: Option<&Known> = inputs.frame.as_ref().map(|f| &f.known).or_else(|| {
                    let support = values.support.as_ref()?;
                    cx.known().into_iter().find(|k| {
                        k.reference.as_ref().is_some_and(|r| r.uuid == support.uuid)
                            && k.interval_units.is_some()
                    })
                });
                let Some(frame) = frame else {
                    return Err(Refusal::on(
                        "frame",
                        "is required to group by unit: the wellbore frame whose intervals name their stratigraphic unit",
                    ));
                };
                if let (Some(a), Some(b)) = (&values.support, &frame.reference)
                    && a.uuid != b.uuid
                {
                    return Err(Refusal::on(
                        "frame",
                        format!(
                            "is `{}` and the values lie on `{}`: the units of one frame say nothing of another's intervals",
                            b.title, a.title
                        ),
                    ));
                }
                if values.on != "intervals" {
                    return Err(Refusal::on(
                        "frame",
                        format!(
                            "names a unit per interval, and the values lie on `{}` — group intervals, or give zones",
                            values.on
                        ),
                    ));
                }
                let Some(units) = &frame.interval_units else {
                    return Err(Refusal::on(
                        "frame",
                        "names no stratigraphic unit for its intervals (no `IntervalStratigraphicUnits`)",
                    ));
                };
                // The rank of each unit in the order the frame first names it.
                let mut order: Vec<Ref> = Vec::new();
                let keys = units
                    .iter()
                    .map(|unit| {
                        unit.as_ref().map(
                            |unit| match order.iter().position(|seen| seen.uuid == unit.uuid) {
                                Some(at) => at as i64 + 1,
                                None => {
                                    order.push(unit.clone());
                                    order.len() as i64
                                }
                            },
                        )
                    })
                    .collect();
                (keys, Some(order))
            }
        };

        let grouped = groups::partition(&values.values, &keys).map_err(|e| {
            Refusal::on(
                if params.by == By::Unit { "frame" } else { "zones" },
                e.to_string(),
            )
        })?;
        if grouped.groups.is_empty() {
            return Err(Refusal::new(
                "groups",
                format!(
                    "no element of `{}` has a {}: there is no group to summarise",
                    values.name,
                    params.by.word()
                ),
            ));
        }
        cx.stop_if_cancelled()?;

        let kind = match params
            .property_kind
            .map(|k| k.0)
            .or_else(|| values.kind.as_ref().map(Ref::dor))
        {
            Some(kind) if kind["Uuid"].is_string() => kind,
            Some(other) => {
                return Err(Refusal::on(
                    "propertyKind",
                    format!("expected a DataObjectReference, got {other}"),
                ));
            }
            None => {
                return Err(Refusal::on(
                    "propertyKind",
                    "is required: each statistics column states the kind of what it summarises, and the values do not say theirs",
                ));
            }
        };
        let uom = params.uom.or(values.uom.clone());

        // The statistics, one summary per group — the domain's, never ours.
        let summaries: Vec<Summary> = grouped
            .groups
            .iter()
            .map(|group| Summary::of(&Values::Floating(group.values.clone()), 1))
            .collect();
        let key_list: Vec<i64> = grouped.groups.iter().map(|g| g.key).collect();
        let finite: Vec<i64> = summaries.iter().map(|s| s.finite as i64).collect();
        let nulls: Vec<i64> = summaries.iter().map(|s| s.non_finite as i64).collect();
        let stat = |pick: fn(&Summary) -> Option<f64>| -> Vec<f64> {
            summaries.iter().map(|s| pick(s).unwrap_or(f64::NAN)).collect()
        };
        let (minimum, maximum, mean, deviation) = (
            stat(|s| s.min),
            stat(|s| s.max),
            stat(|s| s.mean),
            stat(|s| s.deviation),
        );

        let code = reference(crate::table::CODE.uuid, crate::table::CODE.title);
        let count = reference(crate::table::COUNT.uuid, crate::table::COUNT.title);
        let key_title = match params.by {
            By::Zone => "zone",
            By::Unit => "unit rank",
        };
        let mut columns = Vec::new();
        if let Some(order) = &labels {
            let unit = |at: i64| &order[at as usize - 1];
            columns.push(texts(
                "unit",
                reference(crate::table::NAME.uuid, crate::table::NAME.title),
                key_list.iter().map(|&k| unit(k).title.clone()).collect(),
            ));
            columns.push(texts(
                "unit uuid",
                code.clone(),
                key_list.iter().map(|&k| unit(k).uuid.clone()).collect(),
            ));
        }
        columns.push(integers("value count", count.clone(), finite.clone(), -1));
        columns.push(integers("null count", count, nulls.clone(), -1));
        let u = uom.as_deref();
        columns.push(reals(
            "minimum",
            &kind,
            u,
            Some("minimum"),
            "the smallest value of the group, nulls set aside",
            &minimum,
        ));
        columns.push(reals(
            "maximum",
            &kind,
            u,
            Some("maximum"),
            "the largest value of the group, nulls set aside",
            &maximum,
        ));
        columns.push(reals(
            "mean",
            &kind,
            u,
            Some("average"),
            "the arithmetic mean of the group, nulls set aside and never averaged",
            &mean,
        ));
        columns.push(reals(
            "standard deviation",
            &kind,
            u,
            None,
            "the population standard deviation of the group, nulls set aside",
            &deviation,
        ));
        // A key that cannot occur: one below the smallest.
        let key_null = key_list.iter().min().copied().unwrap_or(0).saturating_sub(1);

        let title = match params.title {
            Some(given) => given.0,
            None => format!("{} — statistics by {}", values.name, params.by.word()),
        };
        let seed = cx.seed(&title);
        let mut rank = 0usize;
        let uuid = support::next_uuid(cx.identity, &format!("zone-statistics:{seed}"), &mut rank)?;
        let table = json!({
            "$type": "eml23.ColumnBasedTable",
            "uuid": uuid,
            "schemaVersion": "2.3",
            "Citation": support::citation(cx.when, &title),
            "KeyColumn": [integers(key_title, code, key_list.clone(), key_null)],
            "Column": columns,
        });

        let summarised: i64 = finite.iter().sum();
        let null_total: i64 = nulls.iter().sum();
        if null_total > 0 {
            says.push(Note {
                key: "nulls",
                detail: format!("{null_total} null value(s) set aside and counted per group, never averaged"),
            });
        }
        if grouped.ungrouped > 0 {
            says.push(Note {
                key: "ungrouped",
                detail: format!(
                    "{} of {} element(s) have no {}: in no group",
                    grouped.ungrouped,
                    values.values.len(),
                    params.by.word()
                ),
            });
        }
        let empty = summaries.iter().filter(|s| s.finite == 0).count();
        if empty > 0 {
            says.push(Note {
                key: "all-null",
                detail: format!("{empty} group(s) hold only nulls: their row is kept, its statistics NaN"),
            });
        }

        let mut parameters = Vec::new();
        if let Some(support) = &values.support {
            parameters.push(Parameter::Object {
                title: "input".to_string(),
                reference: support.dor(),
            });
        }
        parameters.push(Parameter::Double {
            title: "groupCount".to_string(),
            value: grouped.groups.len() as f64,
            uom: None,
        });
        parameters.push(Parameter::Double {
            title: "summarisedCount".to_string(),
            value: summarised as f64,
            uom: None,
        });
        parameters.push(Parameter::Double {
            title: "nullCount".to_string(),
            value: null_total as f64,
            uom: None,
        });
        parameters.push(Parameter::Double {
            title: "ungroupedCount".to_string(),
            value: grouped.ungrouped as f64,
            uom: None,
        });

        Ok(Plan {
            objects: vec![NewObject {
                uuid,
                qualified_type: "eml23.ColumnBasedTable".to_string(),
                role: "statistics".to_string(),
                title: title.clone(),
                json: table,
            }],
            datasets: Vec::<Dataset>::new(),
            media: Vec::new(),
            activity: crate::operation::ActivityDraft {
                recipe: "zone-statistics",
                title: format!("zone-statistics → {title}"),
                seed,
                parameters,
            },
            says,
            dropped: Vec::new(),
            // A table is no geometry.
            shapes: Vec::new(),
            blocked: None,
        })
    }

    fn reports(&self) -> Vec<Slot> {
        let number = |name: &'static str, about: &'static str| {
            Slot::required(
                name,
                Need::Number {
                    uom: None,
                    least: None,
                    most: None,
                },
                about,
            )
        };
        vec![
            number("groupCount", "groups — rows of the table"),
            number("summarisedCount", "values summarised, nulls set aside"),
            number("nullCount", "null values, counted and never averaged"),
            number("ungroupedCount", "elements with no zone or unit — in no group"),
        ]
    }
}
