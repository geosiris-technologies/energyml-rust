//! The generated model, reachable from the bridge.
//!
//! The classes generated from the model must be reachable through the bridge,
//! so that objects can be created from Python too.
//!
//! There are no Python classes, and that is the decision: one class per type
//! per release would be a second copy of `crates/generated/` to keep in step.
//! What crosses instead is the **reflection** — which types exist, what fields
//! they hold, what an enumeration accepts — and an empty object of a type,
//! ready to fill. These checks are about that being true and *usable*: a
//! catalogue nobody can build an object from would be a listing, not an access.

use energyml_rust::session::{Package, describe_type, model_types, template};

#[test]
fn the_catalogue_is_the_generated_model_itself() {
    let top = model_types(None, true);
    let all = model_types(None, false);

    // Not pinned to a number: the point is that the catalogue is the Rust
    // registry, so it grows with the workspace. What must hold is the shape —
    // every top-level type is nameable, and there are far more types than
    // top-level ones, because most of the model is nested.
    assert!(
        top.len() > 100,
        "the eleven releases carry more than that: {}",
        top.len()
    );
    assert!(all.len() > top.len(), "most of the model is not top-level");
    for one in &top {
        assert!(one.is_top_level);
        assert!(
            one.qualified_type.is_some(),
            "{} is top-level and unnameable",
            one.type_key
        );
        assert!(!one.package.is_empty() && !one.version.is_empty(), "{one:?}");
    }

    // A development drop is read, never offered: the top-level list is what a
    // caller about to create an object wants, and `2.2_dev3` shares the
    // release's qualified types. Two rows `resqml22.Model` in it is how the
    // front end's chooser listed the same type twice.
    assert!(
        all.iter().any(|one| one.version.contains("dev")),
        "the inventory keeps the drop, or this check proves nothing"
    );
    assert!(
        top.iter().all(|one| !one.version.contains("dev")),
        "a development drop is offered for creation"
    );
    let mut qualified: Vec<&str> = top
        .iter()
        .filter_map(|one| one.qualified_type.as_deref())
        .collect();
    let before = qualified.len();
    qualified.sort_unstable();
    qualified.dedup();
    assert_eq!(qualified.len(), before, "one qualified type is offered twice");

    // And the filter is the package, not a substring of the name.
    let resqml = model_types(Some("resqml"), true);
    assert!(!resqml.is_empty());
    for one in &resqml {
        assert_eq!(one.package, "resqml", "{} is not resqml", one.type_key);
    }
}

#[test]
fn a_type_says_what_it_holds() {
    let described = describe_type("resqml22.TriangulatedSetRepresentation").expect("it exists");
    assert_eq!(described.kind, "struct");
    assert!(!described.summary.is_abstract);

    // **The three things someone filling a dictionary asks**, and the reason
    // this is not just a list of names: what is the field called in JSON, must
    // I fill it, and what type goes in it.
    let citation = described
        .fields
        .iter()
        .find(|one| one.name == "Citation")
        .expect("every EnergyML object carries a citation");
    assert!(citation.min_occurs > 0, "a citation is required");
    assert!(
        citation.type_name.contains("Citation"),
        "the type is nameable, not a pointer: {}",
        citation.type_name
    );

    // An enumeration field offers its values, which is what makes a template
    // fillable without reading the XSD.
    let role = described.fields.iter().find(|one| one.name == "SurfaceRole");
    if let Some(role) = role {
        assert!(!role.allowed.is_empty(), "an enumeration says what it accepts");
    }

    // The same type by its plain name, because both spellings are in use.
    let again = describe_type("TriangulatedSetRepresentation").expect("by name too");
    assert_eq!(again.summary.type_key, described.summary.type_key);
}

#[test]
fn a_name_that_matches_nothing_says_what_was_tried() {
    let why = describe_type("resqml22.NotAThing").expect_err("no such type");
    assert!(why.contains("qualified type"), "{why}");
    assert!(why.contains("type name"), "{why}");
    // The two spellings are easy to confuse, and "not found" alone sends
    // someone reading source code.
    assert!(why.contains("NotAThing"), "{why}");
}

#[test]
fn a_template_is_an_object_the_package_accepts_once_it_is_filled() {
    let made = template("resqml22.BoundaryFeature", None).expect("a template");
    let value: serde_json::Value = serde_json::from_str(&made).expect("JSON");
    assert_eq!(value["$type"], "resqml22.BoundaryFeature", "it knows what it is");
    assert!(value.get("uuid").is_some(), "and where its identity goes");
    assert!(
        value.get("Citation").is_some(),
        "with the fields the type declares"
    );

    // **Empty where the caller must decide**, and the refusal says so rather
    // than inventing: a template that made up a uuid would put an object in a
    // package under an identity nobody chose.
    assert_eq!(value["uuid"], "", "the uuid is the caller's");
    let mut package = Package::create("t.epc").expect("an empty package");
    let why = package
        .put(&made)
        .expect_err("an object with no uuid is not a part");
    assert!(why.contains("uuid"), "and the refusal names it: {why}");

    // Filled in, it goes in.
    let mut filled: serde_json::Value = serde_json::from_str(&made).unwrap();
    filled["uuid"] = serde_json::json!("6a0f5fc2-3d7e-4f6a-bb52-2d2c4b6b5f31");
    let written = package.put(&filled.to_string()).expect("now it is a part");
    assert!(written.contains("6a0f5fc2"), "{written}");
}

#[test]
fn a_uuid_can_be_given_to_the_template_instead_of_after_it() {
    // The same thing, said one call earlier. Both forms stay: setting the
    // field afterwards is what a caller does when the uuid comes from
    // somewhere else, and neither is the default.
    let given = "6a0f5fc2-3d7e-4f6a-bb52-2d2c4b6b5f31";
    let made = template("resqml22.BoundaryFeature", Some(given)).expect("a template");
    let value: serde_json::Value = serde_json::from_str(&made).expect("JSON");
    assert_eq!(value["uuid"], given, "the uuid given is the uuid written");

    let mut package = Package::create("t.epc").expect("an empty package");
    let written = package.put(&made).expect("it is a part straight away");
    assert!(written.contains(given), "{written}");

    // **Refused at the call that made the mistake**, rather than at the write:
    // a malformed uuid here would otherwise travel into the object and come
    // back as "this object cannot be a part", a message about a different
    // line.
    let why = template("resqml22.BoundaryFeature", Some("not-a-uuid")).expect_err("`not-a-uuid` is not one");
    assert!(why.contains("not-a-uuid"), "the refusal names the value: {why}");

    // And a type with nowhere to put an identity is an answer, not a failure:
    // the uuid is simply not written, and the object is what it was.
    let nested = template("eml23.Citation", Some(given));
    if let Ok(text) = nested {
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert!(value.get("uuid").is_none(), "a citation carries no identity");
    }
}

#[test]
fn an_abstract_type_cannot_be_made_and_says_what_can() {
    // The schema says it cannot be instantiated, so asking for one is a
    // question about which subtype — and a refusal that did not list them
    // would leave the caller grepping the XSD.
    let abstract_one = model_types(None, true).into_iter().find(|one| one.is_abstract);
    let Some(one) = abstract_one else {
        eprintln!("skipped: no abstract top-level type is registered in this build");
        return;
    };
    let name = one.qualified_type.expect("top-level types are nameable");
    let why = template(&name, None).expect_err("an abstract type is not a thing");
    assert!(why.contains("abstract"), "{why}");
    assert!(why.contains(&name), "{why}");
}

#[test]
fn the_types_a_store_supports_are_the_ones_it_can_read() {
    let package = Package::create("t.epc").expect("an empty package");
    let supported = package.supported_types();
    assert!(!supported.is_empty(), "a store with a registry supports types");

    // The store's list and the catalogue's are the same model, seen from two
    // places: a store that supported types the model does not carry would be
    // promising to read something nothing can build.
    let known: Vec<String> = model_types(None, false)
        .into_iter()
        .filter_map(|one| one.qualified_type)
        .collect();
    for one in supported.iter().filter_map(|one| one.qualified_type.as_ref()) {
        assert!(known.contains(one), "{one} is supported and not in the catalogue");
    }
}
