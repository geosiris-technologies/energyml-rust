//! `structural-organisation` - a `StructuralOrganizationInterpretation`: the
//! boundaries of a structural scenario in the order it states, the frontiers
//! that close it, and the contacts between them.
//!
//! Step 7 of the build plan of Lutetia, two organisations:
//! A, ordered and dated, and B, unordered,
//! the two scenarios of
//! the use case, which the earth model gathers (`Structure` 0..∞) and the front end
//! offers a selector between.
//!
//! # One ordered list, as the schema has it
//!
//! RESQML 2.2 lists horizons **and** faults in one
//! `OrderedBoundaryFeatureInterpretation`, ordered by the
//! `AscendingOrderingCriteria` (Interpretations.xsd l. 976-1012) - the 2.0.1
//! split into `Horizons` and `Faults`, and the `UnorderedFaultCollection` of
//! the development drop, are gone from the final release. So
//! `boundaries` is **one** slot, `ordered`: the position of each
//! entry is the organisation's statement, and a fault's place among the
//! horizons is part of it. Two slots - horizons, faults - would leave this
//! operation to decide how they interleave, which is the order itself; the
//! role of each entry is what its type says, a `FaultInterpretation` or a
//! horizon. (This workspace's reader places a fault by its contacts and reads
//! its position as meaning nothing, `energyml_domain::stratigraphy`; another
//! reader may not, which is why the position is written as said.)
//!
//! *“BUSINESS RULE: Only BoundaryFeatureInterpretation and
//! FeatureInterpretationSet having faults as homogeneous type must be used to
//! build a StructuralOrganizationInterpretation”* (Interpretations.xsd
//! l. 593-598): an entry that is no boundary interpretation is refused when it
//! was addressed to the slot, and passed over, counted, when it only landed
//! there - the binder gives the one required object slot every object piece
//! nobody named, answers of a form included (`crates/energyml-build/ARCHITECTURE.md`,
//! an object slot receives the answers of the front end).
//!
//! # The feature it interprets: the model
//!
//! The TRG is silent; the rule adopted is the feature whose
//! identity is shared - for Lutetia's own organisations, the `Model`
//! "Lutetia". RESQML 2.2 has no organisation feature any more (the
//! 2.0.1 `OrganizationFeature` is gone), and the `Model` is the feature that
//! gathers features. Two ways to give it:
//!
//! - **`model`**, an object of the package;
//! - **`modelTitle`**, the title `earth-model` gives it. The `Model` is then
//!   written here by `earth-model`'s own writer - the same object, since its
//!   uuid comes from its title (`ops/earth_model.rs`) - so that the
//!   organisations can be written **before** the earth model that gathers
//!   them, which is the order a recipe needs: the earth model's
//!   `structures` names them, and they name its model. Under uuids given by the
//!   caller a title names no object, and this way is refused.
//!
//! # Contacts, ranks, frontiers - said in words
//!
//! A contact is the sentence the schema builds (`BinaryContactInterpretationPart`,
//! Interpretations.xsd l. 533-566): *“subject {side, mode} verb object {side,
//! mode}”*, the verb one of `stops`, `splits`, `crosses` (l. 86), a side one
//! of `ContactSide` (l. 33), a mode one of `ContactMode` (l. 23) - `F3 {east}
//! crosses F1 {west}` is an example. A rank is `name = rank`. A name is an object's
//! **title**, exactly, or the one title it begins - followed by something that
//! is not a letter or a digit, so `F1` is `F1 - interpretation` and never
//! `F10 - interpretation`; two candidates are refused, never chosen.
//!
//! What the guide says and the schema does not is **said**, never refused:
//! *“For boundary interpretation contacts, the qualifier is Contact_Side”* -
//! a
//! mode on a boundary is noted; a fault's side (`footwall`, `hanging wall`, a
//! point of the compass) on what is no fault, or a genetic side (`younger`,
//! `older`) on a fault, too (the enumeration's own words, l. 33-80).
//!
//! `TopFrontier`, `BottomFrontier`, `Sides`: *“It either points to a
//! CulturalFeature whose Kind is model frontier or to a
//! BoundaryFeatureInterpretation if the frontier is actually a geologic
//! surface”* (l. 994-1008). Anything else is refused; the kind of a cultural
//! feature is not known here, and the plan says so.
//!
//! Left out, and said here: `MultipleContactInterpretationPart` - its
//! `With` is a list of positions whose meaning the TRG and the schema do not
//! agree on, and this workspace's reader skips it,
//! and `PartOf`, which the reference model uses on units, not on an organisation.
//!
//! # Identity
//!
//! What the organisation says is what it is: its title, its model, its
//! criterion, its members in order with their ranks, its frontiers and its
//! contacts. The same content is the same object; a horizon added later - H4
//! to H7 wait for a human review (the reference model's recipe) - makes
//! another one, a new snapshot the earth model then names. The title is in:
//! two scenarios that happen to say the same thing today are still two.

use energyml_domain::provenance::Parameter;
use energyml_eml_2_3 as eml;
use energyml_resqml_2_2 as resqml;

use crate::declare::{Choice, Inputs, ObjectIn, Params, Title};
use crate::operation::{
    About, ActivityDraft, Category, Context, Dropped, Identity, Need, NewObject, Note, Operation, Plan,
    Refusal, Slot,
};
use crate::pieces::Ref;

use super::earth_model::{EarthModel, EarthModelInputs, EarthModelParams};

/// The operation.
pub struct StructuralOrganisation;

/// How the list is ordered - RESQML's `OrderingCriteria`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Choice)]
pub enum Criterion {
    /// From youngest to oldest.
    Age,
    /// From the surface downward.
    #[choice(name = "apparent depth")]
    ApparentDepth,
}

impl Criterion {
    fn written(self) -> resqml::OrderingCriteria {
        match self {
            Criterion::Age => resqml::OrderingCriteria::Age,
            Criterion::ApparentDepth => resqml::OrderingCriteria::ApparentDepth,
        }
    }
}

/// What `structural-organisation` is told.
#[derive(Debug, Params)]
pub struct OrganisationParams {
    /// the organisation's title - a scenario
    pub title: Title,
    /// how `boundaries` is ordered: by age, the youngest first, or by apparent depth, the shallowest first
    ///
    /// No default: which comes first is the geology, and a default would state
    /// it for whoever did not.
    pub ordering: Criterion,
    /// the title of the model it interprets, the one `earth-model` gives - the model is then written here, the same object; or give the model as `model`
    pub model_title: Option<String>,
    /// the rank of a boundary in the stratigraphic column, `name = rank` - the first rank on which the boundary is found
    pub ranks: Vec<String>,
    /// the contacts, `subject {side, mode} verb object {side, mode}` - the verb stops, splits or crosses, the braces optional
    pub contacts: Vec<String>,
}

/// What `structural-organisation` takes.
#[derive(Debug, Inputs)]
pub struct OrganisationInputs {
    /// the model the organisation interprets - or name it by `modelTitle`
    #[input(types = "resqml22.Model")]
    pub model: Option<ObjectIn>,
    /// the boundaries - horizons and faults - in the order the organisation states: the youngest first by age, the shallowest first by apparent depth
    #[input(
        types = "resqml22.HorizonInterpretation, resqml22.FaultInterpretation, resqml22.BoundaryFeatureInterpretation, resqml22.GeobodyBoundaryInterpretation",
        least = 1,
        ordered
    )]
    pub boundaries: Vec<ObjectIn>,
    /// what closes the organisation above - a model frontier, or a boundary interpretation such as the ground
    #[input(
        types = "resqml22.CulturalFeature, resqml22.BoundaryFeatureInterpretation, resqml22.HorizonInterpretation"
    )]
    pub top: Vec<ObjectIn>,
    /// what closes it below
    #[input(
        types = "resqml22.CulturalFeature, resqml22.BoundaryFeatureInterpretation, resqml22.HorizonInterpretation"
    )]
    pub bottom: Vec<ObjectIn>,
    /// what closes it around - the model's lateral frontier
    #[input(
        types = "resqml22.CulturalFeature, resqml22.BoundaryFeatureInterpretation, resqml22.HorizonInterpretation"
    )]
    pub sides: Vec<ObjectIn>,
}

/// The bare class of a qualified type, lower case - the comparison
/// `Takes::accepts` makes.
fn class_of(qualified_type: &str) -> String {
    let class = qualified_type
        .rsplit('.')
        .next()
        .unwrap_or(qualified_type)
        .to_ascii_lowercase();
    class.strip_prefix("obj_").map(str::to_string).unwrap_or(class)
}

/// A boundary interpretation, or one of the kinds that extend it
/// (Interpretations.xsd: `FaultInterpretation`, `FluidBoundaryInterpretation`,
/// `GeobodyBoundaryInterpretation`, `HorizonInterpretation`).
fn is_boundary(qualified_type: &str) -> bool {
    matches!(
        class_of(qualified_type).as_str(),
        "boundaryfeatureinterpretation"
            | "faultinterpretation"
            | "horizoninterpretation"
            | "geobodyboundaryinterpretation"
            | "fluidboundaryinterpretation"
    )
}

fn is_fault(qualified_type: &str) -> bool {
    class_of(qualified_type) == "faultinterpretation"
}

/// Whether a piece was addressed to that slot - named after it, `slot` or
/// `slot 2` - rather than handed to it by the binder for want of a name.
fn addressed(slot: &str, name: &str) -> bool {
    name == slot
        || name
            .strip_prefix(slot)
            .and_then(|rest| rest.strip_prefix(' '))
            .is_some_and(|rank| rank.parse::<usize>().is_ok())
}

/// An object the organisation references, with what a contact needs of it.
#[derive(Debug, Clone)]
struct Named {
    reference: Ref,
}

impl Named {
    fn dor(&self) -> eml::DataObjectReference {
        crate::crs::reference_of(&self.reference)
    }
}

/// The reference an input stands for, or a refusal naming its slot.
fn named(slot: &str, input: &ObjectIn) -> Result<Named, Refusal> {
    input
        .known
        .reference
        .clone()
        .map(|reference| Named { reference })
        .ok_or_else(|| {
            Refusal::on(
                slot,
                format!("`{}` was given without a reference to write", input.name),
            )
        })
}

/// The one object a name designates - its exact title, or the one title it
/// begins followed by something that is no letter or digit.
fn resolve<'a>(name: &str, candidates: &'a [Named]) -> Result<&'a Named, String> {
    let exact: Vec<&Named> = candidates
        .iter()
        .filter(|one| one.reference.title == name)
        .collect();
    let found: Vec<&Named> = if exact.is_empty() {
        candidates
            .iter()
            .filter(|one| {
                one.reference
                    .title
                    .strip_prefix(name)
                    .is_some_and(|rest| rest.chars().next().is_some_and(|next| !next.is_alphanumeric()))
            })
            .collect()
    } else {
        exact
    };
    match found.as_slice() {
        [one] => Ok(*one),
        [] => Err(format!(
            "`{name}` names none of the objects given - {}",
            candidates
                .iter()
                .map(|one| format!("`{}`", one.reference.title))
                .collect::<Vec<_>>()
                .join(", ")
        )),
        several => Err(format!(
            "`{name}` could be {} - name one by its whole title",
            several
                .iter()
                .map(|one| format!("`{}`", one.reference.title))
                .collect::<Vec<_>>()
                .join(" or ")
        )),
    }
}

/// The words of `ContactSide` (Interpretations.xsd l. 33).
fn side_of(word: &str) -> Option<resqml::ContactSide> {
    Some(match word {
        "footwall" => resqml::ContactSide::Footwall,
        "hanging wall" => resqml::ContactSide::HangingWall,
        "north" => resqml::ContactSide::North,
        "south" => resqml::ContactSide::South,
        "east" => resqml::ContactSide::East,
        "west" => resqml::ContactSide::West,
        "younger" => resqml::ContactSide::Younger,
        "older" => resqml::ContactSide::Older,
        "both" => resqml::ContactSide::Both,
        _ => return None,
    })
}

/// The words of `ContactMode` (Interpretations.xsd l. 23).
fn mode_of(word: &str) -> Option<resqml::ContactMode> {
    Some(match word {
        "conformable" => resqml::ContactMode::Conformable,
        "unconformable" => resqml::ContactMode::Unconformable,
        "extended" => resqml::ContactMode::Extended,
        _ => return None,
    })
}

/// The words of `ContactVerb` (Interpretations.xsd l. 86).
const VERBS: &[(&str, resqml::ContactVerb)] = &[
    ("stops", resqml::ContactVerb::Stops),
    ("splits", resqml::ContactVerb::Splits),
    ("crosses", resqml::ContactVerb::Crosses),
];

/// One side of a contact as written: a name, and what the braces say.
struct SideSaid<'t> {
    name: &'t str,
    side: Option<(&'t str, resqml::ContactSide)>,
    mode: Option<(&'t str, resqml::ContactMode)>,
}

fn side_said(text: &str) -> Result<SideSaid<'_>, String> {
    let text = text.trim();
    let (name, braces) = match text.strip_suffix('}') {
        Some(open) => {
            let at = open
                .rfind('{')
                .ok_or_else(|| format!("`{text}` closes a brace it never opened"))?;
            (open[..at].trim(), Some(&open[at + 1..]))
        }
        None => (text, None),
    };
    if name.is_empty() {
        return Err(format!("`{text}` names no object"));
    }
    if name.contains('{') || name.contains('}') {
        return Err(format!("`{text}`: the braces come after the name, once"));
    }
    let mut out = SideSaid {
        name,
        side: None,
        mode: None,
    };
    if let Some(inside) = braces {
        let words: Vec<&str> = inside
            .split(',')
            .map(str::trim)
            .filter(|one| !one.is_empty())
            .collect();
        for &word in &words {
            if let Some(side) = side_of(word) {
                if out.side.is_some() {
                    return Err(format!(
                        "`{text}` gives two sides - a contact element has one qualifier"
                    ));
                }
                out.side = Some((word, side));
            } else if let Some(mode) = mode_of(word) {
                if out.mode.is_some() {
                    return Err(format!(
                        "`{text}` gives two modes - a contact element has one secondary qualifier"
                    ));
                }
                out.mode = Some((word, mode));
            } else {
                return Err(format!(
                    "`{word}` is neither a side (footwall, hanging wall, north, south, east, west, younger, older, both) nor a mode (conformable, unconformable, extended)"
                ));
            }
        }
    }
    Ok(out)
}

/// A contact as written: its two sides and its verb.
struct ContactSaid<'t> {
    subject: SideSaid<'t>,
    verb: (&'static str, resqml::ContactVerb),
    object: SideSaid<'t>,
}

/// `subject {…} verb object {…}` - the verb found once, outside the braces,
/// between spaces.
fn contact_said(text: &str) -> Result<ContactSaid<'_>, String> {
    let mut found: Vec<(usize, usize, &'static str, resqml::ContactVerb)> = Vec::new();
    let mut depth = 0i32;
    for (at, ch) in text.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => depth -= 1,
            ' ' if depth == 0 => {
                for (word, verb) in VERBS.iter() {
                    let pattern = format!(" {word} ");
                    if text[at..].starts_with(pattern.as_str()) {
                        found.push((at, at + pattern.len(), *word, verb.clone()));
                    }
                }
            }
            _ => {}
        }
    }
    let [(start, end, word, verb)] = found.as_slice() else {
        return Err(if found.is_empty() {
            "names no verb - `stops`, `splits` or `crosses`, between spaces, between the two sides"
                .to_string()
        } else {
            format!(
                "names {} verbs - a title holding one of `stops`, `splits`, `crosses` cannot be told from the verb here",
                found.len()
            )
        });
    };
    Ok(ContactSaid {
        subject: side_said(&text[..*start])?,
        verb: (*word, verb.clone()),
        object: side_said(&text[*end..])?,
    })
}

impl Operation for StructuralOrganisation {
    type Params = OrganisationParams;
    type Inputs = OrganisationInputs;

    fn about(&self) -> About {
        About {
            name: "structural-organisation",
            describe: "a structural organisation - its boundaries, horizons and faults, in the order it states, the frontiers that close it and the contacts between them - interpreting a model",
            category: Category::Create,
            makes: &["resqml22.StructuralOrganizationInterpretation"],
            recipe: "structural-organisation",
            represents: None,
        }
    }

    fn plan(
        &self,
        inputs: OrganisationInputs,
        params: OrganisationParams,
        cx: &Context<'_>,
    ) -> Result<Plan, Refusal> {
        let title = params.title.0.trim().to_string();
        if title.is_empty() {
            return Err(Refusal::on(
                "title",
                "an organisation needs a title: it is what tells two scenarios apart",
            ));
        }
        let mut objects: Vec<NewObject> = Vec::new();
        let mut says: Vec<Note> = Vec::new();
        let mut dropped: Vec<Dropped> = Vec::new();

        // -- the feature it interprets -------------------------------------------------
        let model_title = params
            .model_title
            .as_deref()
            .map(str::trim)
            .filter(|one| !one.is_empty());
        let model: eml::DataObjectReference = match (&inputs.model, model_title) {
            (Some(_), Some(_)) => {
                return Err(Refusal::on(
                    "modelTitle",
                    "`model` and `modelTitle` both name the model the organisation interprets: give one",
                ));
            }
            (None, None) => {
                return Err(Refusal::on(
                    "model",
                    "an organisation interprets a feature, and the one that gathers features is the `Model`: give it as `model`, or name it by `modelTitle`",
                ));
            }
            (Some(given), None) => {
                let one = named("model", given)?;
                if class_of(&one.reference.qualified_type) != "model" {
                    return Err(Refusal::on(
                        "model",
                        format!(
                            "`{}` is a `{}` - an organisation interprets the `Model` that gathers its features",
                            one.reference.title, one.reference.qualified_type
                        ),
                    ));
                }
                one.dor()
            }
            (None, Some(model_title)) => {
                if matches!(cx.identity, Identity::Given(_)) {
                    return Err(Refusal::on(
                        "modelTitle",
                        "a model named by its title is one object only when uuids are derived - under uuids given by the caller it would be a second model of that title: give the model as `model`",
                    ));
                }
                // **`earth-model`'s writer**, not a second one: the same title is
                // the same `Model`, whichever step writes it first.
                let made = Operation::plan(
                    &EarthModel,
                    EarthModelInputs {
                        column: None,
                        structures: Vec::new(),
                        fluids: Vec::new(),
                        occurrences: Vec::new(),
                        wellbore_sets: Vec::new(),
                    },
                    EarthModelParams {
                        title: Title(model_title.to_string()),
                        interpretation_title: None,
                    },
                    cx,
                )?;
                let written = made
                    .objects
                    .into_iter()
                    .find(|one| one.role == "model")
                    .ok_or_else(|| Refusal::new("refused", "`earth-model` wrote no model"))?;
                let reference =
                    crate::crs::reference_to(&written.uuid, &written.qualified_type, &written.title);
                says.push(Note {
                    key: "model",
                    detail: format!("the `Model` “{}” is written here by `earth-model`'s writer - the same object an earth model of that title writes", written.title),
                });
                objects.push(written);
                reference
            }
        };

        // -- the boundaries, in the order given --------------------------------------
        let mut members: Vec<Named> = Vec::new();
        let mut passed_over: Vec<String> = Vec::new();
        for input in &inputs.boundaries {
            let one = named("boundaries", input)?;
            if !is_boundary(&one.reference.qualified_type) {
                if addressed("boundaries", &input.name) {
                    return Err(Refusal::on(
                        "boundaries",
                        format!(
                            "`{}` is a `{}` - “Only BoundaryFeatureInterpretation and FeatureInterpretationSet having faults as homogeneous type must be used to build a StructuralOrganizationInterpretation” (RESQML 2.2 Interpretations.xsd l. 593-598)",
                            one.reference.title, one.reference.qualified_type
                        ),
                    ));
                }
                passed_over.push(format!(
                    "`{}` ({})",
                    one.reference.title, one.reference.qualified_type
                ));
                continue;
            }
            if let Some(twice) = members
                .iter()
                .find(|seen| seen.reference.uuid.eq_ignore_ascii_case(&one.reference.uuid))
            {
                return Err(Refusal::on(
                    "boundaries",
                    format!(
                        "`{}` is given twice - one boundary has one place in an order",
                        twice.reference.title
                    ),
                ));
            }
            members.push(one);
        }
        if !passed_over.is_empty() {
            dropped.push(Dropped {
                count: passed_over.len(),
                why: format!(
                    "given to no slot and no boundary interpretation, so not a member: {}",
                    passed_over.join(", ")
                ),
            });
        }
        if members.is_empty() {
            return Err(Refusal::on(
                "boundaries",
                "no boundary interpretation is given: an organisation orders boundaries, one at least",
            ));
        }

        // -- the frontiers -------------------------------------------------------------
        let frontier = |slot: &str, list: &[ObjectIn]| -> Result<Vec<Named>, Refusal> {
            let mut out: Vec<Named> = Vec::new();
            for input in list {
                let one = named(slot, input)?;
                let class = class_of(&one.reference.qualified_type);
                if class != "culturalfeature" && !is_boundary(&one.reference.qualified_type) {
                    return Err(Refusal::on(
                        slot,
                        format!(
                            "`{}` is a `{}` - a frontier “either points to a CulturalFeature whose Kind is model frontier or to a BoundaryFeatureInterpretation if the frontier is actually a geologic surface” (RESQML 2.2 Interpretations.xsd l. 994-1008): name the cultural feature itself, or a boundary interpretation",
                            one.reference.title, one.reference.qualified_type
                        ),
                    ));
                }
                if !out
                    .iter()
                    .any(|seen| seen.reference.uuid.eq_ignore_ascii_case(&one.reference.uuid))
                {
                    out.push(one);
                }
            }
            Ok(out)
        };
        let top = frontier("top", inputs.top.as_slice())?;
        let bottom = frontier("bottom", inputs.bottom.as_slice())?;
        let sides = frontier("sides", inputs.sides.as_slice())?;
        let cultural: Vec<&Named> = top
            .iter()
            .chain(&bottom)
            .chain(&sides)
            .filter(|one| class_of(&one.reference.qualified_type) == "culturalfeature")
            .collect();
        if !cultural.is_empty() {
            says.push(Note {
                key: "frontier-kind",
                detail: format!(
                    "{} is a cultural feature: the business rule wants its kind to be `model frontier`, and what is known of it here does not say - not checked",
                    cultural.iter().map(|one| format!("`{}`", one.reference.title)).collect::<Vec<_>>().join(", ")
                ),
            });
        }

        // -- the ranks -------------------------------------------------------------------
        let mut ranks: Vec<Option<i64>> = vec![None; members.len()];
        for (at, entry) in params.ranks.iter().enumerate() {
            let Some((name, rank)) = entry.rsplit_once('=') else {
                return Err(Refusal::on(
                    "ranks",
                    format!("`ranks[{at}]` is `{entry}` - a rank is `name = rank`"),
                ));
            };
            let rank = rank
                .trim()
                .parse::<i64>()
                .ok()
                .filter(|rank| *rank >= 0)
                .ok_or_else(|| {
                    Refusal::on(
                        "ranks",
                        format!(
                            "`ranks[{at}]`: `{}` is not a rank - a whole number, zero or more",
                            rank.trim()
                        ),
                    )
                })?;
            let one = resolve(name.trim(), &members)
                .map_err(|why| Refusal::on("ranks", format!("`ranks[{at}]`: {why}")))?;
            let place = members
                .iter()
                .position(|member| member.reference.uuid == one.reference.uuid)
                .unwrap_or_default();
            if ranks[place].is_some() {
                return Err(Refusal::on(
                    "ranks",
                    format!(
                        "`{}` is given two ranks - the first rank on which it is found is one",
                        one.reference.title
                    ),
                ));
            }
            ranks[place] = Some(rank);
        }

        // -- the contacts ------------------------------------------------------------------
        let mut candidates: Vec<Named> = Vec::new();
        for one in members.iter().chain(&top).chain(&bottom).chain(&sides) {
            if !candidates
                .iter()
                .any(|seen| seen.reference.uuid.eq_ignore_ascii_case(&one.reference.uuid))
            {
                candidates.push(one.clone());
            }
        }
        let mut contacts: Vec<resqml::AbstractContactInterpretationPart> = Vec::new();
        let mut contact_keys: Vec<String> = Vec::new();
        for (at, entry) in params.contacts.iter().enumerate() {
            let said = contact_said(entry)
                .map_err(|why| Refusal::on("contacts", format!("`contacts[{at}]` `{entry}`: {why}")))?;
            let mut element = |side: &SideSaid<'_>| -> Result<(resqml::ContactElement, String), Refusal> {
                let one = resolve(side.name, &candidates)
                    .map_err(|why| Refusal::on("contacts", format!("`contacts[{at}]`: {why}")))?;
                let fault = is_fault(&one.reference.qualified_type);
                if let Some((word, _)) = side.side {
                    let of_fault = matches!(
                        word,
                        "footwall" | "hanging wall" | "north" | "south" | "east" | "west"
                    );
                    let of_genetic = matches!(word, "younger" | "older");
                    if (of_fault && !fault) || (of_genetic && fault) {
                        says.push(Note {
                            key: "contact-side",
                            detail: format!(
                                "`{entry}`: `{word}` is a side of {} (ContactSide, Interpretations.xsd l. 33-80), and `{}` is {} - written as said",
                                if of_fault { "a fault" } else { "a genetic boundary a fault splits" },
                                one.reference.title,
                                if fault { "a fault" } else { "no fault" }
                            ),
                        });
                    }
                }
                if let Some((word, _)) = side.mode {
                    says.push(Note {
                        key: "contact-mode",
                        detail: format!(
                            "`{entry}`: `{word}` is a mode, and `{}` a boundary - “For boundary interpretation contacts, the qualifier is Contact_Side” (RESQML usage guide 2.2, draft, §7.2.2.2 p. 83): written as said",
                            one.reference.title
                        ),
                    });
                }
                let key = format!(
                    "{}{{{},{}}}",
                    one.reference.uuid.to_ascii_lowercase(),
                    side.side.as_ref().map(|(word, _)| *word).unwrap_or_default(),
                    side.mode.as_ref().map(|(word, _)| *word).unwrap_or_default()
                );
                Ok((
                    resqml::ContactElement {
                        uuid: one.reference.uuid.clone(),
                        qualified_type: one.reference.qualified_type.clone(),
                        title: one.reference.title.clone(),
                        qualifier: side.side.as_ref().map(|(_, side)| side.clone()),
                        secondary_qualifier: side.mode.as_ref().map(|(_, mode)| mode.clone()),
                        ..Default::default()
                    },
                    key,
                ))
            };
            let (subject, subject_key) = element(&said.subject)?;
            let (direct_object, object_key) = element(&said.object)?;
            if subject.uuid.eq_ignore_ascii_case(&direct_object.uuid) {
                return Err(Refusal::on(
                    "contacts",
                    format!(
                        "`contacts[{at}]` `{entry}`: its subject and its object are one object - a contact is between two"
                    ),
                ));
            }
            contact_keys.push(format!("{subject_key} {} {object_key}", said.verb.0));
            contacts.push(
                resqml::AbstractContactInterpretationPart::BinaryContactInterpretationPart(
                    resqml::BinaryContactInterpretationPart {
                        part_of: None,
                        direct_object,
                        subject,
                        verb: said.verb.1,
                    },
                ),
            );
        }

        // -- what is said of the order ------------------------------------------------------
        let faults = members
            .iter()
            .filter(|one| is_fault(&one.reference.qualified_type))
            .count();
        says.push(Note {
            key: "order",
            detail: format!(
                "{} boundaries by {}, as given: {}",
                members.len(),
                params.ordering.word(),
                members
                    .iter()
                    .map(|one| format!("`{}`", one.reference.title))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        });
        if faults == members.len() {
            says.push(Note {
                key: "faults-only",
                detail: "the ordered list holds faults only: until horizons join it, their order places them among nothing - a reader of this workspace places a fault by its contacts (energyml_domain::stratigraphy)".to_string(),
            });
        }

        // -- identity: what it says -----------------------------------------------------------
        let seed = format!(
            "structural-organisation:{title}:{}:{}:{}:{}:{}:{}:{}",
            model.uuid.to_ascii_lowercase(),
            params.ordering.word(),
            members
                .iter()
                .zip(&ranks)
                .map(|(one, rank)| format!(
                    "{}#{}",
                    one.reference.uuid.to_ascii_lowercase(),
                    rank.map(|r| r.to_string()).unwrap_or_default()
                ))
                .collect::<Vec<_>>()
                .join(","),
            top.iter()
                .map(|one| one.reference.uuid.to_ascii_lowercase())
                .collect::<Vec<_>>()
                .join(","),
            bottom
                .iter()
                .map(|one| one.reference.uuid.to_ascii_lowercase())
                .collect::<Vec<_>>()
                .join(","),
            sides
                .iter()
                .map(|one| one.reference.uuid.to_ascii_lowercase())
                .collect::<Vec<_>>()
                .join(","),
            contact_keys.join(";")
        );
        let uuid = cx.identity.uuid(&seed, objects.len())?;

        let organisation = resqml::StructuralOrganizationInterpretation {
            uuid: uuid.clone(),
            schema_version: "2.2".to_string(),
            citation: eml::Citation {
                title: title.clone(),
                originator: cx.when.originator.clone(),
                creation: cx.when.creation.clone(),
                format: energyml_core::CITATION_FORMAT.to_string(),
                ..Default::default()
            },
            interpreted_feature: model,
            contact_interpretation: contacts,
            ascending_ordering_criteria: params.ordering.written(),
            bottom_frontier: bottom.iter().map(Named::dor).collect(),
            top_frontier: top.iter().map(Named::dor).collect(),
            sides: sides.iter().map(Named::dor).collect(),
            ordered_boundary_feature_interpretation: members
                .iter()
                .zip(&ranks)
                .map(|(one, rank)| resqml::BoundaryFeatureInterpretationPlusItsRank {
                    stratigraphic_rank: *rank,
                    boundary_feature_interpretation: Some(one.dor()),
                    feature_interpretation_set: None,
                })
                .collect(),
            ..Default::default()
        };
        let contact_count = organisation.contact_interpretation.len();
        objects.push(NewObject {
            uuid,
            qualified_type: "resqml22.StructuralOrganizationInterpretation".to_string(),
            role: "organisation".to_string(),
            title: title.clone(),
            json: energyml_core::json::write_value(&organisation),
        });

        // -- the activity ----------------------------------------------------------------------
        let number = |name: &str, value: usize| Parameter::Double {
            title: name.to_string(),
            value: value as f64,
            uom: None,
        };
        let mut parameters = vec![
            number("memberCount", members.len()),
            number("faultCount", faults),
            number("contactCount", contact_count),
        ];
        // What the registry cannot record - a list is no single value - and a
        // second run needs, in the order given.
        for entry in &params.ranks {
            parameters.push(Parameter::Text {
                title: "ranks".to_string(),
                value: entry.clone(),
            });
        }
        for entry in &params.contacts {
            parameters.push(Parameter::Text {
                title: "contacts".to_string(),
                value: entry.clone(),
            });
        }
        Ok(Plan {
            media: Vec::new(),
            blocked: None,
            objects,
            datasets: Vec::new(),
            activity: ActivityDraft {
                recipe: "structural-organisation",
                title: format!("structural-organisation → {title}"),
                seed,
                parameters,
            },
            says,
            dropped,
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
            count("memberCount", "boundaries in the ordered list"),
            count("faultCount", "of which faults"),
            count("contactCount", "contacts written"),
        ]
    }
}
