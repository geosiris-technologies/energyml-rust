//! `business-associate` — an `eml23.BusinessAssociate`: a company, an
//! organisation, a person's role, named so that other objects can point at it.
//!
//! No operation wrote one before. The synthetic reference model asks for the
//! BRGM's (a "should" of its registry), and its first
//! consumer was already waiting: WITSML 2.1 requires `Channel/LoggingCompany`
//! (1..1, WITSML TRG v2.1 p. 138), so `witsml-logs` takes one by reference and
//! its recipe step could not play while nothing wrote one.
//!
//! # What the schema asks, and what is written
//!
//! `BusinessAssociate` (eml 2.3, BusinessAssociate.xsd) *« Describes any
//! company, person, group, consultant, etc., which is associated within a
//! context (e.g., a well) »*. Its one required element is `Name` (≤ 256
//! characters); everything else is optional. Written, and only when given:
//!
//! - **`Name`**, and `Citation/Title` beside it — the same text: the title of a
//!   business associate is its name;
//! - **`Role`**, 0..∞ `NameStruct`: *« A business associate generally has one
//!   role but the role may be called different things in different naming
//!   systems »* — hence several roles, and one optional `authority` (the naming
//!   system) put on each. A role given twice is refused, by its text;
//! - **`OrganizationKind`**, from the schema's own closed list
//!   (`OrganizationKind`); the open extension (`.*:.*`) is not offered — a
//!   word nobody reads is not a kind;
//! - **`AuthorityCode`** — *« The code used when this business associate is
//!   used as an authority attribute »*: the word an `ObjectAlias/@authority`
//!   writes to mean this associate;
//! - **`Aliases`**, each `authority=identifier` — `ObjectAlias` requires both —,
//!   with an optional `IdentifierKind` per alias from EML's list
//!   (`aliasKinds`, as many as the aliases or none);
//! - **`Citation/Description`**, when given.
//!
//! Every text the schema bounds (`Name` 256, `Role` 64, `@authority` 64,
//! `AuthorityCode` 64, `Identifier` 64, `Description` 2000) is **refused** when
//! longer, never cut: half a name reads exactly like a whole one.
//!
//! # What is never written
//!
//! `Address`, `PhoneNumber`, `Email`, `Contact`, `PersonName`,
//! `EffectiveDateTime`, `TerminationDateTime`, `Purpose`, `IsInternal`,
//! `AssociatedWith`, `PersonnelCount`: this operation takes none of them.
//! A real organisation's contact details are not this build's to state, and
//! a synthetic one's would be invented — the reference model's rule 4 wants the
//! real to keep its provenance and the invented to say so, and an address
//! typed into a recipe is neither. The plan **says** what it left out
//! (`left-out`), so an associate with a name only does not read as one whose
//! contact details were lost. The schema has no field for a website either.
//!
//! # Identity
//!
//! The uuid comes from the **name** alone: the same organisation is the same
//! object from one run to the next, and correcting its role rewrites it
//! rather than adding a second associate that every `LoggingCompany` would
//! then have to choose between.

use std::collections::BTreeSet;

use energyml_domain::provenance::Parameter;
use energyml_eml_2_3 as eml;

use crate::declare::{Choice, Params, Title};
use crate::operation::{About, ActivityDraft, Category, Context, NewObject, Note, Operation, Plan, Refusal};

/// The operation.
pub struct BusinessAssociate;

/// `Name`, `Citation/Title` (BusinessAssociate.xsd, Abstract.xsd).
const NAME_LIMIT: usize = 256;
/// `NameStruct`, `@authority`, `AuthorityCode`, `ObjectAlias/Identifier`.
const SHORT_LIMIT: usize = 64;
/// `Citation/Description`.
const DESCRIPTION_LIMIT: usize = 2000;

/// The kind of organisation — EML's `OrganizationKind`, a closed list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Choice)]
pub enum OrganizationKind {
    /// An academic institution.
    #[choice(name = "academic institution")]
    AcademicInstitution,
    /// A government agency.
    #[choice(name = "government agency")]
    GovernmentAgency,
    /// An industry organisation.
    #[choice(name = "industry organization")]
    IndustryOrganization,
    /// A non-governmental organisation.
    #[choice(name = "non-governmental organization")]
    NonGovernmentalOrganization,
    /// A unit of an organisation.
    #[choice(name = "organization unit")]
    OrganizationUnit,
}

impl OrganizationKind {
    fn written(self) -> eml::OrganizationKindExt {
        match self {
            OrganizationKind::AcademicInstitution => eml::OrganizationKindExt::AcademicInstitution,
            OrganizationKind::GovernmentAgency => eml::OrganizationKindExt::GovernmentAgency,
            OrganizationKind::IndustryOrganization => eml::OrganizationKindExt::IndustryOrganization,
            OrganizationKind::NonGovernmentalOrganization => {
                eml::OrganizationKindExt::NonMinusGovernmentalOrganization
            }
            OrganizationKind::OrganizationUnit => eml::OrganizationKindExt::OrganizationUnit,
        }
    }
}

/// What an alias is — EML's `AliasIdentifierKind`, a closed list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Choice)]
pub enum AliasKind {
    /// An abbreviation.
    #[choice(name = "abbreviation")]
    Abbreviation,
    /// An acronym.
    #[choice(name = "acronym")]
    Acronym,
    /// A common name.
    #[choice(name = "common name")]
    CommonName,
    /// An industry code.
    #[choice(name = "industry code")]
    IndustryCode,
    /// An industry name.
    #[choice(name = "industry name")]
    IndustryName,
    /// A lease identifier.
    #[choice(name = "lease identifier")]
    LeaseIdentifier,
    /// The name in the local language.
    #[choice(name = "local language name")]
    LocalLanguageName,
    /// The preferred name.
    #[choice(name = "preferred name")]
    PreferredName,
    /// A project number.
    #[choice(name = "project number")]
    ProjectNumber,
    /// A regulatory identifier.
    #[choice(name = "regulatory identifier")]
    RegulatoryIdentifier,
    /// A regulatory name.
    #[choice(name = "regulatory name")]
    RegulatoryName,
    /// A short name.
    #[choice(name = "short name")]
    ShortName,
    /// A unique identifier.
    #[choice(name = "unique identifier")]
    UniqueIdentifier,
}

impl AliasKind {
    fn written(self) -> eml::AliasIdentifierKindExt {
        use eml::AliasIdentifierKindExt as K;
        match self {
            AliasKind::Abbreviation => K::Abbreviation,
            AliasKind::Acronym => K::Acronym,
            AliasKind::CommonName => K::CommonName,
            AliasKind::IndustryCode => K::IndustryCode,
            AliasKind::IndustryName => K::IndustryName,
            AliasKind::LeaseIdentifier => K::LeaseIdentifier,
            AliasKind::LocalLanguageName => K::LocalLanguageName,
            AliasKind::PreferredName => K::PreferredName,
            AliasKind::ProjectNumber => K::ProjectNumber,
            AliasKind::RegulatoryIdentifier => K::RegulatoryIdentifier,
            AliasKind::RegulatoryName => K::RegulatoryName,
            AliasKind::ShortName => K::ShortName,
            AliasKind::UniqueIdentifier => K::UniqueIdentifier,
        }
    }
}

/// What `business-associate` is told.
#[derive(Debug, Params)]
pub struct BusinessAssociateParams {
    /// the associate's name — also its title, and what makes it the same associate from one run to the next
    pub title: Title,
    /// its roles in this context, as « logging company » or « data provider » — the same role may be named in several naming systems
    pub roles: Vec<String>,
    /// the naming system of the roles (NameStruct authority), as a company or a standard
    pub role_authority: Option<String>,
    /// the kind of organisation, from EML's closed list
    pub organization_kind: Option<OrganizationKind>,
    /// the code an ObjectAlias writes as its authority to mean this associate
    pub authority_code: Option<String>,
    /// its other names or identifiers, each authority=identifier
    pub aliases: Vec<String>,
    /// what each alias is, in the order of the aliases — as many as the aliases, or none
    pub alias_kinds: Vec<AliasKind>,
    /// a sentence on who the associate is, written in Citation/Description
    pub description: Option<String>,
}

/// A text the schema bounds, trimmed, refused when empty or too long.
fn bounded(slot: &str, what: &str, text: &str, limit: usize) -> Result<String, Refusal> {
    let text = text.trim();
    if text.is_empty() {
        return Err(Refusal::on(slot, format!("{what} is empty")));
    }
    let count = text.chars().count();
    if count > limit {
        return Err(Refusal::on(
            slot,
            format!(
                "{what} `{text}` has {count} characters and the schema holds {limit}: refused rather than cut — half a name reads exactly like a whole one"
            ),
        ));
    }
    Ok(text.to_string())
}

/// An optional text: absent when not given or blank, bounded otherwise.
fn optional(slot: &str, what: &str, text: Option<&str>, limit: usize) -> Result<Option<String>, Refusal> {
    match text.map(str::trim).filter(|text| !text.is_empty()) {
        None => Ok(None),
        Some(text) => bounded(slot, what, text, limit).map(Some),
    }
}

impl Operation for BusinessAssociate {
    type Params = BusinessAssociateParams;
    type Inputs = ();

    fn about(&self) -> About {
        About {
            name: "business-associate",
            describe: "a business associate — a company, an organisation, a person's role — by its name, its roles and its aliases; no contact details — takes no input",
            category: Category::Create,
            makes: &["eml23.BusinessAssociate"],
            recipe: "business-associate",
            represents: None,
        }
    }

    fn plan(&self, _inputs: (), params: BusinessAssociateParams, cx: &Context<'_>) -> Result<Plan, Refusal> {
        let name = params.title.0.trim().to_string();
        if name.is_empty() {
            return Err(Refusal::on(
                "title",
                "a business associate needs a name: it is its one required element, and what makes two runs name the same associate",
            ));
        }
        let name = bounded("title", "the name", &name, NAME_LIMIT)?;

        let authority = optional(
            "roleAuthority",
            "the naming system",
            params.role_authority.as_deref(),
            SHORT_LIMIT,
        )?;
        let mut roles = Vec::with_capacity(params.roles.len());
        let mut seen = BTreeSet::new();
        for (at, role) in params.roles.iter().enumerate() {
            let role = bounded("roles", &format!("role {}", at + 1), role, SHORT_LIMIT)?;
            if !seen.insert(role.clone()) {
                return Err(Refusal::on(
                    "roles",
                    format!("role {} `{role}` is given twice", at + 1),
                ));
            }
            roles.push(eml::NameStruct {
                value: role,
                authority: authority.clone(),
            });
        }
        if authority.is_some() && roles.is_empty() {
            return Err(Refusal::on(
                "roleAuthority",
                "a naming system for the roles, and no role: give the roles it names",
            ));
        }

        let authority_code = optional(
            "authorityCode",
            "the authority code",
            params.authority_code.as_deref(),
            SHORT_LIMIT,
        )?;
        let description = optional(
            "description",
            "the description",
            params.description.as_deref(),
            DESCRIPTION_LIMIT,
        )?;

        if !params.alias_kinds.is_empty() && params.alias_kinds.len() != params.aliases.len() {
            return Err(Refusal::on(
                "aliasKinds",
                format!(
                    "{} kind(s) for {} alias(es): a kind is read by its place, so give one per alias or none",
                    params.alias_kinds.len(),
                    params.aliases.len()
                ),
            ));
        }
        let mut aliases = Vec::with_capacity(params.aliases.len());
        let mut known = BTreeSet::new();
        for (at, alias) in params.aliases.iter().enumerate() {
            let Some((owner, identifier)) = alias.split_once('=') else {
                return Err(Refusal::on(
                    "aliases",
                    format!(
                        "alias {} `{}` names no authority: write it authority=identifier — an alias without its authority resolves nowhere",
                        at + 1,
                        alias.trim()
                    ),
                ));
            };
            let owner = bounded(
                "aliases",
                &format!("the authority of alias {}", at + 1),
                owner,
                SHORT_LIMIT,
            )?;
            let identifier = bounded(
                "aliases",
                &format!("the identifier of alias {}", at + 1),
                identifier,
                SHORT_LIMIT,
            )?;
            if !known.insert((owner.clone(), identifier.clone())) {
                return Err(Refusal::on(
                    "aliases",
                    format!("alias {} `{owner}={identifier}` is given twice", at + 1),
                ));
            }
            aliases.push(eml::ObjectAlias {
                authority: owner,
                identifier,
                identifier_kind: params.alias_kinds.get(at).map(|kind| kind.written()),
                ..Default::default()
            });
        }

        let no_role = roles.is_empty();
        let seed = format!("business-associate:{name}");
        let uuid = cx.identity.uuid(&seed, 0)?;
        let associate = eml::BusinessAssociate {
            uuid: uuid.clone(),
            schema_version: "2.3".to_string(),
            citation: eml::Citation {
                title: name.clone(),
                originator: cx.when.originator.clone(),
                creation: cx.when.creation.clone(),
                format: energyml_core::CITATION_FORMAT.to_string(),
                description,
                ..Default::default()
            },
            aliases,
            name: name.clone(),
            authority_code,
            organization_kind: params.organization_kind.map(OrganizationKind::written),
            role: roles,
            ..Default::default()
        };

        let mut says = vec![Note {
            key: "left-out",
            detail: "no address, phone number, email, contact, person name or dates: this operation takes none of them — an organisation's contact details are never invented here".to_string(),
        }];
        if no_role {
            says.push(Note {
                key: "no-role",
                detail: format!("`{name}` is written with no role: valid — Role is 0..∞ — and what it does in this context is not said"),
            });
        }

        Ok(Plan {
            media: Vec::new(),
            blocked: None,
            objects: vec![NewObject {
                uuid,
                qualified_type: "eml23.BusinessAssociate".to_string(),
                role: "associate".to_string(),
                title: name.clone(),
                json: energyml_core::json::write_value(&associate),
            }],
            datasets: Vec::new(),
            activity: ActivityDraft {
                recipe: "business-associate",
                title: format!("business-associate → {name}"),
                seed,
                parameters: vec![Parameter::Text {
                    title: "title".to_string(),
                    value: name,
                }],
            },
            says,
            dropped: Vec::new(),
            shapes: Vec::new(),
        })
    }
}
