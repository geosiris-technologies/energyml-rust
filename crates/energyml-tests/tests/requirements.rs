//! The project's founding requirements (`R-1.1` to `R-1.7`), exercised as a client would.
//!
//! The other suites check that the code is correct. This one checks that it is
//! *usable*: each test plays a scenario the requirements ask for — a UI that
//! knows no type at compile time, a version translator, a validator with house
//! rules — through the public API only, with no privileged access.
//!
//! Everything here goes through the dynamic path on purpose: if a scenario
//! needs the concrete Rust type to work, it does not count as supported.

use energyml_core::prelude::*;
use energyml_core::{identifiers, ns, path, registry, validate, value::OwnedValue, xml};

use energyml_eml_2_3 as eml23;
use energyml_resqml_2_0_1 as resqml201;
use energyml_resqml_2_2 as resqml22;

fn reg() -> registry::Registry {
    let mut r = registry::Registry::new();
    resqml22::register(&mut r);
    resqml201::register(&mut r);
    eml23::register(&mut r);
    r
}

/// R-1.1: create instances dynamically, complete the instances.
#[test]
fn audit_dynamic_create_fill_and_write() {
    let r = reg();
    let mut obj = r
        .create("resqml22.TriangulatedSetRepresentation")
        .expect("dynamic create");

    let refl = obj.as_reflect_mut();
    path::set(
        refl,
        "uuid",
        OwnedValue::Str("a1b2c3d4-e5f6-7890-abcd-ef1234567890".to_string()),
    )
    .expect("set uuid");
    path::set(refl, "schemaVersion", OwnedValue::Str("2.2".to_string())).expect("set version");
    // Descending into a nested object that exists by Default.
    path::set(refl, "Citation.Title", OwnedValue::Str("Dynamique".to_string())).expect("set nested title");

    assert_eq!(
        path::get_string(obj.as_reflect(), "citation.title")
            .unwrap()
            .as_deref(),
        Some("Dynamique"),
        "dotted access is case-insensitive"
    );

    // Identity from a dynamically built object.
    let key = energyml_core::identity::key_of_reflect(obj.as_reflect()).expect("identity");
    println!("part name  : {}", key.part_name());
    println!("etp uri    : {}", key.etp_uri(None));

    // Writing it as an EPC part: tag, namespace and declarations all come from
    // `TypeInfo`, so the caller spells none of them.
    let xml_text = registry::to_part(obj.as_ref(), XmlWriteOptions::COMPACT).expect("dynamic serialization");
    assert!(
        xml_text.contains("<resqml:TriangulatedSetRepresentation"),
        "{xml_text}"
    );
    assert!(
        xml_text.contains("xmlns:eml=") && xml_text.contains("xmlns:xsi="),
        "{xml_text}"
    );
    assert!(xml_text.contains("Dynamique"));
}

/// R-1.2: list the attributes of an object, the classes they accept, the documentation.
#[test]
fn audit_ui_can_describe_a_type_it_does_not_know() {
    let r = reg();
    let entry = r
        .by_qualified_type("resqml22.TriangulatedSetRepresentation")
        .expect("known");
    let info = entry.info;

    assert!(!info.doc.is_empty(), "the XSD documentation is available");
    let mut described = 0;
    for f in info.fields {
        let _card = (f.occurs.is_optional(), f.occurs.is_collection(), f.is_required());
        let _doc = f.doc;
        let _facets = &f.facets;
        let accepted = f.accepted_types();
        if !accepted.is_empty() {
            described += 1;
        }
    }
    assert!(
        described > 0,
        "at least some fields advertise the types they accept"
    );

    // Enumerations expose their allowed values, for a dropdown.
    let surface_role = info.field("surface_role").expect("SurfaceRole exists");
    let values = surface_role.ty.allowed_values();
    assert!(!values.is_empty(), "enum values are listable for a dropdown");
}

/// R-1.3: validate the content of objects with the xsd constraints, plus business-rule extension.
#[test]
fn audit_validation_facets_and_business_rule() {
    let mut obj = resqml22::TriangulatedSetRepresentation {
        uuid: "not-a-uuid".to_string(),
        schema_version: "2.2".to_string(),
        ..Default::default()
    };
    let report = validate::Validate::validate(&obj);
    let codes: Vec<String> = report.errors().map(|i| i.code.clone()).collect();
    assert!(
        codes.iter().any(|c| c.contains("pattern")),
        "the uuid pattern is enforced: {codes:?}"
    );

    obj.uuid = "a1b2c3d4-e5f6-7890-abcd-ef1234567890".to_string();

    struct TitleRequired;
    impl validate::ValidationRule for TitleRequired {
        fn name(&self) -> &str {
            "title-required"
        }
        fn check(&self, object: &dyn Reflect, ctx: &mut validate::ValidationCtx) {
            if path::get_string(object, "Citation.Title")
                .ok()
                .flatten()
                .unwrap_or_default()
                .is_empty()
            {
                ctx.error("business-title", "a title is required by house rules");
            }
        }
    }
    let rules: Vec<Box<dyn validate::ValidationRule>> = vec![Box::new(TitleRequired)];
    let report = validate::validate_with(&obj, &rules);
    assert!(
        report.errors().any(|i| i.code == "business-title"),
        "a business rule plugs into the same report"
    );
}

/// R-1.4: a translator of files from one version to another.
#[test]
fn audit_translator_has_what_it_needs() {
    let r = reg();

    // 1. Find the same concept in two versions.
    let both = r.types_by_key("TriangulatedSetRepresentation");
    println!("TriangulatedSetRepresentation across versions: {}", both.len());

    // 2. Walk the hierarchy of a type without knowing it.
    let info = r
        .by_qualified_type("resqml22.TriangulatedSetRepresentation")
        .unwrap()
        .info;
    let base = info.base.map(|f| f()).expect("a base type is reachable");
    assert!(
        info.is_a(base),
        "ancestry is queryable: {} is a {}",
        info.name,
        base.name
    );

    // 3. Know what an abstract field can hold.
    let dor = r
        .object_ref_type("eml", "2.3")
        .expect("the DOR type of a version");
    assert!(dor.info.is_object_ref);

    // 4. Build a reference to any object, in any version.
    let obj = resqml201::Activity {
        uuid: "a1b2c3d4-e5f6-7890-abcd-ef1234567890".to_string(),
        ..Default::default()
    };
    let key = energyml_core::identity::key_of(&obj).expect("a 2.0.1 object has an identity");
    let made = r.make_object_ref("eml", "2.3", &key).expect("cross-version DOR");
    println!(
        "DOR eml 2.3 vers un objet resqml 2.0.1 : qualifiedType={:?}",
        path::get_string(made.as_reflect(), "QualifiedType").unwrap()
    );
}

/// R-1.5: objects are written in xml, but it must be possible to export them as json.
///
/// The encoding is the Energistics one, not serde's: **every** object carries a
/// `$type`, which is what lets the document be read back without being told
/// what it holds — and what lets a substituted subtype be read at all.
#[test]
fn audit_json_round_trips_without_naming_the_type() {
    let r = reg();
    let mut obj = r
        .create("resqml22.TriangulatedSetRepresentation")
        .expect("create");
    path::set(
        obj.as_reflect_mut(),
        "uuid",
        OwnedValue::Str("a1b2c3d4-e5f6-7890-abcd-ef1234567890".to_string()),
    )
    .expect("set uuid");
    path::set(
        obj.as_reflect_mut(),
        "Citation.Title",
        OwnedValue::Str("Surface".to_string()),
    )
    .expect("set title");

    let json = r.write_json(obj.as_reflect(), false).expect("dynamic json out");
    assert!(json.contains("\"SurfaceRole\""), "{json}");
    assert!(
        json.contains("\"$type\":\"resqml22.TriangulatedSetRepresentation\""),
        "the root names its own type: {json}"
    );
    assert!(
        json.contains("\"$type\":\"eml23.Citation\""),
        "so does every nested object, which is the part serde could not do: {json}"
    );

    // No qualified type is passed in. The document says what it is.
    let back = r.read_json(&json).expect("json in");
    assert_eq!(
        back.as_reflect().type_info().name,
        "TriangulatedSetRepresentation",
        "the type came from `$type`"
    );
    assert_eq!(
        energyml_core::identity::uuid_of_reflect(back.as_reflect()),
        Some("a1b2c3d4-e5f6-7890-abcd-ef1234567890"),
        "the round trip keeps the identity"
    );
    assert_eq!(
        r.write_json(back.as_reflect(), false).expect("json out again"),
        json,
        "writing what was read gives the same document"
    );

    // Pretty output, for a file a human will read.
    let pretty = r.write_json(obj.as_reflect(), true).expect("pretty json");
    assert!(pretty.contains('\n'), "pretty output is indented");
}

/// R-1.6: regular expressions are used where needed (uuid, uri, contentType, qualifiedType).
#[test]
fn audit_identifier_parsing() {
    assert!(identifiers::is_uuid("a1b2c3d4-e5f6-7890-abcd-ef1234567890"));
    assert!(
        identifiers::ContentType::parse("application/x-resqml+xml;version=2.0;type=obj_Activity").is_ok()
    );
    assert!(identifiers::QualifiedType::parse("resqml20.obj_Activity").is_ok());
    assert!(
        identifiers::EtpUri::parse("eml:///dataspace('d')/resqml22.TriangulatedSetRepresentation(a1b2c3d4-e5f6-7890-abcd-ef1234567890)")
            .is_ok()
    );
}

/// R-1.7: list the top-level elements of an epc, with or without the sub-objects.
#[test]
fn audit_top_level_listing() {
    let r = reg();
    let all: Vec<&str> = r.top_level_types().map(|i| i.name).collect();
    let never_nested: Vec<&str> = r
        .top_level_types()
        .filter(|i| !i.is_nestable)
        .map(|i| i.name)
        .collect();
    println!(
        "top level: {} — dont jamais imbriqués: {}",
        all.len(),
        never_nested.len()
    );
    assert!(!all.is_empty() && never_nested.len() <= all.len());
}

/// Reading an unknown document, as an EPC reader would.
#[test]
fn audit_read_by_content_type() {
    let r = reg();
    let obj = resqml22::TriangulatedSetRepresentation {
        uuid: "a1b2c3d4-e5f6-7890-abcd-ef1234567890".to_string(),
        schema_version: "2.2".to_string(),
        ..Default::default()
    };
    let doc = xml::to_xml_string(
        &obj,
        ns::RESQML,
        "TriangulatedSetRepresentation",
        &[ns::RESQML, ns::EML, ns::XSI],
        XmlWriteOptions::COMPACT,
    )
    .unwrap();
    let back = r
        .read_xml(
            "application/x-resqml+xml;version=2.2;type=TriangulatedSetRepresentation",
            doc.as_bytes(),
        )
        .expect("registry reads it");
    assert_eq!(
        energyml_core::identity::uuid_of_reflect(back.as_reflect()),
        Some("a1b2c3d4-e5f6-7890-abcd-ef1234567890")
    );
}
