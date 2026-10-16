//! Lutetia, played and judged - the final step of the reference model's build:
//! a test of the repository makes it and judges it.
//!
//! The recipe is Lutetia's `recette.toml` and its versioned inputs; the
//! package is made in a temporary folder, never kept. Three things are asserted,
//! and none of them is a number measured on the package:
//!
//! - **`check` finds what the `[ERROR]` model plants, and nothing else** - no
//!   error, and exactly the nine warnings that the planted faults make but one
//!   (the one that waits for a later fix), each on a member of the `[ERROR] Lutetia`
//!   collection. The judge is `energyml_domain::check`, code apart from
//!   the operations that wrote the package, reading it back from the saved
//!   files. Every finding of this clean model is one of those nine;
//! - **the wells' collection holds the wells, all of them and only them** - its
//!   association set's references against the package's own list of
//!   `WellboreInterpretation` parts, two sources for one fact. *“the
//!   DataobjectCollection must only contain WellboreInterpretation”* (RESQML
//!   TRG 2.2 §6.19 p. 187) is the form; which wells, the recipe's;
//! - **the project holds every object of the package** (the model's rule:
//!   *“each own object is in it - an object outside the project is an oversight of the
//!   generator”*) - except what is named: the project, its own association
//!   set and its own step's activity (written after it was planned), and the
//!   workflow's root activity and template, written after the last step (the
//!   known gap of the model). When the gap closes, this fails, and the
//!   exception goes with it. The `[ERROR]` model is kept **out** of the clean
//!   project and gathered by its own collection, the same way. Its first run found the project short of every
//!   template and every grouping collection `apply` writes beside a plan.
//!
//! Needs HDF5: the recipe writes arrays, and later steps read them back.

#![cfg(feature = "hdf5")]

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use serde_json::Value;

use energyml_build::entries::Reader;
use energyml_build::operation::Registry;
use energyml_build::recipe::{self, Recipe};
use energyml_build::source::Source as Where;
use energyml_storage::EpcStore;

use crate::lutetia_package::{self, Variant};

/// The recipe's files, from the disk.
struct Disk;

impl Reader for Disk {
    fn text(&mut self, source: &Where) -> Result<(String, Option<String>), String> {
        match source {
            Where::File(path) => std::fs::read_to_string(path)
                .map(|text| (text, None))
                .map_err(|why| format!("reading {path}: {why}")),
            Where::Url(url) => Err(format!("{url}: this test reads no network")),
        }
    }
    /// Bytes, for the files that are not text - the pictograms of `attach`
    ///. Without it the default goes
    /// through `text`, which refuses a PNG.
    fn bytes(&mut self, source: &Where) -> Result<(Vec<u8>, Option<String>), String> {
        match source {
            Where::File(path) => std::fs::read(path)
                .map(|bytes| (bytes, None))
                .map_err(|why| format!("reading {path}: {why}")),
            Where::Url(url) => Err(format!("{url}: this test reads no network")),
        }
    }
    fn package(&mut self, path: &str) -> Result<&EpcStore, String> {
        Err(format!("opening {path}: the recipe names no package"))
    }
}

fn read_back(store: &EpcStore, uuid: &str) -> Value {
    let summary = store
        .find_by_uuid(uuid)
        .unwrap_or_else(|| panic!("{uuid} is in the package"))
        .clone();
    let object = store.get_object(&summary.uri).expect("it reads");
    energyml_core::json::write_value(object.as_reflect())
}

/// The uuids a collection holds, through the association set that names it.
fn members_of(store: &EpcStore, collection: &str) -> BTreeSet<String> {
    let mut members = BTreeSet::new();
    let mut sets = 0usize;
    for summary in store
        .resources()
        .into_iter()
        .filter(|one| one.qualified_type == "eml23.CollectionsToDataobjectsAssociationSet")
    {
        let set = read_back(store, &summary.uuid);
        for association in set["SingleCollectionAssociation"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if association["Collection"]["Uuid"].as_str() == Some(collection) {
                sets += 1;
                for member in association["Dataobject"].as_array().into_iter().flatten() {
                    members.insert(
                        member["Uuid"]
                            .as_str()
                            .expect("a member names a uuid")
                            .to_string(),
                    );
                }
            }
        }
    }
    assert_eq!(sets, 1, "one association names collection {collection}");
    members
}

#[test]
fn lutetia_is_played_clean_and_its_collections_hold_what_the_package_holds() {
    let Some(base) = lutetia_package::recipe_dir() else {
        return;
    };
    let text = std::fs::read_to_string(base.join("recette.toml")).expect("the recipe is versioned");
    let recipe = Recipe::parse(&text).unwrap_or_else(|stop| panic!("{stop}"));
    let dir = std::env::temp_dir().join(format!("energyml-build-lutetia-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("Lutetia.epc");
    let schemas = Arc::new(energyml_schemas::all());
    let played = recipe::run(&recipe, &base, &Registry::new(), schemas.clone(), &out, &mut Disk)
        .unwrap_or_else(|stop| panic!("{stop}"));
    assert!(
        played.steps.iter().any(|step| step.held.is_none()),
        "something was played"
    );

    let store = EpcStore::open(&out, schemas).expect("the package opens");
    let report = energyml_domain::check::of_store(
        &store,
        &energyml_domain::check::Options {
            schema: true,
            rules: true,
            extra_rules: Vec::new(),
        },
    );
    assert!(report.read > 0, "objects were read");
    let all = (
        &report.schema,
        &report.rules,
        &report.references,
        &report.relationships,
        &report.diagnostics,
        &report.counts,
    );
    // The eight faults weighed as errors:
    // E-01/E-02, E-03, E-06, E-07, E-08 twice, E-09 - so `check` fails, as the error model
    // expects of a package that plants them. Nothing else is an error.
    assert_eq!(report.errors(), 8, "{all:#?}");
    assert!(
        report.schema.is_empty() && report.references.iter().all(|one| !one.is_error()),
        "{all:#?}"
    );

    let resources = store.resources();
    let titled = |title: &str| {
        resources
            .iter()
            .find(|one| {
                one.qualified_type == "eml23.DataobjectCollection"
                    && read_back(&store, &one.uuid)["Citation"]["Title"].as_str() == Some(title)
            })
            .unwrap_or_else(|| panic!("a collection titled {title}"))
            .uuid
            .clone()
    };

    // The `[ERROR]` model: each planted fault found, on an `[ERROR]`
    // object, and nothing else found anywhere.
    let error_folder = titled("[ERROR] Lutetia");
    let planted = members_of(&store, &error_folder);
    let mut rules: Vec<&str> = report
        .rules
        .iter()
        .filter(|one| one.read.is_some())
        .map(|one| one.rule.as_str())
        .collect();
    rules.sort_unstable();
    assert_eq!(
        rules,
        [
            "house/discrete-value-in-lookup",
            "house/time-series-asset-resolves",
            "trg/contact-element-is-geological",
            "trg/homogeneous-collection",
            "trg/is-listric-iff-normal",
            "trg/one-definitive-trajectory-per-wellbore",
            "trg/wellbore-interpretation-set-holds-wellbores",
        ],
        "{all:#?}"
    );
    let kinds: Vec<&String> = report
        .properties
        .iter()
        .filter(|one| one.is_a_defect)
        .map(|one| &one.uuid)
        .collect();
    assert_eq!(kinds.len(), 2, "the two abstract kinds: {:#?}", report.properties);
    assert!(
        report
            .properties
            .iter()
            .filter(|one| one.is_a_defect)
            .all(|one| one.is_error),
        "an abstract kind is an error"
    );
    // The unresolved asset alone stays a warning: an asset outside the package may be resolved
    // elsewhere.
    let weighed = |gravity| {
        report
            .rules
            .iter()
            .filter(|one| one.read.is_some() && one.gravity == gravity)
            .map(|one| one.rule.as_str())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        weighed(energyml_domain::rules::Gravity::Warning),
        ["house/time-series-asset-resolves"],
        "{all:#?}"
    );
    assert_eq!(
        report.warnings(),
        1,
        "the unresolved asset, and nothing from the clean model: {all:#?}"
    );
    for uuid in report
        .rules
        .iter()
        .filter(|one| one.read.is_some())
        .map(|one| &one.uuid)
        .chain(kinds)
    {
        assert!(
            planted.contains(uuid),
            "{uuid} is found but is no member of the [ERROR] collection"
        );
    }

    // The wells' collection, against the package's own wellbore interpretations
    // - the clean ones: the `[ERROR]` well is planted outside it.
    let wells = members_of(&store, &titled("Lutetia - model wells"));
    let interpretations: BTreeSet<String> = resources
        .iter()
        .filter(|one| one.qualified_type == "resqml22.WellboreInterpretation" && !planted.contains(&one.uuid))
        .map(|one| one.uuid.clone())
        .collect();
    assert!(!interpretations.is_empty());
    assert_eq!(wells, interpretations);

    // The project, against every object of the package, the named gap apart.
    let project = titled("Lutetia");
    let members = members_of(&store, &project);
    // A collection, its own association set and its own step's activity -
    // written after it was planned, it cannot name itself among what it gathers.
    let itself = |collection: &str| -> Vec<String> {
        let sets = resources
            .iter()
            .filter(|one| one.qualified_type == "eml23.CollectionsToDataobjectsAssociationSet")
            .filter(|one| {
                read_back(&store, &one.uuid)["SingleCollectionAssociation"][0]["Collection"]["Uuid"].as_str()
                    == Some(collection)
            })
            .map(|one| one.uuid.clone());
        let activity = played
            .steps
            .iter()
            .find(|step| step.objects.iter().any(|one| one.uuid == collection))
            .and_then(|step| step.activity.as_ref())
            .map(|activity| activity.uuid.clone());
        std::iter::once(collection.to_string())
            .chain(sets)
            .chain(activity)
            .collect()
    };
    let apart: BTreeSet<String> = [
        played.workflow.activity.uuid.clone(),
        played.workflow.template.uuid.clone(),
    ]
    .into_iter()
    .chain(itself(&project))
    .chain(itself(&error_folder))
    .chain(planted.iter().cloned())
    .collect();
    let everything: BTreeSet<String> = resources.iter().map(|one| one.uuid.clone()).collect();
    let outside: Vec<String> = everything
        .difference(&members)
        .filter(|uuid| !apart.contains(*uuid))
        .map(|uuid| {
            let one = resources.iter().find(|r| &r.uuid == uuid).unwrap();
            format!(
                "{} ({})",
                read_back(&store, uuid)["Citation"]["Title"],
                one.qualified_type
            )
        })
        .collect();
    assert!(outside.is_empty(), "objects the project forgot: {outside:?}");
    let strangers: Vec<&String> = members.difference(&everything).collect();
    assert!(
        strangers.is_empty(),
        "the project names objects the package does not hold: {strangers:?}"
    );

    // The analysis names the period it analyses by uid (`PrincipalTestPeriodRef`),
    // and the recipe **tells** it: an object in the bag is a reference, not
    // its content (`ops/pta.rs`). The second source is the test itself, read
    // back from the package - every period named must be one it holds.
    let mut analyses = 0usize;
    for one in resources
        .iter()
        .filter(|one| one.qualified_type == "prodml23.PressureTransientAnalysis")
    {
        analyses += 1;
        let analysis = read_back(&store, &one.uuid);
        let test = read_back(
            &store,
            analysis["FlowTestActivity"]["Uuid"]
                .as_str()
                .expect("the analysis names its test"),
        );
        let mut periods = BTreeSet::new();
        let mut stack = vec![&test];
        while let Some(value) = stack.pop() {
            match value {
                Value::Object(map) => {
                    if map.get("$type").and_then(Value::as_str) == Some("prodml23.TestPeriod") {
                        periods.extend(map.get("uid").and_then(Value::as_str).map(str::to_string));
                    }
                    stack.extend(map.values());
                }
                Value::Array(all) => stack.extend(all),
                _ => {}
            }
        }
        for named in analysis["PrincipalTestPeriodRef"]
            .as_array()
            .expect("1..n periods")
        {
            let named = named.as_str().expect("a uid");
            assert!(
                periods.contains(named),
                "`{}` analyses period `{named}`, and its test holds {periods:?}",
                analysis["Citation"]["Title"]
            );
        }
    }
    assert_eq!(analyses, 1, "the one analysis of the recipe");

    // The head images are **parts of the zip**, not text in the XML
    // (a reported need: no image file was visible in the epc).
    // Three sources for one fact: the package's list of files, its media
    // relations (Packaging Conventions v1.0 §3.4.2 pp. 15-16: the Attachment is
    // related to its part by `destinationMedia`), and the reader of the domain
    // that follows an inherited attachment down to its bytes.
    const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n"; // RFC 2083 §3.1
    let files = energyml_domain::attachments::FileSource::files(&store);
    let links = store.media_links();
    let index = energyml_domain::package_index::PackageIndex::build(&store, store.revision());
    for (part, carrier) in [
        (
            "media/symbols/puits-vertical.png",
            Some("Lutetia - vertical wells"),
        ),
        ("media/symbols/doublet.png", None),
    ] {
        let listed = files
            .iter()
            .find(|(name, _, _)| name == part)
            .unwrap_or_else(|| panic!("`{part}` is a part of the zip: {files:?}"));
        assert_eq!(
            listed.2.as_deref(),
            Some("image/png"),
            "`{part}` is typed in [Content_Types].xml"
        );
        let related: Vec<&String> = links
            .iter()
            .filter(|link| link.part == part)
            .map(|link| &link.uuid)
            .collect();
        let [attachment] = related.as_slice() else {
            panic!("`{part}` is related to exactly one object: {related:?}")
        };
        let json = read_back(&store, attachment);
        assert_eq!(
            json["$type"], "eml23.Attachment",
            "the part is a media representation of an Attachment"
        );
        assert_eq!(json["ContentUri"], part);
        assert_eq!(json["Category"], "HeadImage");
        assert_eq!(json["Content"], "", "the bytes are in the zip, not in the XML");
        let bytes = store.raw_part(part).expect("the part reads");
        assert_eq!(&bytes[..8], PNG_SIGNATURE, "`{part}` is a PNG");
        // The reader: a member of the vertical wells' collection finds the
        // image by inheritance, with its bytes.
        if let Some(title) = carrier {
            let member = members_of(&store, &titled(title))
                .into_iter()
                .next()
                .expect("the collection holds wells");
            let found =
                energyml_domain::attachments::attachments_of(&store, &index, &[member.as_str()]).remove(0);
            let piece = found
                .pieces
                .iter()
                .find(|piece| piece.attachment.as_deref() == Some(attachment.as_str()))
                .unwrap_or_else(|| panic!("a vertical well inherits the head image: {found:#?}"));
            assert_eq!(piece.provenance.kind, "collection");
            let energyml_domain::attachments::Fetched::Bytes { bytes: read, .. } =
                energyml_domain::attachments::attachment_bytes(&store, &piece.key)
                    .expect("the bytes of the piece")
            else {
                panic!("bytes")
            };
            assert_eq!(read, bytes, "the piece returns the part's own bytes");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// **The example the front end opens is this recipe, played.**
///
/// The need: Lutetia is meant to be fixed, changing only when the code does, so its epc
/// and h5 are versioned (`package-with-errors/` of `energyml-lutetia`, or
/// `web/public/examples/lutetia/` in the monorepo) and a viewer would only open
/// them rather than rebuild them. This is their `generate --check`: the recipe
/// played again must give the same package. When the code changes what the
/// recipe writes, this fails, and the message says which replay brings the
/// example up to date ([`lutetia_package::refresh`]).
///
/// How the two are compared - byte for byte on the platform that wrote the
/// example, up to float noise on another - is [`confront`]'s.
#[test]
fn the_example_the_page_opens_is_this_recipe_played() {
    let Some((recipe, base)) = recipe_and_base() else {
        return;
    };
    let Some(kept) = lutetia_package::package(Variant::WithErrors) else {
        return;
    };
    confront(&recipe, &base, &kept, "full");
}

/// **And the example without its errors** - a second one is needed to open lutetia
/// without the objects in error. The same
/// recipe played without the group `erreur` (`Recipe::without`), confronted the
/// same way - and **clean**: no `[ERROR]` object, and `check` finds nothing,
/// error or warning. Every finding of the full model is on an `[ERROR]` object
/// (the test above), so a finding here is a fault of the clean model itself.
#[test]
fn the_example_without_errors_is_the_recipe_without_its_error_model() {
    let Some((recipe, base)) = recipe_and_base() else {
        return;
    };
    let Some(kept) = lutetia_package::package(Variant::Clean) else {
        return;
    };
    let clean = recipe
        .without(&["erreur"])
        .expect("the recipe has an `erreur` group");
    let replayed = confront(&clean, &base, &kept, "clean");

    let store = EpcStore::open(&replayed, Arc::new(energyml_schemas::all())).expect("the package opens");
    let erroneous: Vec<String> = store
        .resources()
        .iter()
        .filter_map(|one| {
            read_back(&store, &one.uuid)["Citation"]["Title"]
                .as_str()
                .map(str::to_string)
        })
        .filter(|title| title.contains("[ERROR]"))
        .collect();
    assert!(erroneous.is_empty(), "objects of the error model: {erroneous:?}");
    let report = energyml_domain::check::of_store(
        &store,
        &energyml_domain::check::Options {
            schema: true,
            rules: true,
            extra_rules: Vec::new(),
        },
    );
    assert!(report.read > 0, "objects were read");
    let all = (
        &report.schema,
        &report.rules,
        &report.references,
        &report.relationships,
        &report.diagnostics,
        &report.properties,
    );
    assert_eq!(report.errors(), 0, "{all:#?}");
    assert_eq!(report.warnings(), 0, "{all:#?}");
}

/// Two numbers agree up to the last bits of a float: a relative 1e-8, taken
/// against 1 near zero. Measured on 28/09 between Windows and Linux (glibc)
/// over the whole package: 1.5e-11 at worst in the XML, 1.2e-10 in the arrays.
/// Two NaN agree - an absent value on both sides.
fn close(a: f64, b: f64) -> bool {
    (a.is_nan() && b.is_nan()) || (a - b).abs() <= 1e-8 * 1f64.max(a.abs()).max(b.abs())
}

/// **The same package, up to the last bits of a float.** Found on the CI's
/// first run of this test (28/09): the reprojection of `passes-etages.csv`
/// wrote `6861791.310959258` on Windows and `…259` on Linux - `sin`, `cos`,
/// `exp` are the platform's C library, and two libraries may differ by an ulp.
/// Byte for byte is then out of reach, and the uuids with it: an identity
/// derived from a position follows its last bit, and so does every object that
/// names it (2 144 parts of 2 491 differed, all by this).
///
/// So an object is found by **what it is** - its type, its title and its
/// *skeleton*, the text with every uuid and every number made a token - and the
/// numbers of the two are compared with [`close`]. What differs in the
/// skeleton, an object on one side only, a number beyond the noise: each is a
/// finding, and none is forgiven. The one number set apart is an activity's
/// `byteCount`, the size of a text file whose floats are written at their
/// shortest: it follows the noise by construction.
///
/// `.rels` and `[Content_Types].xml` are left out - both list parts by names
/// that carry uuids, so their order follows the same noise, and both are
/// rebuilt from the objects compared here.
struct FloatNoise {
    /// What differs beyond the noise: empty when the packages agree.
    findings: Vec<String>,
    /// Each uuid of the versioned package, to the uuid of the same object in
    /// the replay.
    twins: std::collections::HashMap<String, String>,
}

impl FloatNoise {
    fn between(ours: &[u8], again: &[u8]) -> FloatNoise {
        type Key = (String, String, String);
        struct Part {
            uuid: Option<String>,
            numbers: Vec<f64>,
        }
        fn parts(
            bytes: &[u8],
        ) -> (
            std::collections::BTreeMap<Key, Vec<Part>>,
            std::collections::BTreeMap<String, Vec<u8>>,
        ) {
            use std::io::Read;
            let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("a zip");
            let (mut objects, mut binary) = (
                std::collections::BTreeMap::<Key, Vec<Part>>::new(),
                std::collections::BTreeMap::new(),
            );
            for i in 0..archive.len() {
                let mut entry = archive.by_index(i).expect("an entry");
                let name = entry.name().expect("a part name").into_owned();
                let mut data = Vec::new();
                entry.read_to_end(&mut data).expect("it inflates");
                if name.contains("_rels/") || name == "[Content_Types].xml" {
                    continue;
                }
                let Ok(text) = String::from_utf8(data.clone()) else {
                    binary.insert(name, data);
                    continue;
                };
                let file = name.rsplit('/').next().unwrap_or(&name);
                let uuid = file
                    .strip_suffix(".xml")
                    .and_then(|stem| stem.rsplit('_').next())
                    .filter(|tail| is_uuid(tail.as_bytes()))
                    .map(str::to_string);
                // An object is keyed by what it is; any other part by its name,
                // which carries no uuid.
                let (kind, title) = match &uuid {
                    Some(_) => (
                        file.split('_').next().unwrap_or_default().to_string(),
                        between_tags(&text, "<eml:Title>", "</eml:Title>")
                            .unwrap_or_default()
                            .to_string(),
                    ),
                    None => (String::new(), name.clone()),
                };
                let (skeleton, numbers) = tokens(&without_byte_count(&text));
                objects
                    .entry((kind, title, skeleton))
                    .or_default()
                    .push(Part { uuid, numbers });
            }
            (objects, binary)
        }
        let ((mut mine, my_binary), (mut theirs, their_binary)) = (parts(ours), parts(again));
        let (mut findings, mut twins) = (Vec::new(), std::collections::HashMap::new());
        let named = |key: &Key| {
            if key.0.is_empty() {
                key.1.clone()
            } else {
                format!("{} `{}`", key.0, key.1)
            }
        };
        for (key, list) in &mut mine {
            let Some(other) = theirs.remove(key) else {
                findings.push(format!("{} only in the example, or its text differs", named(key)));
                continue;
            };
            if list.len() != other.len() {
                findings.push(format!(
                    "{}: {} in the example, {} in the replay",
                    named(key),
                    list.len(),
                    other.len()
                ));
                continue;
            }
            // Twins of one key are told apart by their numbers.
            let order = |a: &Part, b: &Part| {
                a.numbers
                    .iter()
                    .zip(&b.numbers)
                    .map(|(x, y)| x.total_cmp(y))
                    .find(|o| o.is_ne())
                    .unwrap_or(std::cmp::Ordering::Equal)
            };
            let mut other = other;
            list.sort_by(order);
            other.sort_by(order);
            for (a, b) in list.iter().zip(&other) {
                if let Some((x, y)) = a.numbers.iter().zip(&b.numbers).find(|(x, y)| !close(**x, **y)) {
                    findings.push(format!("{}: {x} against {y}", named(key)));
                }
                if let (Some(a), Some(b)) = (&a.uuid, &b.uuid) {
                    twins.insert(a.clone(), b.clone());
                }
            }
        }
        findings.extend(
            theirs
                .keys()
                .map(|key| format!("{} only in the replay, or its text differs", named(key))),
        );
        if my_binary != their_binary {
            findings.push(format!(
                "binary parts differ: {:?} against {:?}",
                my_binary.keys().collect::<Vec<_>>(),
                their_binary.keys().collect::<Vec<_>>()
            ));
        }
        findings.truncate(10);
        FloatNoise { findings, twins }
    }
}

/// Whether 36 bytes spell a uuid, lowercase and hyphenated.
fn is_uuid(bytes: &[u8]) -> bool {
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                *b == b'-'
            } else {
                b.is_ascii_digit() || (b'a'..=b'f').contains(b)
            }
        })
}

/// The text between the first `open` and the `close` after it.
fn between_tags<'a>(text: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = text.find(open)? + open.len();
    Some(&text[start..start + text[start..].find(close)?])
}

/// An activity's `byteCount` value made a word - see [`FloatNoise`].
fn without_byte_count(text: &str) -> String {
    let Some(at) = text.find("<eml:Title>byteCount</eml:Title>") else {
        return text.to_string();
    };
    let Some(open) = text[at..]
        .find("<eml:Value>")
        .map(|o| at + o + "<eml:Value>".len())
    else {
        return text.to_string();
    };
    let Some(close) = text[open..].find("</eml:Value>").map(|c| open + c) else {
        return text.to_string();
    };
    format!("{}bytes{}", &text[..open], without_byte_count(&text[close..]))
}

/// The text with every uuid made `U` and every number `#`, and the numbers in
/// order. A number is digits, a fraction, an exponent, and a minus sign that no
/// letter or digit precedes - so `2023-01-01` is three numbers on both sides,
/// and `BSS000NVHN` keeps its letters.
fn tokens(text: &str) -> (String, Vec<f64>) {
    let bytes = text.as_bytes();
    let (mut skeleton, mut numbers) = (String::with_capacity(text.len() / 2), Vec::new());
    let mut i = 0;
    let mut kept = 0;
    let digits = |mut j: usize| {
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        j
    };
    while i < bytes.len() {
        let before = if i == 0 { b' ' } else { bytes[i - 1] };
        if i + 36 <= bytes.len() && is_uuid(&bytes[i..i + 36]) && !before.is_ascii_hexdigit() {
            skeleton.push_str(&text[kept..i]);
            skeleton.push('U');
            i += 36;
            kept = i;
            continue;
        }
        let signed = bytes[i] == b'-'
            && bytes.get(i + 1).is_some_and(u8::is_ascii_digit)
            && !before.is_ascii_alphanumeric()
            && before != b'.';
        if bytes[i].is_ascii_digit() || signed {
            let mut end = digits(i + 1);
            if bytes.get(end) == Some(&b'.') && bytes.get(end + 1).is_some_and(u8::is_ascii_digit) {
                end = digits(end + 1);
            }
            if matches!(bytes.get(end), Some(b'e' | b'E')) {
                let sign = usize::from(matches!(bytes.get(end + 1), Some(b'-' | b'+')));
                if bytes.get(end + 1 + sign).is_some_and(u8::is_ascii_digit) {
                    end = digits(end + 1 + sign);
                }
            }
            skeleton.push_str(&text[kept..i]);
            skeleton.push('#');
            numbers.push(text[i..end].parse().expect("digits parse"));
            i = end;
            kept = i;
            continue;
        }
        i += 1;
    }
    skeleton.push_str(&text[kept..]);
    (skeleton, numbers)
}

/// The parts two packages disagree on - present on one side only, or with other
/// bytes - the first ten, with the first differing line of each. "The EPC
/// differs" alone sent a CI failure to be reproduced on a second machine before
/// anyone could say *where*.
fn parts_that_differ(ours: &[u8], again: &[u8]) -> Vec<String> {
    use std::io::Read;
    fn parts(bytes: &[u8]) -> std::collections::BTreeMap<String, Vec<u8>> {
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("a zip");
        (0..archive.len())
            .map(|i| {
                let mut entry = archive.by_index(i).expect("an entry");
                let mut data = Vec::new();
                entry.read_to_end(&mut data).expect("it inflates");
                (entry.name().expect("a part name").into_owned(), data)
            })
            .collect()
    }
    let (ours, again) = (parts(ours), parts(again));
    let names: std::collections::BTreeSet<&String> = ours.keys().chain(again.keys()).collect();
    let mut found = Vec::new();
    for name in names {
        let line = match (ours.get(name), again.get(name)) {
            (Some(a), Some(b)) if a == b => continue,
            (Some(a), Some(b)) => {
                let (a, b) = (String::from_utf8_lossy(a), String::from_utf8_lossy(b));
                let first = a.lines().zip(b.lines()).find(|(x, y)| x != y);
                match first {
                    Some((x, y)) => {
                        let short = |s: &str| s.trim().chars().take(160).collect::<String>();
                        format!("{name}: `{}` against `{}`", short(x), short(y))
                    }
                    None => format!("{name}: {} bytes against {}", a.len(), b.len()),
                }
            }
            (Some(_), None) => format!("{name}: only in the example"),
            (None, _) => format!("{name}: only in the replay"),
        };
        found.push(line);
        if found.len() == 10 {
            break;
        }
    }
    if found.is_empty() {
        found.push(
            "every part is the same: the archive itself differs (order, compression, timestamps)".into(),
        );
    }
    found
}

/// The recipe, and the folder it is played from (`None`: not available, skipped).
fn recipe_and_base() -> Option<(Recipe, std::path::PathBuf)> {
    let base = lutetia_package::recipe_dir()?;
    let text = std::fs::read_to_string(base.join("recette.toml")).expect("the recipe is versioned");
    Some((Recipe::parse(&text).unwrap_or_else(|stop| panic!("{stop}")), base))
}

/// **A versioned example against its recipe, played again** - what the front end
/// opens is what the code writes (the epc and h5 of Lutetia are stored in the resources
/// of a front end, kept up to date by the CI, and the front end only
/// opens them). When the code changes
/// what the recipe writes, this fails, and the replay named in the message
/// ([`lutetia_package::refresh`]) brings both examples up to date.
///
/// - the **EPC byte for byte** on the platform that wrote the example (the
///   `written-on` stamp, [`lutetia_package::Stamp`]) - this workspace writes the
///   same bytes twice on one machine;
/// - **the same package up to float noise** on another ([`FloatNoise`]): the
///   platforms' math libraries differ by an ulp, and the CI runs Linux while
///   the examples are written on Windows. Believed otherwise until the CI's
///   first run, which disagreed on 2 144 parts;
/// - the **`.h5` array by array** - shape, type and values of every array the
///   package points at, read through the store from both sides, exactly or
///   within the same noise. Not its bytes: the file layout belongs to the HDF5
///   library, and the CI's (Ubuntu's) is not the one on a developer's machine.
///
/// The replayed package is returned, for what a caller asserts of it besides.
fn confront(recipe: &Recipe, base: &Path, kept: &Path, label: &str) -> std::path::PathBuf {
    assert!(kept.exists(), "the versioned example exists: {}", kept.display());
    let dir = std::env::temp_dir().join(format!("energyml-build-lutetia-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("Lutetia.epc");
    let schemas = Arc::new(energyml_schemas::all());
    recipe::run(recipe, base, &Registry::new(), schemas.clone(), &out, &mut Disk)
        .unwrap_or_else(|stop| panic!("{stop}"));

    let stale = format!(
        "the versioned example is stale - {}",
        lutetia_package::refresh(kept)
    );
    let (again, ours) = (std::fs::read(&out).unwrap(), std::fs::read(kept).unwrap());
    let written_on = lutetia_package::written_on();
    // Missing, the stamp predates this test's second mode: be exact.
    let exact = written_on.as_ref().is_none_or(lutetia_package::Stamp::is_here);
    let noise = if exact {
        assert!(
            again == ours,
            "{label}: the EPC differs in {:?}: {stale}",
            parts_that_differ(&ours, &again)
        );
        None
    } else {
        let noise = FloatNoise::between(&ours, &again);
        let there = written_on.as_ref().map(ToString::to_string).unwrap_or_default();
        let here = lutetia_package::here();
        assert!(
            noise.findings.is_empty(),
            "{label}: written on {there}, replayed on {here}, and more than float noise differs: {:#?}: {stale}",
            noise.findings
        );
        Some(noise)
    };

    let replayed = EpcStore::open(&out, schemas.clone()).expect("the replayed package opens");
    let versioned = EpcStore::open(kept, schemas).expect("the versioned package opens");
    let mut compared = 0usize;
    for summary in versioned.resources() {
        let object = versioned.get_object(&summary.uri).expect("it reads");
        let Some(noise) = &noise else {
            for location in versioned.arrays_of(object.as_reflect()) {
                let ours = versioned
                    .read_array(&location)
                    .unwrap_or_else(|why| panic!("{}: {why}", location.path_in_file));
                let again = replayed
                    .read_array(&location)
                    .unwrap_or_else(|why| panic!("{}: {why}", location.path_in_file));
                // By their exact writing, not `==`: an absent value is a NaN, and
                // NaN != NaN would call two identical arrays different (the first
                // run of this test did, on GTH-1's yearly pressure). Rust's `Debug`
                // of a float is its exact round trip, and writes `NaN` both sides.
                assert!(
                    format!("{ours:?}") == format!("{again:?}"),
                    "{label}: {} differs: {stale}",
                    location.path_in_file
                );
                compared += 1;
            }
            continue;
        };
        // Another platform: the same object is found by what it is, since its
        // uuid is derived from numbers the platform's libm wrote.
        let twin = noise
            .twins
            .get(&summary.uuid)
            .unwrap_or_else(|| panic!("{label}: no twin for {}", summary.uri));
        let other = replayed
            .get_object(
                &replayed
                    .resources()
                    .into_iter()
                    .find(|one| &one.uuid == twin)
                    .expect("the twin is listed")
                    .uri,
            )
            .expect("it reads");
        let (mine, theirs) = (
            versioned.arrays_of(object.as_reflect()),
            replayed.arrays_of(other.as_reflect()),
        );
        assert_eq!(
            mine.len(),
            theirs.len(),
            "{label}: {} points at another number of arrays: {stale}",
            summary.uri
        );
        for (a, b) in mine.iter().zip(&theirs) {
            let ours = versioned
                .read_array(a)
                .unwrap_or_else(|why| panic!("{}: {why}", a.path_in_file));
            let again = replayed
                .read_array(b)
                .unwrap_or_else(|why| panic!("{}: {why}", b.path_in_file));
            assert_eq!(
                ours.metadata.dimensions, again.metadata.dimensions,
                "{label}: {} has another shape: {stale}",
                a.path_in_file
            );
            let far = ours
                .values
                .to_f64()
                .into_iter()
                .zip(again.values.to_f64())
                .find(|(x, y)| !close(*x, *y));
            assert!(
                far.is_none(),
                "{label}: {} differs beyond float noise, {far:?}: {stale}",
                a.path_in_file
            );
            compared += 1;
        }
    }
    // Not a count pinned: a package with no array would make every comparison
    // above pass on nothing.
    assert!(compared > 0, "no array compared - this proves nothing");
    out
}

/// **The section drawing kept beside the model is the model's** - drawn again
/// from what its own metadata says (`energyml_section::redraw`, the
/// `--from-svg` of the section-view design), over the clean
/// example the recipe draws it from, it comes back the same. When a step
/// changes what the section crosses, this fails, and the README of
/// the recipe folder (`recipe/` of `energyml-lutetia`)
/// gives the command that draws it again.
///
/// Byte for byte on the platform that wrote the example; elsewhere the same
/// text with every number within a hundredth of a pixel - the SVG prints two
/// decimals, so an ulp of another libm can move the last digit and no more.
#[test]
fn the_section_drawing_is_drawn_again_the_same_from_the_example() {
    let Some(base) = lutetia_package::recipe_dir() else {
        return;
    };
    let Some(example) = lutetia_package::package(Variant::Clean) else {
        return;
    };
    let kept = std::fs::read_to_string(base.join("donnees/profils/coupe-lutetia.svg"))
        .expect("the drawing is versioned");
    let store = EpcStore::open(example, Arc::new(energyml_schemas::all())).expect("the example opens");
    let listed = energyml_section::read::package::Listed::of(&store).expect("its colour sets read");
    let mut opened = [energyml_section::Opened {
        store: "s1".to_string(),
        source: &store,
        context: listed.context(),
        cache: None,
    }];
    let again = energyml_section::redraw(&kept, &mut opened, &energyml_proj::Proj4::new())
        .unwrap_or_else(|why| panic!("{why}"));

    let stale = format!(
        "{}/donnees/profils/ is stale - draw it again (the README of that folder), then {}",
        base.display(),
        lutetia_package::refresh(&base)
    );
    let written_on = lutetia_package::written_on();
    if written_on.as_ref().is_none_or(lutetia_package::Stamp::is_here) {
        assert!(again == kept, "{stale}");
    } else {
        let ((mine, ours), (theirs, others)) = (tokens(&kept), tokens(&again));
        assert!(mine == theirs && ours.len() == others.len(), "{stale}");
        for (a, b) in ours.iter().zip(&others) {
            assert!(
                (a - b).abs() <= 0.011 || close(*a, *b),
                "{a} against {b}: {stale}"
            );
        }
    }
}
