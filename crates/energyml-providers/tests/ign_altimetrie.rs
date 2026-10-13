//! The IGN Géoplateforme altimetry, without a network.
//!
//! The fixtures are **real** answers, taken from `data.geopf.fr` - same rule as
//! `tests/wfs.rs`: an invented refusal would be a refusal we know how to read.

use energyml_providers::error::Error;
use energyml_providers::ign_altimetrie::{
    Elevation, LIDAR_HD_MNT, MAX_POINTS, RGE_ALTI, elevation_request, profile_request, read_elevations,
};
use energyml_providers::transport::{Answer, Request, Transport};

/// The repository's fake, the shape `tests/wfs.rs` already uses.
struct Canned {
    status: u16,
    body: &'static str,
}

impl Transport for Canned {
    fn fetch(&self, _request: &Request) -> energyml_providers::Result<Answer> {
        Ok(Answer {
            status: self.status,
            content_type: Some("application/json".to_string()),
            body: Box::new(std::io::Cursor::new(self.body.as_bytes().to_vec())),
        })
    }
}

fn answer(status: u16, body: &'static str) -> Answer {
    Canned { status, body }
        .fetch(&Request {
            url: String::new(),
            accept: String::new(),
            about: String::new(),
        })
        .expect("a canned answer")
}

#[test]
fn an_elevation_request_separates_points_with_a_pipe_and_names_the_resource() {
    let request = elevation_request(&[(2.33, 48.86), (2.34, 48.87)], RGE_ALTI).expect("a request");
    assert!(
        request
            .url
            .starts_with("https://data.geopf.fr/altimetrie/1.0/calcul/alti/rest/elevation.json?")
    );

    // `|`, never `,`: a comma makes the service fail (`BAD_PARAMETER`), as
    // measured.
    assert!(request.url.contains("lon=2.33%7C2.34"), "{}", request.url);
    assert!(request.url.contains("lat=48.86%7C48.87"), "{}", request.url);
    assert!(request.url.contains(&format!("resource={RGE_ALTI}")));
    // **Always the complete form**: `zonly=false`, never `true`, so that
    // `read_elevations` can return `lon`/`lat` without asking for them again.
    assert!(request.url.contains("zonly=false"));
}

#[test]
fn a_profile_request_needs_at_least_two_points_and_carries_the_sampling() {
    let request = profile_request(&[(2.33, 48.86), (2.335, 48.865), (2.34, 48.87)], LIDAR_HD_MNT, 3)
        .expect("a request");
    assert!(request.url.contains("elevationLine.json"));
    assert!(request.url.contains(&format!("resource={LIDAR_HD_MNT}")));
    assert!(request.url.contains("sampling=3"));

    let refused = profile_request(&[(2.33, 48.86)], RGE_ALTI, 3).expect_err("refused");
    assert!(format!("{refused}").contains("at least 2"), "{refused}");
}

#[test]
fn no_point_and_too_many_points_are_refused_before_any_request() {
    let empty = elevation_request(&[], RGE_ALTI).expect_err("refused");
    assert!(format!("{empty}").contains("at least 1"), "{empty}");

    let too_many = vec![(2.33, 48.86); MAX_POINTS + 1];
    let refused = elevation_request(&too_many, RGE_ALTI).expect_err("refused");
    assert!(
        format!("{refused}").contains(&MAX_POINTS.to_string()),
        "{refused}"
    );
}

#[test]
fn a_single_point_answer_is_read_whole() {
    // The real answer for a point in Paris, `zonly=false`: `lon`, `lat`, `z`
    // and `acc` come back in one object per point.
    let body = r#"{"elevations": [{"lon": 2.33, "lat": 48.86, "z": 26.68, "acc": "Variable suivant la source de mesure"}]}"#;
    let elevations = read_elevations(answer(200, body)).expect("elevations");
    assert_eq!(
        elevations,
        vec![Elevation {
            lon: 2.33,
            lat: 48.86,
            z: Some(26.68)
        }]
    );
}

#[test]
fn several_points_come_back_in_the_order_asked() {
    let body = r#"{"elevations": [
        {"lon": 2.33, "lat": 48.86, "z": 26.68, "acc": "Variable suivant la source de mesure"},
        {"lon": 2.34, "lat": 48.87, "z": 35.46, "acc": "Variable suivant la source de mesure"}
    ]}"#;
    let elevations = read_elevations(answer(200, body)).expect("elevations");
    assert_eq!(elevations.len(), 2);
    assert_eq!(elevations[0].z, Some(26.68));
    assert_eq!(elevations[1].z, Some(35.46));
}

#[test]
fn a_profile_answer_is_read_the_same_way_height_differences_ignored() {
    // The real `elevationLine.json` answer over three points: it also carries
    // `height_differences`, which this module ignores - it returns points, not
    // a cumulated height difference.
    let body = r#"{"elevations": [
        {"lon": 2.33, "lat": 48.86, "z": 26.68, "acc": "Variable suivant la source de mesure"},
        {"lon": 2.335, "lat": 48.864999999999995, "z": 34.16, "acc": "Variable suivant la source de mesure"},
        {"lon": 2.34, "lat": 48.87, "z": 35.46, "acc": "Variable suivant la source de mesure"}
    ], "height_differences": {"positive": 8.780000000000001, "negative": 0}}"#;
    let elevations = read_elevations(answer(200, body)).expect("elevations");
    assert_eq!(elevations.len(), 3);
    assert_eq!(elevations[1].z, Some(34.16));
}

#[test]
fn no_coverage_is_none_not_a_measured_depth() {
    // The real answer for a point off the Canary Islands, outside the RGE ALTI
    // coverage: `z = -99999`, never a depth.
    let body = r#"{"elevations": [{"lon": -30.0, "lat": 10.0, "z": -99999.0, "acc": "Variable suivant la source de mesure"}]}"#;
    let elevations = read_elevations(answer(200, body)).expect("elevations");
    assert_eq!(elevations[0].z, None);
}

#[test]
fn a_server_refusal_is_json_here_unlike_the_wfs() {
    // The real answer to an unknown resource: a `400`, in JSON - not the XML
    // `ows:ExceptionReport` of the BRGM WFS.
    let body =
        r#"{"error": {"code": "BAD_PARAMETER", "description": "'nope' is not an accepted resource."}}"#;
    let why = read_elevations(answer(400, body)).expect_err("refused");
    match why {
        Error::Server { status, code, says } => {
            assert_eq!(status, 400);
            assert_eq!(code.as_deref(), Some("BAD_PARAMETER"));
            assert!(says.contains("not an accepted resource"), "{says}");
        }
        other => panic!("a 400 with an `error` object must be a server refusal: {other:?}"),
    }
}

#[test]
fn a_mismatched_point_count_is_the_servers_own_refusal() {
    // The real answer when `lon` and `lat` do not have the same number of
    // elements - a fault this module does not detect before sending, because
    // `coordinates()` always builds them with the same length; this test
    // documents what the service would say if a caller bypassed
    // `elevation_request`.
    let body = r#"{"error": {"code": "BAD_PARAMETER", "description": "The number of [lon] values must match the number of [lat] values"}}"#;
    let why = read_elevations(answer(400, body)).expect_err("refused");
    assert!(format!("{why}").contains("must match"), "{why}");
}
