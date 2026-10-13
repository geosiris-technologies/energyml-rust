//! `properties-from-table` — the columns of a CSV laid on frames.
//!
//! The shape of the assertions comes from the specification of positional
//! arrays; the values are written here.
//!
//! The check that carries the module is
//! `a_value_lands_on_the_node_that_states_its_depth`: a positional array
//! that is too short **is not noticed**, it reads fine, and each value is assigned to the
//! wrong node.

use serde_json::{Value, json};

use energyml_build::operation::{Choices, Identity, Numbers, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_build::tabular::{self, Dialect};
use energyml_build::{osdu, tabular::Table};
use energyml_geometry::geometry::{Geometry, Patch, Topology};

fn when() -> When {
    When {
        creation: "2026-09-22T00:00:00Z".into(),
        originator: "test".into(),
    }
}

/// A template entry.
fn column(name: &str, uuid: &str, value_type: &str) -> Value {
    json!({
        "ColumnName": name,
        "ValueType": value_type,
        "PropertyType": { "PropertyTypeID": format!("namespace:reference-data--PropertyType:{uuid}:") },
    })
}

/// Two roles — the name and the MD — and two measurement columns.
fn template() -> Value {
    json!([
        column("SONDAGE", "348c9a4d-e3e4-4beb-8456-f07a46660de1", "string"),
        column("PROF", "c48c65d4-9680-4d10-903c-a1b2e30f66b5", "number"),
        column("PHI", "11111111-1111-4111-8111-111111111111", "number"),
        column("STRA", "22222222-2222-4222-8222-222222222222", "string"),
    ])
}

fn read(text: &str) -> Table {
    tabular::read("essai.csv", text, Dialect::sniff(text)).expect("a table")
}

/// A frame of the package: its shape, and **the MDs of its nodes**.
fn frame(bag: Pieces, at: usize, title: &str, mds: &[f64]) -> Pieces {
    let name = if at == 0 {
        "frames".to_string()
    } else {
        format!("frames {}", at + 1)
    };
    let geometry = Geometry {
        kind: "resqml22.WellboreFrameRepresentation",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points: mds.iter().map(|md| [0.0, 0.0, -md]).collect(),
            topology: Topology::Points,
            closed: Vec::new(),
        }],
    };
    bag.with(
        name.clone(),
        Source::Shape {
            geometry,
            closed: None,
        },
    )
    .with(
        format!("{name} (object)"),
        Source::Object(Known {
            reference: Some(Ref::new(
                format!("uuid-{title}"),
                "resqml22.WellboreFrameRepresentation",
                title,
            )),
            node_mds: Some(mds.to_vec()),
            ..Known::default()
        }),
    )
}

/// A bag: a table and its frames.
fn bag(text: &str, frames: &[(&str, &[f64])]) -> Pieces {
    let table = read(text);
    let template = osdu::from_json(&template()).expect("a template");
    let mut pieces = Pieces::empty().with(
        "table",
        Source::Table {
            name: "essai.csv".into(),
            table,
            template,
        },
    );
    for (at, (title, mds)) in frames.iter().enumerate() {
        pieces = frame(pieces, at, title, mds);
    }
    pieces
}

fn plan(pieces: &Pieces, answers: Value) -> Result<energyml_build::operation::Plan, String> {
    let mut choices = Choices::new();
    for (name, value) in answers.as_object().expect("an object") {
        choices = choices.set(name, value.clone());
    }
    Registry::new()
        .plan(
            "properties-from-table",
            pieces,
            &choices,
            &Identity::derived(),
            &when(),
        )
        .map_err(|why| why.to_string())
}

/// The array of a property, by the title of the object that carries it.
fn values_of(made: &energyml_build::operation::Plan, title: &str) -> Numbers {
    let object = made.objects.iter().find(|o| o.title == title).unwrap_or_else(|| {
        panic!(
            "no property `{title}`: {:?}",
            made.objects.iter().map(|o| &o.title).collect::<Vec<_>>()
        )
    });
    let path = format!("/resqml22/{}/values", object.uuid);
    made.datasets
        .iter()
        .find(|set| set.path == path)
        .unwrap_or_else(|| panic!("no array for `{title}`"))
        .values
        .clone()
}

#[test]
fn a_value_lands_on_the_node_that_states_its_depth() {
    // **The check that carries the module.** The frame has five nodes, the file
    // three rows, and the three are not the first three: 10, 30, 50
    // against nodes at 10, 20, 30, 40, 50.
    //
    // An implementation that simply wrote the values of the file would return
    // `[1, 3, 5]` — three cells for five nodes. RESQML does not complain: the
    // reader takes the value *i* for the node *i*, so 3 lands at 20 m and 5
    // at 30 m, and nothing on screen shows it. This is the silent shift that
    // the positional-array rule describes, and it is exactly what this assertion forbids.
    let text = "SONDAGE,PROF,PHI,STRA\nF1,10,1,a\nF1,30,3,b\nF1,50,5,c\n";
    let made = plan(
        &bag(text, &[("F1 — frame", &[10.0, 20.0, 30.0, 40.0, 50.0])]),
        json!({}),
    )
    .expect("a plan");

    let Numbers::F64(phi) = values_of(&made, "F1 — PHI") else {
        panic!("doubles")
    };
    assert_eq!(phi.len(), 5, "as many values as the frame has nodes");
    assert_eq!(phi[0], 1.0);
    assert!(phi[1].is_nan(), "the node at 20 m has no value in the file");
    assert_eq!(phi[2], 3.0);
    assert!(phi[3].is_nan());
    assert_eq!(phi[4], 5.0);

    // And the filling is **said**: an array of nulls and an array of
    // measurements have the same shape, only this count tells them apart.
    let note = made
        .says
        .iter()
        .find(|n| n.key == "filled")
        .expect("the count of nodes filled");
    assert!(
        note.detail.starts_with("4 "),
        "two nodes × two columns: {}",
        note.detail
    );
}

#[test]
fn text_becomes_a_discrete_property_with_a_stated_null() {
    // A text column has no natural null: RESQML makes it declared, and
    // `0` would be a value that nobody measured.
    let text = "SONDAGE,PROF,PHI,STRA\nF1,10,1,argile\nF1,30,3,sable\n";
    let made = plan(&bag(text, &[("F1 — frame", &[10.0, 20.0, 30.0])]), json!({})).expect("a plan");

    let object = made
        .objects
        .iter()
        .find(|o| o.title == "F1 — STRA")
        .expect("the text column");
    assert_eq!(object.qualified_type, "resqml22.DiscreteProperty");
    // **The null is on the array, not on the object**: measured in the generated
    // type, `resqml22.DiscreteProperty` has no `NullValue` field — it is
    // `eml23.IntegerExternalArray` that carries it, as for triangles.
    assert_eq!(
        object.json["ValuesForPatch"][0]["NullValue"],
        json!(-9999),
        "{}",
        object.json
    );

    let Numbers::I64(stra) = values_of(&made, "F1 — STRA") else {
        panic!("integers")
    };
    // The codes follow the order of the file, never the alphabet: an alphabetical
    // order would make the codes depend on the dataset, so two
    // packages of the same producer would not have the same ones.
    assert_eq!(stra, [0, -9999, 1]);
}

#[test]
fn one_code_means_one_thing_across_the_whole_run() {
    // **Found while answering a question, on 22/09**, and it is a
    // defect of correctness and not a gap: the first attempt assigned the
    // codes *per borehole*. Measured on the BRGM corpus, code 0 of `STRA`
    // designated five things — `e7` for 75 boreholes, `e6` for 35, then `n.d.`,
    // `Q`, `e5`. Two boreholes of the same package were not comparable, and the
    // day a `StringTableLookup` gives their meaning, it would say only one.
    //
    // Here F1 starts with `argile` and F2 with `sable`: a counter per borehole
    // gives `0` to both.
    let text = "SONDAGE,PROF,PHI,STRA
F1,10,1,argile
F1,20,2,sable
F2,10,3,sable
F2,20,4,argile
";
    let made = plan(
        &bag(
            text,
            &[("F1 — frame", &[10.0, 20.0]), ("F2 — frame", &[10.0, 20.0])],
        ),
        json!({}),
    )
    .expect("a plan");

    let (Numbers::I64(one), Numbers::I64(two)) =
        (values_of(&made, "F1 — STRA"), values_of(&made, "F2 — STRA"))
    else {
        panic!("integers")
    };
    assert_eq!(one, [0, 1], "argile then sable, in the order of encounter");
    assert_eq!(two, [1, 0], "and F2 designates them by **the same** codes");
}

#[test]
fn a_code_names_the_table_that_gives_it_meaning() {
    // **A code only has meaning against its table.** RESQML 2.2 says so through
    // `CategoryLookup` — a `DataObjectReference` that the XSD does not type. What
    // it designates is read from the only producer one can consult:
    // `csv-to-energyml` puts an `eml23.ColumnBasedTable` there for its 480
    // properties, and its table carries the distinct values as rows.
    //
    // Without this link, an integer in a file means nothing — and it is
    // **worse** than an absence, because an integer looks like a measurement.
    let text = "SONDAGE,PROF,PHI,STRA
F1,10,1,argile
F1,20,2,sable
";
    let made = plan(&bag(text, &[("F1 — frame", &[10.0, 20.0])]), json!({})).expect("a plan");

    let property = made
        .objects
        .iter()
        .find(|o| o.title == "F1 — STRA")
        .expect("the discrete one");
    let lookup = property.json["CategoryLookup"]["Uuid"]
        .as_str()
        .expect("elle nomme sa table");
    assert_eq!(
        property.json["CategoryLookup"]["QualifiedType"],
        json!("eml23.ColumnBasedTable")
    );

    let table = made
        .objects
        .iter()
        .find(|o| o.uuid == lookup)
        .unwrap_or_else(|| panic!("the table `{lookup}` is not in the plan"));
    assert_eq!(table.qualified_type, "eml23.ColumnBasedTable");
    // **The code is the rank of the row.** The values are in the XML: a text
    // column has a few dozen distinct values, and an
    // external array for two strings would cost a file to open.
    assert_eq!(
        table.json["Column"][0]["Values"]["Values"],
        json!(["argile", "sable"])
    );
    assert_eq!(table.json["Column"][0]["Title"], json!("STRA"));

    let Numbers::I64(stra) = values_of(&made, "F1 — STRA") else {
        panic!("integers")
    };
    assert_eq!(stra, [0, 1], "and the codes are indeed those ranks");
}

#[test]
fn a_table_given_as_an_input_is_continued_and_keeps_its_uuid() {
    // **The first use of the widened slot**: an entry that has no
    // geometry. The observation: operations taking geometric objects
    // are a special case of the operations we want to
    // define — and `declare.rs` only knew how to bind those.
    //
    // Measured on 22/09 on two batches of the BRGM corpus: without that, the second pass
    // writes its own table and code 0 designates `e7` for some, `e6` for the
    // others. Here the given table already carries `argile` at rank 0, and the
    // file starts with `sable` — a new counter would give it 0.
    let text = "SONDAGE,PROF,PHI,STRA
F1,10,1,sable
F1,20,2,craie
";
    let pieces = bag(text, &[("F1 — frame", &[10.0, 20.0])]).with(
        "lookups",
        Source::Object(Known {
            reference: Some(Ref::new("table-existante", "eml23.ColumnBasedTable", "STRA")),
            categories: Some(vec!["argile".into(), "sable".into()]),
            ..Known::default()
        }),
    );
    let made = plan(&pieces, json!({})).expect("a plan");

    let Numbers::I64(stra) = values_of(&made, "F1 — STRA") else {
        panic!("integers")
    };
    // `sable` keeps the rank 1 it had, and `craie` is added at 2.
    assert_eq!(stra, [1, 2], "the ranks of the given table are kept");

    // **And the uuid does not change**: the properties already written against it
    // name it, so making a second one would leave them pointing at an
    // incomplete table.
    let property = made
        .objects
        .iter()
        .find(|o| o.title == "F1 — STRA")
        .expect("the discrete one");
    assert_eq!(property.json["CategoryLookup"]["Uuid"], json!("table-existante"));
    let table = made
        .objects
        .iter()
        .find(|o| o.uuid == "table-existante")
        .expect("the table continued");
    assert_eq!(
        table.json["Column"][0]["Values"]["Values"],
        json!(["argile", "sable", "craie"]),
        "the earlier rows, then the new ones"
    );
    // One table, not two.
    assert_eq!(
        made.objects
            .iter()
            .filter(|o| o.qualified_type == "eml23.ColumnBasedTable")
            .count(),
        1
    );
}

#[test]
fn the_same_values_are_the_same_object_and_different_values_are_a_second_one() {
    // **The identity rule**: overwriting properties of an earlier run loses
    // information. A second property should be created if the values differ […] and if
    // the values are identical there is nothing to do.
    //
    // It falls out of a single decision: identity comes from the **content**, not from the
    // file. The measurement of 23/09 that made it necessary — the same CSV under another
    // name took three boreholes to six.
    let nodes: &[f64] = &[10.0, 20.0];
    let one = "SONDAGE,PROF,PHI,STRA
F1,10,1,argile
F1,20,2,sable
";
    let copy = "SONDAGE,PROF,PHI,STRA
F1,10,1,argile
F1,20,2,sable
";
    let other = "SONDAGE,PROF,PHI,STRA
F1,10,9,argile
F1,20,2,sable
";

    let uuid_of = |text: &str, file: &str| -> String {
        let table = read(text);
        let template = osdu::from_json(&template()).expect("a template");
        let pieces = frame(
            Pieces::empty().with(
                "table",
                Source::Table {
                    name: file.into(),
                    table,
                    template,
                },
            ),
            0,
            "F1 — frame",
            nodes,
        );
        let made = plan(&pieces, json!({})).expect("a plan");
        made.objects
            .iter()
            .find(|o| o.title == "F1 — PHI")
            .expect("the continued one")
            .uuid
            .clone()
    };

    // **The name of the file does not count**: it is what made two sets
    // of objects of two exports of the same producer.
    assert_eq!(
        uuid_of(one, "lot.csv"),
        uuid_of(copy, "lot-copie.csv"),
        "meme contenu, meme objet"
    );
    // A value that changes gives a **second** object, never the first overwritten.
    assert_ne!(
        uuid_of(one, "lot.csv"),
        uuid_of(other, "lot.csv"),
        "valeurs differentes, second objet"
    );
}

#[test]
fn two_versions_carry_no_object_version_at_all() {
    // **Measured on 23/09, and it is what made the label be removed**:
    // writing `objectVersion` put 15 warnings on a package of three
    // boreholes that had none — `energyml check` flags every reference
    // to a versioned object that does not say *which* version, and nothing here
    // writes them. And on the wire side: ETP servers only take the uuid as key.
    //
    // What tells two versions apart is therefore **the uuid**, which comes from the
    // content; the title does not move, and no version field is set.
    let text = "SONDAGE,PROF,PHI,STRA
F1,10,1,argile
";
    let made = plan(&bag(text, &[("F1 — frame", &[10.0])]), json!({})).expect("a plan");
    let property = made
        .objects
        .iter()
        .find(|o| o.title == "F1 — PHI")
        .expect("the continued one");
    assert!(property.json["objectVersion"].is_null(), "{}", property.json);
    assert!(
        property.json["ObjectVersionReason"].is_null(),
        "{}",
        property.json
    );
    assert_eq!(property.title, "F1 — PHI", "the title carries no version");
}

#[test]
fn a_row_the_frame_has_no_node_for_is_counted_and_not_moved() {
    // Moving a value to the nearest node would be inventing. It is
    // set aside **and counted**, and the remedy is named.
    let text = "SONDAGE,PROF,PHI,STRA\nF1,10,1,a\nF1,15,9,z\n";
    let made = plan(&bag(text, &[("F1 — frame", &[10.0, 20.0])]), json!({})).expect("a plan");
    let note = made
        .says
        .iter()
        .find(|n| n.key == "depths-dropped")
        .expect("the count of rows set aside");
    assert!(
        note.detail.contains("subdivide"),
        "the remedy is named: {}",
        note.detail
    );

    let Numbers::F64(phi) = values_of(&made, "F1 — PHI") else {
        panic!("doubles")
    };
    assert_eq!(phi.len(), 2, "the frame did not move");
    assert_eq!(phi[0], 1.0);
    assert!(phi[1].is_nan(), "15 m was not moved to 20 m");
}

#[test]
fn subdividing_adds_the_files_depths_and_says_how_many() {
    let text = "SONDAGE,PROF,PHI,STRA\nF1,10,1,a\nF1,15,9,z\n";
    let made = plan(
        &bag(text, &[("F1 — frame", &[10.0, 20.0])]),
        json!({ "subdivide": true }),
    )
    .expect("a plan");
    let note = made
        .says
        .iter()
        .find(|n| n.key == "depths-added")
        .expect("the count of MDs added");
    assert!(note.detail.starts_with("1 "), "{}", note.detail);

    let Numbers::F64(phi) = values_of(&made, "F1 — PHI") else {
        panic!("doubles")
    };
    // Three nodes: 10, 15 (added), 20 — and the value at 15 is **in its place**,
    // not at the end.
    assert_eq!(phi.len(), 3);
    assert_eq!(phi[0], 1.0);
    assert_eq!(phi[1], 9.0);
    assert!(phi[2].is_nan());
}

#[test]
fn two_rows_of_one_depth_are_refused_by_their_lines() {
    // Three possible readings and the file names none: we
    // refuse, naming the borehole, the depth and the two rows.
    let text = "SONDAGE,PROF,PHI,STRA\nF1,10,1,a\nF1,10,2,b\n";
    let why = plan(&bag(text, &[("F1 — frame", &[10.0])]), json!({})).expect_err("refused");
    assert!(why.contains("F1") && why.contains("10"), "{why}");
}

#[test]
fn a_well_the_package_does_not_have_is_named_rather_than_left_out() {
    let text = "SONDAGE,PROF,PHI,STRA\nF1,10,1,a\nAILLEURS,10,2,b\n";
    let made = plan(&bag(text, &[("F1 — frame", &[10.0])]), json!({})).expect("a plan");
    let note = made
        .says
        .iter()
        .find(|n| n.key == "wells-unmatched")
        .expect("the count of boreholes without a frame");
    assert!(note.detail.contains("AILLEURS"), "{}", note.detail);
}

#[test]
fn the_rows_are_sorted_by_depth_and_the_values_follow_them() {
    // **A column is never sorted, rows are.** A file in
    // disorder must return the same array as a sorted file; sorting the MDs
    // without applying the same permutation to the values changes the depth of
    // each measurement without anything saying so.
    let jumbled = "SONDAGE,PROF,PHI,STRA\nF1,30,3,c\nF1,10,1,a\nF1,20,2,b\n";
    let sorted = "SONDAGE,PROF,PHI,STRA\nF1,10,1,a\nF1,20,2,b\nF1,30,3,c\n";
    let nodes: &[f64] = &[10.0, 20.0, 30.0];
    let one = plan(&bag(jumbled, &[("F1 — frame", nodes)]), json!({})).expect("a plan");
    let two = plan(&bag(sorted, &[("F1 — frame", nodes)]), json!({})).expect("a plan");
    let (Numbers::F64(a), Numbers::F64(b)) = (values_of(&one, "F1 — PHI"), values_of(&two, "F1 — PHI"))
    else {
        panic!("doubles")
    };
    assert_eq!(a, [1.0, 2.0, 3.0]);
    assert_eq!(a, b, "the order of the file does not change the result");
}

#[test]
fn a_frame_that_states_no_depths_is_refused_by_name() {
    // Without the MDs of a frame there is nothing to align on. The refusal names the
    // frame and the reason, because a build without an array backend gives
    // exactly this answer and it is not the fault of the file.
    let table = read("SONDAGE,PROF,PHI,STRA\nF1,10,1,a\n");
    let template = osdu::from_json(&template()).expect("a template");
    let geometry = Geometry {
        kind: "resqml22.WellboreFrameRepresentation",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points: vec![[0.0, 0.0, 0.0]],
            topology: Topology::Points,
            closed: Vec::new(),
        }],
    };
    let pieces = Pieces::empty()
        .with(
            "table",
            Source::Table {
                name: "essai.csv".into(),
                table,
                template,
            },
        )
        .with(
            "frames",
            Source::Shape {
                geometry,
                closed: None,
            },
        )
        .with(
            "frames (object)",
            Source::Object(Known {
                reference: Some(Ref::new(
                    "u",
                    "resqml22.WellboreFrameRepresentation",
                    "F1 — frame",
                )),
                ..Known::default()
            }),
        );
    let why = plan(&pieces, json!({})).expect_err("refused");
    assert!(why.contains("NodeMd"), "the reason is named: {why}");
}

#[test]
fn a_table_of_heads_is_refused_towards_the_operation_that_takes_it() {
    // A table without an MD column carries nothing to lay along a frame,
    // and a refusal that does not say where to go reads as *this cannot be done*.
    let heads = json!([
        column("SONDAGE", "348c9a4d-e3e4-4beb-8456-f07a46660de1", "string"),
        column("PHI", "11111111-1111-4111-8111-111111111111", "number"),
    ]);
    let table = read("SONDAGE,PHI\nF1,1\n");
    let template = osdu::from_json(&heads).expect("a template");
    let pieces = frame(
        Pieces::empty().with(
            "table",
            Source::Table {
                name: "tetes.csv".into(),
                table,
                template,
            },
        ),
        0,
        "F1 — frame",
        &[10.0],
    );
    let why = plan(&pieces, json!({})).expect_err("refused");
    assert!(
        why.contains("wells-from-table"),
        "the refusal names the other operation: {why}"
    );
}

// -- passes: values on the intervals ------------------------------------------

/// The template, and the top of each pass.
fn passes_template() -> Value {
    let mut all = template();
    all.as_array_mut()
        .expect("a list")
        .push(column("DEB", "cb1166d9-3874-4267-abea-1cd97e4bf448", "number"));
    all
}

/// A bag whose template declares the passes.
fn passes_bag(text: &str, frames: &[(&str, &[f64])]) -> Pieces {
    let table = read(text);
    let template = osdu::from_json(&passes_template()).expect("a template");
    let mut pieces = Pieces::empty().with(
        "table",
        Source::Table {
            name: "essai.csv".into(),
            table,
            template,
        },
    );
    for (at, (title, mds)) in frames.iter().enumerate() {
        pieces = frame(pieces, at, title, mds);
    }
    pieces
}

/// The element that a property declares to index.
fn indexable_of(made: &energyml_build::operation::Plan, title: &str) -> String {
    let object = made
        .objects
        .iter()
        .find(|o| o.title == title)
        .expect("the property");
    object.json["IndexableElement"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

#[test]
fn a_pass_lands_on_the_interval_between_its_top_and_its_base() {
    // **The check that carries the passes.** The value of a pass holds *from here to
    // there*: three passes, four nodes, **three** values on the intervals.
    // Laid on the nodes, they would be four — the last invented — or
    // shifted by one rank, and nothing would show it.
    let text = "SONDAGE,DEB,PROF,PHI,STRA
F1,0,2.25,0.1,e7
F1,2.25,3,0.2,e7
F1,3,5.25,0.3,e6
";
    let made = plan(&passes_bag(text, &[("F1", &[0.0, 2.25, 3.0, 5.25])]), json!({})).expect("a plan");
    let Numbers::F64(values) = values_of(&made, "F1 — PHI") else {
        panic!("doubles")
    };
    assert_eq!(values, vec![0.1, 0.2, 0.3]);
    assert_eq!(indexable_of(&made, "F1 — PHI"), "intervals");
    let Numbers::I64(codes) = values_of(&made, "F1 — STRA") else {
        panic!("integers")
    };
    assert_eq!(codes, vec![0, 0, 1], "e7, e7, e6");
    assert_eq!(indexable_of(&made, "F1 — STRA"), "intervals");
}

#[test]
fn a_pass_over_a_finer_frame_holds_on_every_interval_it_spans() {
    // A frame cut more finely by another source: the pass holds
    // on each of its intervals, and a gap between two passes stays null.
    let text = "SONDAGE,DEB,PROF,PHI
F1,0,2,0.5
F1,3,4,0.7
";
    let made = plan(
        &passes_bag(text, &[("F1", &[0.0, 1.0, 2.0, 3.0, 4.0])]),
        json!({}),
    )
    .expect("a plan");
    let Numbers::F64(values) = values_of(&made, "F1 — PHI") else {
        panic!("doubles")
    };
    assert_eq!(values.len(), 4, "one per interval: {values:?}");
    assert_eq!(&values[..2], &[0.5, 0.5]);
    assert!(values[2].is_nan(), "the hole stays null: {values:?}");
    assert_eq!(values[3], 0.7);
}

#[test]
fn a_pass_whose_limit_is_no_node_is_counted_unless_subdivided() {
    let text = "SONDAGE,DEB,PROF,PHI
F1,0,1.5,0.5
F1,1.5,2,0.7
";
    let kept = plan(&passes_bag(text, &[("F1", &[0.0, 1.0, 2.0])]), json!({})).expect("a plan");
    assert!(
        kept.says
            .iter()
            .any(|n| n.key == "depths-dropped" && n.detail.starts_with("2 ")),
        "{:?}",
        kept.says
    );

    let cut = plan(
        &passes_bag(text, &[("F1", &[0.0, 1.0, 2.0])]),
        json!({ "subdivide": true }),
    )
    .expect("a plan");
    let Numbers::F64(values) = values_of(&cut, "F1 — PHI") else {
        panic!("doubles")
    };
    // Nodes 0, 1, 1.5, 2: the first pass covers [0, 1] and [1, 1.5].
    assert_eq!(values, vec![0.5, 0.5, 0.7]);
}
