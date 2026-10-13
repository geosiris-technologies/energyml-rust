//! What an operation declares it writes (`Operation::gives`), held against
//! what its plans really carry — the six operations of the table-to-wells
//! chain.
//!
//! *A role a plan writes and the declaration does not name is a defect of the
//! operation.* The roles are the strings `NewObject::role` takes, many set by
//! shared writers (`well::write`, `crs::build`, `property::write`…), so a
//! declaration cannot be checked by reading one file: it is checked by running
//! plans — several variants per operation, each option that changes what is
//! written — and asking of **every object** of **every plan**:
//!
//! - is its role declared, and its qualified type among that role's types;
//! - does the count under its role sit inside `least..most`;
//!
//! and of the declaration, the other way: **every role and every type it
//! lists was seen** in some variant. Without that half a declaration listing
//! everything would pass, and a Galaxy tool would offer outputs that no run
//! ever fills.
//!
//! The shape of what is written comes from RESQML TRG 2.2 and the operation
//! specifications; the files are written here. Nothing is read outside the
//! repository: the stratigraphy variants use the BRGM chart the build embeds,
//! and say so when it is empty.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use energyml_build::operation::{Choices, Identity, NewObject, Output, Plan, Registry, When};
use energyml_build::pieces::{Known, KnownProperty, Pieces, Ref, Source};
use energyml_build::tabular::{self, Dialect};
use energyml_build::osdu;
use energyml_domain::values::Values;
use energyml_geometry::geometry::{Geometry, Patch, Topology};

fn when() -> When {
    When { creation: "2026-10-07T00:00:00Z".into(), originator: "test".into() }
}

fn choices(answers: &Value) -> Choices {
    let mut choices = Choices::new();
    for (name, value) in answers.as_object().expect("an object") {
        choices = choices.set(name, value.clone());
    }
    choices
}

/// Plan, or fail naming the variant.
fn run(op: &str, label: &str, pieces: &Pieces, answers: Value) -> Plan {
    Registry::new()
        .plan(op, pieces, &choices(&answers), &Identity::derived(), &when())
        .unwrap_or_else(|why| panic!("{op} / {label}: {why}"))
}

fn declared(op: &str) -> Vec<Output> {
    Registry::new().get(op).unwrap_or_else(|| panic!("no operation `{op}`")).gives()
}

/// The declaration, against every plan of its variants.
///
/// `caller_chosen` names the roles whose types are the caller's choice and
/// whose declaration therefore lists none: their objects are accepted under any
/// type, and the caller of `hold` asserts the type itself.
fn hold(op: &str, plans: &[(&str, &Plan)], caller_chosen: &[&str]) {
    let gives = declared(op);
    assert!(!gives.is_empty(), "{op} declares nothing");
    let mut roles = BTreeSet::new();
    for one in &gives {
        assert!(roles.insert(one.role), "{op} declares the role `{}` twice", one.role);
        assert!(!one.describe.is_empty(), "{op}: `{}` has no description", one.role);
        if let Some(most) = one.most {
            assert!(one.least <= most, "{op}: `{}` is {}..{most}", one.role, one.least);
        }
    }

    let mut seen_roles: BTreeSet<String> = BTreeSet::new();
    let mut seen_types: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (label, plan) in plans {
        let mut count: BTreeMap<&str, usize> = BTreeMap::new();
        for object in &plan.objects {
            let Some(one) = gives.iter().find(|one| one.role == object.role) else {
                panic!(
                    "{op} / {label}: the plan writes `{}` ({}) under the role `{}`, which `gives()` does not declare (declared: {:?})",
                    object.title,
                    object.qualified_type,
                    object.role,
                    gives.iter().map(|one| one.role).collect::<Vec<_>>()
                );
            };
            if !caller_chosen.contains(&one.role) {
                assert!(
                    one.types.contains(&object.qualified_type.as_str()),
                    "{op} / {label}: `{}` is a {} under `{}`, whose declared types are {:?}",
                    object.title,
                    object.qualified_type,
                    object.role,
                    one.types
                );
            }
            *count.entry(one.role).or_default() += 1;
            seen_roles.insert(object.role.clone());
            seen_types.entry(object.role.clone()).or_default().insert(object.qualified_type.clone());
        }
        // Counts: a bound is held only where the plan is whole — a plan that
        // writes nothing of a role is checked against `least` too, since
        // `plans` holds runs that are not refused.
        for one in &gives {
            let n = count.get(one.role).copied().unwrap_or(0);
            assert!(n >= one.least, "{op} / {label}: `{}` holds {n}, declared at least {}", one.role, one.least);
            if let Some(most) = one.most {
                assert!(n <= most, "{op} / {label}: `{}` holds {n}, declared at most {most}", one.role);
            }
        }
    }

    // The other way: nothing declared that no variant ever wrote.
    for one in &gives {
        assert!(seen_roles.contains(one.role), "{op}: the role `{}` is declared and no variant wrote it", one.role);
        if caller_chosen.contains(&one.role) {
            continue;
        }
        for kind in one.types {
            assert!(
                seen_types.get(one.role).is_some_and(|set| set.contains(*kind)),
                "{op}: `{kind}` is declared under `{}` and no variant wrote it",
                one.role
            );
        }
    }
}

fn of_type<'a>(plan: &'a Plan, kind: &str) -> Vec<&'a NewObject> {
    plan.objects.iter().filter(|one| one.qualified_type == kind).collect()
}

// -- templates and tables ------------------------------------------------------

fn column(name: &str, uuid: &str, value_type: &str) -> Value {
    json!({
        "ColumnName": name,
        "ValueType": value_type,
        "PropertyType": { "PropertyTypeID": format!("namespace:reference-data--PropertyType:{uuid}:") },
    })
}

const NAME: &str = "348c9a4d-e3e4-4beb-8456-f07a46660de1";
const X: &str = "620cec71-5ce3-4ea5-9120-833756b427d7";
const Y: &str = "cfcfdb04-02c1-4262-b66b-459b0aab3e8e";
const GROUND: &str = "8508cef4-0ade-454a-9ea4-e00d5905e677";
const MD: &str = "c48c65d4-9680-4d10-903c-a1b2e30f66b5";
const TOP: &str = "cb1166d9-3874-4267-abea-1cd97e4bf448";
const PHI: &str = "11111111-1111-4111-8111-111111111111";
const STRA: &str = "22222222-2222-4222-8222-222222222222";
const STRA_NAME: &str = "33333333-3333-4333-8333-333333333333";

/// The five roles, the top of each pass, a porosity, and the stratigraphy with
/// its label. `NOTE`, in the file, is in no template.
fn full_template() -> Value {
    json!([
        column("SONDAGE", NAME, "string"),
        column("X", X, "number"),
        column("Y", Y, "number"),
        column("Z_MNT", GROUND, "number"),
        column("DEB", TOP, "number"),
        column("PROF", MD, "number"),
        column("PHI", PHI, "number"),
        column("STRA", STRA, "string"),
        column("Nom_STRA", STRA_NAME, "string"),
    ])
}

/// Two holes; the passes name the chart's stages.
const FILE: &str = "SONDAGE,X,Y,Z_MNT,DEB,PROF,PHI,STRA,Nom_STRA,NOTE\n\
F1,650000,6800000,100,0,2,0.1,e7,Priabonien,a\n\
F1,650000,6800000,100,2,5,0.2,e6,Bartonien,b\n\
F2,650100,6800100,110,0,3,0.3,e7,Priabonien,c\n";

fn table_bag(text: &str, template: &Value) -> Pieces {
    let table = tabular::read("essai.csv", text, Dialect::sniff(text)).expect("a table");
    let template = osdu::from_json(template).expect("a template");
    Pieces::empty().with("table", Source::Table { name: "essai.csv".into(), table, template })
}

fn without(template: &Value, dropped: &[&str]) -> Value {
    let mut all = template.clone();
    all.as_array_mut().unwrap().retain(|one| !dropped.contains(&one["ColumnName"].as_str().unwrap()));
    all
}

// -- wells-from-table ----------------------------------------------------------

const DATUM: &str = "eml23.ReferencePointInALocalEngineeringCompoundCrs";

/// What a held well gives the bag — copied from `tests/wells_from_table.rs`,
/// where the depths are *stated*, since reading them needs the HDF5 backend.
fn held(text: &str, before: &Plan, head: [f64; 3], mds: &[f64], template: &Value) -> Pieces {
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
        crs: of("eml23.LocalEngineeringCompoundCrs"),
        vertical_up: Some(true),
        wellhead: Some(head),
        node_mds: Some(mds.to_vec()),
        ..Known::default()
    };
    table_bag(text, template).with("wells", Source::Object(known))
}

#[test]
fn wells_from_table_declares_what_its_plans_write() {
    let template = full_template();
    let slim = without(&template, &["DEB", "PHI", "STRA", "Nom_STRA"]);
    let basic = run("wells-from-table", "basic", &table_bag(FILE, &template), json!({ "sourceEpsg": 2154 }));
    // The passes' tops state the first depth: another frame, same roles.
    let passes = run("wells-from-table", "passes", &table_bag(FILE, &template), json!({ "sourceEpsg": 2154, "datumKind": "kelly bushing" }));
    // A table of heads writes the well and its datum, no trajectory, no frame.
    let heads_text = "SONDAGE,X,Y,Z_MNT\nF1,650000,6800000,120\nF2,650100,6800100,131\n";
    let heads_template = json!([
        column("SONDAGE", NAME, "string"),
        column("X", X, "number"),
        column("Y", Y, "number"),
        column("Z_MNT", GROUND, "number"),
    ]);
    let heads = run("wells-from-table", "heads", &table_bag(heads_text, &heads_template), json!({ "sourceEpsg": 2154 }));
    assert!(of_type(&heads, "resqml22.WellboreTrajectoryRepresentation").is_empty(), "the premise of the variant");
    // Degrees in, metres out, a vertical system named.
    let degrees = "SONDAGE,X,Y,Z_MNT,PROF\nF1,2.3,48.85,35,3\nF1,2.3,48.85,35,9\n";
    let reprojected = run(
        "wells-from-table",
        "reprojected",
        &table_bag(degrees, &without(&full_template(), &["DEB", "PHI", "STRA", "Nom_STRA"])),
        json!({ "sourceEpsg": 4326, "project": 2154, "vertical": 5720 }),
    );
    // A well the package holds: enriched (rewritten), then left alone.
    let first = "SONDAGE,X,Y,Z_MNT,PROF\nF1,650000,6800000,120,2.25\nF1,650000,6800000,120,26.16\n";
    let more = "SONDAGE,X,Y,Z_MNT,PROF\nF1,650000,6800000,120,12.0\n";
    let before = run("wells-from-table", "first batch", &table_bag(first, &slim), json!({ "sourceEpsg": 2154 }));
    let enriched = run(
        "wells-from-table",
        "enriched",
        &held(more, &before, [650000.0, 6800000.0, 120.0], &[2.25, 26.16], &slim),
        json!({ "sourceEpsg": 2154, "enrich": true }),
    );
    assert!(of_type(&enriched, "resqml22.WellboreFrameRepresentation").len() == 1, "the premise of the variant");

    hold(
        "wells-from-table",
        &[
            ("basic", &basic),
            ("passes", &passes),
            ("heads", &heads),
            ("reprojected", &reprojected),
            ("enriched", &enriched),
        ],
        &[],
    );

    // A run that changes nothing writes no well object — only the frame it
    // would have used — and still sits inside the declaration.
    let unchanged = run(
        "wells-from-table",
        "unchanged",
        &held(first, &before, [650000.0, 6800000.0, 120.0], &[2.25, 26.16], &slim),
        json!({ "sourceEpsg": 2154 }),
    );
    let gives = declared("wells-from-table");
    for object in &unchanged.objects {
        let one = gives.iter().find(|one| one.role == object.role).expect("a declared role");
        assert!(one.types.contains(&object.qualified_type.as_str()), "{object:?}");
    }
    for one in gives.iter().filter(|one| one.least > 0) {
        assert!(
            unchanged.objects.iter().any(|o| o.role == one.role),
            "`{}` is owed (least {}) and a run that changes nothing writes none",
            one.role,
            one.least
        );
    }
}

// -- properties-from-table -----------------------------------------------------

/// A frame of the package: its shape and its depths.
fn frame(bag: Pieces, at: usize, title: &str, mds: &[f64]) -> Pieces {
    let name = if at == 0 { "frames".to_string() } else { format!("frames {}", at + 1) };
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
    bag.with(name.clone(), Source::Shape { geometry, closed: None }).with(
        format!("{name} (object)"),
        Source::Object(Known {
            reference: Some(Ref::new(format!("uuid-{title}"), "resqml22.WellboreFrameRepresentation", title)),
            node_mds: Some(mds.to_vec()),
            ..Known::default()
        }),
    )
}

const PROPERTIES_FILE: &str = "SONDAGE,PROF,PHI,STRA,LITHO,NOTE\n\
F1,10,0.1,e6,marne,vu\n\
F1,30,0.2,e5,calcaire,\n\
F2,10,0.3,e5,calcaire,revu\n\
F2,30,0.4,,,\n";

fn properties_bag() -> Pieces {
    let template = json!([
        column("SONDAGE", NAME, "string"),
        column("PROF", MD, "number"),
        column("PHI", PHI, "number"),
        column("STRA", STRA, "string"),
        column("LITHO", "44444444-4444-4444-8444-444444444445", "string"),
    ]);
    let pieces = table_bag(PROPERTIES_FILE, &template);
    let pieces = frame(pieces, 0, "F1 — frame", &[10.0, 20.0, 30.0]);
    frame(pieces, 1, "F2 — frame", &[10.0, 30.0])
}

#[test]
fn properties_from_table_declares_what_its_plans_write() {
    let bag = properties_bag();
    let op = "properties-from-table";
    let separate = run(op, "separate", &bag, json!({}));
    let shared = run(op, "shared-table", &bag, json!({ "textColumns": "shared-table" }));
    let per_well = run(op, "table-per-well", &bag, json!({ "textColumns": "table-per-well" }));
    let everything = run(op, "everything", &bag, json!({ "everything": true }));
    let everything_shared = run(op, "everything + shared", &bag, json!({ "everything": true, "textColumns": "shared-table" }));
    let subdivided = run(op, "subdivide", &bag, json!({ "subdivide": true }));

    // The modes differ in the roles they write: which is the point of the declaration.
    let roles = |plan: &Plan| -> BTreeSet<String> { plan.objects.iter().map(|o| o.role.clone()).collect() };
    assert!(roles(&separate).contains("category-lookup") && !roles(&separate).contains("gathered-table"));
    assert!(roles(&shared).contains("gathered-table") && !roles(&shared).contains("category-lookup"));
    assert!(roles(&per_well).contains("gathered-table"));

    hold(
        op,
        &[
            ("separate", &separate),
            ("shared-table", &shared),
            ("table-per-well", &per_well),
            ("everything", &everything),
            ("everything + shared", &everything_shared),
            ("subdivide", &subdivided),
        ],
        &[],
    );
}

// -- wells-and-properties-from-table -------------------------------------------

#[test]
fn wells_and_properties_declares_the_union_of_its_halves() {
    let op = "wells-and-properties-from-table";
    let bag = table_bag(FILE, &full_template());
    let plain = run(op, "plain", &bag, json!({ "sourceEpsg": 2154 }));
    let shared = run(op, "shared-table", &bag, json!({ "sourceEpsg": 2154, "textColumns": "shared-table", "everything": true }));
    let per_well = run(op, "table-per-well", &bag, json!({ "sourceEpsg": 2154, "textColumns": "table-per-well" }));
    // Heads only: no frame, nothing to lay a property on.
    let heads_text = "SONDAGE,X,Y,Z_MNT\nF1,650000,6800000,120\n";
    let heads_template = json!([
        column("SONDAGE", NAME, "string"),
        column("X", X, "number"),
        column("Y", Y, "number"),
        column("Z_MNT", GROUND, "number"),
    ]);
    let heads = run(op, "heads", &table_bag(heads_text, &heads_template), json!({ "sourceEpsg": 2154 }));

    let mut plans = vec![("plain", &plain), ("shared-table", &shared), ("table-per-well", &per_well), ("heads", &heads)];

    // **`units`** adds the chart's column — only with the chart this build embeds.
    let with_units = if chart_is_there() {
        Some(run(op, "units", &bag, json!({ "sourceEpsg": 2154, "units": "STRA", "unitLabels": "Nom_STRA" })))
    } else {
        eprintln!("gives: this build has no BRGM chart, the `units` variant of {op} is skipped");
        None
    };
    if let Some(plan) = &with_units {
        plans.push(("units", plan));
        assert!(of_type(plan, "resqml22.StratigraphicColumn").len() == 1, "the premise of the variant");
        hold(op, &plans, &[]);
    } else {
        // Without the chart's half, only what the other variants write can be
        // held both ways; the one-way check (nothing undeclared) still is.
        for (label, plan) in &plans {
            let gives = declared(op);
            for object in &plan.objects {
                let one = gives.iter().find(|one| one.role == object.role).unwrap_or_else(|| panic!("{label}: `{}`", object.role));
                assert!(one.types.contains(&object.qualified_type.as_str()), "{label}: {object:?}");
            }
        }
    }

    // **The union is the halves' lists, not a copy**: every role either half
    // declares is here, with every type it lists.
    let union = declared(op);
    let halves = [declared("wells-from-table"), declared("properties-from-table")];
    for half in &halves {
        for one in half {
            let here = union.iter().find(|have| have.role == one.role).unwrap_or_else(|| panic!("`{}` is lost", one.role));
            for kind in one.types {
                assert!(here.types.contains(kind), "`{kind}` is lost from `{}`", one.role);
            }
        }
    }
    if chart_is_there() {
        for one in declared("stratigraphy-from-geolts") {
            let here = union.iter().find(|have| have.role == one.role).unwrap_or_else(|| panic!("`{}` is lost", one.role));
            for kind in one.types {
                assert!(here.types.contains(kind), "`{kind}` is lost from `{}`", one.role);
            }
        }
    }
}

/// Whether the build embeds a chart; the plan of a column refuses without one.
fn chart_is_there() -> bool {
    energyml_domain::geolts::with_default(|chart| !chart.is_empty())
}

// -- contacts-from-frames ------------------------------------------------------

const CRS: &str = "44444444-4444-4444-8444-444444444444";
const PHI_KIND: &str = "11111111-1111-4111-8111-111111111111";
const FACIES_KIND: &str = "22222222-2222-4222-8222-222222222222";
const FACIES_TABLE: &str = "33333333-3333-4333-8333-333333333333";

fn unit(name: &str) -> Option<Ref> {
    Some(Ref::new(format!("uuid-{name}"), "resqml22.StratigraphicUnitInterpretation", name))
}

/// A frame of vertical nodes whose intervals name their units.
fn unit_frame(bag: Pieces, at: usize, title: &str, units: Vec<Option<Ref>>) -> Pieces {
    let name = if at == 0 { "frames".to_string() } else { format!("frames {}", at + 1) };
    let points: Vec<[f64; 3]> = (0..=units.len()).map(|k| [at as f64 * 10.0, 0.0, -(k as f64)]).collect();
    let geometry = Geometry {
        kind: "resqml22.WellboreFrameRepresentation",
        patches: vec![Patch { index: 0, node_offset: 0, points, topology: Topology::Points, closed: Vec::new() }],
    };
    bag.with(name.clone(), Source::Shape { geometry, closed: None }).with(
        format!("{name} (object)"),
        Source::Object(Known {
            reference: Some(Ref::new(format!("uuid-{title}"), "resqml22.WellboreFrameRepresentation", title)),
            crs: Some(Ref::new(CRS, "eml23.ProjectedCrs", "EPSG:2154")),
            interval_units: Some(units),
            ..Known::default()
        }),
    )
}

/// A property of a frame, as the edge would resolve it.
fn property(frame: &str, measure: &str, discrete: bool, values: Values) -> Known {
    let class = if discrete { "DiscreteProperty" } else { "ContinuousProperty" };
    Known {
        reference: Some(Ref::new(format!("uuid-{frame}-{measure}"), format!("resqml22.{class}"), format!("{frame} — {measure}"))),
        property: Some(KnownProperty {
            supporting: Some(Ref::new(format!("uuid-{frame}"), "resqml22.WellboreFrameRepresentation", frame)),
            indexable: "nodes".to_string(),
            value_count: vec![1],
            timed: false,
            realizations: false,
            uom: (!discrete).then(|| "m3/m3".to_string()),
            kind: Some(if discrete {
                Ref::new(FACIES_KIND, "eml23.PropertyKind", "facies")
            } else {
                Ref::new(PHI_KIND, "eml23.PropertyKind", "porosity")
            }),
            lookup: discrete.then(|| Ref::new(FACIES_TABLE, "eml23.ColumnBasedTable", "facies")),
            null_value: discrete.then_some(-1),
            facets: Vec::new(),
            values: Ok(values),
        }),
        ..Known::default()
    }
}

fn contacts_bag(with_properties: bool) -> Pieces {
    let pieces = unit_frame(Pieces::empty(), 0, "F1", vec![unit("A"), unit("A"), unit("B")]);
    let mut pieces = unit_frame(pieces, 1, "F2", vec![unit("A"), unit("B"), unit("B")]);
    if with_properties {
        let all = vec![
            property("F1", "PHI", false, Values::Floating(vec![0.1, 0.2, 0.3, 0.4])),
            property("F2", "PHI", false, Values::Floating(vec![0.5, 0.6, 0.7, 0.8])),
            property("F1", "FACIES", true, Values::Integer(vec![0, 1, 1, 2])),
            property("F2", "FACIES", true, Values::Integer(vec![3, 4, 4, 4])),
        ];
        for (at, known) in all.into_iter().enumerate() {
            let name = if at == 0 { "properties".to_string() } else { format!("properties {}", at + 1) };
            pieces = pieces.with(name, Source::Object(known));
        }
    }
    pieces
}

#[test]
fn contacts_from_frames_declares_what_its_plans_write() {
    let op = "contacts-from-frames";
    let with = contacts_bag(true);
    // Both defaults on: the copies and the well-origin property.
    let all = run(op, "copy + wellOrigin", &with, json!({}));
    // Neither: the point sets and their feature and interpretation alone.
    let bare = run(op, "bare", &with, json!({ "copyProperties": false, "wellOrigin": false }));
    // The copies alone, and the well-origin alone.
    let copies = run(op, "copies", &with, json!({ "wellOrigin": false }));
    let origin = run(op, "origin", &with, json!({ "copyProperties": false }));
    // Frames that carry no property.
    let none = run(op, "no property", &contacts_bag(false), json!({}));
    // The frame created here, rather than the frames' own: projected, and in degrees.
    let created = run(op, "frame created", &with, json!({ "sourceEpsg": 2154, "project": 2154, "vertical": 5720 }));
    let degrees = run(op, "frame in degrees", &with, json!({ "sourceEpsg": 4326 }));

    assert!(origin.objects.iter().any(|o| o.role == "category-lookup"), "the premise: wellOrigin writes its table");
    assert!(of_type(&bare, "eml23.ColumnBasedTable").is_empty() && of_type(&bare, "resqml22.DiscreteProperty").is_empty());

    hold(
        op,
        &[
            ("copy + wellOrigin", &all),
            ("bare", &bare),
            ("copies", &copies),
            ("origin", &origin),
            ("no property", &none),
            ("frame created", &created),
            ("frame in degrees", &degrees),
        ],
        &[],
    );
}

// -- pointset-from-wellheads ---------------------------------------------------

fn well(bag: Pieces, name: &str, nodes: &[[f64; 3]], head: [f64; 3], frame: &Ref) -> Pieces {
    let geometry = Geometry {
        kind: "resqml22.WellboreTrajectoryRepresentation",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points: nodes.to_vec(),
            topology: Topology::Polylines(vec![nodes.len() as u32]),
            closed: Vec::new(),
        }],
    };
    bag.with(name, Source::Shape { geometry, closed: None }).with(
        format!("{name} (object)"),
        Source::Object(Known {
            reference: Some(Ref::new(format!("uuid-{name}"), "resqml22.WellboreTrajectoryRepresentation", name)),
            crs: Some(frame.clone()),
            crs_epsg: Some(2154),
            vertical_epsg: Some(5720),
            wellhead: Some(head),
            ..Known::default()
        }),
    )
}

#[test]
fn wellheads_declares_what_its_plans_write() {
    let op = "pointset-from-wellheads";
    let compound = Ref::new("c", "eml23.LocalEngineeringCompoundCrs", "the frame");
    let projected = Ref::new("p", "eml23.ProjectedCrs", "Lambert-93");
    let bag = |frame: &Ref| {
        let bag = well(Pieces::empty(), "F1", &[[0.0, 0.0, 88.5], [0.0, 0.0, 50.0]], [0.0, 0.0, 120.0], frame);
        well(bag, "F2", &[[10.0, 20.0, 119.0], [10.0, 20.0, 90.0]], [10.0, 20.0, 131.0], frame)
    };
    // The wells' own codes make the collars' frame (the compound is not kept).
    let default = run(op, "default", &bag(&compound), json!({ "crs": compound.dor(), "title": "MNT" }));
    // A projected frame named: nothing of the frame is written.
    let named = run(op, "projected named", &bag(&projected), json!({ "crs": projected.dor(), "title": "MNT" }));
    // A frame asked for. (Degrees are refused: an object of the package is in its own frame.)
    let created = run(op, "frame created", &bag(&compound), json!({ "sourceEpsg": 2154, "project": 2154, "vertical": 5720, "title": "MNT" }));
    // What the points represent, asked for.
    let represented = run(
        op,
        "feature asked for",
        &bag(&compound),
        json!({ "crs": compound.dor(), "title": "MNT", "featureType": "resqml22.BoundaryFeature", "interpretationType": "resqml22.HorizonInterpretation" }),
    );
    // Their types are the caller's choice, and are what was asked for.
    assert_eq!(of_type(&represented, "resqml22.BoundaryFeature").iter().filter(|o| o.role == "feature").count(), 1);
    assert_eq!(of_type(&represented, "resqml22.HorizonInterpretation").iter().filter(|o| o.role == "interpretation").count(), 1);
    assert!(of_type(&default, "resqml22.PointSetRepresentation").len() == 1);

    hold(
        op,
        &[
            ("default", &default),
            ("projected named", &named),
            ("frame created", &created),
            ("feature asked for", &represented),
        ],
        &["feature", "interpretation"],
    );
}

// -- stratigraphy-from-geolts --------------------------------------------------

#[test]
fn stratigraphy_declares_what_its_plans_write() {
    if !chart_is_there() {
        eprintln!("gives: this build has no BRGM chart, stratigraphy-from-geolts cannot be planned here");
        return;
    }
    let op = "stratigraphy-from-geolts";
    let empty = Pieces::empty();
    let branch = run(op, "a branch with its ancestors", &empty, json!({ "units": ["Eocène"] }));
    let alone = run(op, "a branch alone", &empty, json!({ "units": ["Eocène"], "ancestors": false }));
    let regional = run(op, "regional, finest", &empty, json!({ "units": ["Eocène"], "regional": true, "finest": 8 }));
    let english = run(op, "english, titled", &empty, json!({ "units": ["Eocène"], "language": "en", "title": "Eocene" }));
    let two = run(op, "two branches", &empty, json!({ "units": ["Eocène", "Crétacé"] }));
    hold(
        op,
        &[("branch", &branch), ("alone", &alone), ("regional", &regional), ("english", &english), ("two", &two)],
        &[],
    );
}

// -- the registry --------------------------------------------------------------

/// The six operations of the chain declare their outputs. The others are
/// listed, not refused: *empty means not declared yet*, and the rest of the
/// registry is somebody else's lot.
#[test]
fn the_chain_operations_are_declared_and_the_rest_are_listed() {
    const CHAIN: [&str; 6] = [
        "wells-and-properties-from-table",
        "wells-from-table",
        "properties-from-table",
        "contacts-from-frames",
        "pointset-from-wellheads",
        "stratigraphy-from-geolts",
    ];
    let registry = Registry::new();
    let mut undeclared: Vec<&'static str> = Vec::new();
    let mut declaring = 0usize;
    for operation in registry.iter() {
        if operation.gives().is_empty() {
            undeclared.push(operation.name());
        } else {
            declaring += 1;
        }
    }
    eprintln!("gives: {declaring} operation(s) declare their outputs; {} do not yet: {}", undeclared.len(), undeclared.join(", "));
    for name in CHAIN {
        assert!(registry.get(name).is_some(), "`{name}` is not in the registry");
        assert!(!undeclared.contains(&name), "`{name}` declares no output");
    }
    assert!(declaring >= CHAIN.len());
}
