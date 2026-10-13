//! `media` — a file put in the package and related to the objects it shows,
//! through the one door: `Registry::plan`, then `apply`, then a recipe.
//!
//! The form of the assertions is the packaging conventions'
//! (Energistics Packaging Conventions v1.0): every
//! part typed in `[Content_Types].xml` (§2.2.1 p. 9); the object tied to the
//! file by `destinationMedia`, the file back by `sourceMedia` (§3.4.2
//! pp. 15–16). The storage half is confronted with the `zip` crate in
//! `energyml-storage/tests/media.rs`; here the package is **saved and
//! reopened**, and its `.rels` and index are read as the bytes the archive
//! holds, not through the store's relationship parser.
//!
//! What would make them fail: a plan that drops the file or its relations, a
//! `apply` that writes the part before the objects or not at all, a reader
//! that turns bytes into text, a content type guessed from a name.

use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;

use energyml_build::entries::Reader;
use energyml_build::operation::{Choices, Identity, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_build::recipe::{self, Mode, Recipe};
use energyml_build::source::Source as Where;
use energyml_storage::{EpcStore, LocalPackage, SaveOptions};
use serde_json::json;

const BASE: &str = "http://schemas.energistics.org/package/2012/relationships/";
const FEATURE: &str = "3c9d1e2f-4a5b-4c6d-8e7f-9a0b1c2d3e4f";
const README: &[u8] = b"# Lutetia\n\nWhat is real, what is invented.\n";

fn when() -> When {
    When {
        creation: "2026-09-25T00:00:00Z".to_string(),
        originator: "tests/media_parts.rs".to_string(),
    }
}

/// A PDF header and every byte value: not text, in any encoding.
fn pdf() -> Vec<u8> {
    let mut bytes = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    bytes.extend(0u8..=255);
    bytes
}

fn feature_ref() -> Ref {
    Ref::new(FEATURE, "resqml22.BoundaryFeature", "the feature shown")
}

/// A package holding the feature the file will show.
fn store() -> EpcStore {
    let registry = Arc::new(energyml_schemas::all());
    let mut store =
        energyml_build::epc::open(None, "media.epc", Arc::clone(&registry)).expect("an empty package");
    let feature = json!({
        "$type": "resqml22.BoundaryFeature",
        "uuid": FEATURE,
        "schemaVersion": "2.2",
        "Citation": { "$type": "eml23.Citation", "Title": "the feature shown", "Originator": "tests", "Creation": "2026-09-25T00:00:00Z" },
        "IsWellKnown": false,
    });
    let document =
        energyml_core::json::read_string(&feature.to_string(), &registry).expect("the feature reads");
    store.put(document.root).expect("it goes in");
    store
}

fn bag(bytes: &[u8], declared: Option<&str>) -> Pieces {
    Pieces::empty()
        .with(
            "file",
            Source::Document {
                name: "README.md".to_string(),
                content_type: declared.map(str::to_string),
                bytes: bytes.to_vec(),
            },
        )
        .with(
            "shows",
            Source::Object(Known {
                reference: Some(feature_ref()),
                ..Default::default()
            }),
        )
}

fn saved_and_reopened(store: &EpcStore) -> EpcStore {
    let mut out = Cursor::new(Vec::new());
    store
        .write_to(&mut out, &SaveOptions::default())
        .expect("the package writes");
    EpcStore::from_reader(
        Box::new(Cursor::new(out.into_inner())),
        "again.epc",
        Arc::new(energyml_schemas::all()),
    )
    .expect("it reads back")
}

/// The archive's own bytes for a part, as text.
fn raw(store: &EpcStore, part: &str) -> String {
    String::from_utf8(store.raw_part(part).unwrap_or_else(|e| panic!("`{part}`: {e}"))).expect("XML is text")
}

/// The `.rels` of the feature's part and of the media part hold the pair.
fn assert_related(store: &EpcStore, media: &str) {
    let object = store.find_by_uuid(FEATURE).expect("the feature").locator.clone();
    let forward = raw(store, &format!("_rels/{object}.rels"));
    assert!(
        forward
            .lines()
            .any(|line| line.contains(&format!("Type=\"{BASE}destinationMedia\""))
                && line.contains(&format!("Target=\"{media}\""))),
        "{forward}"
    );
    let (folder, file) = media.rsplit_once('/').expect("in a folder");
    let backward = raw(store, &format!("{folder}/_rels/{file}.rels"));
    assert!(
        backward
            .lines()
            .any(|line| line.contains(&format!("Type=\"{BASE}sourceMedia\""))
                && line.contains(&format!("Target=\"../{object}\""))),
        "{backward}"
    );
}

#[test]
fn a_plan_carries_the_file_its_type_and_what_it_shows_and_no_object() {
    let plan = Registry::new()
        .plan(
            "media",
            &bag(README, None),
            &Choices::new().set("contentType", "text/markdown"),
            &Identity::derived(),
            &when(),
        )
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(plan.objects.is_empty(), "a file is not an object");
    assert!(
        !plan.is_empty(),
        "a plan that carries a file has something to write"
    );
    let [media] = plan.media.as_slice() else {
        panic!("one file, got {:?}", plan.media)
    };
    assert_eq!(
        media.path, "media/README.md",
        "the part is named after the file, under `media/` (EPC §3.2)"
    );
    assert_eq!(media.content_type, "text/markdown");
    assert_eq!(media.bytes, README);
    assert_eq!(media.related_to, [FEATURE]);
    // The quote says it, without the bytes.
    assert_eq!(plan.summary_json()["media"][0]["bytes"], json!(README.len()));
}

#[test]
fn what_would_be_guessed_or_forbidden_is_refused_on_its_slot() {
    let registry = Registry::new();
    let refused = |pieces: &Pieces, choices: Choices| -> Vec<(Option<String>, String)> {
        registry
            .plan("media", pieces, &choices, &Identity::derived(), &when())
            .expect_err("it was accepted")
            .refusals()
            .iter()
            .map(|one| (one.slot.clone(), one.message.clone()))
            .collect()
    };
    // A file from a disk declares no type, and its name is not one.
    let none = refused(&bag(README, None), Choices::new());
    assert!(
        none.iter()
            .any(|(slot, why)| slot.as_deref() == Some("contentType") && why.contains("never guessed")),
        "{none:?}"
    );
    // Not a media type.
    let bad = refused(&bag(README, None), Choices::new().set("contentType", "markdown"));
    assert!(
        bad.iter().any(|(slot, _)| slot.as_deref() == Some("contentType")),
        "{bad:?}"
    );
    // A name the package keeps for itself (EPC §2.2.2 p. 9).
    let rels = refused(
        &bag(README, Some("text/markdown")),
        Choices::new().set("path", "media/_rels/x.md"),
    );
    assert!(
        rels.iter()
            .any(|(slot, why)| slot.as_deref() == Some("path") && why.contains("_rels")),
        "{rels:?}"
    );
    // What the server declared is taken when nothing is said.
    let declared = registry
        .plan(
            "media",
            &bag(README, Some("text/markdown")),
            &Choices::new(),
            &Identity::derived(),
            &when(),
        )
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(declared.media[0].content_type, "text/markdown");
}

#[test]
fn apply_writes_the_part_and_both_relations_and_a_save_keeps_them() {
    let mut store = store();
    let plan = Registry::new()
        .plan(
            "media",
            &bag(&pdf(), None),
            &Choices::new()
                .set("contentType", "application/pdf")
                .set("path", "media/fiche-GTH-1.pdf"),
            &Identity::derived(),
            &when(),
        )
        .unwrap_or_else(|e| panic!("{e}"));
    let made = energyml_build::apply::apply(&plan, &mut store, &when());
    assert!(
        made.invalid.is_empty(),
        "objects written against their schema: {:#?}",
        made.invalid
    );
    assert!(made.refused.is_empty(), "{:?}", made.refused);
    assert_eq!(made.media, ["media/fiche-GTH-1.pdf"]);
    assert!(made.activity.is_some(), "the file's activity is written");

    let again = saved_and_reopened(&store);
    assert_eq!(
        again.read_attachment("media/fiche-GTH-1.pdf").expect("the part"),
        pdf(),
        "the bytes, exactly"
    );
    let index = raw(&again, "[Content_Types].xml");
    assert!(
        index
            .lines()
            .any(|line| line.contains("PartName=\"/media/fiche-GTH-1.pdf\"")
                && line.contains("ContentType=\"application/pdf\"")),
        "{index}"
    );
    assert_related(&again, "media/fiche-GTH-1.pdf");
    assert_eq!(
        again.media_of(FEATURE).expect("a uuid"),
        ["media/fiche-GTH-1.pdf"]
    );
}

#[test]
fn a_part_the_store_refuses_is_named_by_apply_never_skipped() {
    // The object the plan names is not in this package: the part goes in, the
    // relation cannot, and `apply` says so.
    let mut store = energyml_build::epc::open(None, "empty.epc", Arc::new(energyml_schemas::all())).unwrap();
    let plan = Registry::new()
        .plan(
            "media",
            &bag(README, Some("text/markdown")),
            &Choices::new(),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let made = energyml_build::apply::apply(&plan, &mut store, &when());
    assert!(
        made.invalid.is_empty(),
        "objects written against their schema: {:#?}",
        made.invalid
    );
    assert!(
        made.refused.iter().any(|why| why.contains(FEATURE)),
        "{:?}",
        made.refused
    );
}

/// A recipe's files, from memory. `text` for what is text; `bytes` for all.
struct Files;

impl Reader for Files {
    fn text(&mut self, source: &Where) -> Result<(String, Option<String>), String> {
        let (bytes, declared) = self.bytes(source)?;
        String::from_utf8(bytes)
            .map(|text| (text, declared))
            .map_err(|_| format!("{} is not text", source.as_str()))
    }
    fn bytes(&mut self, source: &Where) -> Result<(Vec<u8>, Option<String>), String> {
        let path = source.as_str().replace('\\', "/");
        if path.ends_with("media/README.md") {
            Ok((README.to_vec(), None))
        } else if path.ends_with("media/fiche-GTH-1.pdf") {
            Ok((pdf(), None))
        } else {
            Err(format!("no file {path}"))
        }
    }
    fn package(&mut self, path: &str) -> Result<&EpcStore, String> {
        Err(format!("opening {path}: this test opens no package"))
    }
}

/// Two files, one binary, related to the frame a first step wrote — by the
/// step's name, never by a uuid.
const RECIPE: &str = r#"
titre = "Media"
creation = "2026-09-25T00:00:00Z"
auteur = "tests/media_parts.rs"
workflow = "workflow:media"

[[etape]]
nom = "reperes"
operation = "crs"
set = { epsg = 2154, vertical = 5720 }

[[etape]]
nom = "lisez-moi"
operation = "media"
set = { contentType = "text/markdown" }
entrees = [
  { creneau = "file", fichier = "media/README.md" },
  { creneau = "shows", etape = "reperes", type = "LocalEngineeringCompoundCrs" },
]

[[etape]]
nom = "fiche"
operation = "media"
set = { contentType = "application/pdf" }
entrees = [ { creneau = "file", fichier = "media/fiche-GTH-1.pdf" } ]
"#;

#[test]
fn a_recipe_step_carries_a_file_as_bytes_and_relates_it() {
    let recipe = Recipe::parse(RECIPE).expect("the recipe reads");
    let mut store = energyml_build::epc::open(None, "recipe.epc", Arc::new(energyml_schemas::all())).unwrap();
    let played = recipe::play(
        &recipe,
        Path::new("."),
        &Registry::new(),
        &mut store,
        &mut Files,
        Mode::Write,
    )
    .unwrap_or_else(|stop| panic!("{stop}"));
    assert_eq!(played.steps.len(), 3);

    let compound = store
        .resources()
        .into_iter()
        .find(|one| one.qualified_type.ends_with("LocalEngineeringCompoundCrs"))
        .expect("the first step wrote a compound");
    let again = saved_and_reopened(&store);
    assert_eq!(again.read_attachment("media/README.md").unwrap(), README);
    assert_eq!(
        again.read_attachment("media/fiche-GTH-1.pdf").unwrap(),
        pdf(),
        "a binary file crossed the recipe intact"
    );
    assert_eq!(again.media_of(&compound.uuid).unwrap(), ["media/README.md"]);
    // The PDF shows nothing, and is still a typed part of the package.
    assert!(
        again
            .media_links()
            .iter()
            .all(|link| link.part != "media/fiche-GTH-1.pdf")
    );
    let index = raw(&again, "[Content_Types].xml");
    assert!(
        index.contains("PartName=\"/media/fiche-GTH-1.pdf\" ContentType=\"application/pdf\""),
        "{index}"
    );
    assert!(
        index.contains("PartName=\"/media/README.md\" ContentType=\"text/markdown\""),
        "{index}"
    );
}
