//! `representation-set` — a `RepresentationSetRepresentation` grouping given
//! representations: *“a "bag" of representations […] If the individual
//! representations are all of the same, then you can indicate that the set is
//! homogenous”* (RESQML TRG 2.2 §8.8 p. 245).
//!
//! Step 8 of the build plan of Lutetia, the grids of the Chalk grouped:
//! (`RepresentationSetRepresentation`,
//! `IsHomogeneous` false) — the class exists in the final XSD even though the release
//! notes say it was removed (the schema wins). And the writer that was
//! named as missing for a cross-section
//! (no writer of `RepresentationSetRepresentation` existed here).
//!
//! # The order is an index
//!
//! *“Representation set representations are an indexed assemblage of
//! individual representations […] This representation index is implicit. It
//! follows the order on which the representations are given in the XML
//! instance by the software writer”* (RESQML Technical Usage Guide 2.2, draft,
//! §4.7 p. 42). So `representations` is **ordered**, written as given, and a
//! representation given twice is **refused**, naming both places: it would
//! hold two indices, and a framework's contacts say which member they join by
//! that index.
//!
//! # `IsHomogeneous` is what the members are
//!
//! Computed, never guessed: true when every member is of one type — *“the
//! reader software need only read the first individual representation to
//! understand the kinds of individual representations that are gathered in the
//! set”* (same guide, §4.7 p. 43). `homogeneous`, when stated, is **checked**
//! against the members and refused with the types found when it disagrees,
//! either way: a caller expecting one type and handing several has made a
//! mistake a boolean would otherwise write down.
//!
//! # A member is a representation
//!
//! *“RepresentationSetRepresentation.Representation 1..* To:
//! AbstractRepresentation”* (TRG §8.8 p. 245). Anything else is refused by its
//! type; every concrete representation class of RESQML ends in
//! `Representation` (`SubRepresentation` included), and that is what is read.
//!
//! # Identity
//!
//! From what the set is — its title and its members, in order: the same bag is
//! the same object from one run to the next, another bag another object.

use std::collections::BTreeSet;

use serde_json::json;

use energyml_domain::provenance::Parameter;

use crate::declare::{Inputs, ObjectIn, Params, Title};
use crate::operation::{About, ActivityDraft, Category, Context, Need, NewObject, Note, Operation, Plan, Refusal, Slot};

use super::support;
use super::written::RepresentedParams;

/// The operation.
pub struct RepresentationSet;

/// What `representation-set` is told.
#[derive(Debug, Params)]
pub struct RepresentationSetParams {
    /// the set's title — with its members, what makes it the same set from one run to the next
    pub title: Title,
    /// what the members must be: true, all of one type; false, of several — left out, what they are is written
    ///
    /// **A check, not a value**: `IsHomogeneous` is written from the
    /// members, and a stated answer that disagrees is refused.
    pub homogeneous: Option<bool>,
    /// What the set represents, when named.
    #[param(flatten)]
    pub represented: RepresentedParams,
}

/// What `representation-set` takes.
#[derive(Debug, Inputs)]
pub struct RepresentationSetInputs {
    /// the representations the set groups, in order — a member's place is its index in the set
    #[input(least = 1, ordered)]
    pub representations: Vec<ObjectIn>,
}

/// The bare class of a qualified type, lower case, without 2.0.1's `obj_`.
fn bare(qualified: &str) -> String {
    let class = qualified.rsplit('.').next().unwrap_or(qualified).to_ascii_lowercase();
    class.strip_prefix("obj_").map(str::to_string).unwrap_or(class)
}

impl Operation for RepresentationSet {
    type Params = RepresentationSetParams;
    type Inputs = RepresentationSetInputs;

    fn about(&self) -> About {
        About {
            name: "representation-set",
            describe: "a set of representations — a bag, in the order given, whose IsHomogeneous is what the members are",
            category: Category::Create,
            makes: &["resqml22.RepresentationSetRepresentation"],
            recipe: "representation-set",
            represents: None,
        }
    }

    fn plan(&self, inputs: RepresentationSetInputs, params: RepresentationSetParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
        let title = params.title.0.trim().to_string();
        if title.is_empty() {
            return Err(Refusal::on("title", "is empty: a set is written with a title"));
        }

        // -- the members, in order, each once ---------------------------------------
        let mut members = Vec::with_capacity(inputs.representations.len());
        let mut placed: Vec<(String, usize)> = Vec::new();
        for (at, member) in inputs.representations.iter().enumerate() {
            let Some(known) = member.known.reference.as_ref() else {
                return Err(Refusal::on(
                    "representations",
                    format!("`{}` was given without a reference to write: a member is an object of the package", member.name),
                ));
            };
            if !bare(&known.qualified_type).ends_with("representation") {
                return Err(Refusal::on(
                    "representations",
                    format!(
                        "`{}` is a `{}`, and a member of a set is a representation (RESQML TRG 2.2 §8.8 p. 245: *“Representation 1..* To: AbstractRepresentation”*)",
                        known.title, known.qualified_type
                    ),
                ));
            }
            let uuid = known.uuid.to_ascii_lowercase();
            if let Some((_, first)) = placed.iter().find(|(one, _)| *one == uuid) {
                return Err(Refusal::on(
                    "representations",
                    format!(
                        "`{}` is given at places {} and {}: a member's place is its index in the set (RESQML Technical Usage Guide 2.2 §4.7 p. 42), and one representation would hold two",
                        known.title,
                        first + 1,
                        at + 1
                    ),
                ));
            }
            placed.push((uuid, at));
            members.push(known.clone());
        }

        // -- homogeneous: what the members are, and what was stated --------------------
        let types: BTreeSet<String> = members.iter().map(|one| one.qualified_type.clone()).collect();
        let homogeneous = members.iter().map(|one| bare(&one.qualified_type)).collect::<BTreeSet<_>>().len() == 1;
        if let Some(stated) = params.homogeneous
            && stated != homogeneous
        {
            return Err(Refusal::on(
                "homogeneous",
                if stated {
                    format!(
                        "says all of one type, and the members are of {}: {}",
                        types.len(),
                        types.iter().cloned().collect::<Vec<_>>().join(", ")
                    )
                } else {
                    format!(
                        "says of several types, and every member is a `{}` — leave it out, or say true",
                        types.iter().next().cloned().unwrap_or_default()
                    )
                },
            ));
        }

        // -- the object ----------------------------------------------------------------
        let seed = format!(
            "representation-set:{title}:{}",
            members.iter().map(|one| one.uuid.to_ascii_lowercase()).collect::<Vec<_>>().join(",")
        );
        let mut rank = 0usize;
        let uuid = support::next_uuid(cx.identity, &seed, &mut rank)?;
        let represented = support::represented_in(&params.represented, None, None, cx.identity, cx.when, &seed, &title, &mut rank)?;
        let mut object = json!({
            "$type": "resqml22.RepresentationSetRepresentation",
            "uuid": uuid,
            "schemaVersion": "2.2",
            "Citation": support::citation(cx.when, &title),
            "IsHomogeneous": homogeneous,
            "Representation": members.iter().map(|one| one.dor()).collect::<Vec<_>>(),
        });
        if let Some(reference) = represented.reference {
            object["RepresentedObject"] = reference;
        }

        let says = vec![
            Note {
                key: "members",
                detail: format!(
                    "{} member(s), indexed in the order given: {}",
                    members.len(),
                    members.iter().map(|one| format!("`{}`", one.title)).collect::<Vec<_>>().join(", ")
                ),
            },
            Note {
                key: "homogeneous",
                detail: if homogeneous {
                    format!("IsHomogeneous true: every member is a `{}`", types.iter().next().cloned().unwrap_or_default())
                } else {
                    format!("IsHomogeneous false: {} types — {}", types.len(), types.iter().cloned().collect::<Vec<_>>().join(", "))
                },
            },
        ];

        let mut objects = represented.objects;
        objects.push(NewObject {
            uuid,
            qualified_type: "resqml22.RepresentationSetRepresentation".to_string(),
            role: "representation".to_string(),
            title: title.clone(),
            json: object,
        });
        Ok(Plan {
            media: Vec::new(),
            blocked: None,
            objects,
            datasets: Vec::new(),
            activity: ActivityDraft {
                recipe: "representation-set",
                title: format!("representation-set → {title}"),
                seed,
                parameters: vec![
                    Parameter::Double { title: "memberCount".to_string(), value: members.len() as f64, uom: None },
                    Parameter::Text { title: "isHomogeneous".to_string(), value: homogeneous.to_string() },
                ],
            },
            says,
            dropped: Vec::new(),
            // A bag has no geometry of its own: its members do.
            shapes: Vec::new(),
        })
    }

    fn reports(&self) -> Vec<Slot> {
        vec![
            Slot::required("memberCount", Need::Number { uom: None, least: None, most: None }, "the representations in the set"),
            Slot::required("isHomogeneous", Need::Flag, "IsHomogeneous as written — what the members are"),
        ]
    }
}
