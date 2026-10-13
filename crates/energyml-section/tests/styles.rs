//! The style of the portions of a well with no strata information (several
//! styles are offered: fixed colour, texture, dashes…, and the front end must
//! adapt dynamically to the styles proposed).
//!
//! What a style draws is confronted with the unstratified spans **computed here
//! from the catalogue's intervals** (a second reading, by sampling, not the
//! layout's walk), laid on the well's path and turned into pixels by the SVG's own
//! `data-frame` / `data-window` / `data-scale` — the map the front end's overlay reads
//! a pointer through.

use crate::layout::{catalogue, id, view};
use crate::strata_legend::{El, catalogue_of_two, elements, inside, interval, well_with};
use energyml_section::layout::layout;
use energyml_section::model::*;
use energyml_section::style::{self, ParamKind};
use energyml_section::svg;

fn strata_view() -> View {
    let mut v = view();
    v.well_colouring = WellColouring::Strata;
    v
}

fn styled(style: UnstratifiedStyle) -> View {
    let mut v = strata_view();
    v.unstratified = style;
    v
}

fn dashed() -> UnstratifiedStyle {
    UnstratifiedStyle::Dashed { colour: "#999999".into(), dash: 6.0, gap: 4.0 }
}

fn hatched() -> UnstratifiedStyle {
    UnstratifiedStyle::Hatched { colour: "#aa5500".into(), spacing: 5.0, angle: 30.0 }
}

fn well_of<'a>(cat: &'a Catalogue, name: &str) -> &'a WellItem {
    let Some(Item::Well(w)) = cat.long.items.iter().find(|i| matches!(i, Item::Well(w) if w.object.title == name)) else {
        panic!("no well {name}")
    };
    w
}

/// The spans of MD no interval covers, between the first and last MD — computed
/// by **sampling**, not by the layout's walk: every MD at a 0.01 step is
/// covered or not, and runs of the uncovered are the spans.
fn uncovered(w: &WellItem) -> Vec<(f64, f64)> {
    let md = w.md.as_ref().unwrap();
    let (lo, hi) = (md[0], *md.last().unwrap());
    let covered = |m: f64| w.intervals.iter().any(|i| i.from <= m && m <= i.to);
    let mut out: Vec<(f64, f64)> = Vec::new();
    let mut open: Option<f64> = None;
    let steps = ((hi - lo) / 0.01).round() as i64;
    for k in 0..=steps {
        let m = lo + k as f64 * 0.01;
        // Uncovered on both sides of the sample: strictly inside a gap.
        let in_gap = !covered(m + 0.005) && !covered(m - 0.005);
        match (in_gap, open) {
            (true, None) => open = Some(m),
            (false, Some(a)) => {
                out.push((a, m));
                open = None;
            }
            _ => {}
        }
    }
    if let Some(a) = open {
        out.push((a, hi));
    }
    out
}

/// The point of the well's path at an MD, by linear interpolation of its nodes.
fn point_at(w: &WellItem, m: f64) -> [f64; 2] {
    let md = w.md.as_ref().unwrap();
    for i in 0..md.len() - 1 {
        if m >= md[i] && m <= md[i + 1] {
            let t = (m - md[i]) / (md[i + 1] - md[i]);
            return [w.path[i][0] + t * (w.path[i + 1][0] - w.path[i][0]), w.path[i][1] + t * (w.path[i + 1][1] - w.path[i][1])];
        }
    }
    panic!("md {m} off the path")
}

fn numbers(text: &str) -> Vec<f64> {
    text.split([' ', ',']).filter(|t| !t.is_empty()).map(|t| t.parse().unwrap()).collect()
}

/// The pixel of a drawing's point, from what the SVG says of its own frame.
fn to_px(root: &El, p: [f64; 2]) -> [f64; 2] {
    let frame = numbers(root.attr("data-frame").unwrap());
    let win = numbers(root.attr("data-window").unwrap());
    let scale = numbers(root.attr("data-scale").unwrap());
    [frame[0] + (p[0] - win[0]) * scale[0], frame[1] + frame[3] - (p[1] - win[2]) * scale[1]]
}

/// The polylines of a well's group that lie under no interval group — the line
/// the style draws — with their attributes.
fn loose(els: &[El], name: &str) -> Vec<El> {
    let key = id(name, "WellboreFrameRepresentation").key().replace('\0', "\u{2400}");
    let start = els
        .iter()
        .position(|e| e.name == "g" && e.attr("data-key") == Some(key.as_str()) && e.attr("data-kind") == Some("well"))
        .expect("the well is drawn");
    let depth = els[start].path.len();
    let mut out = Vec::new();
    for i in start + 1..els.len() {
        if els[i].path.len() <= depth {
            break;
        }
        if els[i].name == "polyline" && !(els[i - 1].name == "g" && els[i - 1].attr("data-unit").is_some()) {
            out.push(els[i].clone());
        }
    }
    out
}

fn first_last(line: &El) -> ([f64; 2], [f64; 2]) {
    let n = numbers(line.attr("points").unwrap());
    ([n[0], n[1]], [n[n.len() - 2], n[n.len() - 1]])
}

fn close(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < 0.06 && (a[1] - b[1]).abs() < 0.06
}

/// Every piece the style draws on a well sits where the sampled spans say: its
/// ends are the well's path at the span's MDs, in pixels.
fn assert_laid_on_the_spans(svg: &str, cat: &Catalogue, name: &str) -> Vec<El> {
    let els = elements(svg);
    let root = els.iter().find(|e| e.name == "svg").unwrap();
    let w = well_of(cat, name);
    let spans = uncovered(w);
    assert!(!spans.is_empty(), "the control: {name} has portions with no strata");
    let lines = loose(&els, name);
    assert_eq!(lines.len(), spans.len(), "{name}: one piece per unstratified span");
    for (line, (a, b)) in lines.iter().zip(&spans) {
        let (first, last) = first_last(line);
        assert!(close(first, to_px(root, point_at(w, *a))), "{name}: starts at md {a}: {first:?}");
        assert!(close(last, to_px(root, point_at(w, *b))), "{name}: ends at md {b}: {last:?}");
    }
    lines
}

#[test]
fn the_default_is_the_solid_grey_line_under_the_intervals_and_says_nothing_in_the_metadata() {
    assert!(UnstratifiedStyle::default().is_default());
    assert_eq!(UnstratifiedStyle::default(), UnstratifiedStyle::Solid { colour: "#555555".into() });
    let cat = catalogue_of_two();
    let svg = svg::write(&layout(&cat, &strata_view()));
    assert!(!svg.contains("unstratified"), "the metadata does not carry the default");
    assert!(!svg.contains("<pattern"), "no pattern unless a shape uses it");
    let els = elements(&svg);
    let root = els.iter().find(|e| e.name == "svg").unwrap();
    for name in ["A", "B"] {
        let lines = loose(&els, name);
        assert_eq!(lines.len(), 1, "{name}: the whole line, once, under the intervals");
        assert_eq!(lines[0].attr("stroke"), Some("#555555"));
        assert!(lines[0].attr("stroke-dasharray").is_none());
        // It spans the whole path: from the head to the last node.
        let w = well_of(&cat, name);
        let (first, last) = first_last(&lines[0]);
        assert!(close(first, to_px(root, w.path[0])) && close(last, to_px(root, *w.path.last().unwrap())), "{name}");
    }
    // Explicitly the default: the same bytes.
    assert_eq!(svg, svg::write(&layout(&cat, &styled(UnstratifiedStyle::default()))));
}

#[test]
fn dashed_draws_a_dashed_line_on_the_unstratified_spans_only() {
    let cat = catalogue_of_two();
    let svg = svg::write(&layout(&cat, &styled(dashed())));
    for name in ["A", "B"] {
        let lines = assert_laid_on_the_spans(&svg, &cat, name);
        for l in &lines {
            assert_eq!(l.attr("stroke"), Some("#999999"));
            assert_eq!(l.attr("stroke-dasharray"), Some("6 4"));
            assert_eq!(l.attr("vector-effect"), Some("non-scaling-stroke"), "the dash keeps its pixels under the zoom script");
            assert!(l.attr("stroke-linecap").is_none(), "a round cap would close the gaps");
        }
    }
    assert!(svg.contains("unstratified"), "a style that is not the default is in the metadata, so a redraw keeps it");
    assert_eq!(svg::read_metadata(&svg).unwrap().view.unstratified, dashed());
    // The intervals are untouched: the wells still have their four and three strata paths.
    let els = elements(&svg);
    assert_eq!(els.iter().filter(|e| e.name == "g" && e.attr("data-unit").is_some() && inside(e, "layer=wells")).count(), 7);
}

#[test]
fn a_dashed_line_in_a_static_svg_keeps_its_dashes_and_has_no_zoom_attribute() {
    let cat = catalogue_of_two();
    let mut v = styled(dashed());
    v.zoom_script = false;
    let svg = svg::write(&layout(&cat, &v));
    let lines = loose(&elements(&svg), "A");
    assert_eq!(lines[0].attr("stroke-dasharray"), Some("6 4"));
    assert!(lines[0].attr("vector-effect").is_none());
}

#[test]
fn a_solid_colour_of_its_own_draws_the_spans_in_that_colour() {
    let cat = catalogue_of_two();
    let svg = svg::write(&layout(&cat, &styled(UnstratifiedStyle::Solid { colour: "#00aaff".into() })));
    for name in ["A", "B"] {
        for l in assert_laid_on_the_spans(&svg, &cat, name) {
            assert_eq!(l.attr("stroke"), Some("#00aaff"));
            assert!(l.attr("stroke-dasharray").is_none());
        }
    }
}

#[test]
fn hatched_strokes_the_spans_with_a_pattern_that_is_defined_once_in_defs() {
    let cat = catalogue_of_two();
    let svg = svg::write(&layout(&cat, &styled(hatched())));
    let els = elements(&svg);
    let pattern: Vec<&El> = els.iter().filter(|e| e.name == "pattern").collect();
    assert_eq!(pattern.len(), 1, "one pattern, however many wells use it");
    let p = pattern[0];
    let pid = p.attr("id").unwrap();
    assert_eq!(pid, "esv-unstratified-hatch", "a prefixed id that no drawing of ours otherwise has");
    assert_eq!(p.attr("width"), Some("5"));
    assert_eq!(p.attr("patternTransform"), Some("rotate(30)"));
    let stripe = els.iter().find(|e| e.name == "line" && e.attr("stroke") == Some("#aa5500")).expect("the stripe is in the style's colour");
    assert_eq!(stripe.attr("stroke-width"), Some("2.5"));
    for name in ["A", "B"] {
        for l in assert_laid_on_the_spans(&svg, &cat, name) {
            assert_eq!(l.attr("stroke"), Some(format!("url(#{pid})").as_str()));
        }
    }
    // Every reference resolves: no `url(#x)` without an id `x` in the document.
    let ids: Vec<&str> = els.iter().filter_map(|e| e.attr("id")).collect();
    let mut references = 0;
    for e in &els {
        for (_, v) in &e.attrs {
            if let Some(r) = v.strip_prefix("url(#").and_then(|r| r.strip_suffix(')')) {
                references += 1;
                assert!(ids.contains(&r), "`{r}` is referenced and defined");
            }
        }
    }
    assert!(references >= 2, "the control: the references exist (and the clip path's)");
    // The pattern is in <defs>, before anything refers to it.
    assert!(svg.find("<pattern").unwrap() < svg.find("<polyline").unwrap());
    assert_eq!(svg::read_metadata(&svg).unwrap().view.unstratified, hatched());
}

#[test]
fn a_pattern_that_nothing_uses_is_not_written() {
    // Hatched, but a well with no portion left unstratified: no shape refers to it.
    let covered = catalogue(vec![well_with("A", 150.0, vec![interval(0.0, 520.0, Some("#123456"), "All")])]);
    let svg = svg::write(&layout(&covered, &styled(hatched())));
    assert!(!svg.contains("<pattern"));
}

#[test]
fn hidden_draws_nothing_there_and_is_what_the_old_boolean_says() {
    let cat = catalogue_of_two();
    let l = layout(&cat, &styled(UnstratifiedStyle::Hidden));
    let els = elements(&svg::write(&l));
    for name in ["A", "B"] {
        assert!(loose(&els, name).is_empty(), "{name}: only the intervals and the head");
    }
    assert!(l.warnings.iter().any(|w| w.starts_with("2 well(s) drawn only where")), "{:?}", l.warnings);

    // A view written before the styles: `drawUnstratified: false`, no `unstratified`.
    let mut json = serde_json::to_value(strata_view()).unwrap();
    assert!(json.get("unstratified").is_none() && json.get("drawUnstratified").is_none(), "neither is written by default");
    json.as_object_mut().unwrap().insert("drawUnstratified".into(), false.into());
    let old: View = serde_json::from_value(json).unwrap();
    assert_eq!(old.unstratified_style(), UnstratifiedStyle::Hidden);
    let from_old = layout(&cat, &old);
    assert!(loose(&elements(&svg::write(&from_old)), "A").is_empty());
    assert_eq!(from_old.warnings, l.warnings, "the same drawing, the same notes");
    // The alias wins over a style: `false` was "not drawn" whatever follows.
    let mut both = old.clone();
    both.unstratified = dashed();
    assert_eq!(both.unstratified_style(), UnstratifiedStyle::Hidden);
    // And a style written by the new code reads back.
    let new: View = serde_json::from_value(serde_json::to_value(styled(dashed())).unwrap()).unwrap();
    assert_eq!(new.unstratified_style(), dashed());
}

#[test]
fn a_style_only_applies_under_strata_colouring() {
    // With the side colours the line is the well's: the style is for the portions *under strata colouring*.
    let cat = catalogue_of_two();
    let mut v = styled(dashed());
    v.well_colouring = WellColouring::Side;
    let els = elements(&svg::write(&layout(&cat, &v)));
    let lines = loose(&els, "A");
    assert_eq!(lines.len(), 1);
    assert!(lines[0].attr("stroke-dasharray").is_none());
    assert_ne!(lines[0].attr("stroke"), Some("#999999"));
}

#[test]
fn a_style_that_cannot_be_drawn_is_said_by_name_and_the_default_is_drawn() {
    let cat = catalogue_of_two();
    for bad in [
        UnstratifiedStyle::Dashed { colour: "#999999".into(), dash: 0.0, gap: 4.0 },
        UnstratifiedStyle::Dashed { colour: "#999999".into(), dash: 6.0, gap: 1.0e9 },
        UnstratifiedStyle::Hatched { colour: "#999999".into(), spacing: 5.0, angle: 400.0 },
        UnstratifiedStyle::Solid { colour: " ".into() },
    ] {
        assert!(bad.refusal().is_some());
        let l = layout(&cat, &styled(bad.clone()));
        assert!(l.warnings.iter().any(|w| w.starts_with("unstratified style not drawn:")), "{bad:?}: {:?}", l.warnings);
        let lines = loose(&elements(&svg::write(&l)), "A");
        assert_eq!(lines[0].attr("stroke"), Some("#555555"), "{bad:?}: the default is drawn");
    }
}

// ---------------------------------------------------------------------------
// The legend

/// The elements of the legend's "no strata" entry (its group, then what is inside it).
fn legend_none(svg: &str) -> Vec<El> {
    let els = elements(svg);
    let Some(at) = els.iter().position(|e| e.attr("data-kind") == Some("strata-none") && inside(e, "id=strata-legend")) else {
        return Vec::new();
    };
    let depth = els[at].path.len();
    els[at..].iter().enumerate().take_while(|(i, e)| *i == 0 || e.path.len() > depth).map(|(_, e)| e.clone()).collect()
}

fn with_legend(style: UnstratifiedStyle) -> View {
    let mut v = styled(style);
    v.strata_legend = true;
    v
}

#[test]
fn the_legend_says_what_a_style_of_its_own_is() {
    let cat = catalogue_of_two();
    // Dashed: a dashed swatch in the style's colour, and the words.
    let group = legend_none(&svg::write(&layout(&cat, &with_legend(dashed()))));
    assert!(!group.is_empty(), "an entry \"no strata\" with the style");
    let swatch = group.iter().find(|e| e.name == "polyline").expect("a dashed swatch");
    assert_eq!(swatch.attr("stroke"), Some("#999999"));
    assert_eq!(swatch.attr("stroke-dasharray"), Some("6 4"));
    assert!(group.iter().any(|e| e.name == "text" && e.text == "No strata"));

    // Hatched: a swatch filled with the same pattern the wells use.
    let group = legend_none(&svg::write(&layout(&cat, &with_legend(hatched()))));
    let swatch = group.iter().find(|e| e.name == "polygon").expect("a hatched swatch");
    assert_eq!(swatch.attr("fill"), Some("url(#esv-unstratified-hatch)"));

    // Solid in another colour.
    let group = legend_none(&svg::write(&layout(&cat, &with_legend(UnstratifiedStyle::Solid { colour: "#00aaff".into() }))));
    assert_eq!(group.iter().find(|e| e.name == "polygon").unwrap().attr("fill"), Some("#00aaff"));
}

#[test]
fn the_legend_has_no_such_entry_for_the_default_hidden_no_legend_or_no_such_portion() {
    let cat = catalogue_of_two();
    assert!(legend_none(&svg::write(&layout(&cat, &with_legend(UnstratifiedStyle::default())))).is_empty(), "the default: nothing to explain");
    assert!(legend_none(&svg::write(&layout(&cat, &with_legend(UnstratifiedStyle::Hidden)))).is_empty(), "hidden: nothing drawn");
    assert!(legend_none(&svg::write(&layout(&cat, &styled(dashed())))).is_empty(), "no legend asked");
    let covered = catalogue(vec![well_with("A", 150.0, vec![interval(0.0, 520.0, Some("#123456"), "All")])]);
    assert!(legend_none(&svg::write(&layout(&covered, &with_legend(dashed())))).is_empty(), "no portion drawn in it");
}

// ---------------------------------------------------------------------------
// The declared list

#[test]
fn the_declared_list_round_trips_through_json() {
    let declared = style::declared();
    let text = serde_json::to_string(&declared).unwrap();
    let back: Vec<style::StyleDecl> = serde_json::from_str(&text).unwrap();
    assert_eq!(back, declared);
    // The shape the front end reads: camelCase keys, a `type` per parameter, its default beside it.
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    let dashed = json.as_array().unwrap().iter().find(|s| s["id"] == "dashed").unwrap();
    assert_eq!(dashed["labelKey"], "sectionView.unstratified.style.dashed");
    let dash = dashed["params"].as_array().unwrap().iter().find(|p| p["id"] == "dash").unwrap();
    assert_eq!(dash["type"], "number");
    assert_eq!(dash["unit"], "px");
    assert_eq!(dash["default"], 6.0);
    assert!(dash.get("min").is_some() && dash.get("max").is_some() && dash.get("step").is_some());
    let colour = dashed["params"].as_array().unwrap().iter().find(|p| p["id"] == "colour").unwrap();
    assert_eq!(colour["type"], "colour");
}

#[test]
fn the_declared_list_is_held_against_the_enum_so_the_two_cannot_drift() {
    let declared = style::declared();
    // The first is the default's: the front end starts from what the library draws.
    assert_eq!(declared[0].default_style().unwrap(), UnstratifiedStyle::default());
    // One suggested for a new view of the front end — hatched — and
    // never the library's own default, which stays what the CLI draws.
    let suggested: Vec<&str> = declared.iter().filter(|d| d.suggested).map(|d| d.id.as_str()).collect();
    assert_eq!(suggested, ["hatched"]);
    assert!(!declared[0].suggested);
    let mut seen = Vec::new();
    for decl in &declared {
        let value = decl.default_style().unwrap_or_else(|why| panic!("{}: {why}", decl.id));
        let json = serde_json::to_value(&value).unwrap();
        assert_eq!(json["kind"], decl.id.as_str(), "the id is what the enum serialises as");
        // The declared parameters are exactly the variant's fields.
        let mut fields: Vec<&str> = json.as_object().unwrap().keys().map(String::as_str).filter(|k| *k != "kind").collect();
        let mut params: Vec<&str> = decl.params.iter().map(|p| p.id.as_str()).collect();
        fields.sort();
        params.sort();
        assert_eq!(fields, params, "{}: parameters and fields", decl.id);
        assert_eq!(value.refusal(), None, "{}: its defaults are drawable", decl.id);
        // A declared range is the one the drawer enforces: the bounds are accepted, beyond them refused.
        for p in &decl.params {
            if let ParamKind::Number { min, max, default, .. } = &p.kind {
                assert!(min <= default && default <= max, "{}.{}: the default is in range", decl.id, p.id);
                for (v, ok) in [(*min, true), (*max, true), (*min - 0.5, false), (*max + 0.5, false)] {
                    let mut edited = json.clone();
                    edited[p.id.as_str()] = v.into();
                    let style: UnstratifiedStyle = serde_json::from_value(edited).unwrap();
                    assert_eq!(style.refusal().is_none(), ok, "{}.{} = {v}", decl.id, p.id);
                }
            }
        }
        seen.push(decl.id.clone());
    }
    // Every variant of the enum is declared (the match has no wildcard: a new variant breaks the build here).
    for variant in [UnstratifiedStyle::Hidden, UnstratifiedStyle::default(), dashed(), hatched()] {
        let kind = match &variant {
            UnstratifiedStyle::Hidden => "hidden",
            UnstratifiedStyle::Solid { .. } => "solid",
            UnstratifiedStyle::Dashed { .. } => "dashed",
            UnstratifiedStyle::Hatched { .. } => "hatched",
        };
        assert!(seen.iter().any(|s| s == kind), "{kind} is declared");
    }
}

#[test]
fn the_command_line_reads_a_style_against_the_declared_list() {
    assert_eq!(style::parse("solid").unwrap(), UnstratifiedStyle::default());
    assert_eq!(style::parse("dashed:colour=#999999,dash=6,gap=4").unwrap(), dashed());
    assert_eq!(style::parse("dashed").unwrap(), UnstratifiedStyle::Dashed { colour: "#555555".into(), dash: 6.0, gap: 4.0 }, "left out: the default");
    assert_eq!(style::parse("hatched:colour=#aa5500,spacing=5,angle=30").unwrap(), hatched());
    assert_eq!(style::parse("hidden").unwrap(), UnstratifiedStyle::Hidden);
    let why = style::parse("zigzag").unwrap_err();
    assert!(why.contains("solid") && why.contains("hatched"), "names what exists: {why}");
    let why = style::parse("dashed:width=3").unwrap_err();
    assert!(why.contains("dash") && why.contains("gap"), "names the parameters: {why}");
    assert!(style::parse("dashed:dash=abc").unwrap_err().contains("not a number"));
    assert!(style::parse("dashed:dash=0").unwrap_err().contains("dash"), "out of range");
}
