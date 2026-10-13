//! `collection` — an `eml23.DataobjectCollection` and the
//! `CollectionsToDataobjectsAssociationSet` that says what is in it.
//!
//! A collection holds nothing itself: its sequence has no member field, and
//! what belongs to it is written in an association set, one
//! `SingleCollectionAssociation` {`Collection`, `Dataobject` 1..∞} per
//! collection (Collection.xsd). The synthetic reference model asks for three
//! kinds of them: a **project**
//! holding every object of the package — *« The RESQML V2.2 Version allows to
//! gather information in a given DataObjectCollection which CollectionKind is a
//! "Project" »* (RESQML usage guide 2.2, draft, §7 p. 66) —, a folder per
//! well, and the set of the model's wellbore interpretations that an
//! `EarthModelInterpretation` names.
//!
//! # What is written, and what is refused
//!
//! - **one association set per collection**, holding its one association. The
//!   schema forbids a collection twice *in the same* set (*« The same
//!   collection CANNOT be used in multiple SingleCollectionAssociations of the
//!   same CollectionsToDataobjectsAssociations »*), not a set per collection;
//!   a set gathering several collections would have to be rewritten by every
//!   step that adds one, and a plan does not read what the package already
//!   holds;
//! - **a collection without members is refused**: `Dataobject` is 1..∞, so
//!   its association could not be written, and a collection nothing is in
//!   reads exactly like one that holds what it should;
//! - **a member given twice is written once, counted and named** — the schema
//!   says the second *« must be ignored »*; writing it would ask every reader
//!   to. It is ordinary, not a fault: the frames an import writes are the same
//!   objects each time, so every step that frames with the same codes gives
//!   them again — and a count nobody can explain is not one to trust;
//! - **`HomogeneousDatatype` is what the members are**, on the collection and
//!   on its association: true when they share one type. Asked for
//!   (`homogeneous`) and not so, it is refused with the types found — the TRG
//!   2.2 §6.19 p. 187 wants the wellbore set of an earth model to hold
//!   *« only WellboreInterpretation »*, and a boolean cannot say which type,
//!   only be checked against the members.
//!
//! The collection's uuid comes from its title and kind — the same folder is
//! the same object from one run to the next — and the association set's from
//! what it associates: other members, another set.

use std::collections::BTreeSet;

use energyml_domain::provenance::Parameter;
use energyml_eml_2_3 as eml;

use crate::declare::{Choice, Inputs, ObjectIn, Params, Title};
use crate::operation::{About, ActivityDraft, Category, Context, NewObject, Note, Operation, Plan, Refusal};

/// The operation.
pub struct Collection;

/// What the collection is for — EML's `CollectionKind`, a closed list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Choice)]
pub enum Kind {
    /// A folder.
    Folder,
    /// A project: what one prospect's results are linked to.
    Project,
    /// A realization.
    Realization,
    /// A scenario.
    Scenario,
    /// A study.
    Study,
}

impl Kind {
    fn written(self) -> eml::CollectionKindExt {
        match self {
            Kind::Folder => eml::CollectionKindExt::Folder,
            Kind::Project => eml::CollectionKindExt::Project,
            Kind::Realization => eml::CollectionKindExt::Realization,
            Kind::Scenario => eml::CollectionKindExt::Scenario,
            Kind::Study => eml::CollectionKindExt::Study,
        }
    }
}

/// What `collection` is told.
#[derive(Debug, Params)]
pub struct CollectionParams {
    /// the collection's title — with its kind, what makes it the same collection from one run to the next
    pub title: Title,
    /// what the collection is for
    #[param(default = Kind::Folder, optional)]
    pub kind: Kind,
    /// require every member to be of one type, and refuse otherwise
    #[param(default = false, optional)]
    pub homogeneous: bool,
}

/// What `collection` takes.
#[derive(Debug, Inputs)]
pub struct CollectionInputs {
    /// the objects the collection holds — of any type, other collections included
    #[input(least = 1)]
    pub members: Vec<ObjectIn>,
}

impl Operation for Collection {
    type Params = CollectionParams;
    type Inputs = CollectionInputs;

    fn about(&self) -> About {
        About {
            name: "collection",
            describe: "a collection of data objects — a folder, a project, a study — and the association set that says what it holds",
            category: Category::Create,
            makes: &[
                "eml23.DataobjectCollection",
                "eml23.CollectionsToDataobjectsAssociationSet",
            ],
            recipe: "collection",
            represents: None,
        }
    }

    fn plan(
        &self,
        inputs: CollectionInputs,
        params: CollectionParams,
        cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        let title = params.title.0.trim().to_string();
        let mut seen = BTreeSet::new();
        let mut members = Vec::with_capacity(inputs.members.len());
        // Each repeated object — by uuid, named by title — with how many times
        // it came again.
        let mut twice: Vec<(String, String, usize)> = Vec::new();
        for member in &inputs.members {
            let Some(known) = member.known.reference.as_ref() else {
                return Err(Refusal::on(
                    "members",
                    format!("`{}` was given without a reference to write", member.name),
                ));
            };
            // By the **uri**: two versions of one object are two members, and
            // only the same version twice is a repetition.
            if !seen.insert(known.uri()) {
                let uri = known.uri();
                match twice.iter_mut().find(|(one, _, _)| *one == uri) {
                    Some((_, _, again)) => *again += 1,
                    None => twice.push((uri, known.title.clone(), 1)),
                }
                continue;
            }
            members.push(crate::crs::reference_of(known));
        }
        if members.is_empty() {
            return Err(Refusal::on(
                "members",
                "a collection holds at least one object: its association cannot be written empty",
            ));
        }

        let types: BTreeSet<&str> = members.iter().map(|dor| dor.qualified_type.as_str()).collect();
        let homogeneous = types.len() == 1;
        if params.homogeneous && !homogeneous {
            return Err(Refusal::on(
                "homogeneous",
                format!(
                    "the members are of {} types — {} — and one was required",
                    types.len(),
                    types.iter().copied().collect::<Vec<_>>().join(", ")
                ),
            ));
        }

        let kind = params.kind;
        let collection_uuid = cx
            .identity
            .uuid(&format!("collection:{}:{title}", kind.word()), 0)?;
        let seed = format!(
            "collection:set:{collection_uuid}:{}",
            members
                .iter()
                // The version joins the seed only when there is one, so that a
                // collection of unversioned members keeps the uuid it had.
                .map(|dor| match &dor.object_version {
                    Some(version) => format!("{}@{version}", dor.uuid),
                    None => dor.uuid.clone(),
                })
                .collect::<Vec<_>>()
                .join(",")
        );
        let set_uuid = cx.identity.uuid(&seed, 1)?;
        let set_title = format!("{title} — members");

        let citation = |title: &str| eml::Citation {
            title: title.to_string(),
            originator: cx.when.originator.clone(),
            creation: cx.when.creation.clone(),
            format: energyml_core::CITATION_FORMAT.to_string(),
            ..Default::default()
        };
        let collection = eml::DataobjectCollection {
            uuid: collection_uuid.clone(),
            schema_version: "2.3".to_string(),
            citation: citation(&title),
            kind: Some(kind.written()),
            homogeneous_datatype: Some(homogeneous),
            ..Default::default()
        };
        let set = eml::CollectionsToDataobjectsAssociationSet {
            uuid: set_uuid.clone(),
            schema_version: "2.3".to_string(),
            citation: citation(&set_title),
            single_collection_association: vec![eml::SingleCollectionAssociation {
                homogeneous_datatype: Some(homogeneous),
                dataobject: members.clone(),
                collection: crate::crs::reference_to(&collection_uuid, "eml23.DataobjectCollection", &title),
            }],
            ..Default::default()
        };

        let mut says = vec![Note {
            key: "members",
            detail: format!(
                "{} member(s) of {} type(s){}",
                members.len(),
                types.len(),
                if homogeneous {
                    format!(" — all {}", types.iter().next().copied().unwrap_or_default())
                } else {
                    String::new()
                }
            ),
        }];
        if !twice.is_empty() {
            let count: usize = twice.iter().map(|(_, _, again)| again).sum();
            let named: Vec<String> = twice
                .iter()
                .map(|(_, title, again)| format!("`{title}` ×{}", again + 1))
                .collect();
            says.push(Note {
                key: "twice",
                detail: format!(
                    "{count} member(s) given more than once, each written once: {}",
                    named.join(", ")
                ),
            });
        }

        Ok(Plan {
            media: Vec::new(),
            blocked: None,
            objects: vec![
                NewObject {
                    uuid: collection_uuid,
                    qualified_type: "eml23.DataobjectCollection".to_string(),
                    role: "collection".to_string(),
                    title: title.clone(),
                    json: energyml_core::json::write_value(&collection),
                },
                NewObject {
                    uuid: set_uuid,
                    qualified_type: "eml23.CollectionsToDataobjectsAssociationSet".to_string(),
                    role: "association".to_string(),
                    title: set_title,
                    json: energyml_core::json::write_value(&set),
                },
            ],
            datasets: Vec::new(),
            activity: ActivityDraft {
                recipe: "collection",
                title: format!("collection → {title}"),
                seed,
                parameters: vec![
                    Parameter::Text {
                        title: "title".to_string(),
                        value: title,
                    },
                    Parameter::Double {
                        title: "memberCount".to_string(),
                        value: members.len() as f64,
                        uom: None,
                    },
                ],
            },
            says,
            dropped: Vec::new(),
            shapes: Vec::new(),
        })
    }
}
