//! `surface-framework` - a `NonSealedSurfaceFrameworkRepresentation`: the
//! representations of a structural organisation gathered in one indexed list,
//! and the contacts between them named as nodes of those representations.
//!
//! Step 7 of the build plan of Lutetia, the frameworks:
//! the same members plus one, and
//! the `NonSealedContact` whose geometry is a fault.
//!
//! # What it is, from the schema and the guide
//!
//! *“A collection of contact representations parts, which are a list of
//! contact patches with no identity. This collection of contact
//! representations is completed by a set of representations gathered at the
//! representation set representation level”* (RESQML 2.2 Structural.xsd
//! l. 234-246; TRG 2.2 §11.11 p. 280). It is what represents a structural
//! organisation - *“Structural organization interpretation represented by a
//! non-sealed surface framework”* (RESQML usage guide 2.2, draft, §7.3.1.2.2,
//! figure 7-21 p. 98) -, so `RepresentedObject` is the organisation, and it
//! gathers *“step by step, the structural information on boundaries and
//! contacts between boundaries, all along the workflow, even if they are not
//! all totally topologically consistent”* (§8.3.2.1 p. 119-120). A framework
//! of the faults alone, while the horizons wait, is that.
//!
//! **The order of `representations` is the framework's statement**: a
//! contact patch names its surface *“by its index, in the list of
//! representations contained in the organization”* (`ContactPatch.
//! RepresentationIndex`, Structural.xsd l. 182-186), so reordering the list
//! breaks every contact - the slot is `ordered`, and the list is written as
//! given.
//!
//! # A contact: a patch of `lips`, its lines nodes of a representation
//!
//! Each patch of a `lips` input is **one contact**, each of its lines **one
//! contact patch**: *“The ordered list of nodes (identified by their global
//! index) in the supporting representation, which constitutes the contact
//! patch”* (`SupportingRepresentationNodes`, l. 187-191). The nodes are found
//! **by position**, exactly, in the representations given - the lips
//! `fault-throw` writes are nodes of the torn surface position for position,
//! the footwall's line then the hanging wall's - and a line that is no line of
//! nodes of a given representation is refused by name: a contact defined by
//! its own geometry (`NonSealedContact.Geometry`) is not written here.
//!
//! # `Index`, stated
//!
//! *“The index of the contact. Indicates identity of the contact in the
//! surface framework context. It is used for contact identities and to find
//! the interpretation of this particular contact”* (l. 119-124). A RESQML 2.2
//! contact interpretation has no index of its own
//! (`AbstractContactInterpretationPart` carries only `PartOf`, Interpretations.xsd
//! l. 463-475), so the one reading left is **its position** in the
//! organisation's `ContactInterpretation` list - and what the bag knows of the
//! organisation does not include that list. `contactIndex` states it, one per
//! contact, distinct; it is **not checked** against the organisation, and the
//! plan says so.
//!
//! # Why not sealed
//!
//! A sealed framework says that its contact patches are collocated nodes -
//! *“When several nodes are topologically associated, they have absolutely
//! the same geometry”* (usage guide §8.3.2.2 p. 120-121) - and its contacts
//! *“must have an identity kind of "colocated" or "equivalent"”*.
//! The reference model asks for one (*“`SealedContact` `collocation`, patches ≥ 2”*); the
//! workspace cannot write it honestly today: the fault surfaces
//! (`fault-from-sticks`) carry no node on the horizons' cutoffs, so no patch of
//! a fault is collocated with a lip; and the two lips of one torn horizon are
//! not collocated with each other - they were, before the throw, which is a
//! *previous* collocation, and a contact interpretation has one `Index` for two
//! lines. Written anyway, `collocation` would state a false fact about every
//! node. This operation writes the non-sealed framework; the sealed one waits
//! for a fault remeshed along its cutoffs, and for the question to be settled.
//!
//! # Identity
//!
//! What the framework says is what it is: its title, its organisation, its
//! representations in order and its contacts. A horizon joining it later - H1
//! to H7 wait for a human review - makes another snapshot.

use serde_json::json;

use energyml_domain::provenance::Parameter;

use crate::declare::{Inputs, ObjectIn, Params, Shape};
use crate::operation::{
    About, ActivityDraft, Category, Context, Dataset, Need, NewObject, Note, Numbers, Operation, Plan,
    Refusal, Slot,
};
use crate::pieces::Ref;

use super::support;
use super::written::NamingParams;

/// The operation.
pub struct SurfaceFramework;

/// What `surface-framework` is told.
#[derive(Debug, Params)]
pub struct FrameworkParams {
    /// for each contact - each patch of `lips`, in order - the position, from 0, of the contact interpretation it represents in the organisation's list
    #[param(min = 0.0)]
    pub contact_index: Vec<i64>,
    /// The title and the file.
    #[param(flatten)]
    pub naming: NamingParams,
}

/// What `surface-framework` takes.
#[derive(Debug, Inputs)]
pub struct FrameworkInputs {
    /// the structural organisation the framework represents
    #[input(types = "resqml22.StructuralOrganizationInterpretation")]
    pub organisation: ObjectIn,
    /// the representations the framework gathers - surfaces of the organisation's horizons and faults - in the order its contacts index them
    #[input(
        types = "resqml22.TriangulatedSetRepresentation, resqml22.Grid2dRepresentation, resqml22.PlaneSetRepresentation, resqml22.PolylineSetRepresentation, resqml22.PointSetRepresentation",
        least = 1,
        ordered
    )]
    pub representations: Vec<Shape>,
    /// the contacts - each patch one contact, each of its lines nodes of one of the representations, as `fault-throw` writes its lips
    #[input(topology = "polylines", ordered)]
    pub lips: Vec<Shape>,
}

/// The bare class of a qualified type, lower case.
fn class_of(qualified_type: &str) -> String {
    qualified_type
        .rsplit('.')
        .next()
        .unwrap_or(qualified_type)
        .to_ascii_lowercase()
}

/// A position, exactly - the key nodes are found by.
fn at(p: &[f64; 3]) -> [u64; 3] {
    [p[0].to_bits(), p[1].to_bits(), p[2].to_bits()]
}

/// One contact patch: a representation's rank in the list, and its nodes.
struct ContactPatch {
    representation: usize,
    nodes: Vec<i64>,
}

impl Operation for SurfaceFramework {
    type Params = FrameworkParams;
    type Inputs = FrameworkInputs;

    fn about(&self) -> About {
        About {
            name: "surface-framework",
            describe: "a non-sealed surface framework - the representations of a structural organisation in one indexed list, and the contacts between them as nodes of those representations",
            category: Category::Faults,
            makes: &["resqml22.NonSealedSurfaceFrameworkRepresentation"],
            recipe: "surface-framework",
            represents: None,
        }
    }

    fn plan(
        &self,
        inputs: FrameworkInputs,
        params: FrameworkParams,
        cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        let title = params.naming.title();
        let file = params.naming.data_file.clone();

        // -- what it represents ---------------------------------------------------------
        let organisation: Ref = inputs.organisation.known.reference.clone().ok_or_else(|| {
            Refusal::on(
                "organisation",
                format!(
                    "`{}` was given without a reference to write",
                    inputs.organisation.name
                ),
            )
        })?;
        if class_of(&organisation.qualified_type) != "structuralorganizationinterpretation" {
            return Err(Refusal::on(
                "organisation",
                format!(
                    "`{}` is a `{}` - a surface framework represents a structural organisation (RESQML usage guide 2.2, draft, §7.3.1.2.2 p. 98)",
                    organisation.title, organisation.qualified_type
                ),
            ));
        }

        // -- the representations, in order ------------------------------------------------
        let mut members: Vec<Ref> = Vec::with_capacity(inputs.representations.len());
        let mut nodes: std::collections::HashMap<[u64; 3], Vec<(usize, i64)>> =
            std::collections::HashMap::new();
        for (rank, shape) in inputs.representations.iter().enumerate() {
            let reference = shape.known.as_ref().and_then(|known| known.reference.clone()).ok_or_else(|| {
                Refusal::on(
                    "representations",
                    format!("`{}` is no object of the package - a framework gathers representations by reference", shape.name),
                )
            })?;
            if let Some(twice) = members
                .iter()
                .find(|one| one.uuid.eq_ignore_ascii_case(&reference.uuid))
            {
                return Err(Refusal::on(
                    "representations",
                    format!(
                        "`{}` is given twice - one representation has one index in a framework",
                        twice.title
                    ),
                ));
            }
            let mut global = 0i64;
            for patch in &shape.geometry.patches {
                for p in &patch.points {
                    nodes.entry(at(p)).or_default().push((rank, global));
                    global += 1;
                }
            }
            members.push(reference);
        }

        // -- the contacts ---------------------------------------------------------------------
        let mut contacts: Vec<Vec<ContactPatch>> = Vec::new();
        for lips in &inputs.lips {
            for patch in &lips.geometry.patches {
                let energyml_geometry::geometry::Topology::Polylines(runs) = &patch.topology else {
                    return Err(Refusal::on(
                        "lips",
                        format!(
                            "`{}` holds a patch that is no set of lines - a contact is lines of nodes",
                            lips.name
                        ),
                    ));
                };
                let mut contact = Vec::new();
                let mut start = 0usize;
                for (line, &run) in runs.iter().enumerate() {
                    let points = patch.points.get(start..start + run as usize).ok_or_else(|| {
                        Refusal::on(
                            "lips",
                            format!(
                                "`{}`: the lines add up to more nodes than the patch holds",
                                lips.name
                            ),
                        )
                    })?;
                    start += run as usize;
                    // Every representation that holds each node once.
                    let mut found = 0usize;
                    for (rank, member) in members.iter().enumerate() {
                        let mut list = Vec::with_capacity(points.len());
                        for p in points {
                            let here: Vec<i64> = nodes
                                .get(&at(p))
                                .map(|all| all.iter().filter(|(r, _)| *r == rank).map(|(_, n)| *n).collect())
                                .unwrap_or_default();
                            match here.as_slice() {
                                [one] => list.push(*one),
                                [] => break,
                                _ => {
                                    return Err(Refusal::on(
                                        "lips",
                                        format!(
                                            "`{}` patch {} line {line}: `{}` holds its node ({}, {}, {}) twice - which one the contact names is not decided here",
                                            lips.name, patch.index, member.title, p[0], p[1], p[2]
                                        ),
                                    ));
                                }
                            }
                        }
                        if list.len() == points.len() {
                            contact.push(ContactPatch {
                                representation: rank,
                                nodes: list,
                            });
                            found += 1;
                        }
                    }
                    if found == 0 {
                        return Err(Refusal::on(
                            "lips",
                            format!(
                                "`{}` patch {} line {line}: its nodes are not nodes of any representation given - a contact patch names nodes of one of them (ContactPatch.SupportingRepresentationNodes, RESQML 2.2 Structural.xsd l. 187-191); give the surface they lie on, or the lips `fault-throw` wrote with it",
                                lips.name, patch.index
                            ),
                        ));
                    }
                }
                contacts.push(contact);
            }
        }
        if params.contact_index.len() != contacts.len() {
            return Err(Refusal::on(
                "contactIndex",
                format!(
                    "{} index(es) for {} contact(s) - one per patch of `lips`, in order: the position of the contact interpretation each represents in the organisation",
                    params.contact_index.len(),
                    contacts.len()
                ),
            ));
        }
        for (at, index) in params.contact_index.iter().enumerate() {
            if *index < 0 {
                return Err(Refusal::on(
                    "contactIndex",
                    format!("`contactIndex[{at}]` is {index} - a position counts from 0"),
                ));
            }
            if params.contact_index[..at].contains(index) {
                return Err(Refusal::on(
                    "contactIndex",
                    format!(
                        "`contactIndex[{at}]` repeats {index} - the index is the contact's identity in the framework (Structural.xsd l. 119-124)"
                    ),
                ));
            }
        }

        // -- identity: what it says --------------------------------------------------------------
        let seed = format!(
            "surface-framework:{title}:{}:{}:{}",
            organisation.uuid.to_ascii_lowercase(),
            members
                .iter()
                .map(|one| one.uuid.to_ascii_lowercase())
                .collect::<Vec<_>>()
                .join(","),
            contacts
                .iter()
                .zip(&params.contact_index)
                .map(|(contact, index)| format!(
                    "{index}={}",
                    contact
                        .iter()
                        .map(|one| format!("{}#{}", one.representation, one.nodes.len()))
                        .collect::<Vec<_>>()
                        .join("+")
                ))
                .collect::<Vec<_>>()
                .join(";")
        );
        let uuid = cx.identity.uuid(&seed, 0)?;

        // -- the object -------------------------------------------------------------------------------
        let mut datasets = Vec::new();
        let mut written_contacts = Vec::new();
        let mut patch_count = 0usize;
        for (rank, (contact, index)) in contacts.iter().zip(&params.contact_index).enumerate() {
            let mut patches = Vec::new();
            for (at, one) in contact.iter().enumerate() {
                let path = format!("/resqml22/{uuid}/contact{rank}/patch{at}/nodes");
                patches.push(json!({
                    "$type": "resqml22.ContactPatch",
                    "RepresentationIndex": one.representation,
                    "SupportingRepresentationNodes": {
                        "$type": "eml23.IntegerExternalArray",
                        "ArrayIntegerType": "arrayOfInt64LE",
                        "NullValue": -1,
                        "CountPerValue": 1,
                        "Values": support::external(&file, &path, &[one.nodes.len() as i64]),
                    },
                }));
                datasets.push(Dataset {
                    file: file.clone(),
                    path,
                    dimensions: vec![one.nodes.len() as i64],
                    values: Numbers::I64(one.nodes.clone()),
                });
                patch_count += 1;
            }
            written_contacts.push(json!({
                "$type": "resqml22.NonSealedContact",
                "Index": index,
                "Patches": patches,
            }));
        }
        let homogeneous = members
            .windows(2)
            .all(|pair| class_of(&pair[0].qualified_type) == class_of(&pair[1].qualified_type));
        let mut object = json!({
            "$type": "resqml22.NonSealedSurfaceFrameworkRepresentation",
            "uuid": uuid,
            "schemaVersion": "2.2",
            "Citation": support::citation(cx.when, &title),
            "RepresentedObject": organisation.dor(),
            "IsHomogeneous": homogeneous,
            "Representation": members.iter().map(Ref::dor).collect::<Vec<_>>(),
        });
        if !written_contacts.is_empty() {
            object["Contacts"] = json!(written_contacts);
        }

        // -- what the plan says --------------------------------------------------------------------------
        let mut says = vec![Note {
            key: "order",
            detail: format!(
                "{} representation(s), indexed as given: {}",
                members.len(),
                members
                    .iter()
                    .enumerate()
                    .map(|(at, one)| format!("{at} `{}`", one.title))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }];
        if contacts.is_empty() {
            says.push(Note {
                key: "no-contact",
                detail: "no contact is written: the representations are gathered, and how they meet is not stated - the non-sealed framework the usage guide describes as a step of the workflow (§8.3.2.1 p. 119-120)".to_string(),
            });
        } else {
            says.push(Note {
                key: "contact-index",
                detail: format!(
                    "{} contact(s), each `Index` the position stated in `contactIndex` - RESQML 2.2 contact interpretations have no index of their own, and the organisation's list is not read here: not checked",
                    contacts.len()
                ),
            });
        }
        says.push(Note {
            key: "non-sealed",
            detail: "non-sealed: a sealed framework states collocated nodes (RESQML usage guide 2.2, draft, §8.3.2.2 p. 120-121), and no fault surface written here has nodes on the horizons' cutoffs".to_string(),
        });

        // -- the activity -----------------------------------------------------------------------------------
        let number = |name: &str, value: usize| Parameter::Double {
            title: name.to_string(),
            value: value as f64,
            uom: None,
        };
        let mut parameters = vec![
            number("representationCount", members.len()),
            number("contactCount", contacts.len()),
            number("contactPatchCount", patch_count),
        ];
        // What the registry cannot record - a list is no single value - and a
        // second run needs, in the order given.
        for index in &params.contact_index {
            parameters.push(Parameter::Double {
                title: "contactIndex".to_string(),
                value: *index as f64,
                uom: None,
            });
        }
        Ok(Plan {
            media: Vec::new(),
            blocked: None,
            objects: vec![NewObject {
                uuid,
                qualified_type: "resqml22.NonSealedSurfaceFrameworkRepresentation".to_string(),
                role: "framework".to_string(),
                title: title.clone(),
                json: object,
            }],
            datasets,
            activity: ActivityDraft {
                recipe: "surface-framework",
                title: format!("surface-framework → {title}"),
                seed,
                parameters,
            },
            says,
            dropped: Vec::new(),
            shapes: Vec::new(),
        })
    }

    /// What was counted while planning.
    fn reports(&self) -> Vec<Slot> {
        let count = |name: &'static str, about: &'static str| {
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
            count("representationCount", "representations gathered"),
            count("contactCount", "contacts written"),
            count(
                "contactPatchCount",
                "contact patches written, one per line of nodes",
            ),
        ]
    }
}
