//! End-to-end tests over the real generated crates.
//!
//! `energyml-core`'s own suite pins the derive macros against a hand-written
//! model. This one checks the other half: that `energyml-codegen` turns the
//! actual Energistics schemas into code that behaves the same way.

use energyml_core::prelude::*;
use energyml_core::{identifiers, path, registry, validate, xml};

use energyml_eml_2_0 as eml20;
use energyml_eml_2_3 as eml;
use energyml_resqml_2_2 as resqml;

fn citation(title: &str) -> eml::Citation {
    eml::Citation {
        title: title.to_string(),
        originator: "geosiris".to_string(),
        creation: "2026-07-30T10:00:00Z".to_string(),
        format: "Geosiris:energyml-rust:0.1.0/xml".to_string(),
        ..Default::default()
    }
}

fn sample() -> resqml::TriangulatedSetRepresentation {
    resqml::TriangulatedSetRepresentation {
        uuid: "a1b2c3d4-e5f6-7890-abcd-ef1234567890".to_string(),
        schema_version: "2.2".to_string(),
        citation: citation("Top Horizon"),
        surface_role: resqml::SurfaceRole::Map,
        ..Default::default()
    }
}

fn to_xml(value: &resqml::TriangulatedSetRepresentation) -> String {
    xml::to_xml_string(
        value,
        ns::RESQML,
        "TriangulatedSetRepresentation",
        &[ns::RESQML, ns::EML, ns::XSI],
        XmlWriteOptions::COMPACT,
    )
    .expect("serializes")
}

#[test]
fn generated_types_round_trip_through_xml() {
    let original = sample();
    let doc = to_xml(&original);
    let parsed: resqml::TriangulatedSetRepresentation = xml::from_xml_str(&doc).expect("parses back");
    assert_eq!(original, parsed);
}

#[test]
fn inherited_fields_keep_the_common_namespace() {
    let doc = to_xml(&sample());

    // Citation is declared by the eml schema and inherited through
    // AbstractObject, so it must be written as eml:Citation even though the
    // object itself is RESQML. This is the interop detail fesapi cares about.
    assert!(doc.contains("<eml:Citation>"), "{doc}");
    assert!(doc.contains("<eml:Title>Top Horizon</eml:Title>"), "{doc}");

    // Fields RESQML declares itself stay in the resqml namespace.
    assert!(
        doc.contains("<resqml:SurfaceRole>map</resqml:SurfaceRole>"),
        "{doc}"
    );

    // uuid is an attribute, not an element.
    assert!(
        doc.contains(r#"uuid="a1b2c3d4-e5f6-7890-abcd-ef1234567890""#),
        "{doc}"
    );
}

#[test]
fn schema_metadata_survives_generation() {
    let info = <resqml::TriangulatedSetRepresentation as EnergymlType>::type_info();
    assert_eq!(info.package, "resqml");
    assert_eq!(info.version, "2.2");
    assert!(info.is_top_level);
    assert_eq!(
        info.qualified_type,
        Some("resqml22.TriangulatedSetRepresentation")
    );
    assert_eq!(
        info.content_type,
        Some("application/x-resqml+xml;version=2.2;type=TriangulatedSetRepresentation")
    );

    // The inherited Citation field carries the eml namespace, not resqml.
    let field = info.field("Citation").expect("Citation is described");
    assert_eq!(field.xml_ns, ns::EML);
    assert!(field.is_required());

    // Documentation made it across from xsd:documentation.
    let title = <eml::Citation as EnergymlType>::type_info()
        .field("Title")
        .unwrap();
    assert!(!title.doc.is_empty(), "xsd:documentation should be preserved");
    assert_eq!(
        title.facets.max_length,
        Some(256),
        "XSD facets should be preserved"
    );
}

#[test]
fn abstract_types_expose_their_concrete_subtypes() {
    // AbstractGeometry is abstract in the schema, so it became a choice enum.
    let info = <resqml::AbstractGeometry as EnergymlType>::type_info();
    assert!(info.is_abstract);
    assert_eq!(info.kind, TypeKind::Choice);

    let concrete = info.concrete_subtypes();
    assert!(
        concrete.len() > 3,
        "expected several geometries, got {}",
        concrete.len()
    );
    assert!(
        concrete.iter().any(|t| t.name == "PointGeometry"),
        "PointGeometry should be one of the options"
    );
}

#[test]
fn open_enumerations_keep_unknown_values() {
    // ExistenceKindExt unions the closed enum with the extension pattern.
    let parsed = <eml::ExistenceKindExt as XmlText>::from_xml_text("something-vendor-specific")
        .expect("open enumerations accept unknown values");
    assert_eq!(parsed.as_xml_str(), "something-vendor-specific");

    let closed = <eml::ExistenceKindExt as XmlText>::from_xml_text("actual").unwrap();
    assert_eq!(closed.as_xml_str(), "actual");
}

#[test]
fn dotted_paths_reach_into_generated_objects() {
    let obj = sample();
    assert_eq!(
        path::get_string(&obj, "Citation.Title").unwrap().as_deref(),
        Some("Top Horizon")
    );
    assert_eq!(
        path::get_string(&obj, "citation.originator").unwrap().as_deref(),
        Some("geosiris")
    );
}

#[test]
fn the_registry_reads_a_document_by_content_type() {
    let mut reg = registry::Registry::new();
    resqml::register(&mut reg);
    eml::register(&mut reg);

    let top_level = reg.top_level_types().count();
    assert!(
        top_level > 50,
        "RESQML 2.2 should register many top-level objects, got {top_level}"
    );

    let doc = to_xml(&sample());
    let object = reg
        .read_xml(
            "application/x-resqml+xml;version=2.2;type=TriangulatedSetRepresentation",
            doc.as_bytes(),
        )
        .expect("reads through the registry");

    assert_eq!(
        path::get_string(object.as_reflect(), "Citation.Title")
            .unwrap()
            .as_deref(),
        Some("Top Horizon")
    );
    let concrete = object
        .as_reflect()
        .downcast_ref::<resqml::TriangulatedSetRepresentation>()
        .expect("downcasts to the concrete type");
    assert_eq!(concrete, &sample());
}

#[test]
fn every_registered_type_has_a_consistent_identity() {
    let mut reg = registry::Registry::new();
    resqml::register(&mut reg);

    for info in reg.top_level_types() {
        let qualified = info
            .qualified_type
            .expect("top-level types have a qualified type");
        let content = info.content_type.expect("top-level types have a content type");

        let qt = identifiers::QualifiedType::parse(qualified)
            .unwrap_or_else(|e| panic!("{qualified} is malformed: {e}"));
        let ct = identifiers::ContentType::parse(content)
            .unwrap_or_else(|e| panic!("{content} is malformed: {e}"));

        assert_eq!(qt.object_type, ct.object_type.unwrap(), "for {}", info.name);
        assert_eq!(qt.dotted_version(), ct.version, "for {}", info.name);
        assert_eq!(qt.domain, "resqml", "for {}", info.name);
    }
}

#[test]
fn validation_enforces_the_schema_on_generated_types() {
    let mut obj = sample();
    // Title has maxLength=256 in the schema.
    obj.citation.title = "x".repeat(300);

    let report = validate::Validate::validate(&obj);
    assert!(!report.is_valid(), "an over-long title must be reported");
    assert!(
        report
            .errors()
            .any(|i| i.code == "max-length" && i.path == "Citation.Title"),
        "{report:?}"
    );

    // A minimally-populated object is legitimately incomplete: the schema
    // requires at least one TrianglePatch, and RepresentedObject.
    let baseline = validate::Validate::validate(&sample());
    let codes: Vec<String> = baseline
        .errors()
        .map(|i| format!("{} {}", i.path, i.code))
        .collect();
    assert!(
        codes.iter().all(|c| c.contains("missing-required")),
        "a default-populated object should only be missing required children, got {codes:?}"
    );
}

#[test]
fn json_round_trips_through_the_energistics_encoding() {
    // Generated types carry no serde derive any more: JSON is written from
    // reflection, which is the only view that can name the concrete subtype of
    // a polymorphic field. This checks the encoding on a real generated type.
    let mut reg = registry::Registry::new();
    resqml::register(&mut reg);
    eml::register(&mut reg);

    let original = sample();
    let json = energyml_core::json::write_string(&original, false).expect("serializes");

    assert!(
        json.starts_with(r#"{"$type":"resqml22.TriangulatedSetRepresentation""#),
        "the object names its own type: {json}"
    );
    // Enumerations are plain strings, matching their XML lexical form.
    assert!(json.contains(r#""SurfaceRole":"map""#), "{json}");

    let parsed = reg.read_json(&json).expect("deserializes");
    let back = parsed
        .as_reflect()
        .downcast_ref::<resqml::TriangulatedSetRepresentation>()
        .expect("the same concrete type came back");
    assert_eq!(&original, back);
}

#[test]
fn nestable_marks_the_objects_that_may_also_be_children() {
    let mut reg = registry::Registry::new();
    resqml::register(&mut reg);

    // A representation is a part of the EPC and nothing else.
    let repr = <resqml::TriangulatedSetRepresentation as EnergymlType>::type_info();
    assert!(repr.is_top_level);
    assert!(
        !repr.is_nestable,
        "a representation is never a child of another object"
    );

    // A colour map is a top-level object too, and `GraphicalInformation`
    // declares it as a child — both are true at once.
    let map = <resqml::ContinuousColorMap as EnergymlType>::type_info();
    assert!(map.is_top_level, "ContinuousColorMap is its own EPC part");
    assert!(
        map.is_nestable,
        "and a `ColorMap` child of a graphical information"
    );
    assert!(map.content_type.is_some(), "so it still carries a content type");

    // The flag is a hint, not a filter: listing top-level objects lists both.
    // Being a variant of the `AbstractObject` choice must not count as nesting,
    // which is what once emptied this list.
    let names: Vec<&str> = reg.top_level_types().map(|i| i.name).collect();
    assert!(
        names.len() > 50,
        "resqml 2.2 registers many top-level objects, got {}",
        names.len()
    );
    assert!(
        names.contains(&"ContinuousColorMap"),
        "including the nestable ones"
    );
    assert!(names.contains(&"TriangulatedSetRepresentation"));
}

#[test]
fn inline_arrays_read_as_numbers_not_as_a_string() {
    // `eml:FloatingPointXmlArrayList` is an `xsd:list`: one `<Values>` element
    // holding the whole array. This is the shape 3D data arrives in when it is
    // not in an HDF5 file, so it has to be a `Vec<f64>` and not a `String`.
    let array = eml::FloatingPointXmlArray {
        count_per_value: 1,
        values: vec![0.0, 1.5, -2.25, 1e3],
        ..Default::default()
    };

    let doc = xml::to_xml_string(
        &array,
        ns::EML,
        "FloatingPointXmlArray",
        &[ns::EML],
        XmlWriteOptions::COMPACT,
    )
    .expect("serializes");
    assert!(doc.contains("<eml:Values>0 1.5 -2.25 1000</eml:Values>"), "{doc}");

    let back: eml::FloatingPointXmlArray = xml::from_xml_str(&doc).expect("reads back");
    assert_eq!(back.values, array.values);

    // And reflection describes it as one element carrying many values.
    let info = <eml::FloatingPointXmlArray as EnergymlType>::type_info();
    let values = info.field("values").expect("the field exists");
    assert!(values.is_list);
    assert!(values.occurs.is_collection());
}

#[test]
fn a_subtype_substituted_on_a_concrete_element_is_not_lost() {
    // `TrianglePatch.Geometry` is declared as `resqml:PointGeometry`, which is
    // concrete — and three grid geometries extend it. A document may therefore
    // write `xsi:type="resqml:IjkGridGeometry"` there. Reading that as a plain
    // `PointGeometry` dropped everything the subtype adds, without a word.
    let patch = resqml::TrianglePatch {
        node_count: 3,
        geometry: resqml::AnyPointGeometry::IjkGridGeometry(resqml::IjkGridGeometry {
            node_is_colocated_in_k_direction: Some(
                resqml::AbstractBooleanArray::BooleanArrayFromDiscretePropertyArray(Default::default()),
            ),
            ..Default::default()
        }),
        ..Default::default()
    };

    let doc = xml::to_xml_string(
        &patch,
        ns::RESQML,
        "TrianglePatch",
        &[ns::RESQML, ns::EML, ns::XSI],
        XmlWriteOptions::COMPACT,
    )
    .expect("serializes");
    assert!(doc.contains(r#"xsi:type="resqml:IjkGridGeometry""#), "{doc}");

    let back: resqml::TrianglePatch = xml::from_xml_str(&doc).expect("reads back");
    assert_eq!(back, patch, "the substituted subtype survives the round trip");

    // The declared type stays the default, so the field is not an `Option`.
    let plain = resqml::TrianglePatch::default();
    assert!(matches!(
        plain.geometry,
        resqml::AnyPointGeometry::PointGeometry(_)
    ));
}

#[test]
fn a_gml_crs_definition_survives_a_round_trip() {
    // `eml:GmlProjectedCrsDefinition` wraps a full GML CRS definition. GML is
    // not generated, and this used to be the last silent loss in the model: the
    // definition was dropped on read, so writing the object back produced a
    // CRS with no definition at all.
    let doc = "<eml:GmlProjectedCrsDefinition \
               xmlns:eml=\"http://www.energistics.org/energyml/data/commonv2\" \
               xmlns:gml=\"http://www.opengis.net/gml/3.2\">\
               <eml:GmlProjectedCrsDefinition gml:id=\"crs-1\">\
               <gml:identifier codeSpace=\"OGP\">urn:ogc:def:crs:EPSG::32631</gml:identifier>\
               <gml:name>WGS 84 / UTM zone 31N</gml:name>\
               <gml:scope>Large and medium scale topographic mapping.</gml:scope>\
               </eml:GmlProjectedCrsDefinition>\
               </eml:GmlProjectedCrsDefinition>";

    let crs: eml20::GmlProjectedCrsDefinition = xml::from_xml_str(doc).expect("reads");
    let kept = &crs.gml_projected_crs_definition;
    assert_eq!(
        kept.attribute("gml:id"),
        Some("crs-1"),
        "attributes of the wrapper are kept"
    );
    assert!(
        kept.content.contains("urn:ogc:def:crs:EPSG::32631"),
        "{}",
        kept.content
    );
    assert!(
        kept.content
            .contains("<gml:name>WGS 84 / UTM zone 31N</gml:name>"),
        "{}",
        kept.content
    );

    let written = xml::to_xml_string(
        &crs,
        ns::EML,
        "GmlProjectedCrsDefinition",
        &[ns::EML, ns::GML],
        XmlWriteOptions::COMPACT,
    )
    .expect("writes");
    assert!(written.contains("urn:ogc:def:crs:EPSG::32631"), "{written}");
    assert!(written.contains("gml:id=\"crs-1\""), "{written}");

    // And reading what we wrote gives the same thing back.
    let again: eml20::GmlProjectedCrsDefinition = xml::from_xml_str(&written).expect("reads its own output");
    assert_eq!(again, crs);
}
