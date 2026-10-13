//! `contacts-from-frames` — naming each point after the well it came from
//! (`wellOrigin`). The need, quoted in
//! `src/ops/contacts.rs`: it should be possible to assign a
//! discreteProperty that assigns, at each point, the name of the
//! borehole of origin.
//!
//! The property is confronted against the frames' own titles, read from the
//! points' coordinates by `frame_at` — an independent source, never anything
//! the operation kept — the same discipline `tests/contacts.rs`'s
//! `where_is`/`a_point_takes_what_its_wellbore_holds_at_that_node` already
//! use for a copied measurement.

use serde_json::json;

use energyml_build::operation::{Choices, Identity, Numbers, Plan, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_geometry::geometry::{Geometry, Patch, Topology};

const CRS: &str = "44444444-4444-4444-8444-444444444444";

fn when() -> When {
    When {
        creation: "2026-09-24T00:00:00Z".into(),
        originator: "test".into(),
    }
}

fn unit(name: &str) -> Option<Ref> {
    Some(Ref::new(
        format!("uuid-{name}"),
        "resqml22.StratigraphicUnitInterpretation",
        name,
    ))
}

/// A frame of vertical nodes at depths 0, 1, 2, … and its intervals' units:
/// frame `at` stands at x = 10·at, its node k at z = −k. Copied from
/// `tests/contacts.rs`, which this file does not have access to.
fn frame(bag: Pieces, at: usize, title: &str, units: Vec<Option<Ref>>) -> Pieces {
    let name = if at == 0 {
        "frames".to_string()
    } else {
        format!("frames {}", at + 1)
    };
    let points: Vec<[f64; 3]> = (0..=units.len())
        .map(|k| [at as f64 * 10.0, 0.0, -(k as f64)])
        .collect();
    let geometry = Geometry {
        kind: "resqml22.WellboreFrameRepresentation",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points,
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
            crs: Some(Ref::new(CRS, "eml23.ProjectedCrs", "EPSG:2154")),
            interval_units: Some(units),
            ..Known::default()
        }),
    )
}

/// Which frame a point came from, **from its coordinates alone**: frame `at`
/// stands at x = 10·at (see `frame`) — nothing the operation kept.
fn frame_at(point: &[f64]) -> usize {
    (point[0] / 10.0).round() as usize
}

fn plan(pieces: &Pieces) -> Result<Plan, String> {
    plan_with(pieces, Choices::new())
}

fn plan_with(pieces: &Pieces, choices: Choices) -> Result<Plan, String> {
    Registry::new()
        .plan(
            "contacts-from-frames",
            pieces,
            &choices,
            &Identity::derived(),
            &when(),
        )
        .map_err(|why| why.to_string())
}

/// The points of a point set, by its title.
fn points_of(made: &Plan, title: &str) -> Vec<Vec<f64>> {
    let set = made
        .objects
        .iter()
        .find(|one| one.qualified_type == "resqml22.PointSetRepresentation" && one.title == title)
        .unwrap_or_else(|| panic!("no point set titled `{title}`"));
    let path = format!("/resqml22/{}/points", set.uuid);
    let dataset = made
        .datasets
        .iter()
        .find(|one| one.path == path)
        .expect("its points");
    let Numbers::F64(values) = &dataset.values else {
        panic!("points are doubles")
    };
    values.chunks(3).map(<[f64]>::to_vec).collect()
}

/// The well each node of a point set's `wellOrigin` property names — read
/// exactly as a consumer would: the property's own codes, against the rows
/// of the table its `CategoryLookup` names, never an index this test already
/// knows.
fn well_names_of(made: &Plan, set_title: &str) -> Vec<String> {
    let property_title = format!("{set_title} — well");
    let object = made
        .objects
        .iter()
        .find(|one| one.title == property_title && one.qualified_type == "resqml22.DiscreteProperty")
        .unwrap_or_else(|| panic!("no well-origin property titled `{property_title}`"));
    assert_eq!(
        object.json["IndexableElement"],
        json!("nodes"),
        "one value per point, on the nodes"
    );

    let path = format!("/resqml22/{}/values", object.uuid);
    let Numbers::I64(codes) = &made
        .datasets
        .iter()
        .find(|one| one.path == path)
        .expect("its codes")
        .values
    else {
        panic!("codes are integers")
    };

    let table_uuid = object.json["CategoryLookup"]["Uuid"]
        .as_str()
        .expect("a lookup, by reference")
        .to_string();
    let table = made
        .objects
        .iter()
        .find(|one| one.uuid == table_uuid)
        .expect("the table it names is in the plan");
    assert_eq!(table.qualified_type, "eml23.ColumnBasedTable");
    let rows: Vec<String> = table.json["Column"][0]["Values"]["Values"]
        .as_array()
        .expect("the table's rows")
        .iter()
        .map(|one| one.as_str().expect("a well name").to_string())
        .collect();

    codes.iter().map(|code| rows[*code as usize].clone()).collect()
}

#[test]
fn a_point_names_the_well_it_came_from() {
    // F1: A, A, B — its contact is node 2. F2: A, B, B — node 1. Both give a
    // point to `A — base` and to `B — top`.
    let pieces = frame(Pieces::empty(), 0, "F1", vec![unit("A"), unit("A"), unit("B")]);
    let pieces = frame(pieces, 1, "F2", vec![unit("A"), unit("B"), unit("B")]);
    let made = plan(&pieces).expect("a plan");

    for title in ["A — base", "B — top"] {
        let points = points_of(&made, title);
        let names = well_names_of(&made, title);
        // As many values as the point set has points.
        assert_eq!(names.len(), points.len(), "{title}: one well name per point");
        // Value i names the well point i came from — read from the point's
        // own coordinates, an independent source.
        for (point, name) in points.iter().zip(&names) {
            let expected = match frame_at(point) {
                0 => "F1",
                1 => "F2",
                other => panic!("no frame at rank {other}"),
            };
            assert_eq!(name, expected, "{title}, the point at {point:?}");
        }
    }
}

#[test]
fn every_point_set_of_the_run_shares_one_table() {
    // A table per point set would let two of them disagree about what a
    // given code names.
    let pieces = frame(Pieces::empty(), 0, "F1", vec![unit("A"), unit("A"), unit("B")]);
    let pieces = frame(pieces, 1, "F2", vec![unit("A"), unit("B"), unit("B")]);
    let made = plan(&pieces).expect("a plan");

    let tables: Vec<&String> = made
        .objects
        .iter()
        .filter(|one| one.qualified_type == "eml23.ColumnBasedTable")
        .map(|one| &one.uuid)
        .collect();
    assert_eq!(
        tables.len(),
        1,
        "one table for the whole run, not one per point set: {tables:?}"
    );

    let properties: Vec<&serde_json::Value> = made
        .objects
        .iter()
        .filter(|one| one.qualified_type == "resqml22.DiscreteProperty" && one.title.ends_with(" — well"))
        .map(|one| &one.json)
        .collect();
    assert_eq!(
        properties.len(),
        2,
        "one property per point set — `A — base` and `B — top`"
    );
    for property in properties {
        assert_eq!(
            property["CategoryLookup"]["Uuid"].as_str(),
            Some(tables[0].as_str()),
            "the same table, by reference"
        );
    }
}

#[test]
fn well_origin_off_writes_no_property_and_no_table_but_keeps_the_point_sets() {
    let pieces = frame(Pieces::empty(), 0, "F1", vec![unit("A"), unit("A"), unit("B")]);
    let pieces = frame(pieces, 1, "F2", vec![unit("A"), unit("B"), unit("B")]);
    let off = plan_with(&pieces, Choices::new().set("wellOrigin", false)).expect("a plan");
    assert!(
        off.objects
            .iter()
            .all(|one| one.qualified_type != "eml23.ColumnBasedTable"),
        "no table"
    );
    assert!(
        off.objects.iter().all(|one| !one.title.ends_with(" — well")),
        "no well-origin property"
    );
    let note = off
        .says
        .iter()
        .find(|n| n.key == "well-origin-off")
        .expect("said, not silent");
    assert!(!note.detail.is_empty());

    // And the point sets are **the same objects** either way: the switch
    // writes objects beside them, never different ones.
    let on = plan(&pieces).expect("a plan");
    let sets = |made: &Plan| {
        made.objects
            .iter()
            .filter(|one| one.qualified_type == "resqml22.PointSetRepresentation")
            .map(|one| one.uuid.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(sets(&off), sets(&on));
}
