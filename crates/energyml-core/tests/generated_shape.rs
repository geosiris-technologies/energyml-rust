//! Exercises the derive macros against a hand-written slice of the model that
//! mirrors what `energyml-codegen` will emit: a top-level object, an inherited
//! field in a foreign namespace, a simpleContent measure type, an open
//! enumeration and an abstract type selected by `xsi:type`.

use energyml_core::prelude::*;
use energyml_core::{identifiers, identity, json, path, registry, validate, value, xml};

// ---------------------------------------------------------------------------
// A slice of eml 2.3
// ---------------------------------------------------------------------------

/// Simple authorship metadata attached to every top-level object.
#[derive(Debug, Default, Clone, PartialEq, EnergymlObject)]
#[energyml(package = "eml", version = "2.3", xml_name = "Citation")]
pub struct Citation {
    /// One-line name of the object.
    #[energyml(elem, max_length = 256)]
    pub title: String,

    /// Person or software that created the object.
    #[energyml(elem)]
    pub originator: Option<String>,

    /// Creation timestamp, kept in its lexical form.
    #[energyml(elem, scalar = "dateTime")]
    pub creation: Option<String>,
}

/// Whether an object describes something actual or planned. Open, like every
/// Energistics `...Ext` type.
#[derive(Debug, Default, Clone, PartialEq, EnergymlEnum)]
#[energyml(package = "eml", version = "2.3", xml_name = "ExistenceKindExt", open)]
pub enum ExistenceKind {
    /// The object exists.
    #[default]
    #[energyml(value = "actual")]
    Actual,
    /// The object is planned.
    #[energyml(value = "planned")]
    Planned,
    /// A value outside the declared set.
    #[energyml(other)]
    Other(String),
}

/// `xsd:simpleContent`: a number with a `uom` attribute - the shape every
/// measure type in `MeasureType.xsd` uses.
#[derive(Debug, Default, Clone, PartialEq, EnergymlObject)]
#[energyml(package = "eml", version = "2.3", xml_name = "LengthMeasure")]
pub struct LengthMeasure {
    /// The measured value.
    #[energyml(text)]
    pub value: f64,

    /// Unit of measure.
    #[energyml(attr, xml_name = "uom")]
    pub uom: String,
}

// ---------------------------------------------------------------------------
// A slice of resqml 2.2
// ---------------------------------------------------------------------------

/// A single 3D point.
#[derive(Debug, Default, Clone, PartialEq, EnergymlObject)]
#[energyml(package = "resqml", version = "2.2", xml_name = "Point3d")]
pub struct Point3d {
    /// Easting.
    #[energyml(elem, xml_name = "Coordinate1")]
    pub coordinate1: f64,
    /// Northing.
    #[energyml(elem, xml_name = "Coordinate2")]
    pub coordinate2: f64,
    /// Elevation.
    #[energyml(elem, xml_name = "Coordinate3")]
    pub coordinate3: f64,
}

/// Geometry given as an explicit list of points.
#[derive(Debug, Default, Clone, PartialEq, EnergymlObject)]
#[energyml(package = "resqml", version = "2.2", xml_name = "PointGeometry")]
pub struct PointGeometry {
    /// The points making up the geometry.
    #[energyml(elem, xml_name = "Points", min = 1)]
    pub points: Vec<Point3d>,
}

/// The abstract geometry type: concrete subtypes are selected by `xsi:type`.
#[derive(Debug, Clone, PartialEq, EnergymlChoice)]
#[energyml(
    package = "resqml",
    version = "2.2",
    xml_name = "AbstractGeometry",
    abstract_type
)]
pub enum AbstractGeometry {
    /// Explicit point list.
    #[energyml(xsi_type = "resqml:PointGeometry", xml_name = "PointGeometry")]
    PointGeometry(PointGeometry),
}

/// One patch of triangles.
#[derive(Debug, Default, Clone, PartialEq, EnergymlObject)]
#[energyml(package = "resqml", version = "2.2", xml_name = "TrianglePatch")]
pub struct TrianglePatch {
    /// Number of triangles in the patch. Must be positive.
    #[energyml(elem, xml_name = "Count", min_inclusive = 1)]
    pub count: u32,

    /// Optional thickness, exercising a simpleContent child.
    #[energyml(elem, xml_name = "Thickness")]
    pub thickness: Option<LengthMeasure>,
}

/// A top-level RESQML object.
#[derive(Debug, Default, Clone, PartialEq, EnergymlObject)]
#[energyml(
    package = "resqml",
    version = "2.2",
    xml_name = "TriangulatedSetRepresentation",
    top_level,
    content_type = "application/x-resqml+xml;version=2.2;type=TriangulatedSetRepresentation",
    qualified_type = "resqml22.TriangulatedSetRepresentation"
)]
pub struct TriangulatedSetRepresentation {
    /// Object identity.
    #[energyml(attr, xml_name = "uuid")]
    pub uuid: String,

    /// Schema version, as written in the document.
    #[energyml(attr, xml_name = "schemaVersion")]
    pub schema_version: Option<String>,

    /// Inherited from `eml:AbstractObject`, so it keeps the common namespace
    /// even though the owning type is RESQML.
    #[energyml(elem, ns = "eml", xml_name = "Citation")]
    pub citation: Citation,

    /// Inherited, and an open enumeration.
    #[energyml(elem, ns = "eml", xml_name = "Existence")]
    pub existence: Option<ExistenceKind>,

    /// Polymorphic child.
    #[energyml(elem, xml_name = "Geometry")]
    pub geometry: Option<AbstractGeometry>,

    /// Repeated child, required at least once by the schema.
    #[energyml(elem, xml_name = "TrianglePatch", min = 1)]
    pub triangle_patch: Vec<TrianglePatch>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn sample() -> TriangulatedSetRepresentation {
    TriangulatedSetRepresentation {
        uuid: "a1b2c3d4-e5f6-7890-abcd-ef1234567890".into(),
        schema_version: Some("2.2".into()),
        citation: Citation {
            title: "Top Horizon".into(),
            originator: Some("geosiris".into()),
            creation: Some("2026-07-30T10:00:00Z".into()),
        },
        existence: Some(ExistenceKind::Actual),
        geometry: Some(AbstractGeometry::PointGeometry(PointGeometry {
            points: vec![
                Point3d {
                    coordinate1: 1.5,
                    coordinate2: 2.5,
                    coordinate3: -100.0,
                },
                Point3d {
                    coordinate1: 3.5,
                    coordinate2: 4.5,
                    coordinate3: -120.0,
                },
            ],
        })),
        triangle_patch: vec![
            TrianglePatch {
                count: 12,
                thickness: Some(LengthMeasure {
                    value: 3.25,
                    uom: "m".into(),
                }),
            },
            TrianglePatch {
                count: 4,
                thickness: None,
            },
        ],
    }
}

impl TriangulatedSetRepresentation {
    /// The nth point of the geometry, for tests that reach through the choice.
    fn geometry_point(&self, i: usize) -> &Point3d {
        match self.geometry.as_ref().expect("the sample has a geometry") {
            AbstractGeometry::PointGeometry(g) => &g.points[i],
        }
    }
}

fn to_xml(value: &TriangulatedSetRepresentation, opts: XmlWriteOptions) -> String {
    xml::to_xml_string(
        value,
        ns::RESQML,
        "TriangulatedSetRepresentation",
        &[ns::RESQML, ns::EML, ns::XSI],
        opts,
    )
    .expect("serialization succeeds")
}

// ---------------------------------------------------------------------------
// XML
// ---------------------------------------------------------------------------

#[test]
fn xml_round_trip_compact() {
    let original = sample();
    let doc = to_xml(&original, XmlWriteOptions::COMPACT);
    let parsed: TriangulatedSetRepresentation = xml::from_xml_str(&doc).expect("parses back");
    assert_eq!(original, parsed);
}

#[test]
fn xml_round_trip_indented() {
    let original = sample();
    let doc = to_xml(&original, XmlWriteOptions::default());
    let parsed: TriangulatedSetRepresentation = xml::from_xml_str(&doc).expect("indented output parses back");
    assert_eq!(original, parsed, "indentation must not alter text content");
}

#[test]
fn xml_uses_the_right_namespace_per_field() {
    let doc = to_xml(&sample(), XmlWriteOptions::COMPACT);

    // Namespaces are declared once, on the root.
    assert!(doc.contains(r#"xmlns:resqml="http://www.energistics.org/energyml/data/resqmlv2""#));
    assert!(doc.contains(r#"xmlns:eml="http://www.energistics.org/energyml/data/commonv2""#));

    // Citation is declared by the common schema, so it stays in eml even though
    // its owner is a RESQML type. Getting this wrong is the classic interop bug.
    assert!(
        doc.contains("<eml:Citation>"),
        "Citation must be in the eml namespace: {doc}"
    );
    assert!(doc.contains("<eml:Title>Top Horizon</eml:Title>"));

    // Fields declared by RESQML stay in resqml.
    assert!(doc.contains("<resqml:TrianglePatch>"));

    // Attributes are unprefixed, and simpleContent carries text plus attribute.
    assert!(doc.contains(r#"uuid="a1b2c3d4-e5f6-7890-abcd-ef1234567890""#));
    assert!(doc.contains(r#"<resqml:Thickness uom="m">3.25</resqml:Thickness>"#));
}

#[test]
fn abstract_types_write_and_read_xsi_type() {
    let doc = to_xml(&sample(), XmlWriteOptions::COMPACT);
    assert!(
        doc.contains(r#"<resqml:Geometry xsi:type="resqml:PointGeometry">"#),
        "the concrete subtype must be selected by xsi:type: {doc}"
    );

    let parsed: TriangulatedSetRepresentation = xml::from_xml_str(&doc).unwrap();
    assert!(matches!(
        parsed.geometry,
        Some(AbstractGeometry::PointGeometry(_))
    ));
}

#[test]
fn unknown_elements_are_skipped_rather_than_rejected() {
    // A newer minor version, or a vendor extension, must not break reading.
    let doc = to_xml(&sample(), XmlWriteOptions::COMPACT).replace(
        "<resqml:TrianglePatch>",
        "<resqml:SomethingFromTheFuture><Nested a=\"1\">x</Nested></resqml:SomethingFromTheFuture>\
         <resqml:TrianglePatch>",
    );
    let parsed: TriangulatedSetRepresentation = xml::from_xml_str(&doc).expect("skips the unknown child");
    assert_eq!(parsed.triangle_patch.len(), 2);
}

/// A missing required element does not stop the read - and does stop a
/// **strict** one.
///
/// The default was the other way round until 02/08, and the arithmetic that
/// changed it is worth keeping: a package of 1618 objects whose producer
/// omitted `Citation.Originator` - one string nothing downstream reads - was
/// 1618 objects unreadable. No geometry, no references, no titles, no export.
/// Refusing there does not protect a caller from bad data; it hands them
/// nothing.
///
/// Saying what a document *should* have carried is a real job, and this
/// workspace already does it - `Validate`, on the object, with a path and a
/// rule. Doing it a second time in the parser, where the only available answer
/// is to refuse, is what cost the 1618.
#[test]
fn a_missing_required_element_is_read_past_and_reported() {
    let doc = to_xml(&sample(), XmlWriteOptions::COMPACT).replace("<eml:Title>Top Horizon</eml:Title>", "");

    // By default: read, and the absent field is its default.
    let parsed = xml::from_xml_str::<TriangulatedSetRepresentation>(&doc)
        .expect("everything the document does carry is still worth having");
    assert_eq!(parsed.citation.title, "");
    assert_eq!(
        parsed.triangle_patch.len(),
        2,
        "and the rest of the object survived"
    );
    assert!(parsed.geometry.is_some());

    // Strictly: refused, for the caller that will write this back. An object
    // read the other way holds an empty title nobody wrote.
    let err = xml::from_xml_str_strict::<TriangulatedSetRepresentation>(&doc).unwrap_err();
    assert!(
        matches!(err, energyml_core::Error::MissingRequired { .. }),
        "got {err:?}"
    );
}

/// A value that is **there and wrong** still refuses, and that asymmetry is the
/// point.
///
/// Silence and nonsense are different news. A producer that omits an element
/// forgot; a producer that writes `hello` where a number belongs wrote
/// something, and defaulting it throws away what the file said. So the first is
/// read past and the second needs `--lenient`, where it is recorded **with the
/// string the document carried**.
#[test]
fn a_value_that_is_there_and_wrong_is_a_different_question() {
    let doc = to_xml(&sample(), XmlWriteOptions::COMPACT).replace(
        "<resqml:Count>12</resqml:Count>",
        "<resqml:Count>douze</resqml:Count>",
    );
    assert!(
        doc.contains("douze"),
        "the substitution missed; the test would pass without testing anything"
    );

    let err = xml::from_xml_str::<TriangulatedSetRepresentation>(&doc)
        .expect_err("tolerating absence is not tolerating nonsense");
    assert!(
        matches!(err, energyml_core::Error::ParseValue { .. }),
        "got {err:?}"
    );

    let (parsed, defaulted) =
        xml::from_xml_str_lenient::<TriangulatedSetRepresentation>(&doc).expect("lenient reads it");
    assert_eq!(parsed.triangle_patch[0].count, 0);
    assert!(defaulted.is_empty(), "nothing was absent here");
}

/// The same document a strict read refuses, read for what it does carry.
///
/// Real producers ship packages missing a mandatory element - `Citation`
/// without its `Originator` is the one seen in the wild - and refusing the part
/// loses the geometry and the references it wrote correctly.
#[test]
fn a_lenient_read_defaults_what_is_missing_and_says_so() {
    let doc = to_xml(&sample(), XmlWriteOptions::COMPACT).replace("<eml:Title>Top Horizon</eml:Title>", "");
    let (parsed, defaulted) =
        xml::from_xml_str_lenient::<TriangulatedSetRepresentation>(&doc).expect("reads what is there");

    // The absent field is its default...
    assert_eq!(parsed.citation.title, "");
    // ...and everything else survived, which is the whole point.
    assert_eq!(parsed.citation.originator, sample().citation.originator);
    assert_eq!(parsed.triangle_patch.len(), sample().triangle_patch.len());

    // Never silent about what it invented.
    assert_eq!(defaulted.len(), 1, "got {defaulted:?}");
    assert_eq!(defaulted[0].kind, "element");
    assert_eq!(defaulted[0].name, "Title");
    assert_eq!(defaulted[0].parent, "Citation");
}

/// A value that is present and wrong is **repaired** in lenient mode, and the
/// string that was there is kept in the record.
///
/// This used to be a refusal - leniency was about cardinality alone. It
/// changed on 01/08/2026, deliberately: for a tool whose job is to open a
/// broken file and repair it, losing the ninety-nine fields that were fine
/// over one bad word is the wrong trade. What has not changed is that nothing
/// is quiet: the replacement is recorded with what the producer actually
/// wrote, which is the only thing a repair can work from.
///
/// Strict mode still refuses, and that is what anything writing back should
/// use.
#[test]
fn a_lenient_read_repairs_an_unparsable_value_and_keeps_what_was_there() {
    let doc = to_xml(&sample(), XmlWriteOptions::COMPACT).replace(
        "<resqml:Count>12</resqml:Count>",
        "<resqml:Count>oops</resqml:Count>",
    );

    let strictly = xml::from_xml_str::<TriangulatedSetRepresentation>(&doc)
        .expect_err("strictly, `oops` is not an integer");
    assert!(
        matches!(strictly, energyml_core::Error::ParseValue { .. }),
        "got {strictly:?}"
    );

    let mut bytes = doc.as_bytes();
    let mut reader = xml::XmlReader::new(&mut bytes).lenient();
    let root = reader.read_root().expect("a root");
    let parsed = <TriangulatedSetRepresentation as xml::XmlDeserialize>::xml_read(&mut reader, &root)
        .expect("leniently, it reads");
    let repaired = reader.take_repaired();

    // The field took its default, and everything around it survived.
    assert_eq!(parsed.triangle_patch[0].count, 0);
    assert_eq!(parsed.citation.title, "Top Horizon");

    assert_eq!(repaired.len(), 1, "got {repaired:?}");
    assert_eq!(repaired[0].kind, "number");
    assert_eq!(repaired[0].found, "oops", "the string in the file is the record");
    assert_eq!(repaired[0].used, "0");
}

/// A conformant document reads identically in both modes, and invents nothing.
#[test]
fn a_lenient_read_of_a_valid_document_changes_nothing() {
    let doc = to_xml(&sample(), XmlWriteOptions::COMPACT);
    let strict: TriangulatedSetRepresentation = xml::from_xml_str(&doc).expect("reads");
    let (lenient, defaulted) =
        xml::from_xml_str_lenient::<TriangulatedSetRepresentation>(&doc).expect("reads");
    assert_eq!(strict, lenient);
    assert!(defaulted.is_empty(), "got {defaulted:?}");
}

#[test]
fn open_enumerations_accept_unknown_values() {
    let doc = to_xml(&sample(), XmlWriteOptions::COMPACT).replace(
        "<eml:Existence>actual</eml:Existence>",
        "<eml:Existence>vendor-specific</eml:Existence>",
    );
    let parsed: TriangulatedSetRepresentation = xml::from_xml_str(&doc).unwrap();
    assert_eq!(
        parsed.existence,
        Some(ExistenceKind::Other("vendor-specific".into()))
    );
}

/// WITSML objects declare their own `witsml:Citation` alongside the
/// `eml:Citation` they inherit from `AbstractObject`. Same local name, different
/// namespaces, different fields.
#[derive(Debug, Default, Clone, PartialEq, EnergymlObject)]
#[energyml(
    package = "witsml",
    version = "2.0",
    xml_name = "CuttingsGeologyInterval",
    top_level
)]
pub struct TwoCitations {
    /// Inherited from `eml:AbstractObject`.
    #[energyml(elem, ns = "eml", xml_name = "Citation")]
    pub citation: Citation,

    /// Declared by the object itself, in its own namespace.
    #[energyml(elem, ns = "witsml", xml_name = "Citation")]
    pub citation2: Option<Citation>,
}

#[test]
fn same_local_name_in_two_namespaces_stays_distinct() {
    let original = TwoCitations {
        citation: Citation {
            title: "Inherited".into(),
            originator: None,
            creation: None,
        },
        citation2: Some(Citation {
            title: "Own".into(),
            originator: None,
            creation: None,
        }),
    };

    let doc = xml::to_xml_string(
        &original,
        ns::WITSML,
        "CuttingsGeologyInterval",
        &[ns::WITSML, ns::EML],
        XmlWriteOptions::COMPACT,
    )
    .unwrap();
    assert!(doc.contains("<eml:Citation>"), "{doc}");
    assert!(doc.contains("<witsml:Citation>"), "{doc}");

    let parsed: TwoCitations = xml::from_xml_str(&doc).expect("parses back");
    assert_eq!(parsed.citation.title, "Inherited");
    assert_eq!(parsed.citation2.as_ref().unwrap().title, "Own");
    assert_eq!(original, parsed);
}

// ---------------------------------------------------------------------------
// JSON
// ---------------------------------------------------------------------------

/// Generated types carry **no serde derive**. JSON is written from reflection,
/// because only reflection can name the concrete subtype behind a polymorphic
/// field - see `energyml_core::json`. What the generator must emit is therefore
/// nothing at all for JSON, which is what this file now shows.
#[test]
fn json_round_trip() {
    // Writing needs no registry; **reading does**, and it needs every nested
    // type, not just the top-level one - `$type` names them all and the reader
    // builds each one by name. A generated crate's `register` covers this by
    // construction; a hand-written model has to say so.
    let mut registry = registry::Registry::new();
    registry.register(registry::TypeEntry::of::<TriangulatedSetRepresentation>());
    registry.register(registry::TypeEntry::of::<Citation>());
    registry.register(registry::TypeEntry::of::<PointGeometry>());
    registry.register(registry::TypeEntry::of::<Point3d>());
    registry.register(registry::TypeEntry::of::<TrianglePatch>());
    registry.register(registry::TypeEntry::of::<LengthMeasure>());

    let original = sample();
    let json = json::write_string(&original, false).unwrap();
    assert!(json.contains(r#""Title":"Top Horizon""#), "{json}");
    assert!(
        json.contains(r#""$type":"eml23.Citation""#),
        "every object names its type: {json}"
    );

    let parsed = registry.read_json(&json).unwrap();
    assert_eq!(
        parsed
            .as_reflect()
            .downcast_ref::<TriangulatedSetRepresentation>(),
        Some(&original)
    );
}

// ---------------------------------------------------------------------------
// Reflection
// ---------------------------------------------------------------------------

#[test]
fn type_info_carries_the_schema() {
    let info = <TriangulatedSetRepresentation as EnergymlType>::type_info();
    assert_eq!(info.name, "TriangulatedSetRepresentation");
    assert_eq!(info.package, "resqml");
    assert_eq!(info.version, "2.2");
    assert!(info.is_top_level);
    assert_eq!(
        info.qualified_type,
        Some("resqml22.TriangulatedSetRepresentation")
    );

    let citation = info.field("Citation").expect("Citation is described");
    assert_eq!(citation.name, "citation");
    assert_eq!(citation.xml_ns, ns::EML);
    assert!(citation.is_required());
    assert_eq!(
        citation.doc.lines().next().unwrap(),
        "Inherited from `eml:AbstractObject`, so it keeps the common namespace"
    );

    let patch = info.field("TrianglePatch").unwrap();
    assert_eq!(patch.occurs.min, 1);
    assert!(patch.occurs.is_collection());

    // Facets survive from the schema.
    let count = <TrianglePatch as EnergymlType>::type_info()
        .field("Count")
        .unwrap();
    assert_eq!(count.facets.min_inclusive, Some(1.0));
}

#[test]
fn abstract_fields_report_their_concrete_options() {
    let info = <TriangulatedSetRepresentation as EnergymlType>::type_info();
    let geometry = info.field("Geometry").unwrap();
    let accepted = geometry.accepted_types();
    assert_eq!(accepted.len(), 1, "a UI offers exactly the concrete subtypes");
    assert_eq!(accepted[0].name, "PointGeometry");
}

#[test]
fn enumeration_fields_report_their_allowed_values() {
    let info = <TriangulatedSetRepresentation as EnergymlType>::type_info();
    let existence = info.field("Existence").unwrap();
    let values: Vec<&str> = existence.ty.allowed_values().iter().map(|v| v.value).collect();
    assert_eq!(values, vec!["actual", "planned", "Other"]);
    assert!(existence.ty.type_info().unwrap().is_open);
}

// ---------------------------------------------------------------------------
// Dotted paths
// ---------------------------------------------------------------------------

#[test]
fn dotted_paths_read_values() {
    let obj = sample();
    assert_eq!(
        path::get_string(&obj, "Citation.Title").unwrap().as_deref(),
        Some("Top Horizon")
    );
    assert_eq!(
        path::get_string(&obj, "TrianglePatch[1].Count")
            .unwrap()
            .as_deref(),
        Some("4")
    );
    assert_eq!(
        path::get_string(&obj, "Geometry.Points[0].Coordinate3")
            .unwrap()
            .as_deref(),
        Some("-100"),
        "paths descend through an abstract type transparently"
    );
}

#[test]
fn dotted_paths_ignore_case() {
    let obj = sample();
    assert_eq!(
        path::get_string(&obj, "citation.title").unwrap().as_deref(),
        Some("Top Horizon")
    );
    assert_eq!(
        path::get_string(&obj, "CITATION.TITLE").unwrap().as_deref(),
        Some("Top Horizon")
    );
}

#[test]
fn dotted_paths_write_values() {
    let mut obj = sample();
    path::set(&mut obj, "Citation.Title", OwnedValue::Str("Renamed".into())).unwrap();
    assert_eq!(obj.citation.title, "Renamed");

    // Clearing an optional field.
    path::set(&mut obj, "Citation.Originator", OwnedValue::Null).unwrap();
    assert_eq!(obj.citation.originator, None);

    let err = path::set(&mut obj, "Citation.Nope", OwnedValue::Str("x".into())).unwrap_err();
    assert!(matches!(err, energyml_core::Error::UnknownField { .. }));
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

#[test]
fn structural_validation_reports_cardinality_and_facets() {
    let mut obj = sample();
    obj.triangle_patch.clear(); // violates minOccurs="1"
    obj.triangle_patch.push(TrianglePatch {
        count: 0,
        thickness: None,
    }); // violates minInclusive=1
    obj.triangle_patch.clear();
    obj.citation.title = "x".repeat(300); // violates maxLength=256

    let report = validate::Validate::validate(&obj);
    assert!(!report.is_valid());

    let codes: Vec<&str> = report.errors().map(|i| i.code.as_str()).collect();
    assert!(codes.contains(&"missing-required"), "{report:?}");
    assert!(codes.contains(&"max-length"), "{report:?}");

    let title_issue = report.errors().find(|i| i.code == "max-length").unwrap();
    assert_eq!(title_issue.path, "Citation.Title");
}

#[test]
fn a_well_formed_object_validates_clean() {
    let report = validate::Validate::validate(&sample());
    assert!(report.is_valid(), "{report:?}");
}

#[test]
fn business_rules_extend_validation() {
    // Stands in for the reference checks the storage layer will add later.
    struct TitlesMustBeCapitalized;
    impl validate::ValidationRule for TitlesMustBeCapitalized {
        fn name(&self) -> &str {
            "title-capitalized"
        }
        fn applies_to(&self, info: &'static TypeInfo) -> bool {
            info.name == "Citation"
        }
        fn check(&self, object: &dyn Reflect, ctx: &mut validate::ValidationCtx<'_>) {
            if let Some(FieldValue::Scalar(Scalar::Str(title))) = object.field("title")
                && title.chars().next().is_some_and(|c| c.is_lowercase())
            {
                ctx.error("title-capitalized", "Citation.Title should start with a capital");
            }
        }
    }

    let mut obj = sample();
    obj.citation.title = "lowercase".into();

    let rules: Vec<Box<dyn validate::ValidationRule>> = vec![Box::new(TitlesMustBeCapitalized)];
    let report = validate::validate_with(&obj, &rules);
    assert!(
        report.errors().any(|i| i.code == "title-capitalized"),
        "{report:?}"
    );

    // The same object passes without the rule registered.
    assert!(validate::Validate::validate(&obj).is_valid());
}

// ---------------------------------------------------------------------------
// Registry - dynamic construction and reading
// ---------------------------------------------------------------------------

fn test_registry() -> registry::Registry {
    let mut reg = registry::Registry::new();
    reg.register(registry::TypeEntry::of::<TriangulatedSetRepresentation>());
    reg
}

#[test]
fn registry_creates_objects_by_qualified_type() {
    let reg = test_registry();
    let obj = reg
        .create("resqml22.TriangulatedSetRepresentation")
        .expect("known type");
    assert_eq!(obj.type_info().name, "TriangulatedSetRepresentation");

    assert!(reg.create("resqml22.NotAThing").is_err());
}

#[test]
fn registry_reads_objects_by_content_type() {
    let reg = test_registry();
    let doc = to_xml(&sample(), XmlWriteOptions::COMPACT);

    let obj = reg
        .read_xml(
            "application/x-resqml+xml;version=2.2;type=TriangulatedSetRepresentation",
            doc.as_bytes(),
        )
        .expect("reads through the registry");

    // Reflection reaches into the object without any compile-time knowledge.
    assert_eq!(
        path::get_string(obj.as_reflect(), "Citation.Title")
            .unwrap()
            .as_deref(),
        Some("Top Horizon")
    );

    // And it downcasts back to the concrete type.
    let concrete = obj
        .as_reflect()
        .downcast_ref::<TriangulatedSetRepresentation>()
        .unwrap();
    assert_eq!(concrete, &sample());
}

#[test]
fn registry_tolerates_content_type_spelling_differences() {
    let reg = test_registry();
    // RESQML 2.0.1 era writers add the obj_ prefix; some add whitespace.
    assert!(
        reg.by_content_type("application/x-resqml+xml; version=2.2; type=obj_TriangulatedSetRepresentation")
            .is_some()
    );
}

#[test]
fn a_path_can_hand_back_the_object_it_names() {
    // What `set` cannot do: change several fields of one nested object without
    // walking to it three times. Indexed segments included, since a repeated
    // field is where the interesting nested objects live.
    let mut obj = sample();

    let patch = path::object_mut(&mut obj as &mut dyn value::Reflect, "TrianglePatch[1]")
        .expect("the second patch is there");
    value::Reflect::set_field(patch, "Count", OwnedValue::UInt(77)).unwrap();
    assert_eq!(obj.triangle_patch[1].count, 77);
    assert_eq!(obj.triangle_patch[0].count, 12, "the first patch moved");

    // Through an abstract slot, which delegates rather than adding a segment.
    let points = path::object_mut(&mut obj as &mut dyn value::Reflect, "Geometry.Points[0]")
        .expect("the first point is there");
    value::Reflect::set_field(points, "Coordinate3", OwnedValue::Double(-42.0)).unwrap();
    assert_eq!(obj.geometry_point(0).coordinate3, -42.0);

    // And it refuses rather than inventing: a scalar is not an object.
    assert!(path::object_mut(&mut obj as &mut dyn value::Reflect, "Citation.Title").is_err());
    assert!(path::object_mut(&mut obj as &mut dyn value::Reflect, "TrianglePatch[9]").is_err());
}

#[test]
fn every_validation_path_can_be_followed_back_to_the_value() {
    // The whole point of a validation path: a UI hangs the message on the
    // widget that shows that value. A path nobody can follow is a message
    // nobody can place.
    let mut obj = sample();
    obj.triangle_patch[1].count = 0; // violates minInclusive=1
    obj.citation.title = "x".repeat(300); // violates maxLength=256

    let report = validate::validate_with(&obj as &dyn validate::Validate, &[]);
    assert!(!report.issues.is_empty(), "the sample should be faulty");

    for issue in &report.issues {
        path::get(&obj as &dyn value::Reflect, &issue.path)
            .unwrap_or_else(|e| panic!("issue path `{}` does not resolve: {e}", issue.path));
    }
}

#[test]
fn dynamic_construction_then_completion() {
    // The flow a generic UI follows: create empty, then fill by path.
    let reg = test_registry();
    let mut obj = reg.create("resqml22.TriangulatedSetRepresentation").unwrap();

    path::set(
        obj.as_reflect_mut(),
        "uuid",
        OwnedValue::Str("a1b2c3d4-e5f6-7890-abcd-ef1234567890".into()),
    )
    .unwrap();
    path::set(
        obj.as_reflect_mut(),
        "Citation.Title",
        OwnedValue::Str("Built dynamically".into()),
    )
    .unwrap();

    assert_eq!(
        path::get_string(obj.as_reflect(), "Citation.Title")
            .unwrap()
            .as_deref(),
        Some("Built dynamically")
    );

    // It is still incomplete: TrianglePatch has minOccurs="1".
    let report = validate::validate_with(obj.as_validate(), &[]);
    assert!(report.errors().any(|i| i.path == "TrianglePatch"), "{report:?}");
}

#[test]
fn setting_an_object_field_dynamically() {
    let mut obj = sample();
    let patch = TrianglePatch {
        count: 99,
        thickness: None,
    };
    value::Reflect::set_field(&mut obj.triangle_patch[0], "Count", OwnedValue::UInt(7)).unwrap();
    assert_eq!(obj.triangle_patch[0].count, 7);

    // Whole-field replacement through a boxed dynamic object.
    value::Reflect::set_field(&mut obj, "TrianglePatch", OwnedValue::Object(Box::new(patch))).unwrap();
    assert_eq!(obj.triangle_patch.len(), 1);
    assert_eq!(obj.triangle_patch[0].count, 99);
}

// ---------------------------------------------------------------------------
// Identifiers
// ---------------------------------------------------------------------------

#[test]
fn object_identity_matches_the_registry() {
    let info = <TriangulatedSetRepresentation as EnergymlType>::type_info();
    let qt = identifiers::QualifiedType::parse(info.qualified_type.unwrap()).unwrap();
    let ct = identifiers::ContentType::parse(info.content_type.unwrap()).unwrap();

    assert_eq!(qt.object_type, ct.object_type.unwrap());
    assert_eq!(qt.dotted_version(), ct.version);

    let uri = identifiers::EtpUri::object(Some("demo"), &qt.to_string(), &sample().uuid);
    let parsed = identifiers::EtpUri::parse(&uri).unwrap();
    assert_eq!(parsed.uuid.as_deref(), Some(sample().uuid.as_str()));
    assert_eq!(
        parsed.qualified_type().unwrap().object_type,
        "TriangulatedSetRepresentation"
    );
}

// ---------------------------------------------------------------------------
// Identity across versions
//
// The generator must emit these three shapes: a RESQML 2.0.1 object, whose
// complexType is prefixed `obj_` while its element is not, and the two spellings
// of a reference - eml 2.0/2.1/2.2 with a ContentType, eml 2.3 with a
// QualifiedType.
// ---------------------------------------------------------------------------

/// eml 2.0 style reference: names its target with a content type.
#[derive(Debug, Default, Clone, PartialEq, EnergymlObject)]
#[energyml(
    package = "eml",
    version = "2.0",
    xml_name = "DataObjectReference",
    object_ref
)]
pub struct DataObjectReference20 {
    /// Content type of the referenced object.
    #[energyml(elem, xml_name = "ContentType")]
    pub content_type: String,

    /// Title of the referenced object, as a hint.
    #[energyml(elem, xml_name = "Title")]
    pub title: String,

    /// Identifier of the referenced object. 2.0 spells it in capitals.
    #[energyml(elem, xml_name = "UUID")]
    pub uuid: String,

    /// Version of the referenced object.
    #[energyml(elem, xml_name = "VersionString")]
    pub version_string: Option<String>,
}

/// eml 2.3 style reference: names its target with a qualified type.
#[derive(Debug, Default, Clone, PartialEq, EnergymlObject)]
#[energyml(
    package = "eml",
    version = "2.3",
    xml_name = "DataObjectReference",
    object_ref
)]
pub struct DataObjectReference23 {
    /// Identifier of the referenced object.
    #[energyml(elem, xml_name = "Uuid")]
    pub uuid: String,

    /// Qualified type of the referenced object.
    #[energyml(elem, xml_name = "QualifiedType")]
    pub qualified_type: String,

    /// Title of the referenced object, as a hint.
    #[energyml(elem, xml_name = "Title")]
    pub title: String,

    /// Version of the referenced object.
    #[energyml(elem, xml_name = "ObjectVersion")]
    pub object_version: Option<String>,
}

/// A RESQML 2.0.1 top-level object: element `Activity`, complexType
/// `obj_Activity`. Identifiers follow the complexType.
#[derive(Debug, Default, Clone, PartialEq, EnergymlObject)]
#[energyml(
    package = "resqml",
    version = "2.0.1",
    xml_name = "Activity",
    object_type = "obj_Activity",
    top_level,
    content_type = "application/x-resqml+xml;version=2.0;type=obj_Activity",
    qualified_type = "resqml20.obj_Activity"
)]
pub struct Activity201 {
    /// Object identity.
    #[energyml(attr, xml_name = "uuid")]
    pub uuid: String,

    /// Inherited from `eml:AbstractObject`.
    #[energyml(elem, ns = "eml", xml_name = "Citation")]
    pub citation: Citation,

    /// A reference to another object, so the graph walk has something to find.
    #[energyml(elem, xml_name = "Parent")]
    pub parent: Option<DataObjectReference20>,
}

fn activity_201() -> Activity201 {
    Activity201 {
        uuid: "11111111-2222-3333-4444-555555555555".into(),
        citation: Citation {
            title: "History match".into(),
            originator: Some("geosiris".into()),
            creation: None,
        },
        parent: None,
    }
}

#[test]
fn identifiers_of_a_2_0_1_object_keep_the_obj_prefix() {
    let info = <Activity201 as EnergymlType>::type_info();
    assert_eq!(info.xml_name, "Activity", "the XML tag has no prefix");
    assert_eq!(info.object_type, "obj_Activity", "identifiers do");
    assert_eq!(info.type_key, "Activity", "grouping ignores the prefix");
    assert_eq!(info.schema_version, "2.0", "2.0.1 is written 2.0 in identifiers");

    // `ObjectKey` borrows from the object, so the object has to outlive it.
    let activity = activity_201();
    let key = identity::key_of(&activity).expect("a top-level object has an identity");
    assert_eq!(key.content_type(), info.content_type.unwrap());
    assert_eq!(key.qualified_type(), info.qualified_type.unwrap());
    assert_eq!(
        key.part_name(),
        "obj_Activity_11111111-2222-3333-4444-555555555555.xml"
    );
}

#[test]
fn title_is_read_through_the_citation() {
    let activity = activity_201();
    assert_eq!(ObjectIdentity::title(&activity), Some("History match"));
    assert_eq!(ObjectIdentity::uuid(&activity), Some(activity.uuid.as_str()));
}

#[test]
fn a_reference_can_be_built_in_either_version() {
    let activity = activity_201();

    // eml 2.0 spelling: a content type, prefix included.
    let old: DataObjectReference20 = activity.as_object_ref().unwrap();
    assert_eq!(
        old.content_type,
        "application/x-resqml+xml;version=2.0;type=obj_Activity"
    );
    assert_eq!(old.uuid, activity.uuid);
    assert_eq!(old.title, "History match");

    // eml 2.3 spelling: a qualified type, same prefix.
    let new: DataObjectReference23 = activity.as_object_ref().unwrap();
    assert_eq!(new.qualified_type, "resqml20.obj_Activity");
    assert_eq!(new.uuid, activity.uuid);
}

#[test]
fn a_reference_converts_to_another_version() {
    let old: DataObjectReference20 = activity_201().as_object_ref().unwrap();
    let new = DataObjectReference23::referencing(&old).unwrap();

    assert_eq!(new.qualified_type, "resqml20.obj_Activity");
    assert_eq!(new.uuid, old.uuid);
    assert_eq!(new.title, old.title);

    // And back again, unchanged.
    let round_tripped = DataObjectReference20::referencing(&new).unwrap();
    assert_eq!(round_tripped.content_type, old.content_type);
}

#[test]
fn a_reference_resolves_to_the_object_it_names() {
    let activity = activity_201();
    let dor: DataObjectReference23 = activity.as_object_ref().unwrap();

    let target = identity::key_of(&dor).unwrap();
    let source = identity::key_of(&activity).unwrap();
    assert!(target.same_object(&source));
    assert!(target.same_concept(&source));
    assert_eq!(target.type_key(), "Activity");
}

#[test]
fn references_are_found_by_walking_reflection() {
    let mut activity = activity_201();
    activity.parent = Some(activity_201().as_object_ref().unwrap());

    let edges = identity::collect_refs(&activity);
    assert_eq!(edges.len(), 1, "one outgoing reference");
    assert_eq!(edges[0].path, "Parent");
    assert_eq!(edges[0].target.object_type, "obj_Activity");
}

#[test]
fn identity_works_without_knowing_the_type() {
    let activity = activity_201();
    let dynamic: &dyn value::Reflect = &activity;

    assert_eq!(identity::uuid_of_reflect(dynamic), Some(activity.uuid.as_str()));
    assert_eq!(identity::title_of_reflect(dynamic), Some("History match"));
    let key = identity::key_of_reflect(dynamic).expect("identity is reachable dynamically");
    assert_eq!(key.qualified_type(), "resqml20.obj_Activity");
}

#[test]
fn the_registry_groups_versions_of_one_concept() {
    let mut reg = registry::Registry::new();
    reg.register(registry::TypeEntry::of::<Activity201>());
    reg.register(registry::TypeEntry::of::<TriangulatedSetRepresentation>());

    let grouped = reg.types_by_key("Activity");
    assert_eq!(grouped.len(), 1);
    assert_eq!(grouped[0].object_type, "obj_Activity");

    // The same lookup tolerates the prefix and the casing a user might type.
    assert_eq!(reg.find_type_loose("obj_activity").len(), 1);

    // A 2.0.1 content type still resolves, prefix or not.
    assert!(
        reg.by_content_type("application/x-resqml+xml;version=2.0;type=obj_Activity")
            .is_some()
    );
    assert!(
        reg.by_content_type("application/x-resqml+xml;version=2.0;type=Activity")
            .is_some()
    );
}

// ---------------------------------------------------------------------------
// `xsd:list`: one element, every value inside it
// ---------------------------------------------------------------------------

/// What `eml:FloatingPointXmlArray` looks like once generated: a count, and a
/// `Values` element whose text is the whole array.
#[derive(Debug, Default, Clone, PartialEq, EnergymlObject)]
#[energyml(package = "eml", version = "2.3", xml_name = "FloatingPointXmlArray")]
pub struct FloatingPointXmlArray {
    /// How many numbers make up one logical value.
    #[energyml(elem, xml_name = "CountPerValue")]
    pub count_per_value: i64,

    /// The array itself, written as one whitespace-separated element.
    #[energyml(elem, xml_name = "Values", list, min = 1, min_inclusive = "0")]
    pub values: Vec<f64>,
}

fn array() -> FloatingPointXmlArray {
    FloatingPointXmlArray {
        count_per_value: 1,
        values: vec![1.5, -2.0, 3.25],
    }
}

fn array_xml(value: &FloatingPointXmlArray) -> String {
    xml::to_xml_string(
        value,
        ns::EML,
        "FloatingPointXmlArray",
        &[ns::EML],
        xml::XmlWriteOptions::COMPACT,
    )
    .expect("serializes")
}

#[test]
fn a_list_field_is_one_element_holding_every_value() {
    let doc = array_xml(&array());
    assert!(doc.contains("<eml:Values>1.5 -2 3.25</eml:Values>"), "{doc}");
    assert_eq!(
        doc.matches("<eml:Values>").count(),
        1,
        "one element, not one per value: {doc}"
    );
}

#[test]
fn a_list_field_round_trips() {
    let original = array();
    let parsed: FloatingPointXmlArray = xml::from_xml_str(&array_xml(&original)).expect("reads back");
    assert_eq!(parsed, original);
}

#[test]
fn a_list_tolerates_the_whitespace_a_real_file_uses() {
    // XSD collapses whitespace, so a value wrapped over several lines is the
    // same list as one written inline. Files with big arrays do wrap.
    let doc = "<eml:FloatingPointXmlArray xmlns:eml=\"http://www.energistics.org/energyml/data/commonv2\">\
               <eml:CountPerValue>1</eml:CountPerValue>\
               <eml:Values>\n  1.5   -2.0\n\t3.25\n</eml:Values>\
               </eml:FloatingPointXmlArray>";
    let parsed: FloatingPointXmlArray = xml::from_xml_str(doc).expect("reads back");
    assert_eq!(parsed.values, vec![1.5, -2.0, 3.25]);
}

#[test]
fn an_empty_list_element_is_an_empty_vec_not_an_error() {
    let doc = "<eml:FloatingPointXmlArray xmlns:eml=\"http://www.energistics.org/energyml/data/commonv2\">\
               <eml:CountPerValue>1</eml:CountPerValue><eml:Values/>\
               </eml:FloatingPointXmlArray>";
    let parsed: FloatingPointXmlArray = xml::from_xml_str(doc).expect("reads back");
    assert!(parsed.values.is_empty());

    // How many values are required is a cardinality question, so validation is
    // what complains - not the reader.
    let report = validate::Validate::validate(&parsed);
    assert!(
        report
            .errors()
            .any(|i| i.code == "missing-required" && i.path.contains("Values")),
        "{:?}",
        report.errors().collect::<Vec<_>>()
    );
}

#[test]
fn a_list_reports_the_first_value_that_does_not_parse() {
    let doc = "<eml:FloatingPointXmlArray xmlns:eml=\"http://www.energistics.org/energyml/data/commonv2\">\
               <eml:CountPerValue>1</eml:CountPerValue><eml:Values>1.5 oops 3.0</eml:Values>\
               </eml:FloatingPointXmlArray>";
    let err = xml::from_xml_str::<FloatingPointXmlArray>(doc).expect_err("`oops` is not a double");
    assert!(
        format!("{err}").contains("oops"),
        "the message names the offending value: {err}"
    );
}

#[test]
fn facets_of_a_list_apply_to_each_value() {
    let mut value = array();
    value.values.push(-9.0); // min_inclusive = 0
    let report = validate::Validate::validate(&value);
    let issue = report
        .errors()
        .find(|i| i.code.contains("min-inclusive"))
        .expect("the facet is checked per value");
    assert!(
        issue.path.contains("Values["),
        "the path points at the value: {}",
        issue.path
    );
}

#[test]
fn reflection_tells_a_list_apart_from_a_repeated_element() {
    let info = <FloatingPointXmlArray as EnergymlType>::type_info();
    let values = info.field("values").expect("the field is described");
    assert!(values.is_list, "a UI needs to know this is one element, not many");
    assert_eq!(values.binding, Binding::Element);
    // Cardinality counts values, which is what a form would show.
    assert_eq!(values.occurs.min, 1);
    assert!(values.occurs.is_collection());

    let count = info.field("count_per_value").expect("the other field");
    assert!(!count.is_list);
}

// ---------------------------------------------------------------------------
// `xsi:type` on a *concrete* declared type
// ---------------------------------------------------------------------------

/// A concrete type other types extend - `witsml:PlaneAngleOperatingRange` is
/// the real shape: concrete, and `AzimuthRange` extends it.
#[derive(Debug, Default, Clone, PartialEq, EnergymlObject)]
#[energyml(package = "witsml", version = "2.1", xml_name = "PlaneAngleOperatingRange")]
pub struct PlaneAngleOperatingRange {
    /// Lower bound.
    #[energyml(elem, xml_name = "MinAngle")]
    pub min_angle: f64,
}

/// The subtype, adding a field of its own.
#[derive(Debug, Default, Clone, PartialEq, EnergymlObject)]
#[energyml(package = "witsml", version = "2.1", xml_name = "AzimuthRange")]
pub struct AzimuthRange {
    /// Inherited, flattened as the generator does.
    #[energyml(elem, xml_name = "MinAngle")]
    pub min_angle: f64,

    /// What the subtype adds, and what a base-only reader would drop.
    #[energyml(elem, xml_name = "IsPreferred")]
    pub is_preferred: bool,
}

/// The declared type, or anything extending it.
#[derive(Debug, Clone, PartialEq, EnergymlChoice)]
#[energyml(package = "witsml", version = "2.1", xml_name = "PlaneAngleOperatingRange")]
pub enum AnyPlaneAngleOperatingRange {
    /// The declared type: written without `xsi:type`.
    #[energyml(
        xsi_type = "witsml:PlaneAngleOperatingRange",
        xml_name = "PlaneAngleOperatingRange",
        implicit
    )]
    PlaneAngleOperatingRange(PlaneAngleOperatingRange),

    /// A subtype, selected by `xsi:type`.
    #[energyml(xsi_type = "witsml:AzimuthRange", xml_name = "AzimuthRange")]
    AzimuthRange(AzimuthRange),
}

/// The owner. Note the element is called `Range`, not after its type - the
/// case that makes matching on the element name alone insufficient.
#[derive(Debug, Default, Clone, PartialEq, EnergymlObject)]
#[energyml(package = "witsml", version = "2.1", xml_name = "ContinuousGyro")]
pub struct ContinuousGyro {
    /// Operating range, possibly substituted.
    #[energyml(elem, xml_name = "Range")]
    pub range: AnyPlaneAngleOperatingRange,
}

fn gyro_xml(value: &ContinuousGyro) -> String {
    xml::to_xml_string(
        value,
        ns::WITSML,
        "ContinuousGyro",
        &[ns::WITSML, ns::XSI],
        XmlWriteOptions::COMPACT,
    )
    .expect("serializes")
}

#[test]
fn a_substitutable_concrete_type_has_the_base_as_its_default() {
    // This is what keeps a required field on such a type out of an `Option`.
    let gyro = ContinuousGyro::default();
    assert!(matches!(
        gyro.range,
        AnyPlaneAngleOperatingRange::PlaneAngleOperatingRange(_)
    ));
}

#[test]
fn the_declared_type_is_written_without_an_xsi_type() {
    let doc = gyro_xml(&ContinuousGyro {
        range: AnyPlaneAngleOperatingRange::PlaneAngleOperatingRange(PlaneAngleOperatingRange {
            min_angle: 10.0,
        }),
    });
    assert!(
        !doc.contains("xsi:type"),
        "other writers put none here either: {doc}"
    );
    assert!(doc.contains("<witsml:Range>"), "{doc}");
}

#[test]
fn a_subtype_is_written_with_its_xsi_type() {
    let doc = gyro_xml(&ContinuousGyro {
        range: AnyPlaneAngleOperatingRange::AzimuthRange(AzimuthRange {
            min_angle: 10.0,
            is_preferred: true,
        }),
    });
    assert!(doc.contains(r#"xsi:type="witsml:AzimuthRange""#), "{doc}");
    assert!(
        doc.contains("<witsml:IsPreferred>true</witsml:IsPreferred>"),
        "{doc}"
    );
}

#[test]
fn a_substituted_subtype_survives_the_round_trip() {
    // Before the `Any…` enums this read back as the base type and
    // `IsPreferred` vanished without a word.
    let original = ContinuousGyro {
        range: AnyPlaneAngleOperatingRange::AzimuthRange(AzimuthRange {
            min_angle: -3.5,
            is_preferred: true,
        }),
    };
    let parsed: ContinuousGyro = xml::from_xml_str(&gyro_xml(&original)).expect("reads back");
    assert_eq!(parsed, original);
}

#[test]
fn an_element_without_an_xsi_type_reads_as_the_declared_type() {
    // `Range` is named after neither variant, so only the implicit fallback
    // can resolve it.
    let doc = "<witsml:ContinuousGyro xmlns:witsml=\"http://www.energistics.org/energyml/data/witsmlv2\">\
               <witsml:Range><witsml:MinAngle>7</witsml:MinAngle></witsml:Range>\
               </witsml:ContinuousGyro>";
    let parsed: ContinuousGyro = xml::from_xml_str(doc).expect("reads back");
    assert_eq!(
        parsed.range,
        AnyPlaneAngleOperatingRange::PlaneAngleOperatingRange(PlaneAngleOperatingRange { min_angle: 7.0 })
    );
}

#[test]
fn an_unknown_xsi_type_is_still_an_error() {
    // The fallback must not swallow a type nobody knows: that would be the
    // silent data loss this whole change exists to remove.
    let doc = "<witsml:ContinuousGyro xmlns:witsml=\"http://www.energistics.org/energyml/data/witsmlv2\" \
               xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\
               <witsml:Range xsi:type=\"witsml:SomethingElse\"><witsml:MinAngle>7</witsml:MinAngle></witsml:Range>\
               </witsml:ContinuousGyro>";
    let err = xml::from_xml_str::<ContinuousGyro>(doc).expect_err("unknown type is rejected");
    assert!(format!("{err}").contains("SomethingElse"), "{err}");
}

#[test]
fn reflection_sees_through_the_substitution() {
    let gyro = ContinuousGyro {
        range: AnyPlaneAngleOperatingRange::AzimuthRange(AzimuthRange {
            min_angle: 1.0,
            is_preferred: false,
        }),
    };
    // A UI walking the object sees the concrete subtype, not the wrapper.
    let inner = gyro.range.type_info();
    assert_eq!(inner.name, "AzimuthRange");
    assert_eq!(
        path::get_string(&gyro, "Range.IsPreferred").unwrap().as_deref(),
        Some("false"),
        "the dotted path reaches the subtype's own field"
    );
}
// ---------------------------------------------------------------------------
// Entities and text runs
// ---------------------------------------------------------------------------

/// An ampersand in a title is ordinary in real data - and quick-xml reports
/// `&amp;` as an event of its own, splitting the surrounding characters into
/// separate text events. Joining them wrongly mangles the value in silence.
#[test]
fn entities_and_the_text_around_them_survive_reading() {
    let doc = "<eml:Citation xmlns:eml=\"http://www.energistics.org/energyml/data/commonv2\">\
               <eml:Title>Fault &amp; Horizon &lt;1&gt; &quot;A&quot; &#233;</eml:Title>\
               </eml:Citation>";
    let citation: Citation = xml::from_xml_str(doc).expect("reads");
    assert_eq!(citation.title, "Fault & Horizon <1> \"A\" \u{e9}");
}

#[test]
fn a_title_with_an_entity_round_trips() {
    let original = Citation {
        title: "Fault & Horizon <1> \"A\"".to_string(),
        originator: Some("g\u{e9}osiris".to_string()),
        creation: None,
    };
    let doc = xml::to_xml_string(
        &original,
        ns::EML,
        "Citation",
        &[ns::EML],
        XmlWriteOptions::COMPACT,
    )
    .expect("writes");
    assert!(
        doc.contains("&amp;"),
        "the ampersand is escaped on the way out: {doc}"
    );

    let parsed: Citation = xml::from_xml_str(&doc).expect("reads its own output");
    assert_eq!(parsed, original);
}

#[test]
fn indentation_between_elements_is_still_not_content() {
    let doc = "<eml:Citation xmlns:eml=\"http://www.energistics.org/energyml/data/commonv2\">\n\
               \t<eml:Title>\n\t\tTop Horizon\n\t</eml:Title>\n</eml:Citation>";
    let citation: Citation = xml::from_xml_str(doc).expect("reads");
    assert_eq!(
        citation.title, "Top Horizon",
        "the value is trimmed, the layout ignored"
    );
}

/// A kept subtree carries the prefixes it uses.
///
/// The declarations that made `resqml:` meaningful lived on the document root
/// of the file it came from. Writing the subtree back under a different root
/// without them produces XML no parser can resolve - which is how a real
/// `eml:GraphicalInformationSet` came back empty.
#[test]
fn a_kept_subtree_carries_its_namespace_declarations() {
    use energyml_core::any_xml::AnyXml;

    let doc = "<eml:Wrapper xmlns:eml=\"http://www.energistics.org/energyml/data/commonv2\" \
               xmlns:resqml=\"http://www.energistics.org/energyml/data/resqmlv2\">\
               <resqml:Inner id=\"1\">value</resqml:Inner></eml:Wrapper>";
    let kept: AnyXml = xml::from_xml_str(doc).expect("reads");
    assert_eq!(
        kept.namespace("resqml"),
        Some("http://www.energistics.org/energyml/data/resqmlv2"),
        "the binding travels with the subtree"
    );

    let written =
        xml::to_xml_string(&kept, ns::EML, "Wrapper", &[ns::EML], XmlWriteOptions::COMPACT).expect("writes");
    assert!(
        written.contains("xmlns:resqml="),
        "the prefix is re-declared: {written}"
    );
    assert!(
        written.contains("<resqml:Inner id=\"1\">value</resqml:Inner>"),
        "{written}"
    );

    // And the result reads back the same, which is what a round trip means.
    let again: AnyXml = xml::from_xml_str(&written).expect("reads its own output");
    assert_eq!(again.content, kept.content);
}
