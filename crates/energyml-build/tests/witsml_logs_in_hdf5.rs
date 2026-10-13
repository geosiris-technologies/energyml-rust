//! `witsml-logs` with `channelDataFile`: each `ChannelSet`'s rows written to a
//! dataset of an HDF5 file, `FileUri` naming the file and `Data` the dataset —
//! **an extension beyond WITSML 2.1**.
//!
//! Two paths meet here and neither checks itself: the writer lays the dataset
//! out by its module doc, the reader (`channel_set::read_with`) reads it by
//! its own. The values are compared to **the test's inputs** — the depths and
//! the property values handed to the plan — and, through a real `.h5`, to the
//! RESQML property the log repeats, read by `energyml_domain::property`, a
//! reader that knows nothing of WITSML.
//!
//! And the default is held: left out, the plan writes no dataset, no
//! `FileUri`, and records no such parameter — `tests/lutetia.rs` holds the rest
//! byte for byte.

use std::cell::Cell;

use serde_json::{Value, json};

use energyml_build::operation::{Choices, Identity, Numbers, Plan, Registry, When};
use energyml_build::pieces::{Known, KnownProperty, Pieces, Ref, Source};
use energyml_core::registry::DynObject;
use energyml_core::value::Reflect;
use energyml_domain::series::{Quality, Series, channel_set};
use energyml_domain::source::{ArraySource, ObjectSource};
use energyml_domain::values::Values;
use energyml_geometry::geometry::{Geometry, Patch, Topology};
use energyml_storage::{ArrayBuffer, ArrayData, ArrayLocation, ArrayMetadata, ArrayValues, ExternalDataReason};

// ---------------------------------------------------------------------------
// A frame and two properties, the test's own values
// ---------------------------------------------------------------------------

const FRAME: &str = "10000000-0000-4000-8000-000000000001";
const INTERPRETATION: &str = "10000000-0000-4000-8000-000000000003";
const DATUM: &str = "10000000-0000-4000-8000-000000000004";
const COMPANY: &str = "40000000-0000-4000-8000-000000000001";
const FILE: &str = "logs.h5";

const MDS: [f64; 5] = [0.0, 10.0, 20.0, 30.0, 40.0];
const TEMPERATURES: [f64; 5] = [f64::NAN, 13.1, 13.4, 13.7, 14.0];
const PRESSURES: [f64; 5] = [0.0, 1.0, 2.0, 3.0, 4.0];

fn when() -> When {
    When { creation: "2026-09-28T00:00:00Z".into(), originator: "test".into() }
}

fn property(uuid: &str, title: &str, kind_uuid: &str, kind: &str, uom: &str, values: &[f64]) -> Source {
    Source::Object(Known {
        reference: Some(Ref::new(uuid, "resqml22.ContinuousProperty", title)),
        property: Some(KnownProperty {
            supporting: Some(Ref::new(FRAME, "resqml22.WellboreFrameRepresentation", "GTH-1 — frame")),
            indexable: "nodes".to_string(),
            value_count: vec![1],
            timed: false,
            realizations: false,
            uom: Some(uom.to_string()),
            kind: Some(Ref::new(kind_uuid, "eml23.PropertyKind", kind)),
            lookup: None,
            null_value: None,
            facets: Vec::new(),
            values: Ok(Values::Floating(values.to_vec())),
        }),
        ..Known::default()
    })
}

fn pieces() -> Pieces {
    let points: Vec<[f64; 3]> = MDS.iter().map(|md| [651_000.0, 6_861_000.0, 50.0 - md]).collect();
    let known = Known {
        reference: Some(Ref::new(FRAME, "resqml22.WellboreFrameRepresentation", "GTH-1 — frame")),
        interpretation: Some(Ref::new(INTERPRETATION, "resqml22.WellboreInterpretation", "GTH-1")),
        node_count: Some(MDS.len()),
        node_mds: Some(MDS.to_vec()),
        datum: Some(Ref::new(DATUM, "eml23.ReferencePointInALocalEngineeringCompoundCrs", "GTH-1 — datum")),
        ..Known::default()
    };
    let shape = Source::Shape {
        geometry: Geometry {
            kind: "polyline",
            patches: vec![Patch { index: 0, node_offset: 0, points, topology: Topology::Polylines(vec![MDS.len() as u32]), closed: Vec::new() }],
        },
        closed: None,
    };
    Pieces::empty()
        .with("frames", shape)
        .with("frames (object)", Source::Object(known))
        .with("properties", property("20000000-0000-4000-8000-000000000001", "GTH-1 — temperature", "30000000-0000-4000-8000-000000000001", "temperature", "degC", &TEMPERATURES))
        .with("properties 2", property("20000000-0000-4000-8000-000000000002", "GTH-1 — pressure", "30000000-0000-4000-8000-000000000002", "pressure", "bar", &PRESSURES))
}

fn choices() -> Choices {
    Choices::new().set(
        "loggingCompany",
        json!({ "$type": "eml23.DataObjectReference", "Uuid": COMPANY, "QualifiedType": "eml23.BusinessAssociate", "Title": "a company" }),
    )
}

fn plan(choices: Choices) -> Plan {
    Registry::new().plan("witsml-logs", &pieces(), &choices, &Identity::derived(), &when()).expect("a plan")
}

/// The plan's one Log, read back through its JSON — the road `apply` takes.
fn log_of(plan: &Plan) -> Box<dyn DynObject> {
    let schemas = energyml_schemas::all();
    let logs: Vec<_> = plan.objects.iter().filter(|one| one.qualified_type == "witsml21.Log").collect();
    assert_eq!(logs.len(), 1, "one Log");
    energyml_core::json::read_value(&logs[0].json, &schemas).expect("the Log reads back").root
}

fn set_json(plan: &Plan) -> Value {
    plan.objects[0].json["ChannelSet"][0].clone()
}

/// A series as the front end receives it, less what names the objects: two forms
/// of one set are two sets, and their channels two channels.
fn values_only(series: &Series) -> Value {
    let mut json = series.to_json();
    json.as_object_mut().unwrap().remove("uuid");
    json.as_object_mut().unwrap().remove("title");
    for channel in json["channels"].as_array_mut().unwrap() {
        channel.as_object_mut().unwrap().remove("uuid");
    }
    json
}

/// The plan's datasets, served as a store would serve them once written —
/// located by the storage's own locator, and counted.
struct Written<'a> {
    plan: &'a Plan,
    reads: Cell<usize>,
}

impl ObjectSource for Written<'_> {
    fn object(&self, _uuid: &str) -> Option<Box<dyn DynObject>> {
        None
    }
}

impl ArraySource for Written<'_> {
    fn locate(&self, object: &dyn Reflect) -> Vec<ArrayLocation> {
        energyml_storage::array::locate(object)
    }
    fn metadata(&self, location: &ArrayLocation) -> energyml_domain::Result<ArrayMetadata> {
        self.read(location).map(|data| data.metadata)
    }
    fn read(&self, location: &ArrayLocation) -> energyml_domain::Result<ArrayData> {
        self.reads.set(self.reads.get() + 1);
        let found = self.plan.datasets.iter().find(|one| one.file == location.file && one.path == location.path_in_file);
        let Some(dataset) = found else {
            return Err(energyml_storage::Error::ExternalData {
                location: location.path_in_file.clone(),
                reason: ExternalDataReason::PathMissing(location.path_in_file.clone()),
            }
            .into());
        };
        let Numbers::F64(values) = &dataset.values else { panic!("the rows are written as doubles") };
        Ok(ArrayData {
            metadata: ArrayMetadata { dimensions: dataset.dimensions.clone(), element_type: "float64".into() },
            values: ArrayValues::F64(values.clone()),
        })
    }
    fn read_into(&self, _location: &ArrayLocation, _out: &mut ArrayBuffer<'_>) -> energyml_domain::Result<ArrayMetadata> {
        Err(energyml_domain::Error::unsupported("reading into a buffer", "this test reads whole"))
    }
}

// ---------------------------------------------------------------------------
// The default does not move
// ---------------------------------------------------------------------------

#[test]
fn left_out_the_values_stay_inline_and_nothing_names_a_file() {
    // To break: a default given to `channelDataFile`, or the recipe's
    // `dataFile` reaching it — the Lutetia logs would move into the `.h5`.
    let inline = plan(choices());
    assert!(inline.datasets.is_empty(), "no dataset by default");
    let data = &set_json(&inline)["Data"];
    assert!(data.get("FileUri").is_none(), "never a FileUri by default: {data}");
    assert!(data["Data"].as_str().is_some_and(|text| text.starts_with('[')), "the rows inline: {data}");
    let recorded: Vec<&str> = inline.activity.parameters.iter().filter_map(|one| match one {
        energyml_domain::provenance::Parameter::Text { title, .. } => Some(title.as_str()),
        _ => None,
    }).collect();
    assert!(!recorded.contains(&"channelDataFile"), "a parameter nobody gave is not recorded: {recorded:?}");
}

// ---------------------------------------------------------------------------
// Given a file: one dataset, named by the set
// ---------------------------------------------------------------------------

#[test]
fn given_a_file_the_rows_go_to_one_dataset_the_set_names() {
    let inline = plan(choices());
    let in_file = plan(choices().set("channelDataFile", FILE));

    assert_eq!(in_file.datasets.len(), 1, "one dataset per ChannelSet");
    let dataset = &in_file.datasets[0];
    assert_eq!(dataset.file, FILE);
    // `[rows, 1 + channels]`: MD, then temperature and pressure as listed.
    assert_eq!(dataset.dimensions, vec![MDS.len() as i64, 3]);
    let Numbers::F64(values) = &dataset.values else { panic!("doubles") };
    for (row, md) in MDS.iter().enumerate() {
        assert_eq!(values[row * 3], *md, "row {row}: MD");
        let (t, p) = (values[row * 3 + 1], values[row * 3 + 2]);
        assert!(t == TEMPERATURES[row] || (t.is_nan() && TEMPERATURES[row].is_nan()), "row {row}: T {t}");
        assert_eq!(p, PRESSURES[row], "row {row}: P");
    }

    let data = &set_json(&in_file)["Data"];
    assert_eq!(data["FileUri"], FILE, "{data}");
    assert_eq!(data["Data"].as_str(), Some(dataset.path.as_str()), "Data is the dataset's path");
    assert!(dataset.path.starts_with('/'), "an absolute path, which no JSON text begins with");

    // Two forms of one set are two sets: nothing of the inline one is
    // rewritten, and the titles say which is which.
    let uuids = |plan: &Plan| {
        let set = set_json(plan);
        let mut all = vec![plan.objects[0].uuid.clone(), set["uuid"].as_str().unwrap().to_string()];
        all.extend(set["Channel"].as_array().unwrap().iter().map(|one| one["uuid"].as_str().unwrap().to_string()));
        all
    };
    for uuid in uuids(&in_file) {
        assert!(!uuids(&inline).contains(&uuid), "`{uuid}` is in both forms");
    }
    assert!(in_file.objects[0].title.ends_with("(HDF5)"), "{}", in_file.objects[0].title);
    assert!(!inline.objects[0].title.contains("HDF5"));

    // What the run was told is what the activity says.
    assert!(in_file.activity.parameters.iter().any(|one| matches!(one,
        energyml_domain::provenance::Parameter::Text { title, value } if title == "channelDataFile" && value == FILE)));
    // And what it says of itself.
    assert!(in_file.says.iter().any(|note| note.key == "written" && note.detail.contains("an extension beyond WITSML 2.1")));
}

#[test]
fn the_dataset_reads_back_as_the_inline_rows_and_as_the_values_given() {
    // To break: the rows written column-major, a channel left out of a row, a
    // NaN written as 0 — each moves the dataset away from the inline rows and
    // from the test's own values.
    let inline = channel_set::read_log(log_of(&plan(choices())).as_reflect()).expect("the inline log reads");
    let written = plan(choices().set("channelDataFile", FILE));
    let source = Written { plan: &written, reads: Cell::new(0) };
    let in_file = channel_set::read_log_with(log_of(&written).as_reflect(), &source).expect("the dataset reads");
    assert_eq!(source.reads.get(), 1);

    assert_eq!(values_only(&in_file[0]), values_only(&inline[0]));
    let series = &in_file[0];
    assert_eq!(series.index.values, MDS.to_vec());
    let temperature = series.channel("temperature").expect("a temperature channel");
    assert_eq!(temperature.quality[0], Quality::Absent, "the NaN is no value");
    assert_eq!(&temperature.numbers().unwrap()[1..], &TEMPERATURES[1..]);
    assert_eq!(series.channel("pressure").expect("a pressure channel").numbers().unwrap(), PRESSURES.to_vec());
}

#[test]
fn a_blank_file_is_refused_on_its_slot() {
    let refused = Registry::new()
        .plan("witsml-logs", &pieces(), &choices().set("channelDataFile", "  "), &Identity::derived(), &when())
        .expect_err("refused");
    assert!(refused.refusals().iter().any(|one| one.slot.as_deref() == Some("channelDataFile")), "{refused}");
}

// ---------------------------------------------------------------------------
// Through a real `.h5`: the recipe, the package, a rename
// ---------------------------------------------------------------------------

#[cfg(feature = "hdf5")]
#[test]
fn a_recipe_writes_both_forms_and_they_read_back_as_the_resqml_property() {
    use std::path::Path;
    use std::sync::Arc;

    use energyml_build::entries::Reader;
    use energyml_build::recipe::{self, Recipe};
    use energyml_build::source::Source as Where;
    use energyml_domain::series::{self, Readout};
    use energyml_storage::EpcStore;
    use energyml_storage::rename;

    struct NoFile;
    impl Reader for NoFile {
        fn text(&mut self, source: &Where) -> Result<(String, Option<String>), String> {
            Err(format!("{source:?}: this test reads no file"))
        }
        fn package(&mut self, path: &str) -> Result<&EpcStore, String> {
            Err(format!("opening {path}: the recipe names no package"))
        }
    }

    // The Lutetia step `logs-doublet` and its copy with the extension on,
    // over one well and one law.
    let text = r#"
titre = "WITSML in HDF5"
creation = "2026-09-28T00:00:00Z"
auteur = "test"
donnees = "logs.h5"
workflow = "workflow:witsml-hdf5"

[[etape]]
nom = "gth"
operation = "well-from-plan"
set = { sourceEpsg = 2154, project = 2154, vertical = 5720, title = "GTH-T", headX = 651800.0, headY = 6861200.0, ground = 50.0, sections = ["hold to 60"], step = 10.0 }

[[etape]]
nom = "pression"
operation = "well-logs"
entrees = [ { creneau = "frames", etape = "gth", type = "WellboreFrameRepresentation" } ]
set = { law = "hydrostatic", waterLevel = 40.0, datumHeight = 0.0, source = "synthetic: the test's own values" }

[[etape]]
nom = "etude"
operation = "business-associate"
set = { title = "Test", roles = ["model author"] }

[[etape]]
nom = "logs"
operation = "witsml-logs"
entrees = [
  { creneau = "frames", etape = "gth", type = "WellboreFrameRepresentation" },
  { creneau = "properties", etape = "pression", type = "ContinuousProperty" },
]
set = { synthetic = true, loggingCompany = { etape = "etude", type = "BusinessAssociate" } }

[[etape]]
nom = "logs-h5"
operation = "witsml-logs"
entrees = [
  { creneau = "frames", etape = "gth", type = "WellboreFrameRepresentation" },
  { creneau = "properties", etape = "pression", type = "ContinuousProperty" },
]
set = { synthetic = true, loggingCompany = { etape = "etude", type = "BusinessAssociate" }, channelDataFile = "logs.h5" }
"#;
    let recipe = Recipe::parse(text).expect("the recipe reads");
    let dir = std::env::temp_dir().join(format!("energyml-build-witsml-logs-in-hdf5-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("logs.epc");
    let schemas = Arc::new(energyml_schemas::all());
    recipe::run(&recipe, Path::new("."), &Registry::new(), schemas.clone(), &out, &mut NoFile).unwrap_or_else(|stop| panic!("{stop}"));

    let read_all = |store: &EpcStore| -> (Vec<(String, String, Value)>, Vec<f64>) {
        let held: Vec<(String, Box<dyn DynObject>)> =
            store.resources().iter().map(|one| (one.uuid.clone(), store.get_object(&one.uri).expect("it reads"))).collect();
        let objects: Vec<(&str, &dyn Reflect)> = held.iter().map(|(uuid, object)| (uuid.as_str(), object.as_reflect())).collect();
        let mut logs = Vec::new();
        for (uuid, object) in &held {
            if object.type_info().type_key.ends_with("Log") {
                let json = energyml_core::json::write_value(object.as_reflect());
                let title = json["Citation"]["Title"].as_str().unwrap().to_string();
                let Ok(Readout::Sets(sets)) = series::read(uuid, &objects, store) else { panic!("`{title}` reads") };
                logs.push((title, uuid.clone(), values_only(&sets[0])));
            }
        }
        // The RESQML property the logs repeat, by a reader that knows no WITSML.
        let property = held
            .iter()
            .find(|(_, object)| object.type_info().type_key.ends_with("ContinuousProperty"))
            .expect("the property");
        let read = energyml_domain::property::read(property.1.as_reflect()).expect("a property");
        let values = read.patches[0].read(store).expect("its values").to_f64().expect("numbers");
        (logs, values)
    };

    let mut store = EpcStore::open(&out, schemas.clone()).expect("the package reads");
    let (logs, resqml) = read_all(&store);
    assert_eq!(logs.len(), 2, "both forms: {:?}", logs.iter().map(|one| &one.0).collect::<Vec<_>>());
    let (inline, in_file) = if logs[0].0.ends_with("(HDF5)") { (&logs[1], &logs[0]) } else { (&logs[0], &logs[1]) };
    assert!(in_file.0.ends_with("(HDF5)"));
    assert_eq!(in_file.2, inline.2, "the two forms read the same");
    let channel: Vec<Value> = in_file.2["channels"][0]["values"].as_array().unwrap().clone();
    assert_eq!(channel.len(), resqml.len());
    assert!(resqml.iter().any(|value| value.is_nan()) && resqml.iter().any(|value| value.is_finite()), "a gap and values: {resqml:?}");
    for (node, (read, expected)) in channel.iter().zip(&resqml).enumerate() {
        match read.as_f64() {
            Some(value) => assert_eq!(value, *expected, "node {node}"),
            None => assert!(expected.is_nan(), "node {node}: null where the property holds {expected}"),
        }
    }

    // What the document says: the file and the dataset.
    let log = store.get_object(&store.find_by_uuid(&in_file.1).unwrap().uri.clone()).unwrap();
    let data = energyml_core::json::write_value(log.as_reflect())["ChannelSet"][0]["Data"].clone();
    assert_eq!(data["FileUri"], "logs.h5");
    let located = store.arrays_of(log.as_reflect());
    assert_eq!(located.len(), 1, "the dataset is an array of the Log: {located:?}");
    assert_eq!(store.array_metadata(&located[0]).expect("the file holds it").dimensions, vec![7, 2]);
    drop(log);

    // A rename follows the Log's `FileUri` like every other pointer.
    let quote = rename::plan(&store, "logs.h5", "renamed.h5");
    assert!(quote.objects.iter().any(|uri| uri.contains(&in_file.1)), "the Log is among what the rename touches: {:?}", quote.objects);
    rename::apply(&mut store, &quote).expect("the rename is carried out");
    energyml_storage::hdf5::close_cached_files();
    let saved = dir.join("after-rename.epc");
    store.save_as(&saved, &energyml_storage::SaveOptions::default()).expect("the package saves");
    let reopened = EpcStore::open(&saved, schemas).expect("the saved package reads");
    let log = reopened.get_object(&reopened.find_by_uuid(&in_file.1).unwrap().uri.clone()).unwrap();
    assert_eq!(energyml_core::json::write_value(log.as_reflect())["ChannelSet"][0]["Data"]["FileUri"], "renamed.h5");
    drop(log);
    let (after, _) = read_all(&reopened);
    let again = after.iter().find(|one| one.1 == in_file.1).expect("the Log");
    assert_eq!(again.2, in_file.2, "the same values after the rename");
}
