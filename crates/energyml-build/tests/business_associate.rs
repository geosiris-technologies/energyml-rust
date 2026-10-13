//! `business-associate`: one `eml23.BusinessAssociate`, by its name, its roles
//! and its aliases — never its contact details.
//!
//! The form of each assertion comes from the schema (eml 2.3,
//! BusinessAssociate.xsd, as generated in `energyml-eml-2-3`): `Name` is the
//! one required element, bounded at 256 characters; `Role` is 0..∞
//! `NameStruct` (≤ 64, with an optional `authority`) — *“A business associate
//! generally has one role but the role may be called different things in
//! different naming systems”*; `OrganizationKind` is a closed list;
//! `AuthorityCode` is *“The code used when this business associate is used as
//! an authority attribute”* (≤ 64); an `ObjectAlias` requires both its
//! `Identifier` (≤ 64) and its `authority`. The names, roles and aliases are
//! the test's own — no real organisation is described here.

use energyml_build::operation::{Choices, Identity, Plan, Registry, When};
use energyml_build::pieces::Pieces;
use serde_json::json;

fn when() -> When {
    When {
        creation: "2026-09-24T00:00:00Z".into(),
        originator: "test".into(),
    }
}

fn planned(choices: Choices) -> Result<Plan, energyml_build::operation::Error> {
    Registry::new().plan(
        "business-associate",
        &Pieces::empty(),
        &choices,
        &Identity::derived(),
        &when(),
    )
}

/// The slot the first refusal is about.
fn refused_on(choices: Choices) -> String {
    let why = planned(choices).expect_err("refused");
    why.refusals()
        .first()
        .and_then(|one| one.slot.clone())
        .unwrap_or_else(|| panic!("a refusal about no slot: {why}"))
}

fn full() -> Choices {
    Choices::new()
        .set("title", "Acme Logging")
        .set("roles", json!(["logging company", "data provider"]))
        .set("roleAuthority", "test")
        .set("organizationKind", "industry organization")
        .set("authorityCode", "ACME")
        .set(
            "aliases",
            json!(["ACME=Acme Logging Services", "registry=AC_0001"]),
        )
        .set("aliasKinds", json!(["preferred name", "regulatory identifier"]))
        .set("description", "A logging company invented for this test.")
}

/// The plan's one object, read back through the registry — the road `apply`
/// takes — into the generated type, so that an element written under a wrong
/// name is lost here, not in a package.
fn read_back(plan: &Plan) -> energyml_eml_2_3::BusinessAssociate {
    assert_eq!(plan.objects.len(), 1, "one associate");
    let schemas = energyml_schemas::all();
    let document = energyml_core::json::read_value(&plan.objects[0].json, &schemas)
        .unwrap_or_else(|why| panic!("the associate does not read back: {why}"));
    let root = document.root.as_reflect();
    root.downcast_ref::<energyml_eml_2_3::BusinessAssociate>()
        .expect("an eml 2.3 BusinessAssociate")
        .clone()
}

#[test]
fn what_is_given_is_what_reads_back() {
    let plan = planned(full()).expect("planned");
    assert_eq!(plan.objects[0].qualified_type, "eml23.BusinessAssociate");
    let associate = read_back(&plan);
    assert_eq!(associate.uuid, plan.objects[0].uuid);
    assert_eq!(associate.schema_version, "2.3");
    assert_eq!(associate.name, "Acme Logging");
    assert_eq!(
        associate.citation.title, "Acme Logging",
        "the title of an associate is its name"
    );
    assert_eq!(
        associate.citation.description.as_deref(),
        Some("A logging company invented for this test.")
    );
    let roles: Vec<(&str, Option<&str>)> = associate
        .role
        .iter()
        .map(|one| (one.value.as_str(), one.authority.as_deref()))
        .collect();
    assert_eq!(
        roles,
        vec![("logging company", Some("test")), ("data provider", Some("test"))]
    );
    assert_eq!(
        associate.organization_kind,
        Some(energyml_eml_2_3::OrganizationKindExt::IndustryOrganization)
    );
    assert_eq!(associate.authority_code.as_deref(), Some("ACME"));
    let aliases: Vec<(&str, &str, Option<energyml_eml_2_3::AliasIdentifierKindExt>)> = associate
        .aliases
        .iter()
        .map(|one| {
            (
                one.authority.as_str(),
                one.identifier.as_str(),
                one.identifier_kind.clone(),
            )
        })
        .collect();
    assert_eq!(
        aliases,
        vec![
            (
                "ACME",
                "Acme Logging Services",
                Some(energyml_eml_2_3::AliasIdentifierKindExt::PreferredName)
            ),
            (
                "registry",
                "AC_0001",
                Some(energyml_eml_2_3::AliasIdentifierKindExt::RegulatoryIdentifier)
            ),
        ]
    );
}

#[test]
fn no_contact_detail_is_written_and_the_plan_says_so() {
    let plan = planned(full()).expect("planned");
    let associate = read_back(&plan);
    assert!(associate.address.is_none(), "{:?}", associate.address);
    assert!(associate.phone_number.is_empty(), "{:?}", associate.phone_number);
    assert!(associate.email.is_empty(), "{:?}", associate.email);
    assert!(associate.contact.is_empty(), "{:?}", associate.contact);
    assert!(associate.person_name.is_none(), "{:?}", associate.person_name);
    assert!(
        plan.says.iter().any(|note| note.key == "left-out"),
        "{:?}",
        plan.says
    );
}

#[test]
fn a_name_alone_is_valid_and_its_missing_role_is_said() {
    let bare =
        planned(Choices::new().set("title", "Acme Logging")).expect("Name is the one required element");
    let associate = read_back(&bare);
    assert_eq!(associate.name, "Acme Logging");
    assert!(associate.role.is_empty() && associate.aliases.is_empty());
    assert!(associate.organization_kind.is_none() && associate.authority_code.is_none());
    assert!(
        bare.says.iter().any(|note| note.key == "no-role"),
        "{:?}",
        bare.says
    );
    let full = planned(full()).expect("planned");
    assert!(
        !full.says.iter().any(|note| note.key == "no-role"),
        "{:?}",
        full.says
    );
}

#[test]
fn the_same_name_is_the_same_associate() {
    let full = planned(full()).expect("planned");
    let bare = planned(Choices::new().set("title", "Acme Logging")).expect("planned");
    let other = planned(Choices::new().set("title", "Another Logging")).expect("planned");
    assert_eq!(
        full.objects[0].uuid, bare.objects[0].uuid,
        "correcting a role rewrites the same associate"
    );
    assert_ne!(
        full.objects[0].uuid, other.objects[0].uuid,
        "another name is another associate"
    );
    let again = planned(Choices::new().set("title", "  Acme Logging ")).expect("planned");
    assert_eq!(
        again.objects[0].uuid, bare.objects[0].uuid,
        "the name is read trimmed"
    );
}

#[test]
fn a_name_is_required_and_bounded() {
    assert_eq!(refused_on(Choices::new()), "title");
    assert_eq!(refused_on(Choices::new().set("title", "   ")), "title");
    // `Name` holds 256 characters: the 256th is written, the 257th refused —
    // counted in characters, not bytes (é is two bytes).
    let at_limit = "é".repeat(256);
    assert_eq!(
        read_back(&planned(Choices::new().set("title", at_limit.as_str())).expect("256 fit")).name,
        at_limit
    );
    assert_eq!(refused_on(Choices::new().set("title", "é".repeat(257))), "title");
}

/// The bounds the operation refuses at are the ones the schema's facets hold:
/// what it writes at the limit passes the generated validator — a reader
/// written from the XSD, not by this operation — and one character more is
/// what both refuse.
#[test]
fn what_is_written_at_the_limits_passes_the_schema_facets() {
    use energyml_core::validate::Validate;
    let plan = planned(
        full()
            .set("title", "é".repeat(256))
            .set("roles", json!(["r".repeat(64)])),
    )
    .expect("planned");
    let mut associate = read_back(&plan);
    let report = associate.validate();
    assert!(report.is_valid(), "{:?}", report.errors().collect::<Vec<_>>());
    // The validator is not idle: the 257th character is what the plan refuses
    // (`a_name_is_required_and_bounded`), and what the facets flag.
    associate.name.push('é');
    assert!(
        !associate.validate().is_valid(),
        "a 257-character Name passes the facets"
    );
}

#[test]
fn a_role_is_bounded_and_given_once() {
    let base = || Choices::new().set("title", "Acme Logging");
    assert!(
        planned(base().set("roles", json!(["r".repeat(64)]))).is_ok(),
        "64 characters fit"
    );
    assert_eq!(refused_on(base().set("roles", json!(["r".repeat(65)]))), "roles");
    assert_eq!(
        refused_on(base().set("roles", json!(["driller", "driller"]))),
        "roles"
    );
    assert_eq!(refused_on(base().set("roles", json!(["driller", " "]))), "roles");
    assert_eq!(
        refused_on(base().set("roleAuthority", "test")),
        "roleAuthority",
        "a naming system for no role"
    );
}

#[test]
fn an_alias_names_its_authority_and_its_kinds_come_one_each_or_not_at_all() {
    let base = || Choices::new().set("title", "Acme Logging");
    assert_eq!(
        refused_on(base().set("aliases", json!(["Acme"]))),
        "aliases",
        "no authority"
    );
    assert_eq!(
        refused_on(base().set("aliases", json!(["=Acme"]))),
        "aliases",
        "an empty authority"
    );
    assert_eq!(
        refused_on(base().set("aliases", json!(["ACME="]))),
        "aliases",
        "an empty identifier"
    );
    assert_eq!(
        refused_on(base().set("aliases", json!([format!("ACME={}", "x".repeat(65))]))),
        "aliases"
    );
    assert_eq!(
        refused_on(base().set("aliases", json!(["ACME=a", "ACME=a"]))),
        "aliases"
    );
    assert_eq!(
        refused_on(
            base()
                .set("aliases", json!(["ACME=a", "ACME=b"]))
                .set("aliasKinds", json!(["acronym"]))
        ),
        "aliasKinds"
    );
    // Only the first `=` separates: an identifier may hold one.
    let plan = planned(base().set("aliases", json!(["web=a=b"]))).expect("planned");
    let associate = read_back(&plan);
    assert_eq!(
        (
            associate.aliases[0].authority.as_str(),
            associate.aliases[0].identifier.as_str()
        ),
        ("web", "a=b")
    );
    assert!(
        associate.aliases[0].identifier_kind.is_none(),
        "no kind given, none written"
    );
}

#[test]
fn an_organisation_kind_outside_the_schema_list_is_refused() {
    let why = planned(
        Choices::new()
            .set("title", "Acme Logging")
            .set("organizationKind", "company"),
    )
    .expect_err("refused");
    assert!(
        why.refusals()
            .iter()
            .any(|one| one.slot.as_deref() == Some("organizationKind")),
        "{:?}",
        why.refusals()
    );
}

#[test]
fn it_takes_no_input() {
    let registry = Registry::new();
    let operation = registry.get("business-associate").expect("in the registry");
    assert!(operation.takes().is_empty());
    assert_eq!(operation.makes(), &["eml23.BusinessAssociate"]);
}
