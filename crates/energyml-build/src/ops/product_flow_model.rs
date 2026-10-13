//! `product-flow-model` — a chain of units, each flowing into the next: the
//! directed graph PRODML's `ProductFlowModel` draws, for a loop like
//! the reference model's producer → exchanger → reinjector.
//!
//! The model has three units, ports and nodes, and the units and nodes are
//! **names**: `ProductFlowUnit.Facility` and `ConnectedNode.Node` hold the
//! names of the facility and of the
//! `ReportingEntity` objects.
//!
//! # A unit per object, a node between two
//!
//! PRODML Technical Usage Guide v2.3, §29.1 p. 249: *“A Unit is essentially a
//! black box that can represent anything (big or small). Ports allow flow in or
//! out of a Unit. Nodes are used to connect ports”*. Each object given in
//! `units` — in the order of the slot, which is the direction of flow —
//! becomes a unit named after its title; the first has an outlet, the last an
//! inlet, the others both; the outlet of one and the inlet of the next name
//! the same node. The order is **declared**, never deduced: which way water
//! flows through a loop is not in any of the objects.
//!
//! # The links are names
//!
//! `ConnectedNode.Node` and `ProductFlowUnit.Facility` are text
//! (ProductFlowModel.xsd, ProdmlCommon.xsd `FacilityIdentifierStruct`): the
//! model does not reference the objects it draws. So a unit's facility is the
//! **title** of the object given — the name its reporting entity or its
//! facility carries in the package — and the activity names the objects
//! themselves, which is the only place a reference to them can live. A
//! wellbore is said to be one (`kind="wellbore"`); a facility or a reporting
//! entity is not typed here, the kind being a field of the object that this
//! operation is not given.
//!
//! An object given twice is refused by its name: a unit drawn twice in one
//! network would be two units under one name, and the graph a loop nobody
//! stated.
//!
//! # Synthetic, and sourced
//!
//! The model draws an invented installation: `synthetic = true` and the
//! source in `Citation/Description` (ADR-0019). Its uuid comes from
//! the objects it chains, in order.

use energyml_domain::provenance::Parameter;
use energyml_prodml_2_3 as prodml;

use super::prodml_doublet::{class_of, reference, same, titled};
use super::prodml_writer::Writer;
use crate::declare::{Inputs, ObjectIn, Params};
use crate::operation::{About, ActivityDraft, Category, Context, Need, Note, Operation, Plan, Refusal, Slot};
use crate::pieces::Ref;

/// The operation.
pub struct ProductFlowModel;

/// What `product-flow-model` is told.
#[derive(Debug, Params)]
pub struct ProductFlowModelParams {
    /// the title of the model — its one network is named after it
    pub title: String,
    /// where the chain comes from — a document with its page or paragraph
    pub source: String,
}

/// What `product-flow-model` takes.
#[derive(Debug, Inputs)]
pub struct ProductFlowModelInputs {
    /// the units, in the direction of flow — each flows into the next
    #[input(
        types = "prodml23.ReportingEntity, prodml23.Facility, witsml21.Wellbore, resqml22.WellboreFeature",
        least = 2,
        ordered
    )]
    pub units: Vec<ObjectIn>,
}

/// The unit of an object: its reference, or a refusal naming the slot.
fn unit_of(input: &ObjectIn) -> Result<Ref, Refusal> {
    let known = reference("units", input)?;
    match class_of(&known.qualified_type) {
        "ReportingEntity" | "Facility" | "Wellbore" | "WellboreFeature" => Ok(known.clone()),
        other => Err(Refusal::on(
            "units",
            format!(
                "`{}` is a `{other}`, and a unit of a flow model is something flow goes through: a reporting entity, a facility or a wellbore",
                known.title
            ),
        )),
    }
}

impl Operation for ProductFlowModel {
    type Params = ProductFlowModelParams;
    type Inputs = ProductFlowModelInputs;

    fn about(&self) -> About {
        About {
            name: "product-flow-model",
            describe: "a chain of units flowing one into the next — a PRODML product flow model whose units are named after the objects given, in the order given",
            category: Category::Synthetic,
            makes: &["prodml23.ProductFlowModel"],
            recipe: "product-flow-model",
            represents: None,
        }
    }

    fn check(&self, inputs: &ProductFlowModelInputs) -> Result<(), Refusal> {
        for input in &inputs.units {
            unit_of(input)?;
        }
        Ok(())
    }

    fn plan(
        &self,
        inputs: ProductFlowModelInputs,
        params: ProductFlowModelParams,
        cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        let title = titled("title", &params.title)?;
        let mut writer = Writer::new(cx, "product-flow-model", &params.source)?;
        let units: Vec<Ref> = inputs.units.iter().map(unit_of).collect::<Result<_, _>>()?;
        if units.len() < 2 {
            return Err(Refusal::on(
                "units",
                format!(
                    "{} unit(s): a flow runs from one unit into another, so two are needed at least",
                    units.len()
                ),
            ));
        }
        for (at, one) in units.iter().enumerate() {
            if let Some(before) = units[..at].iter().position(|other| same(other, one)) {
                return Err(Refusal::on(
                    "units",
                    format!(
                        "`{}` is given as unit {} and as unit {}: a unit is drawn once, and a loop through it twice is nothing stated",
                        one.title,
                        before + 1,
                        at + 1
                    ),
                ));
            }
        }
        for (at, one) in units.iter().enumerate() {
            if let Some(before) = units[..at].iter().position(|other| other.title == one.title) {
                return Err(Refusal::on(
                    "units",
                    format!(
                        "units {} and {} are both titled `{}`: the model links its units by name, so two of one name would be one unit",
                        before + 1,
                        at + 1,
                        one.title
                    ),
                ));
            }
        }

        let last = units.len() - 1;
        let node = |at: usize| format!("node {}", at + 1);
        let flow_units: Vec<prodml::ProductFlowUnit> = units
            .iter()
            .enumerate()
            .map(|(at, one)| {
                let mut ports = Vec::new();
                if at > 0 {
                    ports.push(prodml::ProductFlowPort {
                        direction: prodml::ProductFlowPortType::Inlet,
                        name: "inlet".to_string(),
                        connected_node: vec![prodml::ConnectedNode {
                            node: node(at - 1),
                            uid: format!("unit-{}-inlet-node", at + 1),
                            ..Default::default()
                        }],
                        uid: format!("unit-{}-inlet", at + 1),
                        ..Default::default()
                    });
                }
                if at < last {
                    ports.push(prodml::ProductFlowPort {
                        direction: prodml::ProductFlowPortType::Outlet,
                        name: "outlet".to_string(),
                        connected_node: vec![prodml::ConnectedNode {
                            node: node(at),
                            uid: format!("unit-{}-outlet-node", at + 1),
                            ..Default::default()
                        }],
                        uid: format!("unit-{}-outlet", at + 1),
                        ..Default::default()
                    });
                }
                let wellbore = matches!(class_of(&one.qualified_type), "Wellbore" | "WellboreFeature");
                prodml::ProductFlowUnit {
                    name: Some(one.title.clone()),
                    facility: Some(prodml::FacilityIdentifierStruct {
                        value: one.title.clone(),
                        kind: wellbore.then_some(prodml::ReportingFacility::Wellbore),
                        ..Default::default()
                    }),
                    port: ports,
                    uid: format!("unit-{}", at + 1),
                    ..Default::default()
                }
            })
            .collect();

        let chain = units
            .iter()
            .map(|one| one.title.as_str())
            .collect::<Vec<_>>()
            .join(" → ");
        let uuid = writer.uuid(
            &units
                .iter()
                .map(|one| one.uuid.to_ascii_lowercase())
                .collect::<Vec<_>>()
                .join(">"),
        )?;
        let model = prodml::ProductFlowModel {
            uuid: uuid.clone(),
            schema_version: "2.3".to_string(),
            citation: writer.citation(&title, &format!("the flow of an invented installation, {chain}"))?,
            extension_name_value: vec![writer.synthetic("the flow model of an invented installation")],
            comment: Some(format!(
                "synthetic — {chain}; the units are named after the objects of the package, which the activity references"
            )),
            network: vec![prodml::ProductFlowNetwork {
                name: title.clone(),
                unit: flow_units,
                uid: "network-1".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };
        writer.push(&uuid, "prodml23.ProductFlowModel", "flow-model", &title, &model);

        let says = vec![
            Note { key: "chain", detail: format!("{} unit(s) joined by {} node(s): {chain}", units.len(), last) },
            Note {
                key: "names",
                detail: "the units and nodes are names, not references: each unit carries the title of the object given, and the activity names the objects".to_string(),
            },
            Note {
                key: "synthetic",
                detail: "the model draws an invented installation: `synthetic = true` and the source in Citation/Description".to_string(),
            },
        ];
        let mut parameters = vec![Parameter::Double {
            title: "unitCount".to_string(),
            value: units.len() as f64,
            uom: None,
        }];
        for one in &units {
            parameters.push(Parameter::Object {
                title: "input".to_string(),
                reference: one.dor(),
            });
        }
        Ok(Plan {
            media: Vec::new(),
            blocked: None,
            activity: ActivityDraft {
                recipe: "product-flow-model",
                title: format!("product-flow-model → {title}"),
                seed: writer.activity_seed(),
                parameters,
            },
            objects: writer.objects,
            datasets: Vec::new(),
            says,
            dropped: Vec::new(),
            shapes: Vec::new(),
        })
    }

    /// What the run drew.
    fn reports(&self) -> Vec<Slot> {
        vec![Slot::required(
            "unitCount",
            Need::Number {
                uom: None,
                least: None,
                most: None,
            },
            "the units the network holds, one per object given",
        )]
    }
}
