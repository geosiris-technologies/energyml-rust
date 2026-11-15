//! `along_well::model`: the request, its mirrors of types that take no `serde`, the refusals of `build`.

use energyml_domain::component_axes::AxisLink;
use energyml_domain::package_index::CandidateNeed;
use energyml_geometry::around_well::projection::WellProjections;
use energyml_geometry::around_well::transverse::{AxisSign, PlaneAxis};
use energyml_section::along_well::{
    AlongWellRefusal, AxisLinkAsk, AxisSignAsk, BoundaryAsk, BoundaryMode, CandidateNeedAsk, CurtainAsk,
    CurtainSources, MarkerRefusal, Note, PlaneAxisAsk, Request, ScaleKindAsk, build,
};

use super::{TRAJECTORY, hole_request, package_with_well, uri};

/// Every variant of a mirror goes to the type it mirrors and comes back as itself, and goes through JSON
/// and comes back as itself: the request of a drawing is written in its metadata (README, R19: the domain
/// and the geometry take no `serde`).
///
/// To break it: forget a variant in one of the two `From`.
#[test]
fn mirrors_round_trip() {
    for link in [
        AxisLink::Table {
            table: "eml:///eml23.ColumnBasedTable(00000000-0000-4000-8000-000000000009)".to_string(),
            column: "Depth".to_string(),
            dim: 1,
        },
        AxisLink::PairedProperty {
            property: "eml:///resqml22.ContinuousProperty(00000000-0000-4000-8000-00000000000a)".to_string(),
            dim: 0,
        },
        AxisLink::WitsmlChannel {
            channel: "eml:///witsml21.Channel(00000000-0000-4000-8000-00000000000b)".to_string(),
        },
    ] {
        let ask = AxisLinkAsk::from(link.clone());
        assert_eq!(AxisLink::from(ask.clone()), link);
        let json = serde_json::to_string(&ask).unwrap();
        assert_eq!(serde_json::from_str::<AxisLinkAsk>(&json).unwrap(), ask);
    }
    for axis in [PlaneAxis::HighSide, PlaneAxis::Right] {
        let ask = PlaneAxisAsk::from(axis);
        assert_eq!(PlaneAxis::from(ask), axis);
        let json = serde_json::to_string(&ask).unwrap();
        assert_eq!(serde_json::from_str::<PlaneAxisAsk>(&json).unwrap(), ask);
    }
    for sign in [AxisSign::Positive, AxisSign::Negative] {
        let ask = AxisSignAsk::from(sign);
        assert_eq!(AxisSign::from(ask), sign);
        let json = serde_json::to_string(&ask).unwrap();
        assert_eq!(serde_json::from_str::<AxisSignAsk>(&json).unwrap(), ask);
    }
    for need in [
        CandidateNeed::Axes,
        CandidateNeed::AngleConvention,
        CandidateNeed::ChooseAngles {
            candidates: vec!["a".to_string(), "b".to_string()],
        },
    ] {
        let ask = CandidateNeedAsk::from(need.clone());
        assert_eq!(CandidateNeed::from(ask.clone()), need);
        let json = serde_json::to_string(&ask).unwrap();
        assert_eq!(serde_json::from_str::<CandidateNeedAsk>(&json).unwrap(), ask);
    }
    for kind in [ScaleKindAsk::Linear, ScaleKindAsk::Log10] {
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(serde_json::from_str::<ScaleKindAsk>(&json).unwrap(), kind);
    }
}

/// A complete request goes through JSON and comes back as itself (the metadata of a drawing).
///
/// To break it: skip a field in the derive.
#[test]
fn a_request_round_trips_through_json() {
    let mut request = hole_request("vertical_section");
    request.curtain = Some(CurtainAsk {
        property: uri("resqml22.ContinuousProperty", "a0000000-0000-4000-8000-000000000001"),
        source: "cube-on-frame".to_string(),
        links: vec![AxisLinkAsk::PairedProperty {
            property: uri("resqml22.ContinuousProperty", "a0000000-0000-4000-8000-000000000002"),
            dim: 1,
        }],
        roles: [
            (PlaneAxisAsk::HighSide, AxisSignAsk::Negative),
            (PlaneAxisAsk::Right, AxisSignAsk::Positive),
        ],
        lateral_index: 2,
    });
    request.boundaries = Some(BoundaryAsk {
        radius: uri("resqml22.ContinuousProperty", "a0000000-0000-4000-8000-000000000003"),
        angle: uri("resqml22.ContinuousProperty", "a0000000-0000-4000-8000-000000000004"),
        mode: BoundaryMode::Projected,
    });
    request.md_range = Some([10.5, 99.25]);
    request.step = Some(0.5);
    request.section = Some(super::east_section());
    request.surfaces = vec![uri("resqml22.TriangulatedSetRepresentation", "b0000000-0000-4000-8000-000000000001")];
    let json = serde_json::to_string(&request).unwrap();
    assert_eq!(serde_json::from_str::<Request>(&json).unwrap(), request);
}

/// A marker has no reader in the workspace: its uri is refused **by name** and the section is still built
/// (the hole, drawn; a note says so). A marker object the package does not hold is refused the same way.
///
/// To break it: fail the whole `build` on the first marker.
#[test]
fn markers_are_refused_by_name_and_the_section_still_draws() {
    let package = package_with_well(&[20.0, 40.0, 60.0, 80.0]);
    let mut request = hole_request("md_tvd");
    request.markers = vec![uri("witsml21.WellboreMarker", "c0000000-0000-4000-8000-0000000000aa")];
    let model = build(
        &request,
        &package,
        &[],
        &CurtainSources::builtin(),
        &WellProjections::standard(),
    )
    .expect("the section is built");
    assert_eq!(model.node_md, vec![20.0, 40.0, 60.0, 80.0]);
    assert!(!model.trace.is_empty());
    assert!(model.trace.iter().all(|point| point.is_some()));
    assert_eq!(model.markers_refused.len(), 1);
    assert!(matches!(&model.markers_refused[0], MarkerRefusal::NoReader { .. }));
    assert!(model.notes.contains(&Note::MarkersNotRead { count: 1 }));
    assert!(model.markers.is_empty());
}

/// The refusals of the request, by name: a bare uuid where a uri is required, a projection the registry
/// does not hold, `vertical_section` with no section, a source that is not registered, an object that is
/// not a frame.
///
/// To break it: accept a bare uuid (the first refusal is gone).
#[test]
fn the_request_is_refused_by_name() {
    let package = package_with_well(&[20.0, 40.0, 60.0, 80.0]);
    let sources = CurtainSources::builtin();
    let projections = WellProjections::standard();
    let run = |request: &Request| build(request, &package, &[], &sources, &projections);

    let mut bare = hole_request("md_tvd");
    bare.frame = super::FRAME.to_string();
    assert!(matches!(run(&bare), Err(AlongWellRefusal::NotAUri { field: "frame", .. })));

    assert!(matches!(
        run(&hole_request("sideways")),
        Err(AlongWellRefusal::UnknownProjection { .. })
    ));
    assert!(matches!(
        run(&hole_request("vertical_section")),
        Err(AlongWellRefusal::UnknownProjection { .. })
    ));

    let mut unknown = hole_request("md_tvd");
    unknown.curtain = Some(CurtainAsk {
        property: uri("resqml22.ContinuousProperty", "a0000000-0000-4000-8000-000000000001"),
        source: "nowhere".to_string(),
        links: Vec::new(),
        roles: [
            (PlaneAxisAsk::HighSide, AxisSignAsk::Positive),
            (PlaneAxisAsk::Right, AxisSignAsk::Positive),
        ],
        lateral_index: 0,
    });
    assert!(matches!(run(&unknown), Err(AlongWellRefusal::UnknownSource { .. })));

    let mut not_a_frame = hole_request("md_tvd");
    not_a_frame.frame = uri("resqml22.WellboreTrajectoryRepresentation", TRAJECTORY);
    assert!(matches!(run(&not_a_frame), Err(AlongWellRefusal::NotOnFrame { .. })));
}
