//! The files attached to an object, in every form — `energyml_domain::attachments`.
//!
//! See ADR-0014. The packages are
//! **built in memory** after the specifications, never read from a file:
//!
//! - Energistics Packaging Conventions v1.0 §3.4.2 pp. 15–16: a media part is
//!   tied to an object by `destinationMedia` (form A);
//! - `eml23.Attachment` (`Attachment.xsd`): `Content` is `xsd:base64Binary`
//!   and required — it may be empty; `ContentUri` is *"A URI pointing to the
//!   location of the attached content"*; `ObjectReference` names the object;
//!   `Md` is *"the along-hole measured depth within the RepresentedObject where
//!   the attachment is indexed"* (forms B, C, D);
//! - RFC 4648 §10 for the base 64 test vectors.
//!
//! What would make them fail: an empty `Content` read as an empty file; a form
//! decided on the `Content` only; a part read to list it; an external address
//! followed; an inherited piece listed without its provenance.

use std::io::Cursor;
use std::sync::Arc;

use energyml_core::registry::DynObject;
use energyml_domain::attachments::{
    self, Fetched, Form, Location, attachment_bytes, attachments_of, decode_base64, encode_base64,
};
use energyml_domain::package_index::PackageIndex;
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_2 as resqml22;
use energyml_storage::EpcStore;

fn u(n: u32) -> String {
    format!("00000000-0000-4000-8000-{n:012x}")
}

fn dor(uuid: &str, qualified_type: &str) -> eml23::DataObjectReference {
    eml23::DataObjectReference {
        uuid: uuid.to_string(),
        qualified_type: qualified_type.to_string(),
        title: format!("t-{uuid}"),
        ..Default::default()
    }
}

fn citation(title: &str) -> eml23::Citation {
    eml23::Citation {
        title: title.to_string(),
        ..Default::default()
    }
}

fn store() -> EpcStore {
    EpcStore::from_reader(
        Box::new(Cursor::new(
            energyml_storage::extract::empty_package(None).expect("an empty package"),
        )),
        "attachments.epc",
        Arc::new(energyml_schemas::all()),
    )
    .expect("it opens")
}

fn put(store: &mut EpcStore, object: impl DynObject + 'static) {
    store.put(Box::new(object)).expect("it goes in");
}

fn feature(store: &mut EpcStore, n: u32) {
    put(
        store,
        resqml22::WellboreFeature {
            uuid: u(n),
            schema_version: "2.2".into(),
            citation: citation(&format!("feature {n}")),
            ..Default::default()
        },
    );
}

/// One `Attachment` on `target`.
#[derive(Default)]
struct Spec {
    content: Vec<u8>,
    uri: Option<&'static str>,
    name: Option<&'static str>,
    kind: Option<&'static str>,
    category: Option<&'static str>,
    md: Option<f64>,
}

fn attach(store: &mut EpcStore, n: u32, target: u32, qualified_type: &str, spec: Spec) {
    put(
        store,
        eml23::Attachment {
            uuid: u(n),
            schema_version: "2.3".into(),
            citation: citation(&format!("attachment {n}")),
            object_reference: Some(dor(&u(target), qualified_type)),
            content: encode_base64(&spec.content),
            content_uri: spec.uri.map(str::to_string),
            file_name: spec.name.map(str::to_string),
            file_type: spec.kind.map(str::to_string),
            category: spec.category.map(str::to_string),
            md: spec.md.map(|value| eml23::MeasuredDepth {
                measured_depth: eml23::LengthMeasureExt {
                    value,
                    uom: eml23::LengthUomExt::M,
                },
                datum: None,
            }),
            ..Default::default()
        },
    );
}

fn indexed(store: &EpcStore) -> PackageIndex {
    PackageIndex::build(store, store.revision())
}

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n-not really a picture-";

// -- base 64 ---------------------------------------------------------------------

/// RFC 4648 §10, the seven test vectors.
#[test]
fn base64_gives_the_rfc_4648_vectors() {
    for (plain, coded) in [
        ("", ""),
        ("f", "Zg=="),
        ("fo", "Zm8="),
        ("foo", "Zm9v"),
        ("foob", "Zm9vYg=="),
        ("fooba", "Zm9vYmE="),
        ("foobar", "Zm9vYmFy"),
    ] {
        assert_eq!(encode_base64(plain.as_bytes()), coded);
        assert_eq!(decode_base64(coded).unwrap(), plain.as_bytes());
    }
}

/// `xsd:base64Binary` allows white space between the groups; nothing else
/// outside the alphabet goes, and the padding is the standard's.
#[test]
fn base64_reads_what_the_schema_allows_and_refuses_the_rest() {
    let every: Vec<u8> = (0u8..=255).collect();
    let coded = encode_base64(&every);
    let wrapped: String = coded
        .as_bytes()
        .chunks(76)
        .map(|line| format!("{}\r\n", std::str::from_utf8(line).unwrap()))
        .collect();
    assert_eq!(decode_base64(&wrapped).unwrap(), every);
    assert!(decode_base64("Zm9v!").is_err(), "`!` is not base 64");
    assert!(decode_base64("Zm9").is_err(), "a short group needs its padding");
    assert!(
        decode_base64("Zg==Zg==").is_err(),
        "a character after the padding"
    );
    assert!(decode_base64("Z").is_err(), "one sextet holds no byte");
}

// -- the four forms --------------------------------------------------------------

/// One object, one piece of each form: A by `destinationMedia`, B with its
/// bytes, C by a part name, D by an address.
fn four_forms() -> (EpcStore, PackageIndex) {
    let mut store = store();
    feature(&mut store, 1);
    store
        .set_attachment(
            "media/fiche.pdf",
            b"%PDF-1.4 fiche".to_vec(),
            Some("application/pdf".into()),
        )
        .unwrap();
    store.relate_media(&u(1), "media/fiche.pdf").unwrap();
    store
        .set_attachment(
            "media/symbols/piezometre.png",
            PNG.to_vec(),
            Some("image/png".into()),
        )
        .unwrap();
    let ty = "resqml22.WellboreFeature";
    attach(
        &mut store,
        10,
        1,
        ty,
        Spec {
            content: PNG.to_vec(),
            name: Some("tete.png"),
            kind: Some("image/png"),
            category: Some("HeadImage"),
            md: Some(0.0),
            ..Default::default()
        },
    );
    attach(
        &mut store,
        11,
        1,
        ty,
        Spec {
            uri: Some("media/symbols/piezometre.png"),
            name: Some("symbole.png"),
            kind: Some("image/png"),
            category: Some("Symbol"),
            ..Default::default()
        },
    );
    attach(
        &mut store,
        12,
        1,
        ty,
        Spec {
            uri: Some("https://example.org/fiches/p1.pdf"),
            name: Some("p1.pdf"),
            kind: Some("application/pdf"),
            ..Default::default()
        },
    );
    let index = indexed(&store);
    (store, index)
}

fn piece<'a>(listing: &'a attachments::Listing, key: &str) -> &'a attachments::Piece {
    listing
        .pieces
        .iter()
        .find(|piece| piece.key == key)
        .unwrap_or_else(|| {
            panic!(
                "`{key}` not listed: {:?}",
                listing.pieces.iter().map(|p| &p.key).collect::<Vec<_>>()
            )
        })
}

/// The piece an `Attachment` object makes: its key is the object's **uri**
/// (the version it names), so it is found by the uuid the piece reports.
fn piece_of<'a>(listing: &'a attachments::Listing, uuid: &str) -> &'a attachments::Piece {
    let found = listing.pieces.iter().find(|piece| {
        piece
            .attachment
            .as_deref()
            .is_some_and(|held| held.eq_ignore_ascii_case(uuid))
    });
    found.unwrap_or_else(|| {
        panic!(
            "`{uuid}` not listed: {:?}",
            listing.pieces.iter().map(|p| &p.key).collect::<Vec<_>>()
        )
    })
}

#[test]
fn each_form_is_found_by_its_own_door_with_what_the_object_states() {
    let (store, index) = four_forms();
    let [listing] = attachments_of(&store, &index, &[u(1)])
        .try_into()
        .expect("one object asked, one listing");
    assert_eq!(listing.pieces.len(), 4, "{:#?}", listing.pieces);
    assert!(
        listing
            .pieces
            .iter()
            .all(|piece| piece.provenance.kind == "self" && piece.problems.is_empty()),
        "{:#?}",
        listing.pieces
    );

    // A — a part tied by destinationMedia: no MD, no category, the part's own type and size.
    let a = piece(&listing, "part:media/fiche.pdf");
    assert_eq!(
        (a.form, a.size, a.media_type.as_deref()),
        (Form::Part, Some(14), Some("application/pdf"))
    );
    assert_eq!(a.location, Location::Part("media/fiche.pdf".into()));
    assert!(a.md.is_none() && a.category.is_none() && a.attachment.is_none());

    // B — the bytes are in the XML; Md = 0 is the head of the hole (the datum).
    let b = piece_of(&listing, &u(10));
    assert_eq!(
        (b.form, b.location.clone(), b.size),
        (Form::Inline, Location::Inline, Some(PNG.len() as u64))
    );
    assert_eq!(
        b.md.as_ref().map(|md| (md.value, md.uom.as_str())),
        Some((0.0, "m"))
    );
    assert_eq!(
        (b.category.as_deref(), b.file_name.as_deref()),
        (Some("HeadImage"), Some("tete.png"))
    );

    // C — `Content` empty and the address a part of the package: the address is followed.
    let c = piece_of(&listing, &u(11));
    assert_eq!((c.form, c.size), (Form::PartAddress, Some(PNG.len() as u64)));
    assert_eq!(c.location, Location::Part("media/symbols/piezometre.png".into()));

    // D — an external address, never followed, so no size.
    let d = piece_of(&listing, &u(12));
    assert_eq!((d.form, d.size), (Form::External, None));
    assert_eq!(
        d.location,
        Location::Address("https://example.org/fiches/p1.pdf".into())
    );
}

#[test]
fn bytes_are_read_when_asked_and_an_external_address_is_handed_back_not_followed() {
    let (store, _) = four_forms();
    let bytes = |key: &str| match attachment_bytes(&store, key).unwrap_or_else(|why| panic!("{why}")) {
        Fetched::Bytes {
            bytes,
            media_type,
            file_name,
        } => (bytes, media_type, file_name),
        other => panic!("bytes expected, got {other:?}"),
    };
    assert_eq!(bytes("part:media/fiche.pdf").0, b"%PDF-1.4 fiche");
    let (inline, kind, name) = bytes(&format!("object:{}", u(10)));
    assert_eq!(
        (inline, kind.as_deref(), name.as_deref()),
        (PNG.to_vec(), Some("image/png"), Some("tete.png"))
    );
    assert_eq!(
        bytes(&format!("object:{}", u(11))).0,
        PNG,
        "C reads the part its address names"
    );
    assert_eq!(
        attachment_bytes(&store, &format!("object:{}", u(12))).unwrap(),
        Fetched::External {
            url: "https://example.org/fiches/p1.pdf".into(),
            media_type: Some("application/pdf".into())
        }
    );
    assert!(
        attachment_bytes(&store, "nonsense").is_err(),
        "a key of neither kind is refused by its shape"
    );
}

// -- what does not go ------------------------------------------------------------

/// An empty `Content` is not an empty file when an address is there; with no
/// address it is one, and said. A part that is not there is said. Two sources
/// that differ are said, and the content wins.
#[test]
fn what_does_not_go_is_said_never_silently_listed() {
    let mut store = store();
    feature(&mut store, 1);
    store
        .set_attachment("media/a.png", b"AAAA".to_vec(), Some("image/png".into()))
        .unwrap();
    let ty = "resqml22.WellboreFeature";
    attach(&mut store, 20, 1, ty, Spec::default()); // nothing at all
    attach(
        &mut store,
        21,
        1,
        ty,
        Spec {
            uri: Some("media/absent.png"),
            ..Default::default()
        },
    ); // a part nobody holds
    attach(
        &mut store,
        22,
        1,
        ty,
        Spec {
            content: b"BBBB".to_vec(),
            uri: Some("media/a.png"),
            ..Default::default()
        },
    ); // two sources, two files
    attach(
        &mut store,
        23,
        1,
        ty,
        Spec {
            content: b"AAAA".to_vec(),
            uri: Some("media/a.png"),
            ..Default::default()
        },
    ); // two sources, one file
    attach(
        &mut store,
        24,
        1,
        ty,
        Spec {
            uri: Some("ftp://example.org/x.png"),
            ..Default::default()
        },
    ); // a scheme nobody reads
    let index = indexed(&store);
    let [listing] = attachments_of(&store, &index, &[u(1)]).try_into().unwrap();
    let problems = |n: u32| piece_of(&listing, &u(n)).problems.join(" | ");

    assert!(problems(20).contains("empty file"), "{}", problems(20));
    assert_eq!(piece_of(&listing, &u(20)).size, Some(0));
    assert!(problems(21).contains("media/absent.png"), "{}", problems(21));
    assert!(problems(22).contains("two different files"), "{}", problems(22));
    assert_eq!(
        piece_of(&listing, &u(22)).form,
        Form::Inline,
        "the content is the authority"
    );
    assert!(
        problems(23).is_empty(),
        "the same bytes twice are no disagreement: {}",
        problems(23)
    );
    assert!(problems(24).contains("ftp"), "{}", problems(24));

    assert!(
        attachment_bytes(&store, &format!("object:{}", u(20))).is_err(),
        "nothing to read"
    );
    assert!(
        attachment_bytes(&store, &format!("object:{}", u(21))).is_err(),
        "the part is absent"
    );
    assert!(
        attachment_bytes(&store, &format!("object:{}", u(24))).is_err(),
        "the scheme is not read"
    );
    assert_eq!(
        attachment_bytes(&store, &format!("object:{}", u(22)))
            .map(|f| matches!(f, Fetched::Bytes { bytes, .. } if bytes == b"BBBB")),
        Ok(true),
        "the content wins"
    );
}

// -- who attaches to whom --------------------------------------------------------

/// What an object does not carry it inherits, and the piece says from whom:
/// by a collection (the object a member), by a parent (the feature of its
/// interpretation) — and its own pieces hide both.
#[test]
fn an_inherited_piece_says_where_it_came_from_and_an_own_piece_hides_it() {
    let mut store = store();
    // 1 trajectory -> 2 interpretation -> 3 feature; 4 a folder holding 1.
    feature(&mut store, 3);
    put(
        &mut store,
        resqml22::WellboreInterpretation {
            uuid: u(2),
            schema_version: "2.2".into(),
            citation: citation("interpretation"),
            interpreted_feature: dor(&u(3), "resqml22.WellboreFeature"),
            ..Default::default()
        },
    );
    put(
        &mut store,
        resqml22::WellboreTrajectoryRepresentation {
            uuid: u(1),
            schema_version: "2.2".into(),
            citation: citation("trajectory"),
            represented_object: Some(dor(&u(2), "resqml22.WellboreInterpretation")),
            ..Default::default()
        },
    );
    attach(
        &mut store,
        30,
        3,
        "resqml22.WellboreFeature",
        Spec {
            content: PNG.to_vec(),
            name: Some("feature.png"),
            ..Default::default()
        },
    );
    let by_parent = {
        let index = indexed(&store);
        attachments_of(&store, &index, &[u(1)]).remove(0)
    };
    let [from_parent] = by_parent.pieces.as_slice() else {
        panic!("{by_parent:#?}")
    };
    assert_eq!(from_parent.provenance.kind, "parent");
    assert_eq!(from_parent.provenance.carrier, u(3));
    assert!(
        from_parent
            .provenance
            .says()
            .starts_with("inherited from the parent"),
        "{}",
        from_parent.provenance.says()
    );

    // A folder holding the trajectory, and a symbol on the folder: rank 2 comes
    // before the parents (ADR-0009).
    put(
        &mut store,
        eml23::DataobjectCollection {
            uuid: u(4),
            schema_version: "2.3".into(),
            citation: citation("Piézomètres"),
            kind: Some(eml23::CollectionKindExt::Folder),
            ..Default::default()
        },
    );
    put(
        &mut store,
        eml23::CollectionsToDataobjectsAssociationSet {
            uuid: u(5),
            schema_version: "2.3".into(),
            citation: citation("members"),
            single_collection_association: vec![eml23::SingleCollectionAssociation {
                collection: dor(&u(4), "eml23.DataobjectCollection"),
                dataobject: vec![dor(&u(1), "resqml22.WellboreTrajectoryRepresentation")],
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    attach(
        &mut store,
        31,
        4,
        "eml23.DataobjectCollection",
        Spec {
            content: b"SYMBOL".to_vec(),
            name: Some("symbole.png"),
            category: Some("Symbol"),
            ..Default::default()
        },
    );
    let index = indexed(&store);
    let listing = attachments_of(&store, &index, &[u(1)]).remove(0);
    let [from_collection] = listing.pieces.as_slice() else {
        panic!("{listing:#?}")
    };
    assert_eq!(
        (
            from_collection.provenance.kind,
            from_collection.provenance.carrier.as_str()
        ),
        ("collection", u(4).as_str())
    );
    assert_eq!(
        from_collection.provenance.says(),
        "inherited from the collection Piézomètres"
    );
    assert_eq!(from_collection.category.as_deref(), Some("Symbol"));

    // An own piece: the object lists its own and nothing inherited.
    attach(
        &mut store,
        32,
        1,
        "resqml22.WellboreTrajectoryRepresentation",
        Spec {
            content: b"OWN".to_vec(),
            ..Default::default()
        },
    );
    let index = indexed(&store);
    let listing = attachments_of(&store, &index, &[u(1)]).remove(0);
    let [own] = listing.pieces.as_slice() else {
        panic!("{listing:#?}")
    };
    assert_eq!(
        (own.provenance.kind, own.attachment.as_deref()),
        ("self", Some(u(32).as_str()))
    );

    // The batch says what the unit says: three objects, three answers, in order.
    let batch = attachments_of(&store, &index, &[u(3), u(2), u(1)]);
    assert_eq!(
        batch.iter().map(|l| l.pieces.len()).collect::<Vec<_>>(),
        [1, 1, 1]
    );
    assert_eq!(
        batch.iter().map(|l| l.uuid.clone()).collect::<Vec<_>>(),
        [u(3), u(2), u(1)]
    );
    assert_eq!(
        batch[1].pieces[0].provenance.carrier,
        u(3),
        "the interpretation inherits the feature's piece"
    );
}

/// An object nobody attaches anything to has an empty listing — not an error,
/// not a sentinel.
#[test]
fn nothing_attached_is_an_empty_listing() {
    let mut store = store();
    feature(&mut store, 1);
    let index = indexed(&store);
    let listing = attachments_of(&store, &index, &[u(1)]).remove(0);
    assert!(listing.pieces.is_empty());
}
