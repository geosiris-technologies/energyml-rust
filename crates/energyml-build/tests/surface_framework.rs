//! `surface-framework` — a `NonSealedSurfaceFrameworkRepresentation`
//!.
//!
//! The form of the assertions comes from the schema and the guide: the
//! framework represents the structural organisation (usage guide 2.2, draft,
//! §7.3.1.2.2 p. 98); its `Representation` list is ordered and a contact patch
//! names a surface by its index there (Structural.xsd l. 182–186); a contact
//! patch is *“the ordered list of nodes (identified by their global index) in
//! the supporting representation”* (l. 187–191). The torn horizon and its lips
//! are made by `fault-throw` in the test itself.
//!
//! # What would have to break to make it turn red
//!
//! Reordering the representations; naming a node by its rank in the patch
//! instead of its global rank, or by a neighbouring node; accepting a line whose
//! nodes are those of no surface; writing an `Index` that is not
//! the one stated, or the same one twice; representing something other than
//! the organisation.

use serde_json::json;

use energyml_build::operation::{Choices, Identity, NewObject, Numbers, Plan, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_geometry::geometry::{Geometry, Patch, Topology};

const X0: f64 = 651_000.0;
const Y0: f64 = 6_861_000.0;

fn when() -> When {
    When { creation: "2026-09-25T00:00:00Z".into(), originator: "test".into() }
}

fn frame() -> Ref {
    Ref::new("cccccccc-0000-4000-8000-000000000003", "eml23.LocalEngineeringCompoundCrs", "Lutetia — boreholes (elevation)")
}

fn organisation() -> Ref {
    Ref::new("0a000000-0000-4000-8000-000000000210", "resqml22.StructuralOrganizationInterpretation", "Lutetia — scenario B, unordered")
}

fn object(reference: Ref) -> Source {
    Source::Object(Known { reference: Some(reference), crs: Some(frame()), vertical_uom: Some("m".into()), vertical_up: Some(true), ..Default::default() })
}

fn shape(points: Vec<[f64; 3]>, topology: Topology) -> Source {
    Source::Shape { geometry: Geometry { kind: "test", patches: vec![Patch { index: 0, node_offset: 0, points, topology, closed: Vec::new() }] }, closed: None }
}

/// The fault of `fault_throw.rs`: striking north through x = 105 at −50 m,
/// dipping 60° east.
fn fault_points() -> Vec<[f64; 3]> {
    let at = |z: f64| X0 + 105.0 + (-50.0 - z) / 60f64.to_radians().tan();
    vec![[at(0.0), Y0 + 45.0, 0.0], [at(0.0), Y0 + 155.0, 0.0], [at(-100.0), Y0 + 45.0, -100.0], [at(-100.0), Y0 + 155.0, -100.0]]
}

fn fault() -> Source {
    shape(fault_points(), Topology::Triangles(vec![[0, 1, 2], [1, 3, 2]]))
}

fn fault_ref(k: u32) -> Ref {
    Ref::new(format!("fa000000-0000-4000-8000-00000000000{k}"), "resqml22.TriangulatedSetRepresentation", format!("F{k}"))
}

/// A horizon torn by `fault-throw`, its lips — the plan's own shapes, which
/// are what `apply` writes — and its split edges as declared, one list of
/// pairs per lip: the fact the framework must find again.
fn torn() -> (Source, Source, Vec<Vec<i64>>) {
    let mut points = Vec::new();
    for j in 0..21 {
        for i in 0..21 {
            points.push([X0 + i as f64 * 10.0, Y0 + j as f64 * 10.0, -50.0]);
        }
    }
    let pieces = Pieces::empty()
        .with("horizon", shape(points, Topology::Grid { fastest: 21, slowest: 21 }))
        .with("horizon (object)", object(Ref::new("9d000000-0000-4000-8000-000000000006", "resqml22.Grid2dRepresentation", "H6")))
        .with("faults", fault())
        .with("faults (object)", object(fault_ref(1)))
        .with("feature", Source::Object(Known { reference: Some(Ref::new("fe000000-0000-4000-8000-000000000006", "resqml22.BoundaryFeature", "H6")), ..Default::default() }));
    let choices = Choices::new()
        .set("crs", frame().dor())
        .set("title", "H6, faulted")
        .set("maximumThrow", json!([10.0]))
        .set("throw", json!(["normal"]))
        .set("profile", "parabolic")
        .set("reach", 50.0);
    let plan = Registry::new().plan("fault-throw", &pieces, &choices, &Identity::derived(), &when()).map_err(|why| why.message()).expect("torn");
    let geometry = |at: usize| Source::Shape { geometry: plan.shapes[at].geometry.clone(), closed: None };
    let surface = plan.objects.iter().find(|o| o.qualified_type == "resqml22.TriangulatedSetRepresentation").expect("a surface");
    let lists: Vec<Vec<i64>> = surface.json["TrianglePatch"][0]["SplitEdgePatch"]
        .as_array()
        .expect("split edges")
        .iter()
        .map(|edge_patch| {
            let path = edge_patch["SplitEdges"]["Values"]["ExternalDataArrayPart"][0]["PathInExternalFile"].as_str().unwrap();
            match &plan.datasets.iter().find(|d| d.path == path).unwrap().values {
                Numbers::I64(values) => values.clone(),
                other => panic!("{other:?}"),
            }
        })
        .collect();
    (geometry(0), geometry(1), lists)
}

fn torn_ref() -> Ref {
    Ref::new("7e000000-0000-4000-8000-000000000309", "resqml22.TriangulatedSetRepresentation", "H6, faulted")
}

fn planned(pieces: &Pieces, choices: &Choices) -> Result<Plan, String> {
    Registry::new().plan("surface-framework", pieces, choices, &Identity::derived(), &when()).map_err(|why| why.message())
}

fn framework(plan: &Plan) -> &NewObject {
    assert_eq!(plan.objects.len(), 1);
    let one = &plan.objects[0];
    assert_eq!(one.qualified_type, "resqml22.NonSealedSurfaceFrameworkRepresentation");
    one
}

fn nodes(plan: &Plan, patch: &serde_json::Value) -> Vec<i64> {
    let path = patch["SupportingRepresentationNodes"]["Values"]["ExternalDataArrayPart"][0]["PathInExternalFile"].as_str().expect("external");
    match &plan.datasets.iter().find(|d| d.path == path).expect("a dataset").values {
        Numbers::I64(values) => values.clone(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_faults_alone_are_gathered_in_order_and_represent_the_organisation() {
    let pieces = Pieces::empty()
        .with("organisation", Source::Object(Known { reference: Some(organisation()), ..Default::default() }))
        .with("representations", fault())
        .with("representations (object)", object(fault_ref(2)))
        .with("representations 2", fault())
        .with("representations 2 (object)", object(fault_ref(1)));
    let plan = planned(&pieces, &Choices::new().set("title", "Lutetia — scenario B, non-sealed")).expect("planned");
    let one = framework(&plan);
    assert_eq!(one.json["RepresentedObject"]["Uuid"], json!(organisation().uuid));
    assert_eq!(one.json["IsHomogeneous"], json!(true));
    let listed: Vec<&str> = one.json["Representation"].as_array().unwrap().iter().map(|r| r["Title"].as_str().unwrap()).collect();
    assert_eq!(listed, ["F2", "F1"], "the order given, not another");
    assert!(one.json.get("Contacts").is_none());
    assert!(plan.says.iter().any(|note| note.key == "no-contact"));
    assert_eq!(plan.activity.recipe, "surface-framework");
}

/// **Two readings of one fact**: which nodes of the torn surface are
/// the lips. `fault-throw` **declares** them in its `SplitEdgePatch`, by
/// construction; the frame **finds** them by position, from the geometry
/// of the lips. Neither is computed from the other.
#[test]
fn a_contact_names_the_torn_nodes_fault_throw_declared() {
    let (surface, lips, declared) = torn();
    let pieces = Pieces::empty()
        .with("organisation", Source::Object(Known { reference: Some(organisation()), ..Default::default() }))
        .with("representations", fault())
        .with("representations (object)", object(fault_ref(1)))
        .with("representations 2", surface)
        .with("representations 2 (object)", object(torn_ref()))
        .with("lips", lips);
    let plan = planned(&pieces, &Choices::new().set("title", "B").set("contactIndex", json!([3]))).expect("planned");
    let one = framework(&plan);
    assert_eq!(one.json["IsHomogeneous"], json!(true));
    let contacts = one.json["Contacts"].as_array().expect("contacts");
    assert_eq!(contacts.len(), 1);
    assert_eq!(contacts[0]["$type"], json!("resqml22.NonSealedContact"));
    assert_eq!(contacts[0]["Index"], json!(3), "the position stated, as it was stated");
    let patches = contacts[0]["Patches"].as_array().expect("patches");
    assert_eq!(patches.len(), 2, "the footwall's lip and the hanging wall's, on the torn surface only");
    for (patch, pairs) in patches.iter().zip(&declared) {
        assert_eq!(patch["RepresentationIndex"], json!(1), "the torn surface is second in the list");
        let chain: Vec<i64> = std::iter::once(pairs[0]).chain(pairs.chunks_exact(2).map(|p| p[1])).collect();
        assert_eq!(nodes(&plan, patch), chain);
    }
}

#[test]
fn what_a_contact_cannot_name_is_refused() {
    let (surface, lips, _) = torn();
    let base = Pieces::empty()
        .with("organisation", Source::Object(Known { reference: Some(organisation()), ..Default::default() }))
        .with("representations", fault())
        .with("representations (object)", object(fault_ref(1)));
    // The lips without the surface they lie on.
    let refused = planned(&base.clone().with("lips", lips.clone()), &Choices::new().set("title", "B").set("contactIndex", json!([0]))).expect_err("no support");
    assert!(refused.contains("not nodes of any representation given"), "{refused}");
    let full = base.with("representations 2", surface).with("representations 2 (object)", object(torn_ref())).with("lips", lips);
    // One index per contact, distinct, from 0.
    let refused = planned(&full, &Choices::new().set("title", "B")).expect_err("no index");
    assert!(refused.contains("0 index(es) for 1 contact(s)"), "{refused}");
    let refused = planned(&full, &Choices::new().set("title", "B").set("contactIndex", json!([0, 1]))).expect_err("two indices");
    assert!(refused.contains("2 index(es) for 1 contact(s)"), "{refused}");
}

#[test]
fn a_framework_represents_an_organisation_and_gathers_objects_once() {
    let pieces = Pieces::empty()
        .with("organisation", Source::Object(Known { reference: Some(Ref::new("0b000000-0000-4000-8000-000000000001", "resqml22.EarthModelInterpretation", "Lutetia")), ..Default::default() }))
        .with("representations", fault())
        .with("representations (object)", object(fault_ref(1)));
    let refused = planned(&pieces, &Choices::new().set("title", "B")).expect_err("not an organisation");
    assert!(refused.contains("represents a structural organisation"), "{refused}");
    let pieces = Pieces::empty()
        .with("organisation", Source::Object(Known { reference: Some(organisation()), ..Default::default() }))
        .with("representations", fault())
        .with("representations (object)", object(fault_ref(1)))
        .with("representations 2", fault())
        .with("representations 2 (object)", object(fault_ref(1)));
    let refused = planned(&pieces, &Choices::new().set("title", "B")).expect_err("twice");
    assert!(refused.contains("given twice"), "{refused}");
}
