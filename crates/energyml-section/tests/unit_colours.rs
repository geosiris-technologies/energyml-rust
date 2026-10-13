//! The colour of a unit **now**, and the tick at a change of unit.
//!
//! After the colour of a resqml22.StratigraphicUnitInterpretation was changed,
//! the section colours were not triggered by the update and the legends of the
//! 3D scene (up to date) and of the sections differed -
//! the intervals carried the colour baked in when the catalogue was made, so a
//! colour edited since never reached the drawing. `View.unit_colours` is the table
//! the wells **and** the legend read first; and the black tick at the
//! stratigraphic changes is `View.strata_ticks`.
//!
//! To make them fail: read `iv.colour` before the table in `layout::own_colour`
//! (the first two), or draw a tick at every interval's start (the last).

use crate::layout::{catalogue, view};
use crate::strata_legend::{
    El, elements, entries_of_the_legend, inside, interval, on, strata_of_the_wells, well_with,
};
use energyml_section::layout::layout;
use energyml_section::model::*;
use energyml_section::svg;

fn named(unit: &str, colour: &str, uri: &str) -> Interval {
    let mut one = interval(0.0, 0.0, Some(colour), unit);
    one.unit_uri = Some(uri.to_string());
    one
}

/// One well, two units: Top (0-100, red) then Mid (100-200, green), the catalogue's colours.
fn baked() -> Catalogue {
    let mut top = named(
        "Top",
        "#aa0000",
        "eml:///resqml22.StratigraphicUnitInterpretation(top)",
    );
    top.from = 0.0;
    top.to = 100.0;
    let mut mid = named(
        "Mid",
        "#00aa00",
        "eml:///resqml22.StratigraphicUnitInterpretation(mid)",
    );
    mid.from = 100.0;
    mid.to = 200.0;
    catalogue(vec![well_with("A", 150.0, vec![top, mid])])
}

fn key(uri: &str) -> String {
    object_key("s1", uri)
}

#[test]
fn a_colour_told_now_is_the_one_the_wells_and_the_legend_are_drawn_in() {
    let mut now = on();
    now.unit_colours = vec![(
        key("eml:///resqml22.StratigraphicUnitInterpretation(top)"),
        "#0000ff".into(),
    )];
    let els = elements(&svg::write(&layout(&baked(), &now)));
    let wells = strata_of_the_wells(&els);
    assert!(
        wells.contains(&("Top".to_string(), "#0000ff".to_string())),
        "the well: {wells:?}"
    );
    assert!(
        wells.contains(&("Mid".to_string(), "#00aa00".to_string())),
        "a unit not told keeps its baked colour: {wells:?}"
    );
    let legend = entries_of_the_legend(&els);
    assert!(
        legend.contains(&("Top".to_string(), "#0000ff".to_string())),
        "the legend says what the wells say: {legend:?}"
    );
    assert!(
        !legend.iter().any(|(_, colour)| colour == "#aa0000"),
        "the baked colour is nowhere: {legend:?}"
    );
    // The two read one table: every legend entry is the colour of a well path of that unit.
    for (unit, colour) in &legend {
        assert!(wells.contains(&(unit.clone(), colour.clone())), "{unit} {colour}");
    }
}

#[test]
fn without_the_table_the_drawing_is_what_it_was_byte_for_byte() {
    let plain = on();
    let mut empty = on();
    empty.unit_colours = Vec::new();
    let a = svg::write(&layout(&baked(), &plain));
    assert_eq!(a, svg::write(&layout(&baked(), &empty)));
    assert!(
        !a.contains("unitColours"),
        "not written into the metadata when empty"
    );
    assert!(!a.contains("strataTicks"), "nor is the option, off");
    assert!(
        !serde_json::to_string(&crate::strata_legend::catalogue_of_two())
            .unwrap()
            .contains("unitUri"),
        "an interval that names no unit writes no field"
    );
}

#[test]
fn a_unit_of_another_package_is_not_told_the_colour_of_this_one() {
    let mut now = on();
    now.unit_colours = vec![(
        object_key("s2", "eml:///resqml22.StratigraphicUnitInterpretation(top)"),
        "#0000ff".into(),
    )];
    let wells = strata_of_the_wells(&elements(&svg::write(&layout(&baked(), &now))));
    assert!(
        wells.contains(&("Top".to_string(), "#aa0000".to_string())),
        "{wells:?}"
    );
}

fn ticks_of(svg_text: &str) -> Vec<El> {
    elements(svg_text)
        .into_iter()
        .filter(|e| e.name == "line" && inside(e, "kind=well"))
        .filter(|e| e.attr("stroke") == Some("#000000"))
        .collect()
}

#[test]
fn a_tick_crosses_the_well_at_each_boundary_between_units_when_asked() {
    let mut cat = baked();
    // A third unit after a gap: no boundary there, a gap is not a change of unit.
    if let Item::Well(w) = &mut cat.long.items[0] {
        let mut last = named(
            "Last",
            "#0000aa",
            "eml:///resqml22.StratigraphicUnitInterpretation(last)",
        );
        last.from = 300.0;
        last.to = 350.0;
        w.intervals.push(last);
    }
    let mut asked = on();
    asked.strata_ticks = true;
    let text = svg::write(&layout(&cat, &asked));
    let ticks = ticks_of(&text);
    assert_eq!(ticks.len(), 1, "one boundary (Top|Mid): {ticks:?}");
    let t = &ticks[0];
    let (x1, y1, x2, y2): (f64, f64, f64, f64) = (
        t.attr("x1").unwrap().parse().unwrap(),
        t.attr("y1").unwrap().parse().unwrap(),
        t.attr("x2").unwrap().parse().unwrap(),
        t.attr("y2").unwrap().parse().unwrap(),
    );
    // The well is drawn between x = 150 and 150 + (200 - 0)/320*... : it goes down the front end, so the tick is across it.
    assert!(
        (y1 - y2).abs() < 1e-6 || (x1 - x2).abs() > 1.0,
        "across a vertical-ish path, not along it"
    );
    assert!(
        ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt() > 4.0,
        "wider than nothing"
    );
    assert!(
        text.contains("data-part=\"strata-ticks\""),
        "in a group of its own, inside the well's"
    );
    // In the option's metadata, so a redraw keeps it.
    assert!(svg::read_metadata(&text).unwrap().view.strata_ticks);
    // Off: none, and the bytes are those of before.
    let off = svg::write(&layout(&cat, &on()));
    assert!(ticks_of(&off).is_empty() && !off.contains("strata-ticks"));
}

/// With the zoom script on, a tick is counter-scaled around its middle, as a
/// dot is: zoomed in, it stays a few pixels across the well instead of growing
/// with the drawing (the zoom script used to ignore the black ticks).
/// A well's interval and the legend entry of its unit carry the unit's full
/// uri (`data-unit-uri`): the front end finds a unit by it - to match a hovered
/// interval to its legend entry, to open the unit's colour - and never by its
/// title (internally only the full uri is used: ADR-0003).
#[test]
fn intervals_and_legend_entries_name_their_unit_by_uri() {
    let text = svg::write(&layout(&baked(), &on()));
    let els = elements(&text);
    let intervals: Vec<&El> = els
        .iter()
        .filter(|e| e.name == "g" && e.attr("data-unit").is_some())
        .collect();
    assert!(!intervals.is_empty());
    for iv in &intervals {
        let uri = iv
            .attr("data-unit-uri")
            .expect("an interval names its unit by uri");
        assert!(
            uri.starts_with("eml:///resqml22.StratigraphicUnitInterpretation("),
            "{uri}"
        );
    }
    let entries: Vec<&El> = els
        .iter()
        .filter(|e| e.name == "g" && e.attr("data-decor") == Some("strata-entry"))
        .collect();
    assert!(!entries.is_empty(), "the legend is on");
    for entry in &entries {
        let uri = entry
            .attr("data-unit-uri")
            .expect("a legend entry names its unit by uri");
        assert!(
            intervals.iter().any(|iv| iv.attr("data-unit-uri") == Some(uri)),
            "{uri} is the uri of a drawn interval"
        );
    }
}

#[test]
fn a_tick_keeps_its_pixel_size_when_the_drawing_zooms() {
    let mut asked = on();
    asked.strata_ticks = true;
    assert!(asked.zoom_script, "the script is on by default");
    let text = svg::write(&layout(&baked(), &asked));
    let ticks = ticks_of(&text);
    assert!(!ticks.is_empty());
    for t in &ticks {
        let n = |name: &str| -> f64 { t.attr(name).unwrap().parse().unwrap() };
        let at: Vec<f64> = t
            .attr("data-zoom-fixed")
            .expect("a tick is zoom-fixed")
            .split(' ')
            .map(|v| v.parse().unwrap())
            .collect();
        assert!(
            (at[0] - (n("x1") + n("x2")) / 2.0).abs() < 0.01
                && (at[1] - (n("y1") + n("y2")) / 2.0).abs() < 0.01,
            "around its middle"
        );
    }
    // Without the script, nothing to counter-scale: the bytes of before.
    asked.zoom_script = false;
    assert!(
        ticks_of(&svg::write(&layout(&baked(), &asked)))
            .iter()
            .all(|t| t.attr("data-zoom-fixed").is_none())
    );
}

#[test]
fn no_tick_on_a_well_that_is_not_coloured_by_strata() {
    let mut side = on();
    side.well_colouring = WellColouring::Side;
    side.strata_ticks = true;
    assert!(ticks_of(&svg::write(&layout(&baked(), &side))).is_empty());
}

/// The same unit, Top, in a well of `s1` and a well of `s2` - one uri, as two
/// packages written against one chart carry it.
fn two_packages() -> Catalogue {
    let mut top_a = named(
        "Top",
        "#aa0000",
        "eml:///resqml22.StratigraphicUnitInterpretation(top)",
    );
    top_a.to = 100.0;
    let mut top_b = top_a.clone();
    top_b.to = 150.0;
    let mut b = well_with("B", 700.0, vec![top_b]);
    if let Item::Well(well) = &mut b {
        well.object.store = "s2".into();
    }
    catalogue(vec![well_with("A", 150.0, vec![top_a]), b])
}

/// `(title, data-unit-stores)` of every legend entry.
fn legend_stores(els: &[El]) -> Vec<(String, String)> {
    let titles = entries_of_the_legend(els);
    let stores: Vec<String> = els
        .iter()
        .filter(|e| {
            e.name == "g" && e.attr("data-kind") == Some("strata-entry") && inside(e, "id=strata-legend")
        })
        .map(|e| e.attr("data-unit-stores").unwrap_or_default().to_string())
        .collect();
    titles.into_iter().map(|(title, _)| title).zip(stores).collect()
}

/// Changing the colour of the second "Lutétien" of the
/// legend changed the first - the entry named its unit and not its package.
/// An entry now says **which packages** show the unit in its colour: one line
/// for packages that agree, carrying both.
///
/// To make it fail: give `StrataEntry` one store and stop merging the second.
#[test]
fn packages_that_agree_on_a_unit_are_one_entry_carrying_both() {
    let els = elements(&svg::write(&layout(&two_packages(), &on())));
    assert_eq!(
        legend_stores(&els),
        vec![("Top".to_string(), "s1 s2".to_string())]
    );
}

/// …and two lines for packages that do not, each with its package - named in
/// the title, because two "Top" entries with nothing to tell them apart would be
/// ambiguous.
///
/// To make it fail: write the unit's title alone in `strata_titles`.
#[test]
fn packages_that_disagree_on_a_unit_are_two_entries_each_named_by_its_package() {
    let mut now = on();
    now.unit_colours = vec![(
        object_key("s2", "eml:///resqml22.StratigraphicUnitInterpretation(top)"),
        "#0000ff".into(),
    )];
    now.store_names = vec![("s2".into(), "second.epc".into())];
    let els = elements(&svg::write(&layout(&two_packages(), &now)));
    assert_eq!(
        legend_stores(&els),
        vec![
            ("Top - s1".to_string(), "s1".to_string()),
            ("Top - second.epc".to_string(), "s2".to_string())
        ],
        "a package with no name is named by its id"
    );
    // A unit shown once says nothing of its package.
    let alone = elements(&svg::write(&layout(&baked(), &now)));
    assert!(
        entries_of_the_legend(&alone)
            .iter()
            .all(|(title, _)| !title.contains(" - "))
    );
}

#[test]
fn the_view_of_a_document_that_predates_them_reads_both_off() {
    let json = serde_json::to_value(view()).unwrap();
    let read: View = serde_json::from_value(json).unwrap();
    assert!(!read.strata_ticks && read.unit_colours.is_empty());
}
