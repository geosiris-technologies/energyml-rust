//! The SIGEOL operations against what the CSV path established (01/10): the
//! unit export read the way an external script reads it, and the
//! boreholes of `sigeol-boreholes` laid out so that the CSV enrichment lands on
//! them without moving a node.
//!
//! Every feature here is **made in the test**; the shape of the assertions comes
//! from the specification and from the facts established on the BRGM
//! data (their values are the test's own). Nothing reads a file.
//!
//! What each one needs to break is said on each.

use serde_json::{Value, json};

use energyml_build::geojson;
use energyml_build::operation::{Choices, Identity, Plan, Registry, When};
use energyml_build::osdu;
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_build::tabular::{self, Dialect};

fn when() -> When {
    When { creation: "2026-10-01T00:00:00Z".into(), originator: "test".into() }
}

/// One `GeologicUnit` feature as the SIGEOL exports write it: a point at the
/// **ground** of its borehole, the formation under `composition`, the two ages
/// under `geologicHistory` — `olderNamedAge` carrying whatever is given first.
fn unit(id: &str, formation: &str, older: (&str, f64), younger: (&str, f64)) -> String {
    format!(
        r#"{{"type":"Feature","id":"{id}",
            "geometry":{{"type":"Point","coordinates":[2.4321,48.9135,46]}},
            "properties":{{
              "@featureType":"GeologicUnit",
              "composition":[{{"material":{{"StratiRockMaterial":{{"lithostratigraphy":{{
                 "@href":"https://referentiel-lithostrat.brgm-rec.fr/#/lithostrat/search/details/006fd552-1f63-47b3-bae9-e528f169083c",
                 "@title":"{formation}"}}}}}}}}],
              "geologicHistory":[{{
                 "olderNamedAge":{{"@title":"{}","@href":"https://data.geoscience.fr/ncl/GeolTs/1"}},
                 "youngerNamedAge":{{"@title":"{}","@href":"https://data.geoscience.fr/ncl/GeolTs/2"}},
                 "numericAge":{{"NumericAgeRange":{{
                    "olderBoundDate":{{"Quantity":{{"value":{}}}}},
                    "youngerBoundDate":{{"Quantity":{{"value":{}}}}}}}}}}}]}}}}"#,
        older.0, younger.0, older.1, younger.1
    )
}

fn features(items: &[String]) -> Vec<geojson::Feature> {
    let text = format!(r#"{{"type":"FeatureCollection","features":[{}]}}"#, items.join(","));
    let read = geojson::read(&text).expect("a collection");
    assert!(read.refused.is_empty(), "{:?}", read.refused);
    read.features
}

/// Files as the front end or the CLI would bind them: a name each, and its features.
fn files(named: &[(&str, Vec<geojson::Feature>)]) -> Pieces {
    let mut pieces = Pieces::empty();
    for (at, (name, items)) in named.iter().enumerate() {
        pieces = pieces.with(format!("export{at}"), Source::Features { name: (*name).into(), features: items.clone() });
    }
    pieces
}

fn plan_units(pieces: &Pieces, choices: Choices) -> Result<Plan, String> {
    Registry::new().plan("sigeol-units", pieces, &choices, &Identity::derived(), &when()).map_err(|why| why.to_string())
}

fn number(plan: &Plan, title: &str) -> f64 {
    plan.activity
        .parameters
        .iter()
        .find_map(|one| match one {
            energyml_domain::provenance::Parameter::Double { title: name, value, .. } if name == title => Some(*value),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no `{title}` in the activity"))
}

// -- fact 2 (file names): the stage the export was asked about --------------

/// **The second export's names** — `GeologicUnit_<Stage>_<field>_ZONE_<n>.json`
/// (`Data_Jerome_v2`, 01/10). Only `_pretty` was peeled, so the suggested stage
/// was `Bartonien_olderNamedAge_ZONE_2`: no unit has that at either end, and the
/// run was refused naming the stages the file holds. To break: put the old
/// `stage_of_file_name` back — the three names below each come out whole.
#[test]
fn the_second_exports_file_names_suggest_their_stage() {
    let one = vec![unit("1", "Marnes et Caillasses", ("Bartonien", 37.8), ("Lutétien", 47.8))];
    for (name, stage) in [
        ("GeologicUnit_Bartonien_olderNamedAge_ZONE_2.json", "Bartonien"),
        ("GeologicUnit_Lutétien_youngerNamedAge_ZONE_2.json", "Lutétien"),
        ("GeologicUnit_Priabonien_ZONE_2.json", "Priabonien"),
        ("GeologicUnit_Bartonien_pretty.json", "Bartonien"),
    ] {
        let pieces = files(&[(name, features(&one))]);
        let registry = Registry::new();
        let offer = registry
            .offers(&pieces)
            .into_iter()
            .find(|offer| offer.operation.name() == "sigeol-units")
            .expect("offered");
        let slot = offer.needs.iter().find(|slot| slot.name == "stage").expect("the stage slot");
        assert_eq!(slot.suggested, Some(json!(stage)), "{name}");
    }
}

/// The suggestion is not decoration: the plan runs on it, and **writes the
/// unit's surface of that stage**. To break: the same revert — the plan then
/// refuses (`no unit of the file has … at either end`).
#[test]
fn a_run_on_a_second_exports_name_plans_by_the_suggested_stage_alone() {
    let pieces = files(&[(
        "GeologicUnit_Bartonien_olderNamedAge_ZONE_2.json",
        features(&[unit("1", "Marnes et Caillasses", ("Bartonien", 37.8), ("Lutétien", 47.8))]),
    )]);
    let registry = Registry::new();
    let offer = registry.offers(&pieces).into_iter().find(|o| o.operation.name() == "sigeol-units").unwrap();
    let stage = offer.needs.iter().find(|s| s.name == "stage").and_then(|s| s.suggested.clone()).expect("suggested");
    let made = plan_units(&pieces, Choices::new().set("stage", stage)).expect("a plan");
    let titles: Vec<&str> = made
        .objects
        .iter()
        .filter(|one| one.qualified_type == "resqml22.PointSetRepresentation")
        .map(|one| one.title.as_str())
        .collect();
    assert_eq!(titles, vec!["Marnes et Caillasses — Bartonien Top"]);
}

// -- fact 2: the ends are ordered on the Ma, the side decided against the stage

/// **The names are inverted, the numbers are not.** The unit's `olderNamedAge`
/// is the Bartonien at 37.8 Ma and its `youngerNamedAge` the Lutétien at 47.8:
/// the *young* end is in the field called `older`. Asked about the Bartonien the
/// points mark its **top**; asked about the Lutétien, its **base** — and a
/// producer who wrote the fields the right way round must give the same answer.
/// To break: order the ends on the field names in `sigeol::unit_of`.
#[test]
fn the_side_follows_the_ma_and_the_stage_of_the_file_never_the_field_names() {
    let wrong_way = unit("1", "Calcaire grossier", ("Bartonien", 37.8), ("Lutétien", 47.8));
    let right_way = unit("1", "Calcaire grossier", ("Lutétien", 47.8), ("Bartonien", 37.8));
    for items in [&wrong_way, &right_way] {
        for (stage, side) in [("Bartonien", "Top"), ("Lutétien", "Bottom")] {
            let pieces = files(&[("any.json", features(std::slice::from_ref(items)))]);
            let made = plan_units(&pieces, Choices::new().set("stage", json!(stage))).expect("a plan");
            let title = made
                .objects
                .iter()
                .find(|one| one.qualified_type == "resqml22.PointSetRepresentation")
                .map(|one| one.title.clone())
                .expect("a point set");
            assert_eq!(title, format!("Calcaire grossier — {stage} {side}"));
        }
    }
}

// -- fact 5: a unit in two files is one pass --------------------------------

/// **The Lutétien unit whose two ages are both the Lutétien is in the
/// `olderNamedAge` file and in the `youngerNamedAge` file** — the same feature,
/// the same id, twice (measured 01/10: all 8 311 features of the first file are
/// in the second). It is one pass of one borehole: the activity's
/// `featureCount` and the table's `passes` say 2, not 4, and the repeat is
/// *named* in the plan rather than counted as a duplicate place. To break:
/// remove the `repeated` branch of `assemble::plan` — both numbers double.
#[test]
fn a_unit_stated_in_two_exports_is_one_pass() {
    let items = features(&[
        unit("3127257", "Calcaire grossier", ("Lutétien", 41.2), ("Lutétien", 47.8)),
        // Another borehole, so another place: two passes, two points.
        unit("3127259", "Calcaire grossier", ("Lutétien", 41.2), ("Lutétien", 47.8)).replace("2.4321,48.9135,46", "2.5,48.95,46"),
    ]);
    let pieces = files(&[
        ("GeologicUnit_Lutétien_olderNamedAge_ZONE_2.json", items.clone()),
        ("GeologicUnit_Lutétien_youngerNamedAge_ZONE_2.json", items),
    ]);
    let made = plan_units(&pieces, Choices::new().set("stage", json!("Lutétien"))).expect("a plan");
    assert_eq!(number(&made, "featureCount"), 2.0, "two passes, however many files state them");
    assert_eq!(number(&made, "duplicatesDropped"), 0.0, "a repeat is not a duplicate place");
    assert!(
        made.dropped.iter().any(|one| one.count == 2 && one.why.contains("already read")),
        "the repeat is named: {:?}",
        made.dropped
    );

    // The table says the same, row by row: `passes` is the last column.
    let table = made.objects.iter().find(|one| one.role == "table").expect("the table");
    let passes = table.json["Column"]
        .as_array()
        .and_then(|columns| columns.iter().find(|column| column["Title"] == "passes"))
        .expect("the passes column");
    let counted: Vec<i64> = passes["Values"]["Values"].as_array().expect("values").iter().filter_map(Value::as_i64).collect();
    assert_eq!(counted, vec![2]);
}

/// The counterpart: **two features that merely share an id are two features** —
/// an `id` is the producer's key and a second layer may reuse it, so it only
/// counts as a repeat at the same places. To break: key the repeat on the id
/// alone — the second point is lost.
#[test]
fn a_shared_id_at_another_place_is_not_a_repeat() {
    let here = unit("7", "Calcaire grossier", ("Lutétien", 41.2), ("Lutétien", 47.8));
    let there = here.replace("2.4321,48.9135,46", "2.5,48.95,46");
    let pieces = files(&[("GeologicUnit_Lutétien_ZONE_2.json", features(&[here, there]))]);
    let made = plan_units(&pieces, Choices::new().set("stage", json!("Lutétien"))).expect("a plan");
    assert_eq!(number(&made, "featureCount"), 2.0);
    assert_eq!(number(&made, "pointCount"), 2.0);
}

// -- facts 1 and 4: the frame of an imported hole is the log, whole ----------

/// A borehole as the BRGM writes it, its `logElement`s given as `(begin, end)`.
fn borehole(passes: &[(f64, f64)]) -> String {
    let interval = |at: usize, (begin, end): (f64, f64)| {
        format!(
            r#"{{"type":"Feature","id":"{at}","geometry":{{"type":"GeometryCollection","geometries":[]}},
                 "properties":{{"@featureType":"BoreholeInterval",
                   "specification":{{"@href":"https://data.geoscience.fr/id/GeologicUnit/{at}"}},
                   "mappedIntervalBegin":{{"Quantity":{{"value":{begin},"@uom":"m"}}}},
                   "mappedIntervalEnd":{{"Quantity":{{"value":{end},"@uom":"m"}}}},
                   "collectionIdentifier":{{"value":"B_{at}"}}}}}}"#
        )
    };
    let log: Vec<String> = passes.iter().enumerate().map(|(at, pass)| interval(at, *pass)).collect();
    format!(
        r#"{{"type":"FeatureCollection","features":[{{"type":"Feature","id":"BSS000TEST",
            "geometry":{{"type":"Point","coordinates":[2.4321,48.9135]}},
            "properties":{{"@featureType":"Borehole",
              "referenceLocation":{{"type":"Feature","geometry":{{"type":"Point","coordinates":[2.4321,48.9135]}},
                 "properties":{{"@featureType":"OriginPosition","elevation":46}}}},
              "logElement":[{}],
              "parameter":[
                {{"name":{{"@title":"parameter_id_forage"}},"value":"3ca79312-f3fa-4959-bae0-311c7a5c7359"}},
                {{"name":{{"@title":"parameter_id_bss"}},"value":"BSS000TEST"}}]}}}}]}}"#,
        log.join(",")
    )
}

fn imported(passes: &[(f64, f64)]) -> Plan {
    let read = geojson::read(&borehole(passes)).expect("a collection");
    assert!(read.refused.is_empty(), "{:?}", read.refused);
    let pieces = Pieces::empty().with("features", Source::Features { name: "Borehole.json".into(), features: read.features });
    Registry::new()
        .plan("sigeol-boreholes", &pieces, &Choices::new().set("project", json!(2154)), &Identity::derived(), &when())
        .expect("planned")
}

fn node_mds(plan: &Plan) -> Vec<f64> {
    let nodes = plan.datasets.iter().find(|one| one.path.ends_with("/nodeMd")).expect("the frame's depths");
    match &nodes.values {
        energyml_build::operation::Numbers::F64(values) => values.clone(),
        other => panic!("{other:?}"),
    }
}

/// **The whole log, and not the first pass found.** Passes that overlap
/// (`0–2` and `1–3`, two units mapped on one slice) and one that leaves a hole
/// (`3.5–4`): the frame has a node at **every** distinct bound — the elementary
/// intervals the CSV path writes one row each for — and a zero at the ground
/// when the log does not state one. To break: take the first
/// and last bound only in `Borehole::mds`.
#[test]
fn the_frame_of_an_imported_hole_has_a_node_at_every_bound_of_its_log() {
    let made = imported(&[(0.0, 2.0), (1.0, 3.0), (3.5, 4.0)]);
    assert_eq!(node_mds(&made), vec![0.0, 1.0, 2.0, 3.0, 3.5, 4.0]);
    let late = imported(&[(3.0, 5.0), (5.0, 8.0)]);
    assert_eq!(node_mds(&late), vec![0.0, 3.0, 5.0, 8.0], "no depth at the ground: one is added, and said");
    assert!(late.says.iter().any(|note| note.key == "md-zero-added"), "{:?}", late.says);
}

/// The `Known` the package would give back for the frame `made` wrote — what
/// `wells-from-table`'s `wells` slot receives from a caller. Its depths are
/// **stated**, as in `tests/wells_from_table.rs`: reading them back is the
/// storage layer's job and would make this test depend on an optional feature.
fn known_of(made: &Plan) -> Known {
    let of = |kind: &str| -> Option<Ref> {
        made.objects
            .iter()
            .find(|one| one.qualified_type == kind)
            .map(|one| Ref::new(one.uuid.clone(), one.qualified_type.clone(), one.title.clone()))
    };
    let controls = made.datasets.iter().find(|one| one.path.ends_with("/controlPoints")).expect("the line");
    let energyml_build::operation::Numbers::F64(points) = &controls.values else { panic!("{:?}", controls.values) };
    Known {
        reference: of("resqml22.WellboreFrameRepresentation"),
        interpretation: of("resqml22.WellboreInterpretation"),
        trajectory: of("resqml22.WellboreTrajectoryRepresentation"),
        datum: of("eml23.ReferencePointInALocalEngineeringCompoundCrs"),
        crs: of("eml23.LocalEngineeringCompoundCrs"),
        vertical_up: Some(true),
        wellhead: Some([points[0], points[1], points[2]]),
        node_mds: Some(node_mds(made)),
        ..Known::default()
    }
}

fn column(name: &str, uuid: &str, value_type: &str) -> Value {
    json!({
        "ColumnName": name,
        "ValueType": value_type,
        "PropertyType": { "PropertyTypeID": format!("namespace:reference-data--PropertyType:{uuid}:") },
    })
}

/// The CSV the script writes: one row per **elementary** pass, `DEB` the top
/// depth, `PROF` the base — the structural roles of the light template.
fn passes_csv(known: &Known, rows: &[(f64, f64)]) -> String {
    let [x, y, z] = known.wellhead.expect("a head");
    let mut text = String::from("SONDAGE,X,Y,Z_MNT,DEB,PROF\n");
    for (top, base) in rows {
        text.push_str(&format!("BSS000TEST,{x},{y},{z},{top},{base}\n"));
    }
    text
}

fn enrich(csv: &str, known: Known) -> Plan {
    let template = json!([
        column("SONDAGE", "348c9a4d-e3e4-4beb-8456-f07a46660de1", "string"),
        column("X", "620cec71-5ce3-4ea5-9120-833756b427d7", "number"),
        column("Y", "cfcfdb04-02c1-4262-b66b-459b0aab3e8e", "number"),
        column("Z_MNT", "8508cef4-0ade-454a-9ea4-e00d5905e677", "number"),
        column("DEB", "cb1166d9-3874-4267-abea-1cd97e4bf448", "number"),
        column("PROF", "c48c65d4-9680-4d10-903c-a1b2e30f66b5", "number"),
    ]);
    let table = tabular::read("passes.csv", csv, Dialect::sniff(csv)).expect("a table");
    let pieces = Pieces::empty()
        .with("table", Source::Table { name: "passes.csv".into(), table, template: osdu::from_json(&template).expect("a template") })
        .with("wells", Source::Object(known));
    Registry::new()
        .plan(
            "wells-from-table",
            &pieces,
            &Choices::new().set("sourceEpsg", json!(2154)).set("enrich", json!(true)),
            &Identity::derived(),
            &when(),
        )
        .expect("planned")
}

/// **The chain**: `sigeol-boreholes`, then the CSV the script writes from the
/// same log, with `enrich` on. Every limit of the table's passes is already a
/// node of the imported frame, so the frame is **not** touched — no node
/// inserted, none renumbered, no property to rewrite — and the hole is
/// recognised by its title and its head. That holds when the import added the
/// zero the CSV path never writes (a log that starts at 3 m).
///
/// To break: drop a bound of the log from the import (`Borehole::mds` keeping
/// the first and the last only) — the table then states depths the frame lacks,
/// the plan writes a frame and says `merged` instead of `unchanged`. (An extra
/// depth in the import would not redden it: the frame would hold more than the
/// table, and nothing is renumbered — the zero is that case.)
#[test]
fn the_csv_enrichment_lands_on_the_imported_frame_without_moving_a_node() {
    for (log, rows) in [
        // Overlap in the log: the script writes the elementary intervals.
        (vec![(0.0, 2.0), (1.0, 3.0), (3.5, 4.0)], vec![(0.0, 1.0), (1.0, 2.0), (2.0, 3.0), (3.5, 4.0)]),
        // A log that starts below the ground: the import adds the zero, the
        // table never states it.
        (vec![(3.0, 5.0), (5.0, 8.0)], vec![(3.0, 5.0), (5.0, 8.0)]),
    ] {
        let imported = imported(&log);
        let known = known_of(&imported);
        let csv = passes_csv(&known, &rows);
        let again = enrich(&csv, known);
        assert!(again.says.iter().any(|note| note.key == "unchanged"), "{log:?}: {:?}", again.says);
        let frames = again.objects.iter().filter(|one| one.qualified_type == "resqml22.WellboreFrameRepresentation").count();
        assert_eq!(frames, 0, "{log:?}: the frame stays the one the import wrote");
    }
}
