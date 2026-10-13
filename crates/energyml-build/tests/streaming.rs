//! Reading a GeoJSON without holding it in memory.
//!
//! What is tested here is what can be **wrong** in a streaming
//! reader, and it is a short list: returning something other than the reader that has
//! everything in memory, keeping what it was told to throw away, not knowing how to stop, and
//! reading a document whose members do not arrive in the order that was hoped.
//!
//! The gain is measured elsewhere and not here: a stopwatch or a memory
//! peak in a test measures the machine, not the contract.
//! The figures of 20/09 - 50.8 MB read in 1,185 MB - are recorded in
//! the notes on external sources.

use energyml_build::geojson::{self, Feature, Flow, Keep, Wanted};

/// Two entities, properties, and a `crs` member **after** the features -
/// as a WFS really writes it (measured: `numberMatched` at byte 783,596).
fn collection(crs: Option<&str>) -> String {
    let tail = match crs {
        Some(name) => format!(
            r#","numberMatched":2,"numberReturned":2,"crs":{{"type":"name","properties":{{"name":"{name}"}}}}"#
        ),
        None => r#","numberMatched":2,"numberReturned":2"#.to_string(),
    };
    format!(
        r#"{{"type":"FeatureCollection","features":[
            {{"type":"Feature","id":"un","geometry":{{"type":"Point","coordinates":[2.3,48.9]}},
              "properties":{{"nom":"forage un","profondeur":42,"fouillis":{{"a":[1,2,3]}}}}}},
            {{"type":"Feature","id":"deux","geometry":{{"type":"Point","coordinates":[2.4,48.8]}},
              "properties":{{"nom":"forage deux","profondeur":7}}}}
        ]{tail}}}"#
    )
}

/// Read everything, keeping everything.
fn everything(text: &str) -> (Vec<Feature>, geojson::Summary) {
    let mut out = Vec::new();
    let summary = geojson::read_streaming(text.as_bytes(), &Wanted::default(), |one| {
        out.push(one);
        Flow::Continue
    })
    .expect("a collection");
    (out, summary)
}

#[test]
fn the_stream_says_what_the_whole_document_says() {
    // **The confrontation that counts.** `read` is now built on the
    // streaming reader; if the two were ever to diverge, it is here that it
    // would show - and this check is what allows having only one
    // implementation.
    let text = collection(None);
    let whole = geojson::read(&text).expect("a collection");
    let (streamed, summary) = everything(&text);

    assert_eq!(streamed.len(), whole.features.len());
    assert_eq!(summary.read, whole.features.len());
    assert_eq!(summary.refused, whole.refused);
    for (a, b) in streamed.iter().zip(whole.features.iter()) {
        assert_eq!(a.id, b.id);
        assert_eq!(a.shape, b.shape);
        assert_eq!(a.parts, b.parts);
        assert_eq!(a.properties, b.properties);
    }
}

#[test]
fn what_the_document_says_about_itself_comes_back() {
    // The counts of the service, carried to the caller - and **they arrive at
    // the end**, which is the whole argument for the separate quote.
    let (_, summary) = everything(&collection(None));
    assert_eq!(summary.matched, Some(2));
    assert_eq!(summary.returned, Some(2));
    assert!(!summary.stopped);
}

#[test]
fn what_is_not_asked_for_is_not_kept() {
    // The measured ×400: 2,000 entities weigh 1,185 MB with their properties and
    // 3 MB without. What a test can verify is not the memory peak but
    // **what remains** - the cause, not the consequence.
    let text = collection(None);

    let mut none = Vec::new();
    geojson::read_streaming(
        text.as_bytes(),
        &Wanted {
            properties: Keep::None,
        },
        |one| {
            none.push(one);
            Flow::Continue
        },
    )
    .expect("a collection");
    assert!(
        none.iter().all(|one| one.properties.is_null()),
        "properties were kept"
    );
    // And the shape is still there: a preview needs
    // the extents, not the properties.
    assert_eq!(none.len(), 2);
    assert_eq!(none[0].parts[0].len(), 1);

    let wanted = ["nom"];
    let mut only = Vec::new();
    geojson::read_streaming(
        text.as_bytes(),
        &Wanted {
            properties: Keep::Only(&wanted),
        },
        |one| {
            only.push(one);
            Flow::Continue
        },
    )
    .expect("a collection");
    let first = only[0].properties.as_object().expect("an object");
    assert_eq!(first.len(), 1, "more than what was asked for: {first:?}");
    assert_eq!(first.get("nom").and_then(|v| v.as_str()), Some("forage un"));
    // A key absent from an entity is not an error: a collection where half
    // the entities carry a field is ordinary.
    assert!(
        only[1]
            .properties
            .as_object()
            .expect("an object")
            .contains_key("nom")
    );
}

#[test]
fn a_sink_that_has_had_enough_stops_the_reading() {
    // Without that, “cancel” means reading to the end then throwing away - on a
    // 50 MB response, nine seconds of nothing.
    let text = collection(None);
    let mut seen = Vec::new();
    let summary = geojson::read_streaming(text.as_bytes(), &Wanted::default(), |one| {
        seen.push(one.id.clone());
        Flow::Stop
    })
    .expect("a collection");

    assert_eq!(
        seen.len(),
        1,
        "the sink said stop and got {} features",
        seen.len()
    );
    assert!(summary.stopped);
    assert_eq!(summary.read, 1);
}

#[test]
fn a_collection_that_is_not_in_wgs84_is_refused_wherever_it_says_so() {
    // The guard of the reader, kept word for word: a “GeoJSON” in
    // Lambert-93 read as degrees puts a model of the Paris basin off
    // the Gulf of Guinea, in silence.
    let after = geojson::read_streaming(
        collection(Some("urn:ogc:def:crs:EPSG::2154")).as_bytes(),
        &Wanted::default(),
        |_| Flow::Continue,
    )
    .expect_err("refused");
    assert!(after.0.contains("2154"), "{after}");
    // And when the member arrives **after** the entities, the refusal says that they
    // are already gone: a caller who has drawn them must know that
    // they have to be thrown away.
    assert!(after.0.contains("already handed over"), "{after}");

    // The one that announces it up front - the 2008 form, the one a producer
    // of such files writes - is refused without anything being returned.
    let before = r#"{"crs":{"type":"name","properties":{"name":"EPSG:2154"}},
                     "type":"FeatureCollection","features":[]}"#;
    let mut seen = 0;
    let why = geojson::read_streaming(before.as_bytes(), &Wanted::default(), |_| {
        seen += 1;
        Flow::Continue
    })
    .expect_err("refused");
    assert_eq!(seen, 0);
    assert!(!why.0.contains("already handed over"), "{why}");

    // And WGS 84 said in one way or the other passes.
    for name in [
        "urn:ogc:def:crs:EPSG::4326",
        "EPSG:4326",
        "urn:ogc:def:crs:OGC:1.3:CRS84",
    ] {
        geojson::read_streaming(collection(Some(name)).as_bytes(), &Wanted::default(), |_| {
            Flow::Continue
        })
        .unwrap_or_else(|why| panic!("`{name}` was refused: {why}"));
    }
}

#[test]
fn a_document_that_is_not_a_collection_is_refused_by_name() {
    for (text, expected) in [
        (
            r#"{"type":"Feature","features":[]}"#,
            "not a GeoJSON FeatureCollection",
        ),
        (r#"{"type":"FeatureCollection"}"#, "no `features` array"),
        (r#"{"features":[]}"#, "states no `type`"),
        ("pas du json", "not JSON"),
    ] {
        let why = geojson::read_streaming(text.as_bytes(), &Wanted::default(), |_| Flow::Continue)
            .expect_err("refused");
        assert!(why.0.contains(expected), "`{text}` was refused with: {why}");
    }
}

#[test]
fn a_feature_that_cannot_be_read_does_not_sink_the_rest() {
    // The rule of the reader, unchanged by the stream: a refused entity is
    // **named**, and the others come back.
    let text = r#"{"type":"FeatureCollection","features":[
        {"type":"Feature","id":"bon","geometry":{"type":"Point","coordinates":[2.3,48.9]},"properties":{}},
        {"type":"Feature","id":"sans-lieu","geometry":null,"properties":{}},
        {"type":"Feature","id":"encore","geometry":{"type":"Point","coordinates":[2.4,48.8]},"properties":{}}
    ]}"#;
    let (features, summary) = everything(text);
    assert_eq!(features.len(), 2);
    assert_eq!(summary.refused.len(), 1);
    assert!(summary.refused[0].contains("sans-lieu"), "{:?}", summary.refused);
}
