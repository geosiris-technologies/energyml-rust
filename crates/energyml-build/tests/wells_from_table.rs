//! `wells-from-table`, on fabricated tables.
//!
//! The shape of the assertions comes from the specification and from the operation's
//! design; the values come from the files.
//! The corpus is not in the repository: what depends on it is skipped rather than
//! failing, and says so.

use serde_json::{Value, json};

use energyml_build::operation::{Choices, Identity, Registry, When};
use energyml_build::osdu;
use energyml_build::pieces::{Pieces, Source};
use energyml_build::tabular::{self, Dialect};

/// The type of the datum a run writes: a reference point **in the compound**
/// the run's frame is — decision (b) of 24/09, common TRG v2.3 §13.25, p. 371.
/// It was `eml23.ReferencePointInACrs` until then, pointing its
/// `ProjectedCoordinates` at a `ProjectedCrs` the trajectories also named; the
/// trajectories name the compound now (RESQML TRG 2.2 §3.1 p. 22), and the
/// datum sits in the frame they are in.
const DATUM: &str = "eml23.ReferencePointInALocalEngineeringCompoundCrs";

fn when() -> When {
    When {
        creation: "2026-09-22T00:00:00Z".into(),
        originator: "test".into(),
    }
}

/// A template entry.
fn column(name: &str, uuid: &str) -> Value {
    json!({
        "ColumnName": name,
        "ValueType": "number",
        "PropertyType": { "PropertyTypeID": format!("namespace:reference-data--PropertyType:{uuid}:") },
    })
}

/// The template of the five roles, on the names of a fabricated file.
fn roles() -> Value {
    json!([
        column("SONDAGE", "348c9a4d-e3e4-4beb-8456-f07a46660de1"),
        column("X", "620cec71-5ce3-4ea5-9120-833756b427d7"),
        column("Y", "cfcfdb04-02c1-4262-b66b-459b0aab3e8e"),
        column("Z_MNT", "8508cef4-0ade-454a-9ea4-e00d5905e677"),
        column("PROF", "c48c65d4-9680-4d10-903c-a1b2e30f66b5"),
    ])
}

/// A bag of one table.
fn bag(text: &str, template: &Value) -> Pieces {
    bag_named("essai.csv", text, template)
}

/// The same, under a chosen file name — what the identity rule requires not to see.
fn bag_named(name: &str, text: &str, template: &Value) -> Pieces {
    let table = tabular::read(name, text, Dialect::sniff(text)).expect("a table");
    let template = osdu::from_json(template).expect("a template");
    Pieces::empty().with(
        "table",
        Source::Table {
            name: name.into(),
            table,
            template,
        },
    )
}

/// The uuid of the object of a given type, when there is only one.
fn only(made: &energyml_build::operation::Plan, kind: &str) -> String {
    let found: Vec<&energyml_build::operation::NewObject> =
        made.objects.iter().filter(|o| o.qualified_type == kind).collect();
    assert_eq!(found.len(), 1, "{kind} : {} objet(s)", found.len());
    found[0].uuid.clone()
}

/// Plan, with the given settings.
fn plan(pieces: &Pieces, answers: Value) -> Result<energyml_build::operation::Plan, String> {
    let mut choices = Choices::new();
    for (name, value) in answers.as_object().expect("an object") {
        choices = choices.set(name, value.clone());
    }
    Registry::new()
        .plan(
            "wells-from-table",
            pieces,
            &choices,
            &Identity::derived(),
            &when(),
        )
        .map_err(|why| why.to_string())
}

#[test]
fn every_role_uuid_is_in_the_shipped_dictionary() {
    // **Without this check, `ROLES` would be a wish list.** The operation reads
    // the roles by uuid; a uuid that is in no dictionary would never be
    // matched, and nothing would say so — the template would simply look
    // as if it declared no role.
    let missing: Vec<&str> = energyml_domain::property_kind::with_default(|dictionary| {
        energyml_build::ops::wells_from_table::ROLES
            .iter()
            .filter(|role| dictionary.by_uuid(role.uuid).is_none())
            .map(|role| role.uuid)
            .collect()
    });
    assert!(
        missing.is_empty(),
        "roles name uuids absent from the dictionary: {missing:?}"
    );
}

#[test]
fn every_role_title_matches_the_dictionary() {
    // The title serves **only for messages**, but a message that names a title
    // that the dictionary does not have sends one looking for the wrong thing.
    let wrong: Vec<String> = energyml_domain::property_kind::with_default(|dictionary| {
        energyml_build::ops::wells_from_table::ROLES
            .iter()
            .filter_map(|role| {
                let found = dictionary.by_uuid(role.uuid)?;
                (!found.title.eq_ignore_ascii_case(role.title))
                    .then(|| format!("{} : `{}` ≠ `{}`", role.uuid, role.title, found.title))
            })
            .collect()
    });
    assert!(wrong.is_empty(), "{wrong:?}");
}

#[test]
fn one_hole_writes_the_five_objects_resqml_wants() {
    let text = "SONDAGE,X,Y,Z_MNT,PROF\nF1,650000,6800000,120,0\nF1,650000,6800000,120,25\n";
    let made = plan(&bag(text, &roles()), json!({ "sourceEpsg": 2154 })).expect("a plan");
    let kinds: Vec<&str> = made.objects.iter().map(|o| o.qualified_type.as_str()).collect();
    for wanted in [
        "resqml22.WellboreFeature",
        "resqml22.WellboreInterpretation",
        "resqml22.WellboreTrajectoryRepresentation",
        "resqml22.WellboreFrameRepresentation",
        DATUM,
    ] {
        assert!(kinds.contains(&wanted), "{wanted} manque : {kinds:?}");
    }
    assert!(
        made.says
            .iter()
            .any(|n| n.key == "wells" && n.detail.contains('1')),
        "{:?}",
        made.says
    );
}

#[test]
fn the_rows_of_one_name_make_one_hole() {
    // The grouping is by **name**, and it is what makes a flat CSV a
    // set of boreholes. Two names, two boreholes — whatever the order of the
    // lignes.
    let text = "SONDAGE,X,Y,Z_MNT,PROF\nF1,1,2,10,0\nF2,3,4,20,0\nF1,1,2,10,5\nF2,3,4,20,7\n";
    let made = plan(&bag(text, &roles()), json!({ "sourceEpsg": 2154 })).expect("a plan");
    let features = made
        .objects
        .iter()
        .filter(|o| o.qualified_type == "resqml22.WellboreFeature")
        .count();
    assert_eq!(features, 2);
}

#[test]
fn the_depths_are_sorted_and_no_ground_zero_is_added() {
    // **The nodes are ordered, and there are exactly as many as rows.**
    //
    // A decision of 22/09, taken on the comparison with
    // `csv-to-energyml`: the MD=0 should not be added. It corresponds
    // to the DEM, and there is no data at those MDs. Adding it would risk
    // shifting all the property arrays. A frame node is a property
    // array rank: making one that the file does not state
    // forces each property to carry an absence at rank 0.
    //
    // The ground is not lost: it is the datum, and it is from there that the
    // DEM cloud of a batch of boreholes will be drawn.
    let text = "SONDAGE,X,Y,Z_MNT,PROF
F1,0,0,100,30
F1,0,0,100,10
F1,0,0,100,20
";
    let made = plan(&bag(text, &roles()), json!({ "sourceEpsg": 2154 })).expect("a plan");
    let nodes = made
        .datasets
        .iter()
        .find(|set| set.path.ends_with("nodeMd"))
        .expect("the frame's node depths");
    let energyml_build::operation::Numbers::F64(values) = &nodes.values else {
        panic!("the depths are doubles")
    };
    assert_eq!(values, &[10.0, 20.0, 30.0], "sorted, and nothing in front");
}

#[test]
fn a_hole_whose_first_measurement_is_deep_says_what_nothing_sounded() {
    // **Nothing is drawn across what is not sounded, and the gap is
    // said all the same.** Three boreholes of the BRGM corpus only carry their first
    // measurement at 10.5, 31.5 and 36.25 m below the datum: the trajectory starts
    // at that measurement, and keeping quiet about the gap would make a borehole that starts thirty
    // metres under the ground an ordinary borehole.
    //
    // The longest first, and **named**: a count alone would say *how many* without
    // saying *which*, and it is the borehole that will be looked at.
    let text = "SONDAGE,X,Y,Z_MNT,PROF
F1,0,0,100,31.5
F1,0,0,100,32.5
F2,9,9,100,2
F2,9,9,100,3
";
    let made = plan(&bag(text, &roles()), json!({ "sourceEpsg": 2154 })).expect("a plan");
    let note = made
        .says
        .iter()
        .find(|n| n.key == "unmeasured-top")
        .unwrap_or_else(|| panic!("no note: {:?}", made.says));
    assert!(
        note.detail.contains("F1 (31.5 m)"),
        "the longest, named: {}",
        note.detail
    );
    assert!(
        note.detail.starts_with("2 "),
        "both boreholes are counted: {}",
        note.detail
    );

    // **The negative check.** A file that itself states its measurement at the ground
    // has nothing unexplained, and a note that appeared there would be a note that
    // nobody would read elsewhere any more.
    let stated = "SONDAGE,X,Y,Z_MNT,PROF
F1,0,0,100,0
F1,0,0,100,32.5
";
    let quiet = plan(&bag(stated, &roles()), json!({ "sourceEpsg": 2154 })).expect("a plan");
    assert!(
        !quiet.says.iter().any(|n| n.key == "unmeasured-top"),
        "nothing to say when the file carries its zero: {:?}",
        quiet.says
    );
}

#[test]
fn a_run_of_many_wells_writes_its_collection() {
    // **The collection of a run did not exist, and nothing said so.**
    // `apply` groups a role from eight objects (`MANY`) and writes a
    // `DataobjectCollection`; it carried the **name of the type** in
    // `HomogeneousDatatype`, which `Collection.xsd` declares `xs:boolean` —
    // *"Boolean flag. If true all data objects in the collection are of the
    // same Energistics data type"*. The object therefore could not be read back, `apply`
    // refused it, and the activity of the run named nothing.
    //
    // No check could see it: the only one that applies without HDF5
    // (`operations.rs`, `crs`) writes **two** objects, so stays under the threshold.
    // That is what this test adds — eight boreholes, a full role, no array.
    let mut text = String::from(
        "SONDAGE,X,Y,Z_MNT,PROF
",
    );
    for well in 0..8 {
        for depth in [0, 10] {
            text.push_str(&format!(
                "F{well},650000,6800000,120,{depth}
"
            ));
        }
    }
    let made = plan(&bag(&text, &roles()), json!({ "sourceEpsg": 2154 })).expect("a plan");
    let wells = made
        .objects
        .iter()
        .filter(|o| o.qualified_type == "resqml22.WellboreFeature")
        .count();
    assert_eq!(
        wells, 8,
        "eight boreholes, hence a role above the grouping threshold"
    );

    let schemas = std::sync::Arc::new(energyml_schemas::all());
    let mut store = energyml_build::epc::open(None, "wells.epc", schemas).expect("a new package");
    // Without the arrays: what is checked is the writing of the **objects**, and
    // requiring them would make this check an HDF5 target that `--all-targets` does not
    // compile — hence a check that would not run.
    let written = energyml_build::apply::apply_with(&made, &mut store, &when(), false);
    assert!(written.refused.is_empty(), "{:?}", written.refused);
    assert!(
        written
            .written
            .iter()
            .any(|one| one.contains("DataobjectCollection")),
        "the run must write its collection: {:?}",
        written.written
    );
}

#[test]
fn a_table_of_heads_writes_no_trajectory_at_all() {
    // **An MD that the file does not state does not leave the package.** A rule:
    // if it is not present in the file, it is not wanted in the result.
    //
    // A table of heads says where a borehole is collared and nothing of its path.
    // The node at MD 0 that was made up for it was doubly wrong: it stated
    // a depth absent from the file, and it was of no use since
    // `well::write` then refuses a one-point line — so that path
    // produced **no** object.
    let heads = json!([
        column("SONDAGE", "348c9a4d-e3e4-4beb-8456-f07a46660de1"),
        column("X", "620cec71-5ce3-4ea5-9120-833756b427d7"),
        column("Y", "cfcfdb04-02c1-4262-b66b-459b0aab3e8e"),
        column("Z_MNT", "8508cef4-0ade-454a-9ea4-e00d5905e677"),
    ]);
    let text = "SONDAGE,X,Y,Z_MNT
F1,650000,6800000,120
F2,650100,6800100,131
";
    let made = plan(&bag(text, &heads), json!({ "sourceEpsg": 2154 })).expect("a plan");

    let kinds: Vec<&str> = made.objects.iter().map(|o| o.qualified_type.as_str()).collect();
    assert_eq!(
        kinds
            .iter()
            .filter(|k| ***k == *"resqml22.WellboreFeature")
            .count(),
        2,
        "the two heads make two boreholes: {kinds:?}"
    );
    assert_eq!(
        kinds
            .iter()
            .filter(|k| k.contains("Trajectory") || k.contains("Frame"))
            .count(),
        0,
        "nothing of the path: {kinds:?}"
    );
    assert!(made.datasets.is_empty(), "no array: {:?}", made.datasets.len());

    // And the ground is there, in the datum — it is what will carry the DEM cloud.
    let datums: Vec<f64> = made
        .objects
        .iter()
        .filter(|o| o.qualified_type == DATUM)
        .filter_map(|o| o.json["VerticalCoordinate"].as_f64())
        .collect();
    assert_eq!(
        datums,
        [120.0, 131.0],
        "the two grounds, in the order of the file"
    );
}

#[test]
fn a_head_stated_twice_and_differently_is_refused_by_name() {
    // A head that changes from one row to the next is not an average to be taken:
    // it is a file that says two things. Both values are named.
    let text = "SONDAGE,X,Y,Z_MNT,PROF\nF1,650000,6800000,120,0\nF1,650001,6800000,120,25\n";
    let why = plan(&bag(text, &roles()), json!({ "sourceEpsg": 2154 })).expect_err("refuses");
    assert!(why.contains("F1"), "{why}");
    assert!(why.contains("650000") && why.contains("650001"), "{why}");
}

#[test]
fn two_rows_of_one_depth_are_refused_by_their_lines() {
    // Three possible meanings that the file does not tell apart, and two of them
    // lose a measurement: we refuse rather than choose.
    let text = "SONDAGE,X,Y,Z_MNT,PROF\nF1,0,0,10,5\nF1,0,0,10,5\n";
    let why = plan(&bag(text, &roles()), json!({ "sourceEpsg": 2154 })).expect_err("refuses");
    assert!(why.contains('5'), "{why}");
    assert!(why.to_lowercase().contains("distinct"), "{why}");
}

#[test]
fn a_missing_role_is_refused_by_the_role_and_not_by_the_column() {
    // It is the **template** that must be corrected, not the file: a refusal that
    // named a column would send one to edit the CSV.
    let without_ground = json!([
        column("SONDAGE", "348c9a4d-e3e4-4beb-8456-f07a46660de1"),
        column("X", "620cec71-5ce3-4ea5-9120-833756b427d7"),
        column("Y", "cfcfdb04-02c1-4262-b66b-459b0aab3e8e"),
    ]);
    let text = "SONDAGE,X,Y\nF1,1,2\n";
    let why = plan(&bag(text, &without_ground), json!({ "sourceEpsg": 2154 })).expect_err("refuses");
    assert!(why.contains("elevation"), "the refusal must name the role: {why}");
}

#[test]
fn a_declared_column_the_file_does_not_carry_names_both_lists() {
    // A column declared and absent from the file and a column of the file that the
    // template ignores are two different pieces of news.
    let text = "SONDAGE,X,Y,ALTITUDE,PROF\nF1,1,2,10,0\n";
    let why = plan(&bag(text, &roles()), json!({ "sourceEpsg": 2154 })).expect_err("refuses");
    assert!(why.contains("Z_MNT"), "{why}");
    assert!(
        why.contains("ALTITUDE"),
        "the refusal also names what the file carries: {why}"
    );
}

#[test]
fn a_geographic_target_is_refused_because_a_hole_is_a_vertical_extent() {
    // The argument of `sigeol_boreholes`, and the defect it cost on 19/09:
    // in degrees, each borehole comes out between a hundred and ten thousand times too long,
    // in a package that `check` validates and that nothing draws.
    //
    // **Called directly, and it is the only place from which this is reachable** —
    // the same arrangement as `sigeol-boreholes`. Through the registry, `suggest`
    // proposes a UTM zone drawn from the longitude and `settle` sets it before
    // `plan` runs, so the refusal never falls. What it guards is the
    // **library**: a caller who sets `project: None` themselves
    // does not get a package in degrees. And a check that cannot
    // fail would prove nothing.
    use energyml_build::declare::{Epsg, TableIn};
    use energyml_build::operation::Operation;
    let text = "SONDAGE,X,Y,Z_MNT,PROF
F1,2.3,48.9,120,0
F1,2.3,48.9,120,25
";
    let table = tabular::read("essai.csv", text, Dialect::sniff(text)).expect("a table");
    let template = osdu::from_json(&roles()).expect("a template");
    let why = energyml_build::ops::wells_from_table::WellsFromTable
        .plan(
            energyml_build::ops::wells_from_table::TableInputs {
                table: vec![TableIn {
                    name: "table".into(),
                    file: "essai.csv".into(),
                    table,
                    template,
                }],
                wells: Vec::new(),
            },
            energyml_build::ops::wells_from_table::TableParams {
                source_epsg: Some(Epsg(4326)),
                project: None,
                vertical: None,
                datum_kind: energyml_build::ops::well::DatumKind::GroundLevel,
                data_file: "data.h5".into(),
                head_tolerance: 0.01,
                enrich: false,
            },
            &energyml_build::operation::Context {
                identity: &Identity::derived(),
                when: &when(),
                purpose: energyml_build::operation::Purpose::Writing,
                feedback: &energyml_build::typed::Silent,
                bag: &Pieces::empty(),
            },
        )
        .expect_err("degrees horizontally, metres vertically");
    let said = why.to_string();
    assert!(said.contains("4326"), "it names the system refused: {said}");
    assert!(said.contains("project"), "and the parameter to fill: {said}");
}

#[test]
fn a_geographic_file_is_proposed_a_metric_target() {
    // The other half, and it is the one that counts in use: through the registry, a
    // table in degrees receives a **suggested** UTM zone, drawn from the longitude
    // and not from a country. Visible and replaceable, never a default that applies
    // by itself — a reprojection moves every coordinate.
    let text = "SONDAGE,X,Y,Z_MNT,PROF
F1,2.3,48.9,120,0
F1,2.3,48.9,120,25
";
    let made = plan(&bag(text, &roles()), json!({ "sourceEpsg": 4326 })).expect("a plan");
    let projected = made
        .activity
        .parameters
        .iter()
        .any(|p| format!("{p:?}").contains("project"));
    assert!(
        projected,
        "the suggestion is recorded: {:?}",
        made.activity.parameters
    );
    assert!(
        made.objects
            .iter()
            .any(|o| o.qualified_type == "resqml22.WellboreFeature")
    );
}

#[test]
fn the_file_can_state_its_own_projection_code() {
    // A constant is read in a column **or** imposed as a setting. Here
    // it is the column, and no `sourceEpsg` is given.
    let mut template = roles();
    template
        .as_array_mut()
        .unwrap()
        .push(column("EPSG", "d48ef4e7-f82e-4e91-bda2-685146223a85"));
    let text = "SONDAGE,X,Y,Z_MNT,PROF,EPSG\nF1,650000,6800000,120,0,2154\nF1,650000,6800000,120,25,2154\n";
    let made = plan(&bag(text, &template), json!({})).expect("a plan");
    assert!(
        made.objects
            .iter()
            .any(|o| o.qualified_type == "resqml22.WellboreFeature")
    );
}

#[test]
fn two_projection_codes_in_one_file_are_refused() {
    // The boreholes of a file share a frame: placing them together in
    // either would place some of them wrong.
    let mut template = roles();
    template
        .as_array_mut()
        .unwrap()
        .push(column("EPSG", "d48ef4e7-f82e-4e91-bda2-685146223a85"));
    let text = "SONDAGE,X,Y,Z_MNT,PROF,EPSG\nF1,1,2,10,0,2154\nF2,3,4,20,0,3949\n";
    let why = plan(&bag(text, &template), json!({})).expect_err("refuses");
    assert!(why.contains("2154") && why.contains("3949"), "{why}");
}

#[test]
fn nothing_states_the_system_and_the_refusal_says_both_ways_out() {
    let text = "SONDAGE,X,Y,Z_MNT,PROF\nF1,1,2,10,0\n";
    let why = plan(&bag(text, &roles()), json!({})).expect_err("refuses");
    assert!(why.contains("sourceEpsg"), "{why}");
    assert!(why.contains("template"), "the refusal names both ways: {why}");
}

#[test]
fn a_column_the_template_ignores_is_counted_and_not_refused() {
    // A business file always carries columns that are no concern of ours.
    let text = "SONDAGE,X,Y,Z_MNT,PROF,Commentaire,Entreprise\nF1,1,2,10,0,bla,x\nF1,1,2,10,5,bla,x\n";
    let made = plan(&bag(text, &roles()), json!({ "sourceEpsg": 2154 })).expect("a plan");
    let note = made
        .says
        .iter()
        .find(|n| n.key == "columns-ignored")
        .unwrap_or_else(|| panic!("the ignored columns are said: {:?}", made.says));
    assert!(note.detail.contains('2'), "{}", note.detail);
}

// ---------------------------------------------------------------------------
// Identity comes from the content
// ---------------------------------------------------------------------------

#[test]
fn the_same_hole_under_two_file_names_is_the_same_objects() {
    // **Measured on 23/09, and it is what made this rule necessary**: the name of the
    // file was in the seed, so the same export under another name
    // took three boreholes to six — each doubled under two
    // identities, and nothing said so. A producer that dates its exports
    // (`wells-2026-09.csv`, then `wells-2026-10.csv`) triggered that every time.
    // livraison.
    let text = "SONDAGE,X,Y,Z_MNT,PROF
F1,650000,6800000,120,2.25
F1,650000,6800000,120,26.16
";
    let answers = json!({ "sourceEpsg": 2154 });
    let first = plan(&bag_named("wells-2026-09.csv", text, &roles()), answers.clone()).expect("a plan");
    let second = plan(&bag_named("wells-2026-10.csv", text, &roles()), answers).expect("a plan");

    for kind in [
        "resqml22.WellboreFeature",
        "resqml22.WellboreInterpretation",
        "resqml22.WellboreTrajectoryRepresentation",
        "resqml22.WellboreFrameRepresentation",
        DATUM,
    ] {
        assert_eq!(
            only(&first, kind),
            only(&second, kind),
            "{kind}: two identities for one borehole"
        );
    }
}

#[test]
fn a_changed_depth_writes_a_second_trajectory_on_the_same_interpretation() {
    // The other half of the identity rule, as requested: the
    // wells etc. that already exist […] get a v2 too, but one that is
    // hooked onto the same interpretation. In RESQML an interpretation
    // carries several representations; it is the idiom, not a workaround.
    //
    // What must **not** move is the borehole: a corrected depth does not
    // make a second borehole, it makes a second path in the same one.
    let base = "SONDAGE,X,Y,Z_MNT,PROF
F1,650000,6800000,120,2.25
F1,650000,6800000,120,26.16
";
    let moved = "SONDAGE,X,Y,Z_MNT,PROF
F1,650000,6800000,120,2.75
F1,650000,6800000,120,26.16
";
    let answers = json!({ "sourceEpsg": 2154 });
    let first = plan(&bag(base, &roles()), answers.clone()).expect("a plan");
    let second = plan(&bag(moved, &roles()), answers).expect("a plan");

    assert_eq!(
        only(&first, "resqml22.WellboreFeature"),
        only(&second, "resqml22.WellboreFeature"),
        "a corrected depth made a second borehole"
    );
    let interpretation = only(&first, "resqml22.WellboreInterpretation");
    assert_eq!(interpretation, only(&second, "resqml22.WellboreInterpretation"));

    // The trajectory and the frame, for their part, are a second object.
    for kind in [
        "resqml22.WellboreTrajectoryRepresentation",
        "resqml22.WellboreFrameRepresentation",
    ] {
        assert_ne!(
            only(&first, kind),
            only(&second, kind),
            "{kind}: the correction overwrote the earlier object"
        );
    }

    // And the second trajectory hangs on **the same** interpretation, otherwise it
    // would be an object beside it rather than a second version of the same borehole.
    let trajectory = second
        .objects
        .iter()
        .find(|o| o.qualified_type == "resqml22.WellboreTrajectoryRepresentation")
        .expect("a trajectory");
    assert_eq!(
        trajectory.json["RepresentedObject"]["Uuid"].as_str(),
        Some(interpretation.as_str())
    );

    // **And no version label is set.** Measured: writing
    // one put 15 warnings on a package that had none — a
    // reference to a versioned object must say *which* version, and nothing here
    // writes it — and ETP servers only take the uuid as key. It is
    // the uuid that tells apart, and it comes from the content.
    assert!(trajectory.json["objectVersion"].is_null(), "{}", trajectory.json);
    assert!(
        trajectory.json["ObjectVersionReason"].is_null(),
        "{}",
        trajectory.json
    );
}

#[test]
fn a_second_version_is_named_rather_than_written_in_silence() {
    // **A v2 written in silence is half the information.** Since the identity rule,
    // a different content adds an object instead of overwriting the previous one; on
    // screen, two objects with the same title read as a duplicate as long as
    // nobody says that the second has just been written.
    //
    // `Made::versioned` is that sentence, and it is **exact on both
    // sides**: the feature and the interpretation are replaced, so they are not
    // in it; the trajectory and the frame are written beside, so they are
    // in it. A count that mixed them would say *five versions* of a borehole that
    // has only two.
    let base = "SONDAGE,X,Y,Z_MNT,PROF
F1,650000,6800000,120,2.25
F1,650000,6800000,120,26.16
";
    let moved = "SONDAGE,X,Y,Z_MNT,PROF
F1,650000,6800000,120,2.75
F1,650000,6800000,120,26.16
";
    let answers = json!({ "sourceEpsg": 2154 });
    let first = plan(&bag(base, &roles()), answers.clone()).expect("a plan");
    let second = plan(&bag(moved, &roles()), answers).expect("a plan");

    let schemas = std::sync::Arc::new(energyml_schemas::all());
    let mut store = energyml_build::epc::open(None, "wells.epc", schemas).expect("a new package");
    // Without the arrays: this test is about identity, and the HDF5 backend is
    // optional — making it depend on it would remove it from `cargo test
    // --workspace`, which is where it must turn red.
    let made = energyml_build::apply::apply_with(&first, &mut store, &when(), false);
    assert!(
        made.invalid.is_empty(),
        "objects written against their schema: {:#?}",
        made.invalid
    );
    assert!(made.refused.is_empty(), "{:?}", made.refused);
    assert!(
        made.versioned.is_empty(),
        "nothing was there: {:?}",
        made.versioned
    );

    let made = energyml_build::apply::apply_with(&second, &mut store, &when(), false);
    assert!(
        made.invalid.is_empty(),
        "objects written against their schema: {:#?}",
        made.invalid
    );
    assert!(made.refused.is_empty(), "{:?}", made.refused);
    let versioned = made.versioned.join(" | ");
    assert!(
        versioned.contains("trajectory"),
        "the trajectory is not named: {versioned}"
    );
    assert!(versioned.contains("frame"), "the frame is not named: {versioned}");
    assert!(
        !versioned.contains("interpretation"),
        "the interpretation is replaced, not doubled: {versioned}"
    );
    assert_eq!(made.versioned.len(), 2, "{versioned}");
}

// ---------------------------------------------------------------------------
// Recognising a borehole, and what "enriching" costs
// ---------------------------------------------------------------------------

/// The package of a first batch, and the frames it holds — what the slot
/// `wells` would receive from a caller.
fn written(text: &str) -> (energyml_storage::EpcStore, energyml_build::operation::Plan) {
    let made = plan(&bag(text, &roles()), json!({ "sourceEpsg": 2154 })).expect("a plan");
    let schemas = std::sync::Arc::new(energyml_schemas::all());
    let mut store = energyml_build::epc::open(None, "wells.epc", schemas).expect("a new package");
    let done = energyml_build::apply::apply_with(&made, &mut store, &when(), false);
    assert!(done.refused.is_empty(), "{:?}", done.refused);
    (store, made)
}

/// The bag of a table **plus** the boreholes already written, given by their frame.
///
/// The MDs cannot be read back without an array backend, so they are
/// **stated**: this test is about matching, not about reading HDF5, and making it
/// depend on an optional feature would remove it from `cargo test
/// --workspace`, where it must turn red.
fn bag_with(text: &str, before: &energyml_build::operation::Plan, head: [f64; 3], mds: &[f64]) -> Pieces {
    use energyml_build::pieces::{Known, Ref};

    let of = |kind: &str| -> Option<Ref> {
        before
            .objects
            .iter()
            .find(|one| one.qualified_type == kind)
            .map(|one| Ref::new(one.uuid.clone(), one.qualified_type.clone(), one.title.clone()))
    };
    // **The uuids are those the package really carries** — taken from the plan that
    // has just been written, not invented. What this bag replaces is the *reading*
    // of the frame, which needs the HDF5 backend: `known_of_object` on an
    // unreadable frame answers *nothing known*, and that is the right answer. The
    // resolution at the edge (`read::well_refs_of`) is covered by the `hdf5` test
    // further down, the only place where a frame is really read back.
    let known = Known {
        reference: of("resqml22.WellboreFrameRepresentation"),
        interpretation: of("resqml22.WellboreInterpretation"),
        trajectory: of("resqml22.WellboreTrajectoryRepresentation"),
        datum: of(DATUM),
        // The frame of its geometry, the one the first batch wrote — what
        // a rewritten trajectory keeps.
        // §2.4).
        crs: of("eml23.LocalEngineeringCompoundCrs"),
        vertical_up: Some(true),
        wellhead: Some(head),
        node_mds: Some(mds.to_vec()),
        ..Known::default()
    };
    // **An object piece, and it is its own companion** — the convention
    // of `declare::paired`. No shape is needed: nothing of the matching rule
    // looks at the geometry of a frame.
    bag(text, &roles()).with("wells", Source::Object(known))
}

/// The same bag, the frame of the package being in **another frame** — the compound
/// of another producer (the BRGM writes its own), or none that the package states.
fn bag_in_frame(
    text: &str,
    before: &energyml_build::operation::Plan,
    frame: Option<energyml_build::pieces::Ref>,
) -> Pieces {
    let mut pieces = Pieces::empty();
    for (name, source) in bag_with(text, before, [650000.0, 6800000.0, 120.0], &[2.25, 26.16]).iter() {
        let source = match source {
            Source::Object(known) if name == "wells" => Source::Object(energyml_build::pieces::Known {
                crs: frame.clone(),
                ..known.clone()
            }),
            other => other.clone(),
        };
        pieces = pieces.with(name.to_string(), source);
    }
    pieces
}

/// **Enriching does not make a borehole change frame**: `enrich` used to rewrite the trajectory of a
/// borehole continued **under the frame of this run**, and a BRGM borehole, written
/// in its own compound, changed frame without any
/// line saying so.
///
/// Keeping it is not possible — the numbers of this run are placed, and the
/// placement of the borehole's frame does not reach a pure operation — so three
/// cases, and each turns red on its own: the same frame as this run, the trajectory is
/// rewritten in it; another frame, or a frame that the package does not state, the
/// borehole is set aside **by name** rather than rewritten elsewhere. To make it
/// turn red: remove the guard in `wells_from_table.rs`, and the BRGM trajectory
/// leaves again under our compound.
#[test]
fn enriching_never_moves_the_trajectory_it_rewrites_to_another_frame() {
    let first = "SONDAGE,X,Y,Z_MNT,PROF
F1,650000,6800000,120,2.25
F1,650000,6800000,120,26.16
";
    let more = "SONDAGE,X,Y,Z_MNT,PROF
F1,650000,6800000,120,12.0
";
    let (_store, before) = written(first);

    // The frame of this run, the same as the package's: rewritten in it.
    let same = plan(
        &bag_with(more, &before, [650000.0, 6800000.0, 120.0], &[2.25, 26.16]),
        json!({ "sourceEpsg": 2154, "enrich": true }),
    )
    .expect("a plan");
    let ours = before
        .objects
        .iter()
        .find(|o| o.qualified_type == "eml23.LocalEngineeringCompoundCrs")
        .expect("the frame of the first run");
    let trajectory = same
        .objects
        .iter()
        .find(|o| o.qualified_type == "resqml22.WellboreTrajectoryRepresentation")
        .expect("the trajectory, rewritten");
    assert_eq!(trajectory.json["Geometry"]["LocalCrs"]["Uuid"], json!(ours.uuid));

    // The compound of another producer: set aside, and named.
    let theirs = energyml_build::pieces::Ref::new(
        "abababab-0000-4000-8000-00000000000b",
        "eml23.LocalEngineeringCompoundCrs",
        "Their frame",
    );
    let why = plan(
        &bag_in_frame(more, &before, Some(theirs)),
        json!({ "sourceEpsg": 2154, "enrich": true }),
    )
    .expect_err("nothing left to write");
    assert!(
        why.contains("F1") && why.contains("Their frame") && why.contains("enrich"),
        "{why}"
    );

    // A frame that the package does not state: set aside, and named.
    let why = plan(
        &bag_in_frame(more, &before, None),
        json!({ "sourceEpsg": 2154, "enrich": true }),
    )
    .expect_err("nothing left to write");
    assert!(why.contains("F1") && why.contains("frame"), "{why}");

    // And without `enrich`, nothing is rewritten: a second path, in the frame
    // of this run, beside theirs — the way the refusal points to.
    let beside = plan(
        &bag_in_frame(
            more,
            &before,
            Some(energyml_build::pieces::Ref::new(
                "abababab-0000-4000-8000-00000000000b",
                "eml23.LocalEngineeringCompoundCrs",
                "Their frame",
            )),
        ),
        json!({ "sourceEpsg": 2154 }),
    )
    .expect("a second trajectory");
    let trajectory = beside
        .objects
        .iter()
        .find(|o| o.qualified_type == "resqml22.WellboreTrajectoryRepresentation")
        .expect("a second trajectory");
    assert_eq!(trajectory.json["Geometry"]["LocalCrs"]["Uuid"], json!(ours.uuid));
}

#[test]
fn a_well_the_package_already_holds_is_recognised_by_its_title_and_head() {
    // The identity rule: a well can be identified as the
    // same one by its wellhead position and its title. Recognised, it
    // receives **neither a new feature nor interpretation nor datum**: they are already
    // there, and rewriting one would repoint it at a derived feature that the
    // package does not know.
    let first = "SONDAGE,X,Y,Z_MNT,PROF
F1,650000,6800000,120,2.25
F1,650000,6800000,120,26.16
";
    let more = "SONDAGE,X,Y,Z_MNT,PROF
F1,650000,6800000,120,2.25
F1,650000,6800000,120,12.0
F1,650000,6800000,120,26.16
";
    let (_store, before) = written(first);

    let again = plan(
        &bag_with(more, &before, [650000.0, 6800000.0, 120.0], &[2.25, 26.16]),
        json!({ "sourceEpsg": 2154 }),
    )
    .expect("a plan");
    let kinds: Vec<&str> = again.objects.iter().map(|o| o.qualified_type.as_str()).collect();
    for absent in [
        "resqml22.WellboreFeature",
        "resqml22.WellboreInterpretation",
        DATUM,
    ] {
        assert!(
            !kinds.contains(&absent),
            "{absent} was rewritten although the borehole is recognised: {kinds:?}"
        );
    }
    // A second path, beside the first, on the same interpretation.
    assert!(
        kinds.contains(&"resqml22.WellboreTrajectoryRepresentation"),
        "{kinds:?}"
    );
    assert!(
        again.says.iter().any(|n| n.key == "continued"),
        "{:?}",
        again.says
    );
}

#[test]
fn a_well_whose_depths_are_all_there_already_writes_nothing() {
    // If the values are identical there is nothing to do. The check is
    // exact: one more object, even identical, would make the package grow without adding anything to it — and two
    // paths with the same title for the same thing are
    // precisely what the identity rule avoids.
    let text = "SONDAGE,X,Y,Z_MNT,PROF
F1,650000,6800000,120,2.25
F1,650000,6800000,120,26.16
";
    let (_store, before) = written(text);

    let again = plan(
        &bag_with(text, &before, [650000.0, 6800000.0, 120.0], &[2.25, 26.16]),
        json!({ "sourceEpsg": 2154 }),
    )
    .expect("a plan");
    // **No object of the borehole** — the plan keeps its frame, which it creates
    // at the level of the run and not of the hole; it is the borehole that does not move.
    let wells: Vec<&str> = again
        .objects
        .iter()
        .map(|o| o.qualified_type.as_str())
        .filter(|kind| kind.contains("Wellbore") || *kind == DATUM)
        .collect();
    assert!(wells.is_empty(), "nothing had changed: {wells:?}");
    assert!(
        again.says.iter().any(|n| n.key == "unchanged"),
        "{:?}",
        again.says
    );
}

#[test]
fn enriching_reuses_the_frame_and_says_the_properties_must_be_rewritten() {
    // The option, and **the news that it obliges to say**. Adding a node in the
    // middle of a frame renumbers the following ones, so every property already
    // laid on it then describes the wrong depths. Keeping quiet about it
    // would silently do exactly what the identity rule warns against.
    let first = "SONDAGE,X,Y,Z_MNT,PROF
F1,650000,6800000,120,2.25
F1,650000,6800000,120,26.16
";
    let more = "SONDAGE,X,Y,Z_MNT,PROF
F1,650000,6800000,120,12.0
";
    let (_store, before) = written(first);
    let held: Vec<&str> = before
        .objects
        .iter()
        .filter(|o| o.qualified_type == "resqml22.WellboreFrameRepresentation")
        .map(|o| o.uuid.as_str())
        .collect();
    assert_eq!(held.len(), 1);

    let bag = bag_with(more, &before, [650000.0, 6800000.0, 120.0], &[2.25, 26.16]);
    let again = plan(&bag, json!({ "sourceEpsg": 2154, "enrich": true })).expect("a plan");
    let frame = again
        .objects
        .iter()
        .find(|o| o.qualified_type == "resqml22.WellboreFrameRepresentation")
        .expect("a frame");
    assert_eq!(
        frame.uuid, held[0],
        "enriching rewrites the frame of the package, it does not make a second one"
    );
    assert_eq!(
        frame.json["NodeCount"].as_u64(),
        Some(3),
        "the union of the depths: 2.25 — 12 — 26.16"
    );

    let note = again
        .says
        .iter()
        .find(|n| n.key == "merged")
        .unwrap_or_else(|| panic!("{:?}", again.says));
    assert!(note.detail.contains("renumbered"), "{}", note.detail);

    // **And without the option, the frame of the package is not touched**: it is what
    // protects the properties already written.
    let safe = plan(&bag, json!({ "sourceEpsg": 2154 })).expect("a plan");
    let frame = safe
        .objects
        .iter()
        .find(|o| o.qualified_type == "resqml22.WellboreFrameRepresentation")
        .expect("a frame");
    assert_ne!(frame.uuid, held[0], "without `enrich`, a second pair beside");
}

#[test]
fn a_ground_that_moved_is_a_conflict_and_not_a_correction() {
    // The ground is a **value**, not an identity: two files that place
    // the same head at two altitudes disagree on a measurement on which
    // every node depends. Correcting silently would lose what the package says.
    let first = "SONDAGE,X,Y,Z_MNT,PROF
F1,650000,6800000,120,2.25
F1,650000,6800000,120,26.16
";
    let moved = "SONDAGE,X,Y,Z_MNT,PROF
F1,650000,6800000,118.5,2.25
F1,650000,6800000,118.5,26.16
";
    let (_store, before) = written(first);

    let again = plan(
        &bag_with(moved, &before, [650000.0, 6800000.0, 120.0], &[2.25, 26.16]),
        json!({ "sourceEpsg": 2154 }),
    )
    .expect("a plan");
    let kinds: Vec<&str> = again.objects.iter().map(|o| o.qualified_type.as_str()).collect();
    // A second reference point, because it is another one.
    assert!(kinds.contains(&DATUM), "{kinds:?}");
    // But still the same borehole: neither a feature nor an interpretation more.
    assert!(!kinds.contains(&"resqml22.WellboreInterpretation"), "{kinds:?}");
    assert!(
        again.says.iter().any(|n| n.key == "ground-moved"),
        "{:?}",
        again.says
    );
}

#[test]
fn two_holes_of_the_same_name_at_two_places_are_two_wells() {
    // **The other half of the rule**, and without it the matching would be done on
    // the title alone. Two campaigns both name `SP1`; they are two
    // holes, five hundred metres from each other, and confusing them would lay the
    // log of the second along the trajectory of the first — a valid package
    // in which nothing on screen would say that the measurements changed borehole.
    let here = "SONDAGE,X,Y,Z_MNT,PROF
F1,650000,6800000,120,2.25
F1,650000,6800000,120,26.16
";
    let there = "SONDAGE,X,Y,Z_MNT,PROF
F1,650500,6800000,120,2.25
F1,650500,6800000,120,26.16
";
    let (_store, before) = written(here);

    let again = plan(
        &bag_with(there, &before, [650000.0, 6800000.0, 120.0], &[2.25, 26.16]),
        json!({ "sourceEpsg": 2154 }),
    )
    .expect("a plan");

    // A whole borehole, feature included: it is not the package's.
    let kinds: Vec<&str> = again.objects.iter().map(|o| o.qualified_type.as_str()).collect();
    for wanted in [
        "resqml22.WellboreFeature",
        "resqml22.WellboreInterpretation",
        "resqml22.WellboreTrajectoryRepresentation",
        "resqml22.WellboreFrameRepresentation",
        DATUM,
    ] {
        assert!(
            kinds.contains(&wanted),
            "{wanted} is missing — the borehole was taken for the package's: {kinds:?}"
        );
    }
    assert!(
        !again.says.iter().any(|n| n.key == "continued"),
        "{:?}",
        again.says
    );
}

#[cfg(feature = "hdf5")]
#[test]
fn a_written_frame_names_its_trajectory_and_its_datum_when_it_is_read_back() {
    // **The resolution at the edge**, and it is the only place that can see it:
    // `read::well_refs_of` opens the frame, follows its `Trajectory`, then the
    // `Datum` of its `MdInterval`. All the rest of the matching is tested without a backend
    // because the matching works on `Known`; **this** needs a
    // package that is really read back.
    //
    // Without these two references, continuing a borehole would write a derived datum
    // that nothing else in the package names — and the package would then say
    // two different things about *counted from what* the MDs are counted.
    let text = "SONDAGE,X,Y,Z_MNT,PROF
F1,650000,6800000,120,2.25
F1,650000,6800000,120,26.16
";
    let made = plan(&bag(text, &roles()), json!({ "sourceEpsg": 2154 })).expect("a plan");

    let dir = std::env::temp_dir().join(format!("energyml-build-wells-refs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("wells.epc");
    let schemas = std::sync::Arc::new(energyml_schemas::all());
    let mut store = energyml_build::epc::open(None, "wells.epc", schemas.clone()).expect("a new package");
    store.external_files_mut().set_root(&dir);
    let done = energyml_build::apply::apply(&made, &mut store, &when());
    assert!(done.refused.is_empty(), "{:?}", done.refused);
    store
        .save_as(&out, &energyml_storage::SaveOptions::default())
        .unwrap();

    let reread = energyml_storage::EpcStore::open(&out, schemas).unwrap();
    let of = |kind: &str| {
        made.objects
            .iter()
            .find(|one| one.qualified_type == kind)
            .expect(kind)
            .uuid
            .clone()
    };
    let frame = of("resqml22.WellboreFrameRepresentation");
    let known = energyml_build::read::pieces_of_object(&reread, &frame)
        .expect("the frame's pieces")
        .into_iter()
        .find_map(|(_, source)| match source {
            Source::Object(known) => Some(known),
            _ => None,
        })
        .expect("what is known");

    // **`known_of_object` is not that path, and checking it is the point.**
    // Its first branch asks *does this object read as a coordinate
    // system?* — and `crs::read_with` says yes of a frame, which names a
    // `LocalCrs` in its geometry. It then returns a `Known` where everything else
    // is absent. This is not what the matching rule borrows (the bag goes through
    // `pieces_of_object`), but a caller who relied on it would believe that a
    // frame says nothing of itself — hence this check, which **pins the difference**
    // rather than letting it be discovered elsewhere.
    // (a known open point).
    let stripped = energyml_build::read::known_of_object(&reread, &frame).expect("what is known");
    assert!(
        stripped.trajectory.is_none() && stripped.wellhead.is_none(),
        "`known_of_object` has stopped taking the CRS branch for a frame — if that is intended, this check is to be turned around"
    );
    assert_eq!(
        known.trajectory.as_ref().map(|one| one.uuid.as_str()),
        Some(of("resqml22.WellboreTrajectoryRepresentation").as_str())
    );
    assert_eq!(
        known.datum.as_ref().map(|one| one.uuid.as_str()),
        Some(of(DATUM).as_str())
    );
    // And the head comes from the datum, never from the geometry: the first node is
    // at 2.25 m, the ground at 120.
    assert_eq!(known.wellhead, Some([650000.0, 6800000.0, 120.0]));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_representation_given_to_an_object_slot_binds_through_its_companion() {
    // **The path of the CLI and of the front end**, and nothing else exercised it.
    // `read::in_slot` of a representation writes **two** pieces — the shape
    // under the name of the slot, and `"<slot> (object)"` for what is
    // known of it. An object slot wants the companion; refusing on the nature of
    // the first made `wells` unreachable from a package, since a
    // frame *is* a representation.
    //
    // The bag is built as `read::in_slot` builds it, by hand, because
    // reading a real frame needs the HDF5 backend — and what is tested here
    // is the **binding**, not the reading.
    use energyml_build::pieces::{Known, Ref};

    let first = "SONDAGE,X,Y,Z_MNT,PROF
F1,650000,6800000,120,2.25
F1,650000,6800000,120,26.16
";
    let (_store, before) = written(first);
    let of = |kind: &str| -> Option<Ref> {
        before
            .objects
            .iter()
            .find(|one| one.qualified_type == kind)
            .map(|one| Ref::new(one.uuid.clone(), one.qualified_type.clone(), one.title.clone()))
    };
    let known = Known {
        reference: of("resqml22.WellboreFrameRepresentation"),
        interpretation: of("resqml22.WellboreInterpretation"),
        trajectory: of("resqml22.WellboreTrajectoryRepresentation"),
        datum: of(DATUM),
        wellhead: Some([650000.0, 6800000.0, 120.0]),
        node_mds: Some(vec![2.25, 26.16]),
        ..Known::default()
    };
    let pieces = bag(first, &roles())
        .with(
            "wells",
            Source::Shape {
                geometry: energyml_geometry::Geometry {
                    kind: "wellbore",
                    patches: Vec::new(),
                },
                closed: None,
            },
        )
        .with("wells (object)", Source::Object(known));

    // It binds, and the borehole is recognised: it is the proof that the companion was
    // taken. Without the binding, the plan would refuse *"`wells` is shape"*.
    let again = plan(&pieces, json!({ "sourceEpsg": 2154 })).expect("a plan");
    assert!(
        again.says.iter().any(|n| n.key == "unchanged"),
        "{:?}",
        again.says
    );
}

// -- the boreholes of the package, by default ------------------------------

/// What the `wells` slot knows of an object, reduced to its reference and to what
/// one wants to say of it.
fn known_as(kind: &str, title: &str, head: Option<[f64; 3]>, mds: Option<&[f64]>) -> Source {
    use energyml_build::pieces::{Known, Ref};
    Source::Object(Known {
        reference: Some(Ref::new("00000000-0000-0000-0000-000000000009", kind, title)),
        wellhead: head,
        node_mds: mds.map(<[f64]>::to_vec),
        ..Known::default()
    })
}

const ONE_WELL: &str = "SONDAGE,X,Y,Z_MNT,PROF
F1,650000,6800000,120,2.25
F1,650000,6800000,120,26.16
";

#[test]
fn what_is_no_frame_is_refused_by_its_type() {
    // One day, the scene's list gave the trajectories with the frames,
    // and the refusal said *"state no wellhead or no node depths"* — true, and
    // mute on the one thing to correct. What one breaks to make it
    // turn red: remove the type check from `Standing::of`.
    let pieces = bag(ONE_WELL, &roles()).with(
        "wells",
        known_as(
            "resqml22.WellboreTrajectoryRepresentation",
            "F1 — trajectory",
            Some([650000.0, 6800000.0, 120.0]),
            None,
        ),
    );
    let refused = plan(&pieces, json!({ "sourceEpsg": 2154 })).expect_err("a trajectory is no frame");
    assert!(refused.contains("no wellbore frame"), "{refused}");
    assert!(
        refused.contains("resqml22.WellboreTrajectoryRepresentation"),
        "the type is named: {refused}"
    );
    // And the message reads in one piece: a lost continuation left twenty
    // spaces in the middle of the sentence.
    assert!(!refused.contains("   "), "{refused}");
}

#[test]
fn a_mute_frame_no_row_names_does_not_block_the_import() {
    // Since `wells` takes all the frames of the package, a frame of another
    // producer, with no datum, must not block the import of a borehole
    // that has nothing to do with it.
    let pieces = bag(ONE_WELL, &roles()).with(
        "wells",
        known_as("resqml22.WellboreFrameRepresentation", "F9 — frame", None, None),
    );
    let made = plan(&pieces, json!({ "sourceEpsg": 2154 })).expect("F9 does not concern F1");
    assert!(
        made.objects
            .iter()
            .any(|one| one.qualified_type == "resqml22.WellboreFeature" && one.title == "F1"),
        "F1 is written"
    );
}

#[test]
fn a_mute_frame_a_row_names_is_refused_by_that_name() {
    // The other half, and it is for it that the refusal existed: F1 may
    // already be there, and without its head we cannot know. Writing it
    // believing to create would be a duplicate.
    let pieces = bag(ONE_WELL, &roles()).with(
        "wells",
        known_as(
            "resqml22.WellboreFrameRepresentation",
            "F1 — frame",
            None,
            Some(&[2.25]),
        ),
    );
    let refused = plan(&pieces, json!({ "sourceEpsg": 2154 })).expect_err("F1 cannot be matched");
    assert!(refused.contains("`F1 — frame`"), "the frame is named: {refused}");
    assert!(!refused.contains("   "), "{refused}");
}

#[test]
fn an_empty_every_slot_does_not_make_an_operation_unfit() {
    // The front end judges its offers before the edge fills the slot
    // (`read::every`): an empty `frames` must not make `properties-from-table`
    // unfit while it is being chosen. At plan time, empty means the package holds
    // none, and that is said.
    let registry = Registry::new();
    let operation = registry.get("properties-from-table").expect("registered");
    let frames = operation
        .takes()
        .into_iter()
        .find(|one| one.name == "frames")
        .expect("a frames slot");
    assert_eq!(frames.otherwise, energyml_build::operation::Otherwise::Every);

    let only_table = bag(ONE_WELL, &roles());
    assert!(
        matches!(
            operation.fitness(&only_table),
            energyml_build::operation::Fitness::Fit
        ),
        "unfit on an empty `every` slot"
    );
    let refused = registry
        .plan(
            "properties-from-table",
            &only_table,
            &Choices::new(),
            &Identity::derived(),
            &when(),
        )
        .expect_err("no frame, neither given nor in the package");
    assert!(
        refused.to_string().contains("the package gives none"),
        "{refused}"
    );
}

#[test]
fn a_frame_that_cannot_be_read_fails_the_default_rather_than_hide() {
    // **An absence and a stupidity are not the same news.** A package
    // whose MDs cannot be read — here, the arrays were not written —
    // must not pass for a package without boreholes: each hole would then be written
    // a second time.
    let (store, _) = written(ONE_WELL);
    let takes = Registry::new()
        .get("wells-from-table")
        .expect("registered")
        .takes();
    let refused = energyml_build::read::every(
        &store,
        &takes,
        bag(ONE_WELL, &roles()),
        energyml_geometry::Placement::Local,
    )
    .expect_err("the frame cannot be read");
    assert_eq!(
        refused.slot.as_deref(),
        Some("wells"),
        "filed under its slot: {refused}"
    );
}

#[cfg(feature = "hdf5")]
#[test]
fn every_frame_of_the_package_is_taken_when_none_is_given() {
    // A decision: all the boreholes of the current stage should be used by default
    // to enrich them.
    // Replaying the same file without designating anything must therefore recognise the
    // two boreholes — not write two others.
    let text = "SONDAGE,X,Y,Z_MNT,PROF
F1,650000,6800000,120,2.25
F1,650000,6800000,120,26.16
F2,650100,6800100,131,1.5
F2,650100,6800100,131,9.0
";
    let made = plan(&bag(text, &roles()), json!({ "sourceEpsg": 2154 })).expect("a plan");
    let dir = std::env::temp_dir().join(format!("energyml-build-wells-every-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("wells.epc");
    let schemas = std::sync::Arc::new(energyml_schemas::all());
    let mut store = energyml_build::epc::open(None, "wells.epc", schemas.clone()).expect("a new package");
    store.external_files_mut().set_root(&dir);
    let done = energyml_build::apply::apply(&made, &mut store, &when());
    assert!(done.refused.is_empty(), "{:?}", done.refused);
    store
        .save_as(&out, &energyml_storage::SaveOptions::default())
        .unwrap();
    let reread = energyml_storage::EpcStore::open(&out, schemas).unwrap();

    let takes = Registry::new()
        .get("wells-from-table")
        .expect("registered")
        .takes();
    let filled = energyml_build::read::every(
        &reread,
        &takes,
        bag(text, &roles()),
        energyml_geometry::Placement::Local,
    )
    .expect("the frames read");
    // **Numbered**: two pieces named alike both took the
    // first companion, and the CLI saw the same borehole 120 times.
    let names: Vec<&str> = filled
        .iter()
        .map(|(name, _)| name)
        .filter(|name| name.starts_with("wells"))
        .collect();
    assert_eq!(names, ["wells", "wells (object)", "wells 2", "wells 2 (object)"]);

    let again = plan(&filled, json!({ "sourceEpsg": 2154 })).expect("a plan");
    let unchanged = again
        .says
        .iter()
        .find(|n| n.key == "unchanged")
        .expect("the two wells are recognised");
    assert!(
        unchanged.detail.contains("F1") && unchanged.detail.contains("F2"),
        "{}",
        unchanged.detail
    );

    // And a given piece wins: the default is not added to a choice.
    let given = bag(text, &roles()).with(
        "wells",
        known_as(
            "resqml22.WellboreFrameRepresentation",
            "F9 — frame",
            Some([0.0, 0.0, 0.0]),
            Some(&[1.0]),
        ),
    );
    let kept =
        energyml_build::read::every(&reread, &takes, given, energyml_geometry::Placement::Local).unwrap();
    assert_eq!(
        kept.iter().filter(|(name, _)| name.starts_with("wells")).count(),
        1
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// -- passes: `[top, MD]` ------------------------------------------------------

/// The roles, and the top of each pass.
fn passes() -> Value {
    let mut all = roles();
    all.as_array_mut()
        .expect("a list")
        .push(column("DEB", "cb1166d9-3874-4267-abea-1cd97e4bf448"));
    all
}

/// The depths of the nodes of the written frame.
fn node_depths(made: &energyml_build::operation::Plan) -> Vec<f64> {
    let nodes = made
        .datasets
        .iter()
        .find(|set| set.path.ends_with("nodeMd"))
        .expect("the frame's node depths");
    let energyml_build::operation::Numbers::F64(values) = &nodes.values else {
        panic!("the depths are doubles")
    };
    values.clone()
}

#[test]
fn passes_give_the_frame_one_node_per_limit() {
    // **The top of a pass is a fact of the file**, and it is what makes
    // the limit between two units exact: the `PROF` of the last pass of the
    // one above is the `DEB` of the first of the one below. Keeping only `PROF`
    // placed the contact at the end of the following pass — 1.75 m too low on
    // the first borehole of that file, with nothing to say so.
    //
    // The zero comes here **from the file** (`DEB` = 0), not from an added node: the
    // rule of 22/09 holds — nothing is made up at the ground.
    let text = "SONDAGE,X,Y,Z_MNT,DEB,PROF
F1,0,0,100,0,2.25
F1,0,0,100,2.25,3
F1,0,0,100,3,5.25
";
    let made = plan(&bag(text, &passes()), json!({ "sourceEpsg": 2154 })).expect("a plan");
    assert_eq!(
        node_depths(&made),
        vec![0.0, 2.25, 3.0, 5.25],
        "one node per limit, shared between two contiguous passes"
    );
    assert!(
        !made.says.iter().any(|n| n.key == "unmeasured-top"),
        "the file says where the first pass begins: {:?}",
        made.says
    );

    // **The negative check**: the same file without the declared top keeps the
    // earlier frame — otherwise this test would pass on a template that is ignored.
    let without = plan(&bag(text, &roles()), json!({ "sourceEpsg": 2154 })).expect("a plan");
    assert_eq!(node_depths(&without), vec![2.25, 3.0, 5.25]);
}

#[test]
fn a_gap_between_passes_keeps_both_limits() {
    // A gap is not a pass: both limits are nodes, and
    // the interval between them will carry nothing.
    let text = "SONDAGE,X,Y,Z_MNT,DEB,PROF
F1,0,0,100,0,1
F1,0,0,100,2,3
";
    let made = plan(&bag(text, &passes()), json!({ "sourceEpsg": 2154 })).expect("a plan");
    assert_eq!(node_depths(&made), vec![0.0, 1.0, 2.0, 3.0]);
}

#[test]
fn overlapping_passes_are_refused_by_their_lines() {
    // On the common interval, the file says two things; keeping one
    // would be deciding which.
    let text = "SONDAGE,X,Y,Z_MNT,DEB,PROF
F1,0,0,100,0,2
F1,0,0,100,1.5,3
";
    let why = plan(&bag(text, &passes()), json!({ "sourceEpsg": 2154 })).expect_err("a refusal");
    assert!(why.contains("overlap") && why.contains("lines 1 and 2"), "{why}");
}

#[test]
fn a_pass_whose_top_is_not_above_its_base_is_refused() {
    let text = "SONDAGE,X,Y,Z_MNT,DEB,PROF
F1,0,0,100,3,2
";
    let why = plan(&bag(text, &passes()), json!({ "sourceEpsg": 2154 })).expect_err("a refusal");
    assert!(
        why.contains("line 1 ") && why.contains("top lies above its base"),
        "{why}"
    );
}
