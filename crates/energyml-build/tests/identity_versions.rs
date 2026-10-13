//! **An object is addressed by its full uri** (ADR 0003), the part that lives in
//! `energyml-build`.
//!
//! The rule: internally only the full uri is used. It prevents
//! sources of bugs. A package may hold one uuid
//! twice, under two `objectVersion`s, and everything here once named an object
//! by its uuid alone: `Ref` kept no version, the readers followed a reference
//! with `find_by_uuid` (which answers *one of them*), a recipe named what a step
//! wrote by uuid, a collection counted two versions as a repetition.
//!
//! Every package here is fabricated in memory — never a file — with one uuid in
//! two versions **titled apart**, so that reading the wrong entry shows.
//!
//! What would have to break, each one tried (see the report of the change):
//!
//! - `read::followed` answering `find_by_uuid` — the vertical half of the
//!   compound that names version 2 comes back as version 1;
//! - `Object::reference` dropping the version — the DOR a recipe passes to the
//!   next step names no version;
//! - `Ref::uri` without the version — every address below is the short one.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use serde_json::{Value, json};

use energyml_build::entries::Reader;
use energyml_build::operation::{AnyOperation, Choices, Error, Fitness, Identity, Need, NewObject, Note, Plan, Registry, Slot, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_build::recipe::{self, Mode, Recipe};
use energyml_build::source::Source as Where;
use energyml_core::registry::{DynObject, ReadMode};
use energyml_eml_2_3 as eml;
use energyml_storage::EpcStore;

const RESQML: &str = "http://www.energistics.org/energyml/data/resqmlv2";
const EML: &str = "http://www.energistics.org/energyml/data/commonv2";
const X: &str = "7e200000-0000-4000-8000-000000000001";
const PLANE: &str = "7e200000-0000-4000-8000-000000000002";
const VERTICAL: &str = "7e200000-0000-4000-8000-000000000003";
const ON_ONE: &str = "7e200000-0000-4000-8000-000000000004";
const ON_TWO: &str = "7e200000-0000-4000-8000-000000000005";
const ON_NONE: &str = "7e200000-0000-4000-8000-000000000006";

fn when() -> When {
    When { creation: "2026-10-06T00:00:00Z".into(), originator: "test".into() }
}

fn citation(title: &str) -> eml::Citation {
    eml::Citation {
        title: title.to_string(),
        originator: "test".to_string(),
        creation: "2026-10-06T00:00:00Z".to_string(),
        format: "test".to_string(),
        ..Default::default()
    }
}

fn empty_store() -> EpcStore {
    energyml_build::epc::open(None, "identity.epc", Arc::new(energyml_schemas::all())).expect("an empty package")
}

// -- a boundary feature in two versions: the plainest object with a version ---

fn feature(version: &str, title: &str) -> Box<dyn DynObject> {
    let xml = format!(
        "<resqml:BoundaryFeature xmlns:resqml=\"{RESQML}\" xmlns:eml=\"{EML}\" uuid=\"{X}\" \
         schemaVersion=\"2.2\" objectVersion=\"{version}\">\
         <eml:Citation><eml:Title>{title}</eml:Title><eml:Originator>test</eml:Originator>\
         <eml:Creation>2026-10-06T00:00:00Z</eml:Creation><eml:Format>test</eml:Format></eml:Citation>\
         <resqml:IsWellKnown>false</resqml:IsWellKnown></resqml:BoundaryFeature>"
    );
    energyml_schemas::all()
        .read_xml_as("application/x-resqml+xml;version=2.2;type=BoundaryFeature", xml.as_bytes(), ReadMode::Tolerant)
        .expect("readable")
        .0
}

/// `X` in versions 1 and 2, titled apart.
fn two_features() -> EpcStore {
    let mut store = empty_store();
    store.put(feature("1", "X, first")).expect("the first");
    store.put_as_new_version(feature("2", "X, second")).expect("the second");
    store
}

fn uri_with(store: &EpcStore, version: &str) -> String {
    store
        .resources()
        .into_iter()
        .map(|one| one.uri)
        .find(|uri| uri.contains(&format!("version='{version}'")))
        .unwrap_or_else(|| panic!("an entry at version {version}"))
}

// -- a `Ref` is an identity, and its address is the full uri -------------------

#[test]
fn a_ref_names_its_version_in_its_address_and_in_the_reference_it_writes() {
    let plain = Ref::new(X, "resqml22.BoundaryFeature", "X");
    assert_eq!(plain.object_version, None, "`Ref::new` names no version: the callers written before it keep compiling");
    assert_eq!(plain.uri(), format!("eml:///resqml22.BoundaryFeature({X})"));
    assert!(plain.dor().get("ObjectVersion").is_none(), "no version, no `ObjectVersion`: {}", plain.dor());

    let second = plain.clone().with_version(Some("2".to_string()));
    assert_eq!(second.uri(), format!("eml:///resqml22.BoundaryFeature(uuid={X},version='2')"));
    assert_eq!(second.dor()["ObjectVersion"], json!("2"), "{}", second.dor());
    assert_ne!(plain, second, "two versions are two references");
    assert!(!plain.same_object(&second) && second.same_object(&second.clone()));

    // A blank version is no version, and a quote is doubled (ETP v1.2 §25.3.10).
    assert_eq!(plain.clone().with_version(Some("  ".to_string())).object_version, None);
    let quoted = plain.clone().with_version(Some("O'Neil".to_string()));
    assert_eq!(quoted.uri(), format!("eml:///resqml22.BoundaryFeature(uuid={X},version='O''Neil')"));
    let back = Ref::of_uri(&quoted.uri(), "X").expect("a uri of an object");
    assert_eq!(back.object_version.as_deref(), Some("O'Neil"), "read by `EtpUri`, not by hand");
    assert_eq!((back.uuid.as_str(), back.qualified_type.as_str()), (X, "resqml22.BoundaryFeature"));

    // The address is the one the store emits, and the uuid is lowercased in it.
    let upper = Ref::new(X.to_ascii_uppercase(), "resqml22.BoundaryFeature", "X");
    assert_eq!(upper.uri(), plain.uri());
    assert!(Ref::of_uri("eml:///dataspace('only')", "").is_none(), "a dataspace is no object");
}

#[test]
fn what_is_known_of_each_version_names_that_version() {
    let store = two_features();
    for version in ["1", "2"] {
        let uri = uri_with(&store, version);
        let known = energyml_build::read::known_of_object(&store, &uri).expect("read by its uri");
        let reference = known.reference.expect("a reference");
        assert_eq!(reference.object_version.as_deref(), Some(version));
        assert_eq!(reference.uri(), uri, "the reference's address is the entry's");
        assert_eq!(reference.title, format!("X, {}", if version == "1" { "first" } else { "second" }));
        // And the reference it writes follows back to the same entry.
        assert_eq!(store.resolve(&reference.uri()).expect("it resolves").uri, uri);
    }
}

// -- following a reference: by the version it names ----------------------------

fn vertical(version: &str, title: &str) -> eml::VerticalCrs {
    eml::VerticalCrs {
        uuid: VERTICAL.to_string(),
        schema_version: "2.3".to_string(),
        object_version: Some(version.to_string()),
        citation: citation(title),
        direction: eml::VerticalDirection::Up,
        uom: eml::LengthUomExt::M,
        abstract_vertical_crs: Some(eml::AbstractVerticalCrs::VerticalEpsgCrs(eml::VerticalEpsgCrs { epsg_code: 5720 })),
        ..Default::default()
    }
}

fn compound(uuid: &str, names: Option<&str>) -> eml::LocalEngineeringCompoundCrs {
    eml::LocalEngineeringCompoundCrs {
        uuid: uuid.to_string(),
        schema_version: "2.3".to_string(),
        citation: citation("compound"),
        vertical_crs: eml::DataObjectReference {
            uuid: VERTICAL.to_string(),
            qualified_type: "eml23.VerticalCrs".to_string(),
            title: "NGF".to_string(),
            object_version: names.map(str::to_string),
            ..Default::default()
        },
        vertical_axis: eml::VerticalAxis {
            direction: eml::VerticalDirection::Up,
            uom: eml::LengthAndTimeUomExt::M,
            is_time: false,
        },
        local_engineering2d_crs: eml::DataObjectReference {
            uuid: PLANE.to_string(),
            qualified_type: "eml23.LocalEngineering2dCrs".to_string(),
            title: "plane".to_string(),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// A plane, a `VerticalCrs` in versions 1 and 2, and three compounds: one
/// naming version 1, one naming version 2, one naming none.
fn three_compounds() -> EpcStore {
    let mut store = empty_store();
    store
        .put(Box::new(eml::LocalEngineering2dCrs {
            uuid: PLANE.to_string(),
            schema_version: "2.3".to_string(),
            citation: citation("plane"),
            azimuth: eml::PlaneAngleMeasureExt { value: 0.0, uom: eml::PlaneAngleUomExt::Dega },
            azimuth_reference: eml::NorthReferenceKind::GridNorth,
            horizontal_axes: eml::HorizontalAxes {
                direction1: eml::AxisDirectionKind::East,
                direction2: eml::AxisDirectionKind::North,
                uom: eml::LengthAndTimeUomExt::M,
                is_time: false,
            },
            origin_projected_crs: eml::ProjectedCrs {
                abstract_projected_crs: Some(eml::AbstractProjectedCrs::ProjectedEpsgCrs(eml::ProjectedEpsgCrs { epsg_code: 2154 })),
                ..Default::default()
            },
            ..Default::default()
        }))
        .expect("the plane");
    store.put(Box::new(vertical("1", "NGF, first"))).expect("version 1");
    store.put_as_new_version(Box::new(vertical("2", "NGF, second"))).expect("version 2");
    store.put(Box::new(compound(ON_ONE, Some("1")))).expect("names 1");
    store.put(Box::new(compound(ON_TWO, Some("2")))).expect("names 2");
    store.put(Box::new(compound(ON_NONE, None))).expect("names none");
    store
}

fn uri_of(store: &EpcStore, uuid: &str) -> String {
    store.lookup_uuid(uuid).unwrap_or_else(|why| panic!("{uuid}: {why}")).uri.clone()
}

#[test]
fn the_vertical_half_is_the_version_the_compound_names() {
    let store = three_compounds();
    for (compound, version, title) in [(ON_ONE, "1", "NGF, first"), (ON_TWO, "2", "NGF, second")] {
        let known = energyml_build::read::known_of_object(&store, &uri_of(&store, compound)).expect("a compound reads");
        let half = known.vertical_crs.unwrap_or_else(|| panic!("{compound} names a vertical half"));
        assert_eq!(half.object_version.as_deref(), Some(version), "{compound}: {half:?}");
        assert_eq!(half.title, title, "the title of the entry followed, not of its sibling");
        assert_eq!(half.uri(), uri_with(&store, version));
    }
}

#[test]
fn a_reference_that_names_no_version_among_two_is_not_guessed() {
    let store = three_compounds();
    // Both entries declare a version, and the reference names none: *which* is
    // not something the package says, so there is no vertical half — never one
    // of the two.
    let known = energyml_build::read::known_of_object(&store, &uri_of(&store, ON_NONE)).expect("the compound reads");
    assert_eq!(known.vertical_crs, None, "a guess would be one of the two");
}

// -- a plan names an object by its uri -----------------------------------------

#[test]
fn a_new_object_is_addressed_by_its_uri_and_the_plan_says_it() {
    let object = |json: Value| NewObject {
        uuid: X.to_string(),
        qualified_type: "resqml22.BoundaryFeature".to_string(),
        role: "feature".to_string(),
        title: "X".to_string(),
        json,
    };
    let plain = object(json!({ "uuid": X, "Citation": { "Title": "X" } }));
    assert_eq!(plain.uri(), format!("eml:///resqml22.BoundaryFeature({X})"));

    let versioned = object(json!({ "uuid": X, "objectVersion": " 2 ", "Citation": { "Title": "X" } }));
    assert_eq!(versioned.object_version().as_deref(), Some("2"), "trimmed");
    assert_eq!(versioned.uri(), format!("eml:///resqml22.BoundaryFeature(uuid={X},version='2')"));
    assert_eq!(versioned.reference().uri(), versioned.uri());

    // eml 2.0 keeps it in the citation.
    let old = object(json!({ "uuid": X, "Citation": { "Title": "X", "VersionString": "v3" } }));
    assert_eq!(old.object_version().as_deref(), Some("v3"));

    // The summary of a plan keys its objects by that uri.
    let mut plan = empty_plan();
    plan.objects = vec![plain.clone(), versioned.clone()];
    let summary = plan.summary_json();
    let uris: Vec<&str> = summary["objects"].as_array().expect("objects").iter().map(|one| one["uri"].as_str().expect("a uri")).collect();
    assert_eq!(uris.iter().collect::<BTreeSet<_>>().len(), 2, "one uuid, two objects, two keys: {uris:?}");
    assert_eq!(summary["objects"][1]["objectVersion"], json!("2"));
}

fn empty_plan() -> Plan {
    Plan {
        objects: Vec::new(),
        datasets: Vec::new(),
        media: Vec::new(),
        activity: energyml_build::operation::ActivityDraft {
            recipe: "crs",
            title: "t".to_string(),
            seed: "s".to_string(),
            parameters: Vec::new(),
        },
        says: Vec::new(),
        dropped: Vec::new(),
        shapes: Vec::new(),
        blocked: None,
    }
}

// -- a collection: two versions are two members --------------------------------

fn member(version: Option<&str>, title: &str) -> Source {
    Source::Object(Known {
        reference: Some(Ref::new(X, "resqml22.BoundaryFeature", title).with_version(version.map(str::to_string))),
        ..Default::default()
    })
}

fn collection(pieces: &Pieces) -> Plan {
    Registry::new()
        .plan("collection", pieces, &Choices::new().set("title", "features"), &Identity::derived(), &when())
        .unwrap_or_else(|why| panic!("{why}"))
}

#[test]
fn two_versions_of_one_object_are_two_members_of_a_collection() {
    let both = Pieces::empty().with("members", member(Some("1"), "X")).with("members", member(Some("2"), "X"));
    let plan = collection(&both);
    let set = &plan.objects.iter().find(|one| one.qualified_type == "eml23.CollectionsToDataobjectsAssociationSet").expect("the set").json;
    let members = set["SingleCollectionAssociation"][0]["Dataobject"].as_array().expect("members");
    assert_eq!(members.len(), 2, "{set}");
    assert!(plan.says.iter().all(|note| note.key != "twice"), "one version twice is a repetition, two versions are not");
    let versions: Vec<&str> = members.iter().map(|one| one["ObjectVersion"].as_str().expect("the version is written")).collect();
    assert_eq!(versions, ["1", "2"], "{set}");

    // Another set for the other pair, since a set is derived from its members.
    let one = Pieces::empty().with("members", member(Some("1"), "X"));
    let other = collection(&one);
    assert_ne!(
        plan.objects.iter().find(|o| o.qualified_type == "eml23.CollectionsToDataobjectsAssociationSet").map(|o| &o.uuid),
        other.objects.iter().find(|o| o.qualified_type == "eml23.CollectionsToDataobjectsAssociationSet").map(|o| &o.uuid),
    );

    // The same version twice is still the repetition it was.
    let twice = Pieces::empty().with("members", member(Some("2"), "X")).with("members", member(Some("2"), "X"));
    assert!(collection(&twice).says.iter().any(|note| note.key == "twice"));
}

// -- a recipe names what a step wrote by its uri -------------------------------

/// Names what it is given, and says by which reference: the DOR the setting
/// became, and the address of the companion the bag carries for it.
struct SaysWhatItIsGiven;

impl AnyOperation for SaysWhatItIsGiven {
    fn name(&self) -> &'static str {
        "says-what-it-is-given"
    }
    fn describe(&self) -> &'static str {
        "reports the reference it was given — a test operation"
    }
    fn makes(&self) -> &'static [&'static str] {
        &["resqml22.BoundaryFeature"]
    }
    fn recipe(&self) -> &'static str {
        "crs"
    }
    fn fitness(&self, _pieces: &Pieces) -> Fitness {
        Fitness::Fit
    }
    fn needs(&self, _pieces: &Pieces) -> Vec<Slot> {
        vec![
            Slot::optional("frame", Need::Object { types: &["eml23.VerticalCrs"] }, "the frame to name"),
            Slot::required("title", Need::Title, "the feature's title"),
        ]
    }
    fn plan(&self, pieces: &Pieces, choices: &Choices, identity: &Identity, when: &When) -> Result<Plan, Error> {
        let title = choices.text("title").unwrap_or("untitled").to_string();
        let uuid = identity.uuid(&format!("says:{title}"), 0)?;
        let companions: Vec<String> = pieces
            .iter()
            .filter(|(name, _)| name.ends_with(" (object)"))
            .filter_map(|(_, source)| match source {
                Source::Object(known) => known.reference.as_ref().map(Ref::uri),
                _ => None,
            })
            .collect();
        let mut plan = empty_plan();
        plan.objects = vec![NewObject {
            uuid: uuid.clone(),
            qualified_type: "resqml22.BoundaryFeature".to_string(),
            role: "feature".to_string(),
            title: title.clone(),
            json: json!({
                "$type": "resqml22.BoundaryFeature",
                "uuid": uuid,
                "schemaVersion": "2.2",
                "Citation": { "$type": "eml23.Citation", "Title": title, "Originator": when.originator, "Creation": when.creation },
                "IsWellKnown": false,
            }),
        }];
        plan.activity.title = format!("says → {title}");
        plan.activity.seed = title;
        plan.says = vec![Note {
            key: "given",
            detail: json!({ "frame": choices.get("frame"), "companions": companions }).to_string(),
        }];
        Ok(plan)
    }
}

struct NoFile;

impl Reader for NoFile {
    fn text(&mut self, source: &Where) -> Result<(String, Option<String>), String> {
        Err(format!("reading {}: this test reads no file", source.as_str()))
    }
    fn package(&mut self, path: &str) -> Result<&EpcStore, String> {
        Err(format!("opening {path}: this test opens no package"))
    }
}

/// A step writes a vertical system; the next copies it under version 2; the
/// last names the copy — by the step that wrote it, never by a uuid.
const COPY_THEN_NAME: &str = r#"
titre = "Mini"
creation = "2026-10-06T00:00:00Z"
auteur = "tests/identity_versions.rs"
workflow = "workflow:mini"

[[etape]]
nom = "reperes"
operation = "crs"
set = { epsg = 2154, vertical = 5720 }

[[etape]]
nom = "copie"
operation = "new-version"
entrees = [ { creneau = "object", etape = "reperes", type = "VerticalCrs" } ]
set = { version = "2" }

[[etape]]
nom = "nomme"
operation = "says-what-it-is-given"
set = { frame = { etape = "copie", type = "VerticalCrs" }, title = "a limit" }
"#;

#[test]
fn a_recipe_names_what_a_step_wrote_by_its_uri_the_version_included() {
    let recipe = Recipe::parse(COPY_THEN_NAME).expect("the recipe reads");
    let mut registry = Registry::new();
    registry.register(Box::new(SaysWhatItIsGiven));
    let mut store = empty_store();
    let played =
        recipe::play(&recipe, Path::new("."), &registry, &mut store, &mut NoFile, Mode::Write).unwrap_or_else(|stop| panic!("{stop}"));

    // The package holds the vertical system twice, under one uuid.
    let verticals: Vec<_> = store.resources().into_iter().filter(|one| one.qualified_type == "eml23.VerticalCrs").collect();
    assert_eq!(verticals.len(), 2, "the original and its copy: {verticals:?}");
    let original = played.steps[0].objects.iter().find(|one| one.qualified_type == "eml23.VerticalCrs").expect("the original");
    let copy = played.steps[1].objects.iter().find(|one| one.qualified_type == "eml23.VerticalCrs").expect("the copy");
    assert_eq!(original.uuid, copy.uuid, "one uuid");
    assert_ne!(original.uri, copy.uri, "two addresses");
    assert!(copy.uri.contains("version='2'") && !original.uri.contains("version="), "{} / {}", original.uri, copy.uri);
    assert!(store.resolve(&copy.uri).is_ok() && store.resolve(&original.uri).is_ok(), "both addresses resolve");

    // **No object of a step is listed twice** — the copy is written by the plan
    // *and* reported by `apply`, and the two meet by their uri.
    for done in &played.steps {
        let uris: Vec<&String> = done.objects.iter().map(|one| &one.uri).collect();
        assert_eq!(uris.iter().collect::<BTreeSet<_>>().len(), uris.len(), "{}: {uris:?}", done.step);
    }

    // The step that names the copy was handed the copy: the DOR of its setting
    // states the version, and so does the companion it received.
    let given = played.steps[2].says.iter().find(|note| note.key == "given").expect("the step reports what it was given");
    let given: Value = serde_json::from_str(&given.detail).expect("JSON");
    assert_eq!(given["frame"]["ObjectVersion"], json!("2"), "the reference names version 2: {given}");
    assert_eq!(given["frame"]["Uuid"], json!(copy.uuid), "{given}");
    assert_eq!(given["companions"], json!([copy.uri]), "the bag carries the copy, not its original: {given}");
}

// -- a section's polyline set, by the identity the activity states -------------

#[test]
fn a_stored_section_names_the_version_of_its_polyline_set() {
    let mut store = two_features();
    let template = energyml_domain::provenance::template_uuid("section-view").expect("a canonical template");
    let record = energyml_domain::provenance::Record {
        recipe: "section-view",
        uuid: "7e200000-0000-4000-8000-0000000000aa",
        creation: "2026-10-06T00:00:00Z",
        title: Some("section view → A-A'"),
        output_title: None,
        descriptor: json!({ "$type": "eml23.DataObjectReference", "Uuid": template, "QualifiedType": "eml23.ActivityTemplate", "Title": "t" }),
        parameters: vec![energyml_domain::provenance::Parameter::Object {
            title: "output".to_string(),
            // The declared type is what the section wrote; the entries are a
            // feature — the version is what is under test, and a type that
            // matches nothing leaves the uuid and the version to decide.
            reference: json!({
                "$type": "eml23.DataObjectReference",
                "Uuid": X,
                "QualifiedType": "resqml22.PolylineSetRepresentation",
                "ObjectVersion": "2",
                "Title": "A-A'",
            }),
        }],
    };
    let activity = energyml_domain::provenance::activity(&record).expect("an activity");
    let read = energyml_core::json::read_string(&activity.to_string(), &store.registry_arc()).expect("it reads back");
    store.put(read.root).expect("the activity is a part");

    let sections = energyml_build::read::stored_sections(&store);
    assert_eq!(sections.len(), 1, "{sections:?}");
    assert_eq!(sections[0].polyline.as_deref(), Some(uri_with(&store, "2").as_str()), "version 2, not the first entry of the uuid");
}

// -- a selection refuses a uuid that names two ----------------------------------

#[test]
fn a_selection_refuses_a_bare_uuid_that_names_two_and_names_both() {
    use energyml_build::selection::{Request, Rings, Test, select};
    let store = two_features();
    let request = Request {
        uris: vec![X.to_string()],
        rings: Rings { epsg: None, outer: vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]]], inner: Vec::new() },
        test: Test::Touches,
        invert: false,
    };
    let selected = select(&store, &request.uris, &request);
    let refused = selected.refused.first().expect("the uuid names two objects: refused");
    for version in ["1", "2"] {
        assert!(refused.why.contains(&uri_with(&store, version)), "the refusal names the candidate at version {version}: {}", refused.why);
    }
}
