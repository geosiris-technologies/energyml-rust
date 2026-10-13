//! `attach` - a file attached to an object, in the four forms, through the one
//! door: `Registry::plan`, then `apply`, then **the reader of the domain** (the
//! writer and the reader are two code paths of one fact, and they are confronted).
//!
//! The form of the assertions comes from the specifications:
//!
//! - Energistics Packaging Conventions v1.0 §3.4.2 pp. 15-16: `destinationMedia`
//!   from the object, `sourceMedia` back - the part form carries no depth and no
//!   category;
//! - `eml23.Attachment` (`Attachment.xsd`): `Content` is required base 64 and may
//!   be empty; `ContentUri` is *"A URI pointing to the location of the attached
//!   content"*; `Md` is *"the along-hole measured depth within the
//!   RepresentedObject where the attachment is indexed"*; `FileName` and
//!   `Category` hold 64 characters; `FileName` is *"NOT a file path"*;
//! - the attachment design for the refusals of the
//!   operation (a release without `Attachment`, a depth on what has none).
//!
//! What would make them fail: a part form that writes its own XML instead of
//! `media`'s plan, a form decided on one field, an empty `Content` with an
//! address that reads as an empty file, a depth accepted on a surface, a name
//! cut instead of refused, a release check that lets 2.0.1 through.

use std::io::Cursor;
use std::sync::Arc;

use energyml_build::operation::{Choices, Identity, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_domain::attachments::{self, Fetched, Form, Location};
use energyml_domain::package_index::PackageIndex;
use energyml_resqml_2_2 as resqml22;
use energyml_storage::{EpcStore, SaveOptions};
use serde_json::{Value, json};

const WELL: &str = "3c9d1e2f-4a5b-4c6d-8e7f-9a0b1c2d3e4f";
const PICTURE: &[u8] = b"\x89PNG\r\n\x1a\n--head image--";

fn when() -> When {
    When {
        creation: "2026-10-02T00:00:00Z".to_string(),
        originator: "tests/attach.rs".to_string(),
    }
}

fn well_ref() -> Ref {
    Ref::new(WELL, "resqml22.WellboreFeature", "GTH-1")
}

fn store() -> EpcStore {
    let mut store = energyml_build::epc::open(None, "attach.epc", Arc::new(energyml_schemas::all()))
        .expect("an empty package");
    store
        .put(Box::new(resqml22::WellboreFeature {
            uuid: WELL.to_string(),
            schema_version: "2.2".to_string(),
            citation: energyml_eml_2_3::Citation {
                title: "GTH-1".to_string(),
                ..Default::default()
            },
            ..Default::default()
        }))
        .expect("the well goes in");
    store
}

fn bag(target: Ref, file: Option<(&[u8], Option<&str>)>) -> Pieces {
    let mut pieces = Pieces::empty().with(
        "target",
        Source::Object(Known {
            reference: Some(target),
            ..Default::default()
        }),
    );
    if let Some((bytes, declared)) = file {
        pieces = pieces.with(
            "file",
            Source::Document {
                name: "tete.png".to_string(),
                content_type: declared.map(str::to_string),
                bytes: bytes.to_vec(),
            },
        );
    }
    pieces
}

fn plan(
    pieces: &Pieces,
    choices: Choices,
) -> Result<energyml_build::operation::Plan, Vec<(Option<String>, String)>> {
    Registry::new()
        .plan("attach", pieces, &choices, &Identity::derived(), &when())
        .map_err(|error| {
            error
                .refusals()
                .iter()
                .map(|one| (one.slot.clone(), one.message.clone()))
                .collect()
        })
}

fn refused(pieces: &Pieces, choices: Choices) -> Vec<(Option<String>, String)> {
    plan(pieces, choices).expect_err("it was accepted")
}

fn said(refusals: &[(Option<String>, String)], slot: &str) -> String {
    refusals
        .iter()
        .find(|(on, _)| on.as_deref() == Some(slot))
        .map(|(_, why)| why.clone())
        .unwrap_or_else(|| panic!("nothing refused on `{slot}`: {refusals:?}"))
}

/// Apply a plan, save and reopen: what a person would have on disk.
fn applied(mut store: EpcStore, plan: &energyml_build::operation::Plan) -> EpcStore {
    let made = energyml_build::apply::apply(plan, &mut store, &when());
    assert!(
        made.invalid.is_empty(),
        "objects written against their schema: {:#?}",
        made.invalid
    );
    assert!(made.refused.is_empty(), "{:?}", made.refused);
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

fn listing(store: &EpcStore) -> attachments::Listing {
    let index = PackageIndex::build(store, store.revision());
    attachments::attachments_of(store, &index, &[WELL]).remove(0)
}

fn json_of(plan: &energyml_build::operation::Plan) -> Value {
    let [object] = plan.objects.as_slice() else {
        panic!(
            "one Attachment, got {:?}",
            plan.objects.iter().map(|o| &o.role).collect::<Vec<_>>()
        )
    };
    assert_eq!(object.qualified_type, "eml23.Attachment");
    object.json.clone()
}

// -- A: the part form is `media`'s plan, byte for byte ------------------------------

#[test]
fn the_part_form_is_what_media_writes_and_nothing_else() {
    let attach = plan(
        &bag(well_ref(), Some((PICTURE, Some("image/png")))),
        Choices::new(),
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    let media = Registry::new()
        .plan(
            "media",
            &Pieces::empty()
                .with(
                    "file",
                    Source::Document {
                        name: "tete.png".into(),
                        content_type: Some("image/png".into()),
                        bytes: PICTURE.to_vec(),
                    },
                )
                .with(
                    "shows",
                    Source::Object(Known {
                        reference: Some(well_ref()),
                        ..Default::default()
                    }),
                ),
            &Choices::new(),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    assert_eq!(
        attach.media, media.media,
        "the file, its type, its relations: `media`'s own plan"
    );
    assert!(attach.objects.is_empty(), "the part form writes no object");
    assert_eq!(
        attach.activity.recipe, "attach",
        "signed with the operation's own name"
    );
    assert_eq!(attach.media[0].path, "media/tete.png");
    assert_eq!(attach.media[0].related_to, [WELL]);

    let again = applied(store(), &attach);
    let found = listing(&again);
    let [piece] = found.pieces.as_slice() else {
        panic!("{found:#?}")
    };
    assert_eq!((piece.form, piece.size), (Form::Part, Some(PICTURE.len() as u64)));
    assert_eq!(piece.media_type.as_deref(), Some("image/png"));
}

// -- B, C, D: an Attachment, read back by the domain's reader -------------------------

#[test]
fn inline_writes_the_bytes_in_the_object_and_the_head_of_the_hole() {
    let choices = Choices::new()
        .set("form", "inline")
        .set("md", 0.0)
        .set("category", "HeadImage");
    let attach = plan(&bag(well_ref(), Some((PICTURE, Some("image/png")))), choices)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert!(attach.media.is_empty(), "inline writes no part");
    let json = json_of(&attach);
    assert_eq!(json["ObjectReference"]["Uuid"], json!(WELL));
    assert_eq!(
        json["Md"]["MeasuredDepth"]["_"],
        json!(0.0),
        "Md = 0: the head of the hole, {json}"
    );
    assert_eq!(json["Md"]["MeasuredDepth"]["uom"], json!("m"));
    assert_eq!(json["Category"], json!("HeadImage"));
    assert_eq!(json["FileType"], json!("image/png"));
    assert_eq!(
        json["Content"],
        json!(energyml_domain::attachments::encode_base64(PICTURE))
    );
    assert!(
        json.get("ContentUri").is_none() || json["ContentUri"].is_null(),
        "no address with inline: {json}"
    );

    let again = applied(store(), &attach);
    let found = listing(&again);
    let [piece] = found.pieces.as_slice() else {
        panic!("{found:#?}")
    };
    assert_eq!((piece.form, &piece.location), (Form::Inline, &Location::Inline));
    assert_eq!(piece.md.as_ref().map(|md| md.value), Some(0.0));
    assert_eq!(piece.category.as_deref(), Some("HeadImage"));
    let Fetched::Bytes { bytes, .. } = attachments::attachment_bytes(&again, &piece.key).unwrap() else {
        panic!("bytes")
    };
    assert_eq!(bytes, PICTURE, "what was attached is what comes back");
}

#[test]
fn part_address_writes_the_part_related_to_the_attachment_that_names_it() {
    let attach = plan(
        &bag(well_ref(), Some((PICTURE, Some("image/png")))),
        Choices::new()
            .set("form", "part-address")
            .set("path", "media/symbols/piezometre.png")
            .set("category", "Symbol"),
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    let [part] = attach.media.as_slice() else {
        panic!("one part: {:?}", attach.media)
    };
    // Packaging Conventions v1.0 §3.4.2 pp. 15-16: destinationMedia - *“The
    // object in Target is a media representation for the current object”* -
    // from the Attachment, not from the target (which would list the file twice).
    assert_eq!(
        part.related_to,
        [attach.objects[0].uuid.to_ascii_lowercase()],
        "related to the Attachment"
    );
    assert!(
        !part.related_to.contains(&WELL.to_string()),
        "and not to the target"
    );
    assert_eq!(part.path, "media/symbols/piezometre.png");
    let json = json_of(&attach);
    assert_eq!(json["ContentUri"], json!("media/symbols/piezometre.png"));
    assert_eq!(json["Content"], json!(""), "empty: the bytes are in the zip");
    assert!(
        attach.says.iter().any(|note| note.key == "convention"),
        "the convention is said: {:?}",
        attach.says
    );

    let again = applied(store(), &attach);
    let found = listing(&again);
    let [piece] = found.pieces.as_slice() else {
        panic!("exactly one piece, not two: {found:#?}")
    };
    assert_eq!(
        (piece.form, &piece.location),
        (
            Form::PartAddress,
            &Location::Part("media/symbols/piezometre.png".into())
        )
    );
    assert!(piece.problems.is_empty(), "{:?}", piece.problems);
    let Fetched::Bytes { bytes, .. } = attachments::attachment_bytes(&again, &piece.key).unwrap() else {
        panic!("bytes")
    };
    assert_eq!(bytes, PICTURE);

    // The relation is in the saved package, from the Attachment (EPC §3.4.2).
    let attachment = attach.objects[0].uuid.to_ascii_lowercase();
    let links = again.media_links();
    assert!(
        links
            .iter()
            .any(|link| link.uuid == attachment && link.part == "media/symbols/piezometre.png"),
        "the Attachment is related to its part: {links:?}"
    );

    // A control that goes red: with the part removed, the same attachment
    // reads as a part that is absent, and its bytes are refused by name.
    let mut broken = again;
    broken
        .remove_attachment("media/symbols/piezometre.png")
        .expect("the part goes");
    let found = listing(&broken);
    let [piece] = found.pieces.as_slice() else {
        panic!("{found:#?}")
    };
    assert!(
        piece.problems.iter().any(|why| why.contains("no such part")),
        "{:?}",
        piece.problems
    );
    assert!(
        attachments::attachment_bytes(&broken, &piece.key).is_err(),
        "no bytes without the part"
    );
}

#[test]
fn external_names_an_address_and_fetches_nothing() {
    let pieces = bag(well_ref(), None);
    let attach = plan(
        &pieces,
        Choices::new()
            .set("address", "https://example.org/fiches/p1.pdf?v=2")
            .set("contentType", "application/pdf"),
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    assert!(attach.media.is_empty());
    let json = json_of(&attach);
    assert_eq!(json["ContentUri"], json!("https://example.org/fiches/p1.pdf?v=2"));
    assert_eq!(
        json["FileName"],
        json!("p1.pdf"),
        "the name the address names, query cut"
    );

    let again = applied(store(), &attach);
    let found = listing(&again);
    let [piece] = found.pieces.as_slice() else {
        panic!("{found:#?}")
    };
    assert_eq!(piece.form, Form::External);
    assert!(
        piece.problems.is_empty(),
        "an empty Content with an address is no empty file: {:?}",
        piece.problems
    );
    assert!(matches!(
        attachments::attachment_bytes(&again, &piece.key),
        Ok(Fetched::External { .. })
    ));
}

#[test]
fn the_same_inputs_are_the_same_attachment_and_other_bytes_another() {
    let choices = || Choices::new().set("form", "inline");
    let uuid = |bytes: &[u8]| {
        plan(&bag(well_ref(), Some((bytes, Some("image/png")))), choices())
            .unwrap()
            .objects[0]
            .uuid
            .clone()
    };
    assert_eq!(uuid(PICTURE), uuid(PICTURE));
    assert_ne!(
        uuid(PICTURE),
        uuid(b"another picture"),
        "a new file under the same name is a new attachment"
    );
}

// -- what is refused, on its slot -----------------------------------------------------

#[test]
fn what_the_inputs_cannot_make_is_refused_on_the_slot_that_says_why() {
    let file = bag(well_ref(), Some((PICTURE, Some("image/png"))));
    let nothing = bag(well_ref(), None);

    assert!(said(&refused(&nothing, Choices::new()), "file").contains("nothing to attach"));
    assert!(
        said(
            &refused(&file, Choices::new().set("address", "https://example.org/x.png")),
            "address"
        )
        .contains("both")
    );
    assert!(said(&refused(&file, Choices::new().set("form", "external")), "form").contains("address"));
    assert!(
        said(
            &refused(
                &nothing,
                Choices::new()
                    .set("address", "https://example.org/x.png")
                    .set("form", "inline")
            ),
            "form"
        )
        .contains("file")
    );
    // The scheme: only http and https are read.
    assert!(
        said(
            &refused(&nothing, Choices::new().set("address", "ftp://example.org/x.png")),
            "address"
        )
        .contains("ftp")
    );
    assert!(
        said(
            &refused(
                &nothing,
                Choices::new().set("address", "https://example.org/a b.png")
            ),
            "address"
        )
        .contains("space")
    );
    // A type is stated, never guessed - for every form that carries a file.
    for form in ["part", "inline", "part-address"] {
        let undeclared = bag(well_ref(), Some((PICTURE, None)));
        assert!(
            said(
                &refused(&undeclared, Choices::new().set("form", form)),
                "contentType"
            )
            .contains("never guessed"),
            "{form}"
        );
    }
    // A name is refused, not cut; and it is a name, not a path.
    let long = "n".repeat(65);
    assert!(
        said(
            &refused(
                &file,
                Choices::new()
                    .set("form", "inline")
                    .set("fileName", long.as_str())
            ),
            "fileName"
        )
        .contains("64")
    );
    assert!(
        said(
            &refused(
                &file,
                Choices::new()
                    .set("form", "inline")
                    .set("category", long.as_str())
            ),
            "category"
        )
        .contains("64")
    );
    assert!(
        said(
            &refused(
                &file,
                Choices::new()
                    .set("form", "inline")
                    .set("fileName", "media/tete.png")
            ),
            "fileName"
        )
        .contains("path")
    );
    // A depth has a unit of length.
    assert!(
        said(
            &refused(
                &file,
                Choices::new()
                    .set("form", "inline")
                    .set("md", 0.0)
                    .set("mdUom", "kg")
            ),
            "mdUom"
        )
        .contains("kg")
    );
}

#[test]
fn a_part_has_no_depth_and_no_category() {
    let file = bag(well_ref(), Some((PICTURE, Some("image/png"))));
    let md = refused(&file, Choices::new().set("md", 0.0));
    assert!(
        said(&md, "md").contains("inline"),
        "the forms that carry one are named: {md:?}"
    );
    assert!(
        said(
            &refused(&file, Choices::new().set("category", "Symbol")),
            "category"
        )
        .contains("carries neither")
    );
}

#[test]
fn a_depth_goes_on_what_has_one() {
    let surface = Ref::new(
        "11111111-2222-4333-8444-555555555555",
        "resqml22.TriangulatedSetRepresentation",
        "Horizon",
    );
    let why = refused(
        &bag(surface, Some((PICTURE, Some("image/png")))),
        Choices::new().set("form", "inline").set("md", 10.0),
    );
    assert!(said(&why, "md").contains("no measured depth"), "{why:?}");
    // A collection may hold wells: md = 0 on it is the head of the hole of each.
    let folder = Ref::new(
        "11111111-2222-4333-8444-666666666666",
        "eml23.DataobjectCollection",
        "Piezometers",
    );
    let made = plan(
        &bag(folder, Some((PICTURE, Some("image/png")))),
        Choices::new().set("form", "inline").set("md", 0.0),
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    assert!(made.says.iter().any(|note| note.key == "collection"));
    // And a trajectory takes one, as does a log.
    for kind in [
        "resqml22.WellboreTrajectoryRepresentation",
        "resqml22.WellboreFrameRepresentation",
        "witsml21.Log",
    ] {
        let target = Ref::new(WELL, kind, "x");
        plan(
            &bag(target, Some((PICTURE, Some("image/png")))),
            Choices::new().set("form", "inline").set("md", 0.0),
        )
        .unwrap_or_else(|e| panic!("{kind}: {e:?}"));
    }
}

/// A RESQML 2.0.1 package has no `Attachment` object: B, C and D are refused
/// by name and the part form is offered - which still works there.
#[test]
fn a_release_without_attachment_is_refused_by_name_and_the_part_form_is_offered() {
    let old = Ref::new(
        "11111111-2222-4333-8444-777777777777",
        "resqml20.obj_TriangulatedSetRepresentation",
        "Horizon 2.0.1",
    );
    let file = bag(old.clone(), Some((PICTURE, Some("image/png"))));
    for form in ["inline", "part-address"] {
        let why = refused(&file, Choices::new().set("form", form));
        let message = said(&why, "form");
        assert!(
            message.contains("no `Attachment`") && message.contains("`part`"),
            "{form}: {message}"
        );
    }
    let address = bag(old.clone(), None);
    assert!(
        said(
            &refused(
                &address,
                Choices::new().set("address", "https://example.org/x.png")
            ),
            "form"
        )
        .contains("no `Attachment`")
    );
    // The part form needs no Attachment.
    let made = plan(&file, Choices::new()).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(made.media[0].related_to, ["11111111-2222-4333-8444-777777777777"]);
    // And 2.2, 2.3 take them all.
    plan(
        &bag(well_ref(), Some((PICTURE, Some("image/png")))),
        Choices::new().set("form", "inline"),
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
}

/// The files view hands a part over **by its part name**: it is related, not
/// written a second time under `media/` + its name.
#[test]
fn a_document_named_with_a_folder_is_a_part_already_and_keeps_its_name() {
    let pieces = Pieces::empty()
        .with(
            "target",
            Source::Object(Known {
                reference: Some(well_ref()),
                ..Default::default()
            }),
        )
        .with(
            "file",
            Source::Document {
                name: "media/symbols/piezometre.png".into(),
                content_type: Some("image/png".into()),
                bytes: PICTURE.to_vec(),
            },
        );
    let made = plan(&pieces, Choices::new()).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        made.media
            .iter()
            .map(|part| part.path.as_str())
            .collect::<Vec<_>>(),
        ["media/symbols/piezometre.png"]
    );
    assert_eq!(made.media[0].related_to, [WELL]);
    // Inline: the FileName is a name, not the path.
    let inline = plan(&pieces, Choices::new().set("form", "inline")).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(json_of(&inline)["FileName"], json!("piezometre.png"));
    // An explicit path still wins.
    let chosen = plan(&pieces, Choices::new().set("path", "media/autre.png")).unwrap();
    assert_eq!(chosen.media[0].path, "media/autre.png");
}

/// Lutetia's recipe names `attach`, and
/// the recipe is **valid against the operation's own declaration** - its step
/// names, the *names* of its settings, its entries (not their values: only
/// playing it judges those) - without playing it (playing needs HDF5,
/// `tests/lutetia.rs`). The recipe is a versioned file of the repository, like
/// the one `lutetia.rs` reads.
#[test]
fn lutetias_attach_steps_are_valid_against_the_operation_and_the_project_gathers_them() {
    let Some(base) = crate::lutetia_package::recipe_dir() else {
        return;
    };
    let text = std::fs::read_to_string(base.join("recette.toml")).expect("the recipe is versioned");
    let recipe = energyml_build::recipe::Recipe::parse(&text).unwrap_or_else(|stop| panic!("{stop}"));
    recipe
        .check(&Registry::new())
        .unwrap_or_else(|stop| panic!("{stop}"));
    for step in ["puits-verticaux", "tete-puits-verticaux", "tete-gth-1"] {
        assert!(
            text.contains(&format!("{{ creneau = \"members\", etape = \"{step}\" }}")),
            "the project gathers `{step}`: an object outside it is a generator's omission"
        );
    }
    // The two pictograms are bytes of the repository, and PNGs.
    for file in ["puits-vertical.png", "doublet.png"] {
        let bytes =
            std::fs::read(base.join("donnees/media/symbols").join(file)).expect("the pictogram is versioned");
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "{file} is a PNG");
    }
}
