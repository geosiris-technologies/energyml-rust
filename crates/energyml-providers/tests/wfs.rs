//! What is sent and what is read, **without a network**.
//!
//! Everything that can be wrong in this crate is here: the encoding of a filter,
//! the axis order of an area, the reading of a server refusal, the listing of a
//! quote. The round trip itself is behind a trait and only serves to make it —
//! which is precisely the boundary this crate exists to draw.
//!
//! The fixtures are **real** answers, taken from `data.geoscience.fr`: an
//! invented refusal would be a refusal we know how to read.

use energyml_build::geojson::{Flow, Keep, Wanted};
use energyml_providers::query::{Area, Page, Query, Term};
use energyml_providers::transport::{Answer, Request, Transport};
use energyml_providers::{Error, Providers, Provider};

/// A transport that returns what it was given — the repository's fake, the
/// shape the ETP client already uses.
struct Canned {
    status: u16,
    body: String,
}

impl Transport for Canned {
    fn fetch(&self, _request: &Request) -> energyml_providers::Result<Answer> {
        Ok(Answer {
            status: self.status,
            content_type: Some("application/xml".to_string()),
            body: Box::new(std::io::Cursor::new(self.body.clone().into_bytes())),
        })
    }
}

fn brgm() -> impl Provider {
    energyml_providers::brgm::bss()
}

fn paris() -> Area {
    Area::Ring(vec![
        (2.2857475, 48.8167257),
        (2.2835689, 48.9692698),
        (2.5439083, 48.97213),
        (2.5494799, 48.8150852),
    ])
}

#[test]
fn a_query_becomes_the_url_the_service_expects() {
    let provider = brgm();
    let query = Query {
        collection: "boreholes".to_string(),
        area: Some(paris()),
        terms: vec![Term { name: "codeSpace".to_string(), value: "BRGM".to_string() }],
        most: None,
    };
    let request = provider.page(&query, Page::first(1000)).expect("a request");

    // The parameters the service wants, and the format it is asked for.
    for needle in [
        "service=WFS",
        "version=2.0.0",
        "request=GetFeature",
        "typenames=gsmlbh%3ABorehole",
        "outputFormat=application%2Fjson",
        "count=1000",
        "startIndex=0",
        "cql_filter=",
    ] {
        assert!(request.url.contains(needle), "`{needle}` missing from {}", request.url);
    }

    // **The axis order is the one this service imposes in CQL**, and it is the
    // thing that cannot be guessed: compared against the service, this order
    // returns 16,984 entities and the other **zero**, with nothing complaining.
    // Yet the same service wants `lat, lon` in a `bbox` — hence a field of the
    // provider, and not a rule.
    let decoded = percent_decode(&request.url);
    assert!(
        decoded.contains("POLYGON((2.2857475 48.8167257,"),
        "the axes are not in the service's order: {decoded}"
    );
    // The ring is closed by the reader, not by the caller: it is a rule of the
    // WKT format, not one more click to ask for.
    assert!(decoded.contains("2.2857475 48.8167257)))"), "the ring is not closed: {decoded}");
    // The filter path is the one the service expects, not the short name.
    assert!(
        decoded.contains("gsmlbh:logElement/gsmlbh:BoreholeInterval/gsmlbh:collectionIdentifier/@codeSpace")
    );
    assert!(decoded.contains("='BRGM'"));
}

#[test]
fn the_quote_asks_for_a_count_and_nothing_else() {
    let provider = brgm();
    let query = Query { collection: "boreholes".to_string(), area: Some(paris()), ..Default::default() };
    let request = provider.count(&query).expect("a request");
    assert!(request.url.contains("resultType=hits"), "{}", request.url);
    // **No output format**: the quote answers in XML whatever is asked, and
    // pretending otherwise would make a refusal read as a document.
    assert!(!request.url.contains("outputFormat"), "{}", request.url);
}

#[test]
fn what_the_service_does_not_offer_is_refused_before_any_request() {
    let provider = brgm();

    let unknown = provider
        .page(&Query { collection: "wells".to_string(), ..Default::default() }, Page::first(10))
        .expect_err("refused");
    match &unknown {
        // The refusal **lists what exists**: somebody who mistyped a name needs
        // the names, not a finding.
        Error::Refused(says) => {
            assert!(says.contains("boreholes") && says.contains("intervals"), "{says}")
        }
        other => panic!("wrong family of refusal: {other:?}"),
    }

    let bad_filter = provider
        .page(
            &Query {
                collection: "boreholes".to_string(),
                terms: vec![Term { name: "depth".to_string(), value: "10".to_string() }],
                ..Default::default()
            },
            Page::first(10),
        )
        .expect_err("refused");
    assert!(format!("{bad_filter}").contains("codeSpace"), "{bad_filter}");

    // And a collection with no geometry of its own refuses the area rather than
    // ignoring it: a drawn polygon that filters nothing is a gesture with no
    // effect, which is worse than an absent button.
    let no_area = provider
        .page(
            &Query {
                collection: "intervals".to_string(),
                area: Some(paris()),
                ..Default::default()
            },
            Page::first(10),
        )
        .expect_err("refused");
    assert!(format!("{no_area}").contains("no geometry"), "{no_area}");
}

#[test]
fn a_refusal_of_the_server_is_read_and_repeated() {
    // The service's real answer to a filter on an attribute it does not expose
    // — **XML, with a 400, although JSON was asked for**.
    let body = r#"<?xml version="1.0" encoding="UTF-8"?><ows:ExceptionReport xmlns:ows="http://www.opengis.net/ows/1.1" version="2.0.0">
  <ows:Exception exceptionCode="NoApplicableCode">
    <ows:ExceptionText>java.lang.IllegalArgumentException: Attribute &quot;identifier&quot; not found in type</ows:ExceptionText>
  </ows:Exception>
</ows:ExceptionReport>"#;
    let provider = brgm();
    let answer = Canned { status: 400, body: body.to_string() }
        .fetch(&Request { url: String::new(), accept: String::new(), about: String::new() })
        .expect("an answer");

    let why = provider
        .read_page(answer, &Wanted::default(), &mut |_| Flow::Continue)
        .expect_err("refused");
    match why {
        Error::Server { status, code, says } => {
            assert_eq!(status, 400);
            assert_eq!(code.as_deref(), Some("NoApplicableCode"));
            // **The server's text, as is** — XML entities resolved, because a
            // refusal that says `&quot;` is a refusal one rereads twice.
            assert!(says.contains(r#"Attribute "identifier" not found"#), "{says}");
        }
        other => panic!("a 400 answer must be a server refusal: {other:?}"),
    }
}

#[test]
fn a_quote_is_read_off_the_answer() {
    // The real answer to a `resultType=hits`: the count is an attribute, and
    // `numberReturned` is zero — that is the whole point.
    let body = r#"<?xml version="1.0" encoding="UTF-8"?><wfs:FeatureCollection xmlns:wfs="http://www.opengis.net/wfs/2.0" numberMatched="16984" numberReturned="0" timeStamp="2026-09-20T01:00:00Z"/>"#;
    let provider = brgm();
    let answer = Canned { status: 200, body: body.to_string() }
        .fetch(&Request { url: String::new(), accept: String::new(), about: String::new() })
        .expect("an answer");
    assert_eq!(provider.read_count(answer).expect("a count"), 16_984);
}

#[test]
fn a_page_is_read_as_a_stream_and_keeps_what_was_asked_for() {
    let body = r#"{"type":"FeatureCollection","features":[
        {"type":"Feature","id":"BSS1","geometry":{"type":"Point","coordinates":[2.3,48.9]},
         "properties":{"identifier":"https://data.geoscience.fr/id/Borehole/BSS1","indexData":{"gros":[1,2,3]}}}
    ],"numberMatched":2940,"numberReturned":1}"#;
    let provider = brgm();
    let answer = Canned { status: 200, body: body.to_string() }
        .fetch(&Request { url: String::new(), accept: String::new(), about: String::new() })
        .expect("an answer");

    let wanted = ["identifier"];
    let mut seen = Vec::new();
    let summary = provider
        .read_page(answer, &Wanted { properties: Keep::Only(&wanted) }, &mut |one| {
            seen.push(one);
            Flow::Continue
        })
        .expect("a page");

    assert_eq!(summary.read, 1);
    assert_eq!(summary.matched, Some(2940));
    let kept = seen[0].properties.as_object().expect("an object");
    assert_eq!(kept.len(), 1, "more was kept than asked for: {kept:?}");
    assert!(kept.contains_key("identifier"));
}

#[test]
fn the_registry_lists_what_this_build_can_ask() {
    let providers = Providers::builtin();
    let names: Vec<&str> = providers.all().map(|one| one.name()).collect();
    assert!(names.contains(&"brgm-bss"), "{names:?}");

    // `expect_err` would demand a `Debug` on `&dyn Provider` — which a trait
    // object does not have, and which we will not require of every provider for
    // a test message.
    match providers.get("unknown") {
        Err(why) => assert!(format!("{why}").contains("brgm-bss"), "{why}"),
        Ok(_) => panic!("`unknown` is not a provider of this build"),
    }

    // What a provider declares is what the front end will render: if a collection
    // announces neither its filters nor what it feeds, the form is empty and
    // nobody knows what to do with it.
    let brgm = providers.get("brgm-bss").expect("a provider");
    let collections = brgm.collections();
    assert_eq!(collections.len(), 2);
    let boreholes = collections.iter().find(|one| one.name == "boreholes").expect("boreholes");
    assert!(!boreholes.about.is_empty());
    assert!(boreholes.area.is_some());
    assert_eq!(boreholes.feeds, vec!["sigeol-boreholes".to_string()]);
    let filter = &boreholes.filters[0];
    assert!(!filter.about.is_empty(), "a filter with no sentence is a field nobody fills in");
    assert_eq!(filter.suggest.as_deref(), Some("BRGM"));

    // And the credit travels with the address, never separately.
    let about = brgm.about();
    assert!(about.endpoint.starts_with("https://"));
    assert!(about.attribution.contains("BRGM"));
}

#[test]
fn an_area_gives_its_bounds_for_a_service_that_only_takes_a_box() {
    let (west, south, east, north) = paris().bounds();
    assert!((west - 2.2835689).abs() < 1e-9);
    assert!((south - 48.8150852).abs() < 1e-9);
    assert!((east - 2.5494799).abs() < 1e-9);
    assert!((north - 48.97213).abs() < 1e-9);
}

/// Percent-decode — to read a URL in a test message.
fn percent_decode(url: &str) -> String {
    let bytes = url.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%'
            && at + 2 < bytes.len()
            && let Ok(byte) = u8::from_str_radix(&url[at + 1..at + 3], 16)
        {
            out.push(byte);
            at += 3;
            continue;
        }
        out.push(bytes[at]);
        at += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// An invented provider, for the only question that matters: **does an added
/// API reach the front end without touching the front end?**
struct Elsewhere;

impl Provider for Elsewhere {
    fn name(&self) -> &str {
        "elsewhere"
    }
    fn describe(&self) -> &str {
        "an invented service, to exercise the catalogue"
    }
    fn about(&self) -> energyml_providers::About {
        energyml_providers::About {
            endpoint: "https://example.invalid/ows".to_string(),
            attribution: "nobody".to_string(),
            licence: None,
        }
    }
    fn collections(&self) -> Vec<energyml_providers::Collection> {
        vec![energyml_providers::Collection {
            name: "pebbles".to_string(),
            title: "Pebbles".to_string(),
            about: "what one picks up".to_string(),
            feature_type: "x:Pebble".to_string(),
            feeds: vec![],
            area: None,
            filters: vec![energyml_providers::Filter {
                name: "colour".to_string(),
                label: "Colour".to_string(),
                about: "the colour of the pebble".to_string(),
                kind: energyml_providers::Kind::OneOf(vec!["grey".to_string(), "beige".to_string()]),
                required: true,
                suggest: Some("grey".to_string()),
                property: "colour".to_string(),
            }],
        }]
    }
    fn count(&self, _query: &Query) -> energyml_providers::Result<Request> {
        unimplemented!("this fake only serves its declaration")
    }
    fn page(&self, _query: &Query, _page: Page) -> energyml_providers::Result<Request> {
        unimplemented!("this fake only serves its declaration")
    }
    fn read_count(&self, _answer: Answer) -> energyml_providers::Result<u64> {
        unimplemented!("this fake only serves its declaration")
    }
    fn read_page(
        &self,
        _answer: Answer,
        _wanted: &Wanted<'_>,
        _sink: &mut dyn FnMut(energyml_build::geojson::Feature) -> Flow,
    ) -> energyml_providers::Result<energyml_build::geojson::Summary> {
        unimplemented!("this fake only serves its declaration")
    }
}

#[test]
fn the_catalogue_is_enough_to_build_a_request_knowing_nothing() {
    // **The property this crate exists to hold.** This test plays the front end: it
    // spells no provider name, no collection name, no filter name. It only reads
    // the catalogue's JSON, fills what is declared required with what is
    // declared suggested, and returns the request. If one day a line has to be
    // added here for a new API, a line will have to be added in the front end.
    let providers = Providers::builtin();
    let json = serde_json::to_string(&providers.catalogue()).expect("the catalogue serialises");

    // Past the next line, we know nothing: we only have data.
    let seen: serde_json::Value = serde_json::from_str(&json).expect("and reads back");
    let provider = &seen["providers"][0];
    let name = provider["name"].as_str().expect("a name");
    let collection = provider["collections"]
        .as_array()
        .expect("collections")
        .iter()
        .find(|one| !one["area"].is_null())
        .expect("at least one collection that takes an area");

    let mut terms = Vec::new();
    for filter in collection["filters"].as_array().expect("filters") {
        if filter["required"].as_bool() == Some(true) {
            let value = filter["suggest"]
                .as_str()
                .expect("a required filter must carry a suggestion, otherwise the form is a trap");
            terms.push(Term {
                name: filter["name"].as_str().expect("a name").to_string(),
                value: value.to_string(),
            });
        }
    }

    let query = Query {
        collection: collection["name"].as_str().expect("a name").to_string(),
        area: Some(paris()),
        terms,
        most: None,
    };

    // And the request thus built is accepted by the provider it names.
    let provider = providers.get(name).expect("the provider the catalogue names");
    let request = provider.page(&query, Page::first(100)).expect("a request");
    assert!(request.url.starts_with("https://"), "{}", request.url);
    assert!(request.about.contains(name) || !request.about.is_empty());
}

#[test]
fn an_added_provider_reaches_the_catalogue_without_touching_anything() {
    let mut providers = Providers::builtin();
    providers.push(Box::new(Elsewhere));
    let catalogue = providers.catalogue();

    // **The added ones first**, the repository's registry rule: a shipped one
    // must be replaceable, not only completed.
    assert_eq!(catalogue.providers[0].name, "elsewhere");
    assert!(catalogue.providers.iter().any(|one| one.name == "brgm-bss"));

    // And its declaration travels whole — it is what becomes the form.
    let added = &catalogue.providers[0];
    assert_eq!(added.collections.len(), 1);
    assert_eq!(added.collections[0].filters[0].suggest.as_deref(), Some("grey"));

    // **What the front end reads to choose a control is a tag, never a shape.** A
    // closed choice is told from a text by `is`, and the values are beside it:
    // asking the front end to tell an object from a string is asking it to forget one
    // the day a type is added.
    let json = serde_json::to_value(&catalogue).expect("serialisable");
    let kind = &json["providers"][0]["collections"][0]["filters"][0]["kind"];
    assert_eq!(kind["is"], "oneOf");
    assert_eq!(kind["values"][0], "grey");

    // A field name crosses in `camelCase`, the convention on the other side.
    assert!(json["providers"][0]["collections"][0]["featureType"].is_string());
}

#[test]
fn a_request_from_the_front_end_comes_back_unchanged() {
    // The other direction: the front end sends back what the form filled in, and a
    // drawn area is part of it. A ring that lost a point or swapped its axes in
    // transit would return boreholes from elsewhere.
    let query = Query {
        collection: "boreholes".to_string(),
        area: Some(paris()),
        terms: vec![Term { name: "codeSpace".to_string(), value: "BRGM".to_string() }],
        most: Some(500),
    };
    let json = serde_json::to_string(&query).expect("serialisable");
    let back: Query = serde_json::from_str(&json).expect("and readable");
    assert_eq!(back, query);

    // A minimal request from the front end — just the collection — reads back:
    // without `default`, a missing field would be a read refusal where it is a
    // perfectly ordinary absence.
    let bare: Query = serde_json::from_str(r#"{"collection":"boreholes"}"#).expect("readable");
    assert_eq!(bare.collection, "boreholes");
    assert!(bare.area.is_none() && bare.terms.is_empty() && bare.most.is_none());
}
