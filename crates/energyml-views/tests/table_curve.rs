//! `table-curve`: the curves of a KrPc table, the pairs, the two ordinate axes, the units, the colours, the refusals.
//!
//! Every package is built in memory: a KrPc table (the saturation as the key column, two relative permeabilities
//! with no unit, a capillary pressure in bar and the same pressure in kPa). What is drawn is confronted with what
//! the library reads (`Table::read_cells`, `curve_pairs`, `units::convert`, the categorical palette).

use std::io::Cursor;
use std::sync::Arc;

use energyml_core::registry::DynObject;
use energyml_domain::ObjectSource;
use energyml_domain::palettes::{Palettes, Reading};
use energyml_domain::table::{Cell, CellState, Table, classify_column, curve_pairs};
use energyml_domain::units;
use energyml_eml_2_3 as eml23;
use energyml_sheet::draw::{DisplayList, Element, Item};
use energyml_storage::EpcStore;
use energyml_views::table_curve::TableCurve;
use energyml_views::{Drawn, ViewAsk, ViewKinds, ViewRefusal, draw_list, draw_svg, redraw};
use serde_json::json;

use crate::fixtures::{NO_MAPS, Opened};

const ID: &str = "table-curve";

fn kinds() -> ViewKinds {
    let mut kinds = ViewKinds::empty();
    kinds.register(Box::new(TableCurve));
    kinds
}

fn u(n: u32) -> String {
    format!("00000000-0000-4000-8000-{n:012x}")
}

fn empty_store() -> EpcStore {
    EpcStore::from_reader(
        Box::new(Cursor::new(
            energyml_storage::extract::empty_package(None).expect("an empty package"),
        )),
        "curves.epc",
        Arc::new(energyml_schemas::all()),
    )
    .expect("it opens")
}

fn put(store: &mut EpcStore, object: impl DynObject + 'static) {
    store.put(Box::new(object)).expect("it goes in");
}

/// A column of numbers; NaN is an absence.
fn numbers(title: &str, uom: Option<eml23::UnitOfMeasureExt>, values: &[f64]) -> eml23::Column {
    eml23::Column {
        title: Some(title.to_string()),
        uom,
        value_count_per_row: 1,
        values: Some(eml23::AbstractValueArray::FloatingPointXmlArray(
            eml23::FloatingPointXmlArray {
                values: values.to_vec(),
                count_per_value: 1,
                ..Default::default()
            },
        )),
        ..Default::default()
    }
}

/// A column of text.
fn words(title: &str, values: &[&str]) -> eml23::Column {
    eml23::Column {
        title: Some(title.to_string()),
        value_count_per_row: 1,
        values: Some(eml23::AbstractValueArray::StringXmlArray(eml23::StringXmlArray {
            values: values.iter().map(|text| text.to_string()).collect(),
            count_per_value: 1,
            ..Default::default()
        })),
        ..Default::default()
    }
}

fn table(store: &mut EpcStore, n: u32, title: &str, keys: Vec<eml23::Column>, columns: Vec<eml23::Column>) {
    put(
        store,
        eml23::ColumnBasedTable {
            uuid: u(n),
            schema_version: "2.3".into(),
            citation: eml23::Citation {
                title: title.to_string(),
                ..Default::default()
            },
            key_column: keys,
            column: columns,
            ..Default::default()
        },
    );
}

const SW: [f64; 4] = [0.2, 0.4, 0.6, 0.8];
const KRW: [f64; 4] = [0.0, 0.1, 0.3, 0.6];
const KRO: [f64; 4] = [0.9, 0.5, 0.2, 0.0];
const PC_BAR: [f64; 4] = [3.0, 2.0, 1.0, 0.5];
const PC_KPA: [f64; 4] = [300.0, 200.0, 100.0, 50.0];

/// - 1 `KrPc`: key Sw; Krw, Kro (no unit), Pc (bar), PcKpa (kPa): the columns 0 to 4;
/// - 2 `facies`: text only, no curve;
/// - 3 `holes`: key Sw; Krw with an absence at the row 1.
fn store() -> EpcStore {
    use eml23::UnitOfMeasureExt::{Bar, Euc, KPa};
    let mut store = empty_store();
    table(
        &mut store,
        1,
        "KrPc",
        vec![numbers("Sw", Some(Euc), &SW)],
        vec![
            numbers("Krw", Some(Euc), &KRW),
            numbers("Kro", Some(Euc), &KRO),
            numbers("Pc", Some(Bar), &PC_BAR),
            numbers("PcKpa", Some(KPa), &PC_KPA),
        ],
    );
    table(
        &mut store,
        2,
        "facies",
        vec![words("code", &["a", "b"])],
        vec![words("name", &["sand", "shale"])],
    );
    table(
        &mut store,
        3,
        "holes",
        vec![numbers("Sw", Some(Euc), &SW)],
        vec![numbers("Krw", Some(Euc), &[0.0, f64::NAN, 0.3, 0.6])],
    );
    store
}

fn opened() -> Opened {
    Opened::of(store())
}

fn uri_in(opened: &Opened, n: u32) -> String {
    opened
        .index
        .uri_of(&u(n))
        .expect("the package holds it")
        .to_string()
}

fn ask(opened: &Opened, n: u32) -> ViewAsk {
    ViewAsk::new(ID).object(uri_in(opened, n))
}

fn drawn(opened: &Opened, ask: ViewAsk) -> Drawn {
    draw_list(&kinds(), &ask, &opened.context(&NO_MAPS))
        .expect("drawn")
        .0
}

fn refused(opened: &Opened, ask: ViewAsk) -> ViewRefusal {
    draw_list(&kinds(), &ask, &opened.context(&NO_MAPS)).expect_err("refused")
}

fn elements<'a>(list: &'a DisplayList, layer: &str) -> &'a [Element] {
    &list.layer(layer).expect("the layer").elements
}

fn of_kind<'a>(list: &'a DisplayList, layer: &str, kind: &str) -> Vec<&'a Element> {
    elements(list, layer)
        .iter()
        .filter(|element| element.kind == kind)
        .collect()
}

fn line_of(element: &Element) -> Vec<[f64; 2]> {
    element
        .items
        .iter()
        .find_map(|item| match item {
            Item::Polyline { points, .. } => Some(points.clone()),
            _ => None,
        })
        .expect("a polyline")
}

fn stroke_colour(element: &Element) -> [u8; 3] {
    element
        .items
        .iter()
        .find_map(|item| match item {
            Item::Polyline { stroke, .. } => Some([stroke.colour.r, stroke.colour.g, stroke.colour.b]),
            _ => None,
        })
        .expect("a polyline")
}

fn texts_of(list: &DisplayList, layer: &str) -> Vec<String> {
    elements(list, layer)
        .iter()
        .flat_map(|element| element.items.iter())
        .filter_map(|item| match item {
            Item::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// The curve keyed `#col=<y>`.
fn curve<'a>(list: &'a DisplayList, uri: &str, y: usize) -> &'a Element {
    let key = format!("{uri}#col={y}");
    of_kind(list, "curves", "curve")
        .into_iter()
        .find(|element| element.key.as_deref() == Some(key.as_str()))
        .unwrap_or_else(|| panic!("no curve `{key}`"))
}

/// The numbers of the cells of a column, as the library reads them.
fn cells_of(opened: &Opened, uri: &str, column: usize) -> Vec<Option<f64>> {
    let object = opened.store.read_object_at(uri).expect("the table");
    let table = Table::read(object.as_reflect()).expect("a table");
    let cells = table.read_cells(&opened.store);
    let read = cells[column].as_ref().expect("the column reads");
    let rows = read.rows("column").expect("rows");
    (0..rows)
        .map(|row| match read.cell(row) {
            Some(CellState::Value(Cell::Number(value))) => Some(value),
            _ => None,
        })
        .collect()
}

/// That the page positions are an affine image of the data: every point sits where the first and the last say.
fn assert_affine(page: &[f64], data: &[f64], what: &str) {
    let (p0, p1) = (page[0], page[page.len() - 1]);
    let (d0, d1) = (data[0], data[data.len() - 1]);
    for (at, (p, d)) in page.iter().zip(data).enumerate() {
        let expected = p0 + (d - d0) / (d1 - d0) * (p1 - p0);
        assert!(
            (p - expected).abs() < 1e-6,
            "{what}: row {at} is at {p}, expected {expected}"
        );
    }
}

#[test]
fn a_table_with_a_curve_is_offered_and_one_without_is_not() {
    let opened = opened();
    let offers = kinds().offers(&opened.index);
    let objects: Vec<&str> = offers.iter().map(|offer| offer.objects[0].as_str()).collect();
    assert!(objects.contains(&uri_in(&opened, 1).as_str()), "{objects:?}");
    assert!(objects.contains(&uri_in(&opened, 3).as_str()), "{objects:?}");
    assert!(
        !objects.contains(&uri_in(&opened, 2).as_str()),
        "text only: {objects:?}"
    );
    assert_eq!(offers.len(), 2);
    assert!(
        offers
            .iter()
            .all(|offer| offer.missing.is_empty() && offer.view == ID)
    );
    // Break it: offer every table, or none.
}

#[test]
fn a_curve_per_proposed_pair_with_the_points_the_cells_give() {
    let opened = opened();
    let uri = uri_in(&opened, 1);
    let list = drawn(&opened, ask(&opened, 1)).list;
    // The pairs are the library's own proposal over the same sheet.
    let sheet = opened.index.table_sheet(&uri).expect("a sheet");
    let classes: Vec<_> = sheet
        .columns
        .iter()
        .map(|column| classify_column(column, None, None))
        .collect();
    let proposed = curve_pairs(sheet, &classes);
    assert_eq!(proposed, vec![(0, 1), (0, 2), (0, 3), (0, 4)]);
    assert_eq!(of_kind(&list, "curves", "curve").len(), proposed.len());
    for (x, y) in proposed {
        let element = curve(&list, &uri, y);
        let line = line_of(element);
        let (xs, ys) = (cells_of(&opened, &uri, x), cells_of(&opened, &uri, y));
        assert_eq!(line.len(), xs.len(), "one vertex per row");
        // The vertices are the cells in the order of the rows: the page is an affine image of them.
        let page_x: Vec<f64> = line.iter().map(|p| p[0]).collect();
        let data_x: Vec<f64> = xs.iter().map(|v| v.expect("a number")).collect();
        assert_affine(&page_x, &data_x, "abscissa");
        let page_y: Vec<f64> = line.iter().map(|p| p[1]).collect();
        let data_y: Vec<f64> = ys.iter().map(|v| v.expect("a number")).collect();
        assert_affine(&page_y, &data_y, "ordinate");
        // And the points are keyed by their row, at the vertices.
        let points: Vec<&Element> = of_kind(&list, "curves", "point")
            .into_iter()
            .filter(|point| {
                point
                    .key
                    .as_deref()
                    .is_some_and(|key| key.starts_with(&format!("{uri}#col={y}&row=")))
            })
            .collect();
        assert_eq!(points.len(), line.len());
        for (row, point) in points.iter().enumerate() {
            assert_eq!(
                point.key.as_deref(),
                Some(format!("{uri}#col={y}&row={row}").as_str())
            );
            let Item::Symbol { at, .. } = &point.items[0] else {
                panic!("a symbol")
            };
            assert_eq!(*at, line[row]);
        }
    }
    // Break it: sort the points by x, or draw the columns in another order.
}

#[test]
fn two_families_of_units_get_two_axes_and_a_legend_per_column() {
    let opened = opened();
    let list = drawn(&opened, ask(&opened, 1)).list;
    assert!(
        list.layer("axis-right").is_some(),
        "no unit and a pressure: two axes"
    );
    let right = texts_of(&list, "axis-right");
    assert!(
        right
            .iter()
            .any(|text| text.contains("Pc") && text.contains("bar")),
        "{right:?}"
    );
    let left = texts_of(&list, "axes");
    assert!(left.iter().any(|text| text == "Krw, Kro"), "{left:?}");
    assert!(left.iter().any(|text| text == "Sw"), "{left:?}");
    let legend = texts_of(&list, "legend");
    assert_eq!(legend.len(), 5, "four columns and the colours: {legend:?}");
    assert!(
        legend[0].starts_with("Krw (Euc)") && legend[0].contains("Sw"),
        "{legend:?}"
    );
    assert!(
        legend[2].starts_with("Pc (bar)") && legend[2].contains("right axis"),
        "{legend:?}"
    );
    assert!(legend[3].starts_with("PcKpa (kPa)"), "{legend:?}");
    // The pressures are one family: the capillary pressure in kPa goes on the same axis, in bar.
    assert!(legend[4].contains("palette"), "{legend:?}");
    // Break it: one axis per column, or a second axis for a single family.
}

#[test]
fn a_single_family_has_no_second_axis() {
    let opened = opened();
    let list = drawn(&opened, ask(&opened, 1).set("pairs", json!([[0, 1], [0, 2]]))).list;
    assert!(list.layer("axis-right").is_none());
    assert_eq!(of_kind(&list, "curves", "curve").len(), 2);
    // Break it: always add the second axis.
}

#[test]
fn a_column_in_another_unit_of_the_family_is_converted_to_the_first() {
    let opened = opened();
    let uri = uri_in(&opened, 1);
    let list = drawn(&opened, ask(&opened, 1).set("pairs", json!([[0, 3], [0, 4]]))).list;
    assert!(list.layer("axis-right").is_none(), "both are pressures");
    let (bar, kpa) = (line_of(curve(&list, &uri, 3)), line_of(curve(&list, &uri, 4)));
    // The same pressure written in kPa lands where the one in bar does: `units::convert` is the one conversion.
    let converted: Vec<f64> = cells_of(&opened, &uri, 4)
        .iter()
        .map(|v| units::convert(v.expect("a number"), "kPa", "bar").expect("convertible"))
        .collect();
    let in_bar: Vec<f64> = cells_of(&opened, &uri, 3)
        .iter()
        .map(|v| v.expect("a number"))
        .collect();
    for (a, b) in converted.iter().zip(&in_bar) {
        assert!((a - b).abs() < 1e-9, "{a} against {b}");
    }
    assert_eq!(bar.len(), kpa.len());
    for (a, b) in bar.iter().zip(&kpa) {
        assert!(
            (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-6,
            "{a:?} against {b:?}"
        );
    }
    // Break it: draw the kPa values as they are.
}

#[test]
fn a_pairs_setting_wins_over_the_proposal() {
    let opened = opened();
    let uri = uri_in(&opened, 1);
    // By rank, and by title.
    for pairs in [json!([[1, 2]]), json!([["Krw", "Kro"]])] {
        let list = drawn(&opened, ask(&opened, 1).set("pairs", pairs)).list;
        let curves = of_kind(&list, "curves", "curve");
        assert_eq!(curves.len(), 1);
        assert_eq!(curves[0].key.as_deref(), Some(format!("{uri}#col=2").as_str()));
        // Kro against Krw: the abscissa is Krw's cells, not the key column's.
        let page_x: Vec<f64> = line_of(curves[0]).iter().map(|p| p[0]).collect();
        let data_x: Vec<f64> = cells_of(&opened, &uri, 1)
            .iter()
            .map(|v| v.expect("a number"))
            .collect();
        assert_affine(&page_x, &data_x, "abscissa");
    }
    // Break it: ignore the setting.
}

#[test]
fn a_bad_pairs_setting_is_refused_by_its_name() {
    let opened = opened();
    for (pairs, word) in [
        (json!([[0, 9]]), "no column 9"),
        (json!([["Nope", "Krw"]]), "no column titled"),
        (json!([[1, 1]]), "twice"),
        (json!([]), "empty"),
        (json!("Krw"), "a list"),
        (json!([[1, 2, 3]]), "not a couple"),
    ] {
        match refused(&opened, ask(&opened, 1).set("pairs", pairs.clone())) {
            ViewRefusal::BadSetting { name, why } => {
                assert_eq!(name, "pairs");
                assert!(why.contains(word), "{pairs}: {why}");
            }
            other => panic!("{pairs}: {other}"),
        }
    }
    // Break it: take an unknown column as the last one.
}

#[test]
fn a_pair_with_a_text_column_is_refused_and_so_is_a_table_with_no_pair() {
    let opened = opened();
    match refused(&opened, ask(&opened, 2).set("pairs", json!([[0, 1]]))) {
        ViewRefusal::Computing { view, why } => {
            assert_eq!(view, ID);
            assert!(why.contains("not a number"), "{why}");
        }
        other => panic!("{other}"),
    }
    match refused(&opened, ask(&opened, 2)) {
        ViewRefusal::Computing { view, why } => {
            assert_eq!(view, ID);
            assert!(
                why.contains("no pair of numeric columns") && why.contains("facies"),
                "{why}"
            );
        }
        other => panic!("{other}"),
    }
    // Break it: draw an empty plot for the table of texts.
}

#[test]
fn an_absent_cell_leaves_its_row_out_and_the_picture_says_so() {
    let opened = opened();
    let uri = uri_in(&opened, 3);
    let drawn = drawn(&opened, ask(&opened, 3));
    let rows = cells_of(&opened, &uri, 1);
    assert_eq!(
        rows.iter().filter(|v| v.is_none()).count(),
        1,
        "the library reads one absence"
    );
    let points = of_kind(&drawn.list, "curves", "point");
    assert_eq!(points.len(), 3);
    assert!(
        !points
            .iter()
            .any(|point| point.key.as_deref() == Some(format!("{uri}#col=1&row=1").as_str())),
        "the absent row is not drawn"
    );
    let notes: Vec<&str> = drawn
        .notes
        .iter()
        .filter(|note| note.key == "rows-not-drawn")
        .map(|note| note.says.as_str())
        .collect();
    assert_eq!(notes.len(), 1);
    assert!(notes[0].contains("1 rows"), "{notes:?}");
    // Break it: draw the absence as a zero.
}

#[test]
fn the_curves_take_the_categorical_palette_in_turn_and_a_palette_by_name() {
    let opened = opened();
    let uri = uri_in(&opened, 1);
    let palettes = Palettes::new();
    let default = palettes
        .default_for(Reading::Categorical)
        .expect("a categorical palette");
    let list = drawn(&opened, ask(&opened, 1)).list;
    for (rank, y) in (1..=4).enumerate() {
        assert_eq!(
            stroke_colour(curve(&list, &uri, y)),
            default.category(rank),
            "curve {y}"
        );
    }
    assert!(
        texts_of(&list, "legend")
            .last()
            .expect("a line")
            .contains(default.name)
    );
    // Another categorical palette, by name (any but the default).
    let other = palettes
        .for_reading(Reading::Categorical)
        .into_iter()
        .find(|palette| palette.name != default.name && palette.category(0) != default.category(0))
        .expect("a second categorical palette");
    let list = drawn(&opened, ask(&opened, 1).set("palette", json!(other.name))).list;
    assert_eq!(stroke_colour(curve(&list, &uri, 1)), other.category(0));
    assert!(
        texts_of(&list, "legend")
            .last()
            .expect("a line")
            .contains(other.name)
    );
    // An unknown palette and a palette for a ramp are refused by their names.
    let continuous = palettes
        .for_reading(Reading::Continuous)
        .into_iter()
        .next()
        .expect("a ramp");
    for name in ["no-such-palette", continuous.name] {
        match refused(&opened, ask(&opened, 1).set("palette", json!(name))) {
            ViewRefusal::Palette { asked, .. } => assert_eq!(asked, name),
            other => panic!("{other}"),
        }
    }
    // Break it: colour the curves with the continuous palette, or all with the first colour.
}

#[test]
fn a_redraw_makes_the_same_bytes() {
    let opened = opened();
    let cx = opened.context(&NO_MAPS);
    let first = draw_svg(&kinds(), &ask(&opened, 1), &cx).expect("drawn");
    let again = redraw(&kinds(), &first.svg, &cx).expect("drawn again");
    assert_eq!(first.svg, again.svg);
    // Break it: let a default that moved change the drawing.
}
