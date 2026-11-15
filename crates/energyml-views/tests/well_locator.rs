//! `well-locator`: the line, the interval of the frame, the mark, the projections, the refusals.

use energyml_sheet::draw::Item;
use energyml_views::{ViewAsk, ViewKinds, ViewRefusal, draw_list};
use serde_json::json;

use crate::fixtures::{NO_MAPS, Opened, distance_to_line, frame_uri, package, polylines, trajectory_uri};

fn drawn(east: f64, ask: ViewAsk) -> energyml_views::Drawn {
    let opened = Opened::of(package(east, &[20.0, 80.0]));
    draw_list(&ViewKinds::with_builtins(), &ask, &opened.context(&NO_MAPS))
        .expect("drawn")
        .0
}

fn mark_centre(list: &energyml_sheet::draw::DisplayList) -> [f64; 2] {
    list.layers
        .iter()
        .filter(|layer| layer.name == "mark")
        .flat_map(|layer| layer.elements.iter())
        .flat_map(|element| element.items.iter())
        .find_map(|item| match item {
            Item::Symbol { at, .. } => Some(*at),
            _ => None,
        })
        .expect("a mark")
}

#[test]
fn every_frame_and_trajectory_is_offered() {
    let opened = Opened::of(package(50.0, &[20.0, 80.0]));
    let offers = ViewKinds::with_builtins().offers(&opened.index);
    let objects: Vec<&str> = offers.iter().map(|offer| offer.objects[0].as_str()).collect();
    assert!(
        objects.contains(&frame_uri().as_str()) && objects.contains(&trajectory_uri().as_str()),
        "{objects:?}"
    );
    assert!(
        offers.iter().all(|offer| offer.missing.is_empty()),
        "nothing to declare"
    );
    // Break it: offer only the frames.
}

#[test]
fn the_mark_lies_on_the_line_at_every_projection() {
    for mode in ["md_tvd", "md_bar", "developed", "plan"] {
        let list = drawn(
            50.0,
            ViewAsk::new("well-locator")
                .object(frame_uri())
                .set("mode", json!(mode))
                .set("md", json!(50.0)),
        )
        .list;
        let line = &polylines(&list, "well", "well")[0];
        let at = mark_centre(&list);
        assert!(
            distance_to_line(at, line) < 0.5,
            "{mode}: the mark is {} px off the line",
            distance_to_line(at, line)
        );
    }
    // Break it: place the mark at the MD read as a fraction of the line's length rather than by `locator_mark`.
}

#[test]
fn the_frame_interval_is_brought_forward_over_its_share_of_the_line() {
    // MD against TVD: x is the MD, so the span from MD 20 to 80 covers 60 % of the line's width.
    let list = drawn(50.0, ViewAsk::new("well-locator").object(frame_uri())).list;
    let line = &polylines(&list, "well", "well")[0];
    let spans = polylines(&list, "well", "span");
    assert_eq!(spans.len(), 1);
    let width = |points: &[[f64; 2]]| {
        let xs = points.iter().map(|p| p[0]);
        xs.clone().fold(f64::NEG_INFINITY, f64::max) - xs.fold(f64::INFINITY, f64::min)
    };
    let share = width(&spans[0]) / width(line);
    assert!(
        (share - 0.6).abs() < 1.0e-6,
        "the span covers {share} of the line"
    );
    // A trajectory alone carries no data: no span.
    let alone = drawn(50.0, ViewAsk::new("well-locator").object(trajectory_uri())).list;
    assert!(polylines(&alone, "well", "span").is_empty());
    // Break it: highlight the frame's extent read from the trajectory's MD interval.
}

#[test]
fn in_plan_the_well_heads_east_at_one_scale() {
    // The trajectory goes 50 m east and none north: in plan, a horizontal line, left to right.
    let list = drawn(
        50.0,
        ViewAsk::new("well-locator")
            .object(frame_uri())
            .set("mode", json!("plan")),
    )
    .list;
    let line = &polylines(&list, "well", "well")[0];
    let (first, last) = (line[0], line[line.len() - 1]);
    assert!(
        (first[1] - last[1]).abs() < 1.0e-6,
        "northing does not change: {first:?} {last:?}"
    );
    assert!(last[0] > first[0], "easting grows to the right");
    // Break it: lay plan with northing down the page (y_down), and a north-going well would go down.
}

#[test]
fn a_depth_grows_down_the_page() {
    let list = drawn(50.0, ViewAsk::new("well-locator").object(frame_uri())).list;
    let line = &polylines(&list, "well", "well")[0];
    assert!(
        line[line.len() - 1][1] > line[0][1],
        "the bottom of the hole is lower on the page"
    );
    // Break it: lay MD against TVD with y up.
}

#[test]
fn a_vertical_section_needs_its_azimuth_and_never_guesses_one() {
    let opened = Opened::of(package(50.0, &[20.0, 80.0]));
    let kinds = ViewKinds::with_builtins();
    let cx = opened.context(&NO_MAPS);
    let refused = draw_list(
        &kinds,
        &ViewAsk::new("well-locator")
            .object(frame_uri())
            .set("mode", json!("vertical_section")),
        &cx,
    )
    .expect_err("refused");
    assert!(
        matches!(&refused, ViewRefusal::MissingSetting { name, .. } if name == "azimuth"),
        "{refused}"
    );
    // An azimuth of 90 (east) lays the eastward well along its whole 50 m.
    let list = draw_list(
        &kinds,
        &ViewAsk::new("well-locator")
            .object(frame_uri())
            .set("mode", json!("vertical_section"))
            .set("azimuth", json!(90.0)),
        &cx,
    )
    .expect("drawn")
    .0
    .list;
    let line = &polylines(&list, "well", "well")[0];
    assert!(line[line.len() - 1][0] > line[0][0], "the section runs east");
    // Break it: take an azimuth of 0 when none is given.
}
