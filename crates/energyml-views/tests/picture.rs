//! The door to a picture: objects checked, the request kept in the metadata, the same bytes drawn again, the notes
//! written, the PNG only from the SVG.

use energyml_views::{ViewAsk, ViewKinds, ViewRefusal, draw_svg, read_ask, redraw};
use serde_json::json;

use crate::fixtures::{CRS, NO_MAPS, Opened, frame_uri, package, uri};

fn locator(md: f64) -> ViewAsk {
    ViewAsk::new("well-locator")
        .object(frame_uri())
        .set("md", json!(md))
}

#[test]
fn a_drawing_keeps_its_request_completed_and_is_drawn_again_to_the_same_bytes() {
    let opened = Opened::of(package(50.0, &[20.0, 80.0]));
    let cx = opened.context(&NO_MAPS);
    let kinds = ViewKinds::with_builtins();
    let picture = draw_svg(&kinds, &locator(50.0), &cx).expect("drawn");
    let kept = read_ask(&picture.svg).expect("its request reads back");
    assert_eq!(kept, picture.ask);
    assert_eq!(
        kept.settings.get("mode"),
        Some(&json!("md_tvd")),
        "the default is written, not left out"
    );
    assert_eq!(kept.size, Some([420.0, 640.0]), "the view's own size is written");
    let again = redraw(&kinds, &picture.svg, &cx).expect("drawn again");
    assert_eq!(again.svg, picture.svg);
    // Break it: write the request as given rather than completed, and `mode` is missing from the metadata.
}

#[test]
fn a_drawing_without_the_metadata_does_not_draw_again() {
    let refused = read_ask("<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>").expect_err("refused");
    assert!(matches!(refused, ViewRefusal::Metadata(_)), "{refused}");
    // Break it: fall back on a default request when the metadata is absent.
}

#[test]
fn an_object_the_package_does_not_hold_or_the_view_does_not_take_is_refused_by_name() {
    let opened = Opened::of(package(50.0, &[20.0, 80.0]));
    let cx = opened.context(&NO_MAPS);
    let kinds = ViewKinds::with_builtins();
    let absent = uri(
        "resqml22.WellboreFrameRepresentation",
        "99999999-9999-4999-8999-999999999999",
    );
    let refused =
        draw_svg(&kinds, &ViewAsk::new("well-locator").object(absent.clone()), &cx).expect_err("refused");
    assert!(
        matches!(&refused, ViewRefusal::NotInPackage { object } if *object == absent),
        "{refused}"
    );

    let crs = uri("resqml20.obj_LocalDepth3dCrs", CRS);
    let refused = draw_svg(&kinds, &ViewAsk::new("well-locator").object(crs), &cx).expect_err("refused");
    assert!(
        matches!(&refused, ViewRefusal::ObjectNotTaken { .. }),
        "{refused}"
    );

    let refused = draw_svg(&kinds, &ViewAsk::new("well-locator"), &cx).expect_err("refused");
    assert!(
        matches!(&refused, ViewRefusal::MissingObject { role, least: 1, given: 0, .. } if role == "well"),
        "{refused}"
    );
    // Break it: skip the slots in `fit_objects`, and the CRS is handed to the view.
}

#[test]
fn a_bare_uuid_is_turned_into_the_full_uri_at_the_door() {
    let opened = Opened::of(package(50.0, &[20.0, 80.0]));
    let picture = draw_svg(
        &ViewKinds::with_builtins(),
        &ViewAsk::new("well-locator").object(crate::fixtures::FRAME),
        &opened.context(&NO_MAPS),
    )
    .expect("drawn");
    assert_eq!(
        picture.ask.objects,
        vec![frame_uri()],
        "the metadata names the full uri"
    );
    // Break it: keep the object as given in the completed request.
}

#[test]
fn a_note_is_written_in_a_band_under_the_picture() {
    let opened = Opened::of(package(50.0, &[20.0, 80.0]));
    let picture = draw_svg(
        &ViewKinds::with_builtins(),
        &locator(500.0),
        &opened.context(&NO_MAPS),
    )
    .expect("drawn");
    assert_eq!(picture.notes.len(), 1, "{:?}", picture.notes);
    assert_eq!(picture.notes[0].key, "mark-clamped");
    assert!(
        picture.svg.contains("is below the last point"),
        "the note is in the picture"
    );
    assert!(
        !picture.svg.contains("height=\"640\""),
        "the page grew by the band of notes"
    );
    // Break it: drop the notes layer in `draw_list`.
}

#[cfg(not(feature = "png"))]
#[test]
fn without_the_png_feature_the_png_is_refused_by_name() {
    let refused = energyml_views::png(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"10\"/>",
        1.0,
    )
    .expect_err("refused");
    assert!(refused.to_string().contains("png"), "{refused}");
    // Break it: answer an empty PNG without the feature.
}

#[cfg(feature = "png")]
#[test]
fn the_png_is_the_svg_rasterised_at_its_scale() {
    let opened = Opened::of(package(50.0, &[20.0, 80.0]));
    let picture = draw_svg(
        &ViewKinds::with_builtins(),
        &locator(50.0),
        &opened.context(&NO_MAPS),
    )
    .expect("drawn");
    let png = energyml_views::png(&picture.svg, 2.0).expect("rasterised");
    assert_eq!(&png[1..4], b"PNG");
    let word = |at: usize| u32::from_be_bytes([png[at], png[at + 1], png[at + 2], png[at + 3]]);
    assert_eq!((word(16), word(20)), (840, 1280), "twice the 420 x 640 page");
    // Break it: rasterise at scale 1 whatever is asked.
}
