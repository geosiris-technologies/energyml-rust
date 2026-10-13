//! `stratigraphy-from-geolts`, and the passes of a borehole file mapped to its
//! units.
//!
//! **The chart is an extract written here**, installed as the default for this
//! test binary alone — the five divisions of `energyml-domain/tests/geolts.rs`,
//! and the chain above the Eocene with its two sister series and the
//! Precambrian, the chart's only supereonothem; every row copied from the BRGM
//! export of 24/09/2026 (`https://data.geoscience.fr/ncl/GeolTs`, CC BY 4.0),
//! the colour columns put in red-green-blue order. The shape of the assertions
//! comes from the RESQML 2.2 reference guide (§5.34, §6.34, §6.44, §6.45); the
//! values from the extract.

use std::io::Cursor;
use std::sync::Arc;

use serde_json::{Value, json};

use energyml_build::declare::ChartTerm;
use energyml_build::operation::{Choices, Identity, NewObject, Plan, Registry, When};
use energyml_build::ops::stratigraphy::{self, ANCESTOR, StratigraphyParams};
use energyml_build::osdu;
use energyml_build::pieces::{Pieces, Source};
use energyml_build::tabular::{self, Dialect};
use energyml_domain::geolts::Chart;
use energyml_storage::{EpcStore, SaveOptions};

#[path = "support/geolts_extract.rs"]
mod geolts_extract;
use geolts_extract::{BOUNDS, CONCEPTS, extract};

fn when() -> When {
    When { creation: "2026-09-24T00:00:00Z".into(), originator: "test".into() }
}

fn plan(operation: &str, pieces: &Pieces, answers: Value) -> Result<Plan, String> {
    extract();
    let mut choices = Choices::new();
    for (name, value) in answers.as_object().expect("an object") {
        choices = choices.set(name, value.clone());
    }
    Registry::new().plan(operation, pieces, &choices, &Identity::derived(), &when()).map_err(|why| why.to_string())
}

/// The objects of a type, by title.
fn titles(made: &Plan, kind: &str) -> Vec<String> {
    made.objects.iter().filter(|one| one.qualified_type == kind).map(|one| one.title.clone()).collect()
}

/// The URL-namespace uuid anyone can compute from a URI.
fn url_uuid(text: &str) -> String {
    uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, text.as_bytes()).to_string()
}

#[test]
fn a_branch_becomes_one_rank_per_level_youngest_first() {
    // The branch alone — its parents are the next tests' business.
    let made = plan("stratigraphy-from-geolts", &Pieces::empty(), json!({ "units": ["Eocène"], "ancestors": false }))
        .expect("a plan");
    // The reference divisions under the Eocene: the Eocene itself (named, so
    // kept), and its two reference stages. The Upper Eocene is a *usual*
    // division and the Ludien a *regional* one — not taken by default.
    let mut units = titles(&made, "resqml22.StratigraphicUnitInterpretation");
    units.sort();
    assert_eq!(units, vec!["Bartonien", "Eocène", "Priabonien"]);

    let ranks: Vec<&Value> = made
        .objects
        .iter()
        .filter(|one| one.qualified_type == "resqml22.StratigraphicColumnRankInterpretation")
        .map(|one| &one.json)
        .collect();
    assert_eq!(ranks.len(), 2, "series and stage");
    let stage = ranks.iter().find(|one| one["StratigraphicUnits"].as_array().is_some_and(|u| u.len() == 2)).expect("the stages");
    // **Youngest first** (TRG 2.2 §6.34, `age`): the Priabonien (37.8–33.9)
    // is listed above the Bartonien (41.2–37.8).
    let order: Vec<&str> =
        stage["StratigraphicUnits"].as_array().unwrap().iter().filter_map(|one| one["Title"].as_str()).collect();
    assert_eq!(order, vec!["Priabonien", "Bartonien"]);
    assert_eq!(stage["AscendingOrderingCriteria"], json!("age"));

    // **The same Priabonien in every package**: its feature's uuid is the
    // URL-namespace v5 of its URI, which anyone can recompute.
    let feature = made
        .objects
        .iter()
        .find(|one| one.qualified_type == "resqml22.RockVolumeFeature" && one.title == "Priabonien")
        .expect("the feature");
    assert_eq!(feature.uuid, url_uuid("https://data.geoscience.fr/ncl/GeolTs/1263"));

    // Opening the regional divisions takes the Ludien too.
    let wide = plan(
        "stratigraphy-from-geolts",
        &Pieces::empty(),
        json!({ "units": ["Eocène"], "regional": true, "ancestors": false }),
    )
    .expect("a plan");
    assert!(titles(&wide, "resqml22.StratigraphicUnitInterpretation").contains(&"Ludien".to_string()));
}

#[test]
fn a_term_the_chart_does_not_know_is_refused_by_name() {
    let why = plan("stratigraphy-from-geolts", &Pieces::empty(), json!({ "units": ["Crétacé"] })).expect_err("unknown");
    assert!(why.contains("Crétacé"), "{why}");
    // A code shared by several divisions is refused towards the id.
    let why = plan("stratigraphy-from-geolts", &Pieces::empty(), json!({ "units": ["e7"] })).expect_err("ambiguous");
    assert!(why.contains("give the id"), "{why}");
}

// -- a branch and the chain above it ------------------------------------------

/// The rank object that lists a unit.
fn rank_object<'a>(made: &'a Plan, unit: &str) -> &'a NewObject {
    made.objects
        .iter()
        .filter(|one| one.qualified_type == "resqml22.StratigraphicColumnRankInterpretation")
        .find(|one| one.json["StratigraphicUnits"].as_array().is_some_and(|units| units.iter().any(|u| u["Title"] == unit)))
        .unwrap_or_else(|| panic!("no rank lists {unit}"))
}

/// The rank that lists a unit: its title and its `RankInStratigraphicColumn`.
fn rank_holding(made: &Plan, unit: &str) -> (String, i64) {
    let rank = rank_object(made, unit);
    (rank.title.clone(), rank.json["RankInStratigraphicColumn"].as_i64().expect("a rank states its number"))
}

/// An object of the plan, by type and title.
fn object<'a>(made: &'a Plan, kind: &str, title: &str) -> &'a NewObject {
    made.objects
        .iter()
        .find(|one| one.qualified_type == kind && one.title == title)
        .unwrap_or_else(|| panic!("no {kind} titled {title}"))
}

/// The need: when a sub-part of the complete group is imported,
/// the parents are not imported, and there seems to be a
/// shift of rank (the Eocene, for example, if it is the one ticked when
/// selecting for the import operation, lands at rank 0).
///
/// The shape is RESQML's — a column is ranks, a rank lists the units of one
/// level (TRG 2.2 §6.44 p. 212, §6.45 p. 213); the hierarchy is the chart's.
#[test]
fn a_series_chosen_alone_sits_in_the_rank_of_the_series_under_its_system() {
    let alone = plan("stratigraphy-from-geolts", &Pieces::empty(), json!({ "units": ["Eocène"] })).expect("a plan");
    let units = titles(&alone, "resqml22.StratigraphicUnitInterpretation");
    // **The chain above it, and only the chain**: the Paleogene's two other
    // series were not chosen and are above nothing that was, nor is the
    // Precambrian.
    for parent in ["Paléogène", "Cénozoïque", "Phanérozoïque"] {
        assert!(units.iter().any(|one| one == parent), "{parent} is above the Eocene: {units:?}");
    }
    for other in ["Paléocène", "Oligocène", "Précambrien"] {
        assert!(!units.iter().any(|one| one == other), "{other} was not chosen: {units:?}");
    }

    // Each in the rank of its level.
    let (series, eocene_at) = rank_holding(&alone, "Eocène");
    let (system, paleogene_at) = rank_holding(&alone, "Paléogène");
    assert!(series.ends_with("— series"), "{series}");
    assert!(system.ends_with("— system"), "{system}");

    // **The numbers are those of a column that misses nothing**: both roots of
    // the extract, the Precambrian (the only supereonothem) included. Counting
    // the levels a column happens to hold gives the Eocene 3 here and 4 there —
    // the complaint, one rank further down — so the numbers must agree.
    let whole = plan("stratigraphy-from-geolts", &Pieces::empty(), json!({ "units": ["Précambrien", "Phanérozoïque"] }))
        .expect("a plan");
    assert_eq!(rank_holding(&whole, "Eocène").1, eocene_at, "the Eocene has one rank number, whatever else is chosen");
    assert_eq!(rank_holding(&whole, "Paléogène").1, paleogene_at);
    assert_eq!(rank_holding(&whole, "Priabonien").1, rank_holding(&alone, "Priabonien").1);
    assert_ne!(eocene_at, 0, "the Eocene is not the top of its column");
    assert_eq!(paleogene_at + 1, eocene_at, "a system's rank is right above its series'");

    // Nothing above is chosen with `ancestors` off, and the numbers still
    // do not move: they are the levels', not positions.
    let bare = plan("stratigraphy-from-geolts", &Pieces::empty(), json!({ "units": ["Eocène"], "ancestors": false }))
        .expect("a plan");
    assert!(!titles(&bare, "resqml22.StratigraphicUnitInterpretation").iter().any(|one| one == "Paléogène"));
    assert_eq!(rank_holding(&bare, "Eocène").1, eocene_at);

    // A level the column holds nothing of has no rank — a rank lists 1..*
    // units (§6.45) — and the gap is said: the supereonothem and the
    // sub-series here, the Upper Eocene being a *usual* division.
    let gap = alone.says.iter().find(|note| note.key == "ranks-absent").expect("the gap is said");
    assert!(gap.detail.contains("supereonothem") && gap.detail.contains("sub-series"), "{}", gap.detail);
    assert!(!whole.says.iter().any(|note| note.key == "ranks-absent" && note.detail.contains("supereonothem")));
}

#[test]
fn a_parent_nobody_chose_is_said_and_marked_on_its_rank_not_on_itself() {
    let made = plan("stratigraphy-from-geolts", &Pieces::empty(), json!({ "units": ["Eocène"] })).expect("a plan");
    let note = made.says.iter().find(|note| note.key == "ancestors").expect("the plan says what it added");
    for parent in ["Paléogène", "Cénozoïque", "Phanérozoïque"] {
        assert!(note.detail.contains(parent), "{}", note.detail);
    }
    assert!(!note.detail.contains("Eocène ("), "the Eocene was chosen: {}", note.detail);

    // On the rank that lists it: its URI, under the operation's name.
    let marks = |rank: &NewObject| -> Vec<String> {
        rank.json["ExtensionNameValue"]
            .as_array()
            .map(|all| {
                all.iter()
                    .filter(|one| one["Name"] == ANCESTOR)
                    .filter_map(|one| one["Value"][energyml_core::json::TEXT_KEY].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };
    assert_eq!(marks(rank_object(&made, "Paléogène")), vec!["https://data.geoscience.fr/ncl/GeolTs/1242".to_string()]);
    assert_eq!(marks(rank_object(&made, "Phanérozoïque")), vec!["https://data.geoscience.fr/ncl/GeolTs/1028".to_string()]);
    assert!(marks(rank_object(&made, "Eocène")).is_empty(), "a division chosen is not marked");

    // **Not on the division**: the Paleogene added is the very object the
    // Paleogene chosen is — the same uuid, and the same content.
    let chosen = plan("stratigraphy-from-geolts", &Pieces::empty(), json!({ "units": ["Paléogène"] })).expect("a plan");
    for kind in ["resqml22.RockVolumeFeature", "resqml22.StratigraphicUnitInterpretation"] {
        assert_eq!(object(&made, kind, "Paléogène").json, object(&chosen, kind, "Paléogène").json, "{kind}");
    }
    assert!(!chosen.says.iter().any(|note| note.key == "ancestors" && note.detail.contains("Paléogène (")));
}

#[test]
fn a_division_has_one_uuid_alone_with_its_parents_or_inside_its_parent() {
    let alone = plan("stratigraphy-from-geolts", &Pieces::empty(), json!({ "units": ["Eocène"] })).expect("a plan");
    let bare = plan("stratigraphy-from-geolts", &Pieces::empty(), json!({ "units": ["Eocène"], "ancestors": false }))
        .expect("a plan");
    let inside = plan("stratigraphy-from-geolts", &Pieces::empty(), json!({ "units": ["Paléogène"] })).expect("a plan");
    for kind in ["resqml22.RockVolumeFeature", "resqml22.StratigraphicUnitInterpretation"] {
        let one = &object(&alone, kind, "Eocène").uuid;
        assert_eq!(one, &object(&bare, kind, "Eocène").uuid, "{kind}: with or without its parents");
        assert_eq!(one, &object(&inside, kind, "Eocène").uuid, "{kind}: chosen, or inside the branch chosen");
    }
    // The rule anyone can recompute: uuid v5 over the division's URI.
    assert_eq!(object(&alone, "resqml22.RockVolumeFeature", "Eocène").uuid, url_uuid("https://data.geoscience.fr/ncl/GeolTs/1249"));
    // The organisation is the selection's: with its parents, the Eocene's
    // column is another column.
    let column_of = |made: &Plan| {
        made.objects.iter().find(|one| one.qualified_type == "resqml22.StratigraphicColumn").expect("a column").uuid.clone()
    };
    assert_ne!(column_of(&alone), column_of(&bare));
}

#[test]
fn a_parent_the_settings_leave_out_is_passed_over_not_a_stop() {
    // The Priabonien's parent in the tree is the Upper Eocene — a sub-series
    // of *usual* nature, not taken by default. Stopping there would lose the
    // Eocene and everything above it.
    let made = plan("stratigraphy-from-geolts", &Pieces::empty(), json!({ "units": ["Priabonien"] })).expect("a plan");
    let units = titles(&made, "resqml22.StratigraphicUnitInterpretation");
    assert!(!units.iter().any(|one| one == "Eocène supérieur"), "{units:?}");
    for parent in ["Eocène", "Paléogène", "Cénozoïque", "Phanérozoïque"] {
        assert!(units.iter().any(|one| one == parent), "{parent}: {units:?}");
    }
    // The chain, not the family: the Bartonien is the Eocene's other stage.
    assert!(!units.iter().any(|one| one == "Bartonien"), "{units:?}");

    // Opening the other natures takes the Upper Eocene, as a parent — and
    // still none of its other children, the Ludien.
    let wide = plan("stratigraphy-from-geolts", &Pieces::empty(), json!({ "units": ["Priabonien"], "regional": true }))
        .expect("a plan");
    let units = titles(&wide, "resqml22.StratigraphicUnitInterpretation");
    assert!(units.iter().any(|one| one == "Eocène supérieur"), "{units:?}");
    assert!(!units.iter().any(|one| one == "Ludien"), "{units:?}");
}

#[test]
fn a_parent_without_an_age_is_refused_on_ancestors_by_name() {
    // The extract, with the Paleogene's two bounds taken out.
    let bounds = "<https://data.geoscience.fr/ncl/GeolTsBound/1241>,<https://data.geoscience.fr/ncl/GeolTsBound/1269>";
    assert_eq!(CONCEPTS.matches(bounds).count(), 1, "the Paleogene's row is where this test expects it");
    let chart = Chart::from_csv(&CONCEPTS.replace(bounds, ","), Some(BOUNDS)).expect("the extract reads");
    let params = |ancestors: bool| StratigraphyParams {
        units: vec![ChartTerm("Eocène".to_string())],
        ancestors,
        regional: false,
        finest: 7,
        title: None,
        language: stratigraphy::Language::Fr,
    };
    let why = stratigraphy::selection(&chart, &params(true)).expect_err("an undated parent cannot be placed");
    assert_eq!(why.slot.as_deref(), Some("ancestors"), "{why}");
    assert!(why.message.contains("Paléogène"), "{why}");
    // It is the parent that is refused, not the branch: alone, it is dated.
    let bare = stratigraphy::selection(&chart, &params(false)).expect("the branch alone is dated");
    assert!(bare.added.is_empty());
    assert!(bare.taken.contains(&1249));
}

// -- what an extension value is, in the bytes ---------------------------------

/// The `(Name, Value)` of every `ExtensionNameValue` of a part, read from its
/// **text** by a scanner of its own — not this library's XML reader, so a value
/// the reader would invent or drop cannot pass for one written. A `<Value/>`
/// reads as an empty value. No entity is unescaped: none of the values checked
/// here holds one.
fn extension_pairs(xml: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut inside: Option<(String, String)> = None;
    for chunk in xml.split('<').skip(1) {
        let (tag, text) = chunk.split_once('>').unwrap_or((chunk, ""));
        let closing = tag.starts_with('/');
        let empty = tag.ends_with('/');
        let name = tag.trim_start_matches('/').split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or("");
        let local = name.rsplit(':').next().unwrap_or(name);
        match local {
            "ExtensionNameValue" if closing => out.extend(inside.take()),
            "ExtensionNameValue" => inside = Some((String::new(), String::new())),
            "Name" if !closing && !empty => {
                if let Some(pair) = inside.as_mut() {
                    pair.0 = text.to_string();
                }
            }
            "Value" if !closing && !empty => {
                if let Some(pair) = inside.as_mut() {
                    pair.1 = text.to_string();
                }
            }
            _ => {}
        }
    }
    out
}

/// The report: in the extensionNameValue there are
/// StringMeasures in the model generated by geolTS; with the specialised
/// component the values are visible below, but when unfolded, the value part for
/// example is always empty! […] And nothing is detected by the
/// validation — is that normal? I thought value was mandatory.
///
/// It is: `ExtensionNameValue` declares `Value` with `minOccurs="1"` (common
/// v2.3 `Abstract.xsd`, complexType `ExtensionNameValue`), and a
/// `StringMeasure` is **simple content** — its value is the text of the element
/// itself, beside an optional `uom` attribute (`BaseTypes.xsd`, complexType
/// `StringMeasure`, extending `eml:String2000`). So the value is read back from
/// **the saved package's bytes**, not from the plan.
#[test]
fn an_extension_value_is_the_text_of_its_element_in_the_saved_part() {
    let made = plan("stratigraphy-from-geolts", &Pieces::empty(), json!({ "units": ["Priabonien"] })).expect("a plan");
    let schemas = Arc::new(energyml_schemas::all());
    let mut store = energyml_build::epc::open(None, "extensions.epc", schemas.clone()).expect("a new package");
    let applied = energyml_build::apply::apply(&made, &mut store, &when());
    assert!(applied.refused.is_empty(), "{:?}", applied.refused);
    let mut saved = Cursor::new(Vec::new());
    store.write_to(&mut saved, &SaveOptions::default()).expect("the package writes");
    let reopened = EpcStore::from_reader(Box::new(Cursor::new(saved.into_inner())), "extensions.epc", schemas)
        .expect("and reads back");
    let part_of = |one: &NewObject| -> String {
        let summary = reopened.find_by_uuid(&one.uuid).unwrap_or_else(|| panic!("{} is in the saved package", one.title));
        String::from_utf8(reopened.part_bytes(&summary.uri).expect("its part")).expect("UTF-8")
    };

    let feature = part_of(object(&made, "resqml22.RockVolumeFeature", "Priabonien"));
    let pairs = extension_pairs(&feature);
    for (name, value) in [("geolts:level", "stage"), ("geolts:nature", "reference"), ("geolts:status", "valid"), ("geolts:spatial", "Monde")] {
        assert!(pairs.contains(&(name.to_string(), value.to_string())), "{name} = {value}: {pairs:?}\n{feature}");
    }
    assert!(pairs.iter().all(|(_, value)| !value.is_empty()), "no Value is empty: {pairs:?}");

    // The rank's mark goes through the same writer.
    let rank = part_of(rank_object(&made, "Paléogène"));
    assert_eq!(extension_pairs(&rank), vec![(ANCESTOR.to_string(), "https://data.geoscience.fr/ncl/GeolTs/1242".to_string())]);

    // **And the key the front end reads**: in the JSON the module hands over, the
    // text of simple content is `_` — the encoding's own key
    // (`energyml_core::json::TEXT_KEY`), never the element's name `Value`.
    let json = &object(&made, "resqml22.RockVolumeFeature", "Priabonien").json;
    let level = json["ExtensionNameValue"]
        .as_array()
        .and_then(|all| all.iter().find(|one| one["Name"] == "geolts:level"))
        .expect("the level is an extension");
    assert_eq!(level["Value"][energyml_core::json::TEXT_KEY], json!("stage"));
    assert!(level["Value"].get("Value").is_none(), "{level}");
}

// -- the passes of a borehole file, mapped to the chart's units --------------

fn column(name: &str, uuid: &str, value_type: &str) -> Value {
    json!({
        "ColumnName": name,
        "ValueType": value_type,
        "PropertyType": { "PropertyTypeID": format!("namespace:reference-data--PropertyType:{uuid}:") },
    })
}

/// The five structural roles, the top of each pass, and the stratigraphy.
fn template() -> Value {
    json!([
        column("SONDAGE", "348c9a4d-e3e4-4beb-8456-f07a46660de1", "string"),
        column("X", "620cec71-5ce3-4ea5-9120-833756b427d7", "number"),
        column("Y", "cfcfdb04-02c1-4262-b66b-459b0aab3e8e", "number"),
        column("Z_MNT", "8508cef4-0ade-454a-9ea4-e00d5905e677", "number"),
        column("DEB", "cb1166d9-3874-4267-abea-1cd97e4bf448", "number"),
        column("PROF", "c48c65d4-9680-4d10-903c-a1b2e30f66b5", "number"),
        column("STRA", "22222222-2222-4222-8222-222222222222", "string"),
        column("Nom_STRA", "33333333-3333-4333-8333-333333333333", "string"),
    ])
}

fn bag(text: &str) -> Pieces {
    let table = tabular::read("essai.csv", text, Dialect::sniff(text)).expect("a table");
    let template = osdu::from_json(&template()).expect("a template");
    Pieces::empty().with("table", Source::Table { name: "essai.csv".into(), table, template })
}

const PASSES: &str = "SONDAGE,X,Y,Z_MNT,DEB,PROF,STRA,Nom_STRA
F1,650000,6800000,100,0,2,e7,Priabonien
F1,650000,6800000,100,2,5,e6,Bartonien
F1,650000,6800000,100,5,6,n.d.,No data
";

#[test]
fn each_interval_names_its_unit_in_a_rank_of_the_chart() {
    // **TRG 2.2 §5.34**: `IntervalStratigraphicUnits` gives one unit index per
    // interval, into a `StratigraphicColumnRankInterpretation`, and *“Array
    // length must equal the number of intervals”*. Three passes, four nodes,
    // three intervals: the Priabonien, the Bartonien, and a null for `n.d.`,
    // which no division of the chart is.
    let made = plan(
        "wells-and-properties-from-table",
        &bag(PASSES),
        json!({ "sourceEpsg": 2154, "units": "STRA", "unitLabels": "Nom_STRA" }),
    )
    .expect("a plan");
    let frame = made
        .objects
        .iter()
        .find(|one| one.qualified_type == "resqml22.WellboreFrameRepresentation")
        .expect("the frame");
    let maps = frame.json["IntervalStratigraphicUnits"].as_array().expect("the frame maps its intervals");
    assert_eq!(maps.len(), 1, "one rank: both units are stages");
    let values: Vec<i64> = maps[0]["UnitIndices"]["Elements"]["Values"]
        .as_array()
        .expect("inline values")
        .iter()
        .filter_map(Value::as_i64)
        .collect();
    // The rank lists the Priabonien (0) above the Bartonien (1): youngest first.
    assert_eq!(values, vec![0, 1, -1]);
    assert_eq!(maps[0]["UnitIndices"]["Elements"]["NullValue"], json!(-1));

    // The rank it names is one this plan writes, holding those two units.
    let rank_uuid = maps[0]["StratigraphicOrganizationInterpretation"]["Uuid"].as_str().expect("a reference");
    let rank = made.objects.iter().find(|one| one.uuid == rank_uuid).expect("the rank is written");
    let units: Vec<&str> =
        rank.json["StratigraphicUnits"].as_array().unwrap().iter().filter_map(|one| one["Title"].as_str()).collect();
    assert_eq!(units, vec!["Priabonien", "Bartonien"]);

    // `n.d.` is said, not silently dropped.
    assert!(made.says.iter().any(|note| note.key == "units-unknown" && note.detail.contains("n.d.")), "{:?}", made.says);
}

#[test]
fn the_unit_mapping_is_part_of_the_frames_identity() {
    // §15: another classification of the same passes is a second frame, never
    // the first one rewritten — and no mapping asked, the frame of before.
    let frame_of = |answers: Value| -> String {
        let made = plan("wells-and-properties-from-table", &bag(PASSES), answers).expect("a plan");
        made.objects
            .iter()
            .find(|one| one.qualified_type == "resqml22.WellboreFrameRepresentation")
            .expect("the frame")
            .uuid
            .clone()
    };
    let plain = frame_of(json!({ "sourceEpsg": 2154 }));
    let mapped = frame_of(json!({ "sourceEpsg": 2154, "units": "STRA", "unitLabels": "Nom_STRA" }));
    assert_ne!(plain, mapped, "a frame that says more is another object");
    let again = frame_of(json!({ "sourceEpsg": 2154, "units": "STRA", "unitLabels": "Nom_STRA" }));
    assert_eq!(mapped, again, "the same mapping, the same frame");
}

#[test]
fn a_mapping_without_passes_is_refused_towards_the_top_depth() {
    let text = "SONDAGE,X,Y,Z_MNT,PROF,STRA,Nom_STRA\nF1,650000,6800000,100,2,e7,Priabonien\n";
    let table = tabular::read("essai.csv", text, Dialect::sniff(text)).expect("a table");
    let mut columns = template();
    columns.as_array_mut().unwrap().retain(|one| one["ColumnName"] != "DEB");
    let pieces = Pieces::empty().with(
        "table",
        Source::Table { name: "essai.csv".into(), table, template: osdu::from_json(&columns).expect("a template") },
    );
    let why = plan("wells-and-properties-from-table", &pieces, json!({ "sourceEpsg": 2154, "units": "STRA" }))
        .expect_err("no passes, no intervals");
    assert!(why.contains("top depth"), "{why}");
}

#[test]
fn the_charts_colours_are_written_and_read_back_on_each_division() {
    // The register gives every division its colour on the chart (698 of 698 on
    // the snapshot). Written as a `DefaultGraphicalInformation` on the unit and
    // its feature, in a set of its own — and read back **by the package's own
    // colour reader**, not by looking at what this plan built.
    let made = plan("stratigraphy-from-geolts", &Pieces::empty(), json!({ "units": ["Eocène"] })).expect("a plan");
    let schemas = std::sync::Arc::new(energyml_schemas::all());
    let mut store = energyml_build::epc::open(None, "colours.epc", schemas).expect("a new package");
    let applied = energyml_build::apply::apply(&made, &mut store, &when());
    assert!(applied.refused.is_empty(), "{:?}", applied.refused);

    let set = made.objects.iter().find(|one| one.qualified_type == "eml23.GraphicalInformationSet").expect("the colours");
    let summary = store.find_by_uuid(&set.uuid).expect("written").clone();
    let object = store.get_object(&summary.uri).expect("reads back");
    let read = energyml_domain::color::GraphicalSet::read(object.as_reflect());
    let priabonian = url_uuid("https://data.geoscience.fr/ncl/GeolTs/1263#interpretation");
    let entry = read
        .entries
        .iter()
        .find(|one| one.targets.iter().any(|target| target.eq_ignore_ascii_case(&priabonian)))
        .unwrap_or_else(|| panic!("no entry for the Priabonien: {:?}", read.notes));
    let energyml_domain::color::Information::Default { elements, .. } = &entry.information else {
        panic!("a default graphical information")
    };
    let colour = elements.first().and_then(|one| one.constant_color.clone()).expect("a constant colour");
    // 253, 205, 161 on the chart — through HSV and back, to the unit.
    let rgba = colour.to_rgba();
    let byte = |v: f64| (v * 255.0).round() as i64;
    assert_eq!((byte(rgba.r), byte(rgba.g), byte(rgba.b)), (253, 205, 161));
    // Both the unit and the rock it interprets: the cascade reads either.
    assert!(entry.targets.iter().any(|target| target.eq_ignore_ascii_case(&url_uuid("https://data.geoscience.fr/ncl/GeolTs/1263"))));
}

#[test]
fn a_written_column_reads_back_with_the_age_of_each_unit() {
    // The writer states each division's span as a `GeologicTimeBasedTimeInterval`;
    // the reader the column view stands on (`stratigraphy::columns`) must give
    // it back as numbers — that is what aligns a stage under its series.
    let made = plan("stratigraphy-from-geolts", &Pieces::empty(), json!({ "units": ["Eocène"] })).expect("a plan");
    let schemas = std::sync::Arc::new(energyml_schemas::all());
    let mut store = energyml_build::epc::open(None, "ages.epc", schemas).expect("a new package");
    let applied = energyml_build::apply::apply(&made, &mut store, &when());
    assert!(applied.refused.is_empty(), "{:?}", applied.refused);

    let held: Vec<Box<dyn energyml_core::registry::DynObject>> =
        store.resources().iter().filter_map(|one| store.get_object(&one.uri).ok()).collect();
    let read = energyml_domain::stratigraphy::columns(held.iter().map(|one| one.as_reflect()));
    let units: Vec<&energyml_domain::stratigraphy::ColumnUnit> =
        read.columns.iter().flat_map(|column| column.ranks.iter().flat_map(|rank| rank.units.iter())).collect();
    let age = |title: &str| units.iter().find(|one| one.unit.title == title).and_then(|one| one.age);
    assert_eq!(age("Priabonien"), Some([37.8, 33.9]), "[old, young] in Ma");
    assert_eq!(age("Bartonien"), Some([41.2, 37.8]));
    assert_eq!(age("Eocène"), Some([56.0, 33.9]));
}

/// The decision: the units may be titled in English — the BRGM register
/// states every division in both languages (`'Eocene'@en|'Eocène'@fr` in the
/// extract), so English is the register's word, not a translation of ours.
#[test]
fn the_same_divisions_titled_in_english_keep_their_uuids_and_the_french_as_an_alias() {
    let french = plan("stratigraphy-from-geolts", &Pieces::empty(), json!({ "units": ["Eocène"], "ancestors": false })).expect("a plan");
    let english = plan("stratigraphy-from-geolts", &Pieces::empty(), json!({ "units": ["Eocène"], "ancestors": false, "language": "en" }))
        .expect("a plan");
    let units = |made: &Plan| titles(made, "resqml22.StratigraphicUnitInterpretation");
    assert!(units(&french).contains(&"Eocène".to_string()), "{:?}", units(&french));
    assert!(units(&english).contains(&"Eocene".to_string()), "{:?}", units(&english));
    assert!(units(&english).contains(&"Bartonian".to_string()), "{:?}", units(&english));
    // The uuids come from the register's URI, whatever the language.
    let uuids = |made: &Plan| made.objects.iter().map(|one| one.uuid.clone()).collect::<Vec<_>>();
    assert_eq!(uuids(&french), uuids(&english));
    // The other language stays findable, as an alias of the register.
    let eocene = english
        .objects
        .iter()
        .find(|one| one.qualified_type == "resqml22.StratigraphicUnitInterpretation" && one.title == "Eocene")
        .expect("the Eocene");
    let aliases = eocene.json["Aliases"].as_array().expect("aliases");
    assert!(
        aliases.iter().any(|alias| alias["authority"] == "BRGM label (fr)" && alias["Identifier"] == "Eocène"),
        "{aliases:?}"
    );
}

