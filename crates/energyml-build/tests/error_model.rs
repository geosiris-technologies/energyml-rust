//! The `[ERROR]` model of the reference model, in miniature: each
//! deliberate anomaly written the way the Lutetia recipe writes it — an
//! ordinary operation, then the fault stated in one field by `edit-object` —
//! and `energyml_domain::check` must find **exactly** it, on the object
//! names, and nothing on the clean objects beside.
//!
//! *“A test that cannot fail proves nothing”* (the project's testing rule): a clean
//! package proves that the checks stay silent, not that they speak. This one
//! proves they speak, each once, each where it should.
//!
//! **The form comes from the documents, the values from the test.** Which rule
//! fires on which object is what the texts cited say (E-01 … E-09 are this
//! file's own labels for the anomalies):
//!
//! - E-01, E-02 — an abstract property kind carries no value (common TRG 2.3
//!   §11.9; the Energistics dictionary 2.3 marks `force per length` and
//!   `property` `IsAbstract`);
//! - E-03 — a discrete value names a row of its table by the key column (common
//!   TRG 2.3 §9.1 p. 190);
//! - E-04 — an `asset identifier` names a well of the package (Energistics
//!   Identifier Specification v4.0 §6.3 p. 14);
//! - E-06 — *"There can only be one trajectory per wellbore with
//!   definitive=true"* (WITSML TRG 2.1 §16.6 p. 350), filed on the wellbore;
//! - E-07 — IsListric *"Must not be present if the fault is not normal"*
//!   (RESQML Interpretations.xsd l. 659–662, TRG 2.2 §6.20 p. 188);
//! - E-08 — *"If true all data objects in the collection are of the same
//!   Energistics data type"* (Collection.xsd l. 66–70) on the collection, and
//!   *"the DataobjectCollection must only contain WellboreInterpretation"*
//!   (RESQML TRG 2.2 §6.19 p. 187) on the earth model — two findings, the
//!   association keeping its own flag false;
//! - E-09 — a contact element is a geological interpretation or a frontier,
//!   never a representation (RESQML TRG 2.2 §6.9 p. 173), one finding when the
//!   direct object is geological.
//!
//! E-05 (a unit against its kind's quantity class) is something `check`
//! cannot judge yet: it is left out, not pinned as a silence.
//!
//! The package is played from a recipe written here — never the Lutetia
//! recipe's output — into a temporary folder, and read back from the saved
//! files. Needs HDF5: the properties and the wells write arrays, and E-03 reads
//! them back.

#![cfg(feature = "hdf5")]

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use energyml_build::entries::Reader;
use energyml_build::operation::Registry;
use energyml_build::recipe::{self, Recipe};
use energyml_build::source::Source as Where;
use energyml_storage::EpcStore;

/// The recipe: the clean objects the anomalies stand beside, then the
/// anomalies, as the reference model's recipe writes them.
const RECIPE: &str = r#"
titre = "Anomalies"
workflow = "workflow:anomalies"
creation = "2026-09-25T00:00:00Z"
auteur = "test"
donnees = "anomalies.h5"

[[etape]]
nom = "repere"
operation = "crs"
set = { epsg = 2154, vertical = 5720 }

# -- clean: two wells, their WITSML side, a doublet's series, a fault ---------

[[etape]]
nom = "gth-1"
operation = "well-from-plan"
set = { crs = { etape = "repere", type = "LocalEngineeringCompoundCrs" }, title = "GTH-1", headX = 651620.0, headY = 6861170.0, ground = 35.0, datumHeight = 3.0, datumKind = "kelly bushing", sections = ["hold to 60"], step = 5.0 }

[[etape]]
nom = "gth-2"
operation = "well-from-plan"
set = { crs = { etape = "repere", type = "LocalEngineeringCompoundCrs" }, title = "GTH-2", headX = 651595.0, headY = 6861145.0, ground = 35.0, datumHeight = 3.0, datumKind = "kelly bushing", sections = ["hold to 60"], step = 5.0 }

[[etape]]
nom = "witsml"
operation = "witsml-wells"
entrees = [
  { creneau = "frames", etape = "gth-1", type = "WellboreFrameRepresentation" },
  { creneau = "frames", etape = "gth-2", type = "WellboreFrameRepresentation" },
]
set = { existence = "planned", synthetic = true }

[[etape]]
nom = "temps"
operation = "time-series"
set = { title = "months", times = ["2025-01-01T00:00:00Z", "2025-02-01T00:00:00Z", "2025-03-01T00:00:00Z"] }

[[etape]]
nom = "doublet"
operation = "prodml-doublet"
entrees = [
  { creneau = "producer", etape = "witsml", type = "Wellbore", titre = "GTH-1" },
  { creneau = "injector", etape = "witsml", type = "Wellbore", titre = "GTH-2" },
  { creneau = "series", etape = "temps", type = "TimeSeries" },
]
set = { facility = "exchanger", fluid = "water", standardConditions = "15 degC 1 atm", rate = 40.0, decline = 0.05, producerHeadTemperature = 16.5, producerHeadPressure = 3.2, assetIdentifier = "eml://test.example.com/well({wellbore})/wellbore(01)/wellhead", samplesFrom = "2025-01-02T00:00:00Z", sampleDays = 1, sampleStep = 60, times = ["2025-01-01T00:00:00Z", "2025-02-01T00:00:00Z", "2025-03-01T00:00:00Z"], source = "invented for a test" }

[[etape]]
nom = "f1"
operation = "fault-from-sticks"
[etape.set]
crs = { etape = "repere", type = "LocalEngineeringCompoundCrs" }
title = "F1"
geometry = "plane"
trace = "651300 6861100 20; 651400 6861273.2 20"
dip = 65.0
dipAzimuth = 120.0
verticalExtent = 120.0
throw = ["normal"]
listric = false
synthetic = true
source = "invented for a test"

# -- the anomalies --------------------------------------------------------------

[[etape]]
nom = "erreur-gth-9"
operation = "well-from-plan"
set = { crs = { etape = "repere", type = "LocalEngineeringCompoundCrs" }, title = "[ERROR] GTH-9", headX = 651700.0, headY = 6861250.0, ground = 35.0, datumHeight = 3.0, datumKind = "kelly bushing", sections = ["hold to 60"], step = 5.0 }

[[etape]]
nom = "erreur-witsml"
operation = "witsml-wells"
entrees = [ { creneau = "frames", etape = "erreur-gth-9", type = "WellboreFrameRepresentation" } ]
set = { existence = "planned", synthetic = true }

[[etape]]
nom = "erreur-trajectoire-bis"
operation = "edit-object"
entrees = [ { creneau = "object", etape = "erreur-witsml", type = "Trajectory" } ]
set = { copy = "[ERROR] GTH-9 — trajectory, a second definitive one", fields = ['Citation.Description = "E-06"'] }

[[etape]]
nom = "erreur-proprietes"
operation = "properties-from-table"
entrees = [
  { creneau = "table", fichier = "proprietes.csv" },
  { creneau = "frames", etape = "erreur-gth-9", type = "WellboreFrameRepresentation" },
]
template = "proprietes.template.json"

[[etape]]
nom = "erreur-table"
operation = "edit-object"
entrees = [ { creneau = "object", etape = "erreur-proprietes", type = "ColumnBasedTable" } ]
set = { fields = ['KeyColumn = [{"Title": "code", "ValueCountPerRow": 1, "PropertyKind": {"$type": "eml23.DataObjectReference", "Uuid": "311dcaba-6512-4144-aa7c-79a9de19b4ab", "QualifiedType": "eml23.PropertyKind", "Title": "identifier"}, "Values": {"$type": "eml23.IntegerXmlArray", "CountPerValue": 1, "NullValue": -9999, "Values": [1, 2, 3]}}]'] }

[[etape]]
nom = "erreur-serie"
operation = "edit-object"
entrees = [ { creneau = "object", etape = "doublet", type = "TimeSeriesData", titre = "GTH-1 — wellhead pressure" } ]
set = { copy = "[ERROR] GTH-99 — wellhead pressure", fields = ['Key[0]._ = "eml://test.example.com/well(GTH-99)/wellbore(01)/wellhead"'] }

[[etape]]
nom = "erreur-f9"
operation = "fault-from-sticks"
[etape.set]
crs = { etape = "repere", type = "LocalEngineeringCompoundCrs" }
title = "[ERROR] F9"
geometry = "plane"
trace = "651300 6860900 20; 651493.19 6860951.76 20"
dip = 60.0
dipAzimuth = 165.0
verticalExtent = 120.0
throw = ["normal"]
listric = true
synthetic = true
source = "invented for a test"

[[etape]]
nom = "erreur-f9-inverse"
operation = "edit-object"
entrees = [ { creneau = "object", etape = "erreur-f9", type = "FaultInterpretation" } ]
set = { fields = ['ThrowInterpretation[0].Throw = ["reverse"]'] }

[[etape]]
nom = "erreur-structure"
operation = "structural-organisation"
entrees = [
  { creneau = "boundaries", etape = "erreur-f9", type = "FaultInterpretation" },
  { creneau = "boundaries", etape = "f1", type = "FaultInterpretation" },
]
set = { title = "[ERROR] structure", ordering = "age", modelTitle = "[ERROR] model", contacts = ["[ERROR] F9 stops F1"] }

[[etape]]
nom = "erreur-contact"
operation = "edit-object"
entrees = [ { creneau = "object", etape = "erreur-structure", type = "StructuralOrganizationInterpretation" } ]
set = { reference = { etape = "erreur-f9", type = "TriangulatedSetRepresentation" }, referenceAt = "ContactInterpretation[0].Subject" }

[[etape]]
nom = "erreur-puits"
operation = "collection"
entrees = [
  { creneau = "members", etape = "erreur-gth-9", type = "WellboreInterpretation" },
  { creneau = "members", etape = "f1", type = "FaultInterpretation" },
]
set = { title = "[ERROR] wells", kind = "folder" }

[[etape]]
nom = "erreur-puits-homogene"
operation = "edit-object"
entrees = [ { creneau = "object", etape = "erreur-puits", type = "DataobjectCollection" } ]
set = { fields = ['HomogeneousDatatype = true'] }

[[etape]]
nom = "erreur-modele"
operation = "earth-model"
entrees = [
  { creneau = "structures", etape = "erreur-structure", type = "StructuralOrganizationInterpretation" },
  { creneau = "wellboreSets", etape = "erreur-puits", type = "DataobjectCollection" },
]
set = { title = "[ERROR] model", interpretationTitle = "[ERROR] model" }
"#;

const CSV: &str = "well,md,force per length,property,[ERROR] lithology
[ERROR] GTH-9,10,1.5,0.1,sand
[ERROR] GTH-9,20,1.5,0.2,sand
[ERROR] GTH-9,30,2.5,0.3,marl
[ERROR] GTH-9,40,2.5,0.4,chalk
";

/// The kinds by their uuid in the Energistics dictionary 2.3: `well name` and
/// `measured depth` (the structural roles), `force per length` and `property`
/// (both `IsAbstract` true), `litho type` (not abstract).
const TEMPLATE: &str = r#"{
  "KeyColumns": [
    { "ColumnName": "well", "ValueType": "string", "PropertyType": { "PropertyTypeID": "namespace:reference-data--PropertyType:348c9a4d-e3e4-4beb-8456-f07a46660de1:" } },
    { "ColumnName": "md", "ValueType": "number", "PropertyType": { "PropertyTypeID": "namespace:reference-data--PropertyType:c48c65d4-9680-4d10-903c-a1b2e30f66b5:" }, "UnitOfMeasureID": "namespace:reference-data--UnitOfMeasure:m:" }
  ],
  "Columns": [
    { "ColumnName": "force per length", "ValueType": "number", "PropertyType": { "PropertyTypeID": "namespace:reference-data--PropertyType:e75d460c-12d3-4610-b580-a2e95c0002f1:" }, "UnitOfMeasureID": "namespace:reference-data--UnitOfMeasure:N/m:" },
    { "ColumnName": "property", "ValueType": "number", "PropertyType": { "PropertyTypeID": "namespace:reference-data--PropertyType:a48c9c25-1e3a-43c8-be6a-044224cc69cb:" }, "UnitOfMeasureID": "namespace:reference-data--UnitOfMeasure:Euc:" },
    { "ColumnName": "[ERROR] lithology", "ValueType": "string", "PropertyType": { "PropertyTypeID": "namespace:reference-data--PropertyType:43bbe22e-78d9-4a4f-b2d1-584c4dff4de3:" } }
  ]
}"#;

/// The recipe's two files, from memory.
struct Files;

impl Reader for Files {
    fn text(&mut self, source: &Where) -> Result<(String, Option<String>), String> {
        match source {
            Where::File(path) if path.ends_with("proprietes.csv") => Ok((CSV.to_string(), None)),
            Where::File(path) if path.ends_with("proprietes.template.json") => Ok((TEMPLATE.to_string(), None)),
            other => Err(format!("{other:?}: not a file of this test")),
        }
    }
    fn package(&mut self, path: &str) -> Result<&EpcStore, String> {
        Err(format!("opening {path}: the recipe names no package"))
    }
}

#[test]
fn each_anomaly_is_found_once_on_its_object_and_the_clean_objects_are_silent() {
    let recipe = Recipe::parse(RECIPE).unwrap_or_else(|stop| panic!("{stop}"));
    let dir = std::env::temp_dir().join(format!("energyml-build-error-model-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("anomalies.epc");
    let schemas = Arc::new(energyml_schemas::all());
    recipe::run(&recipe, Path::new("."), &Registry::new(), schemas.clone(), &out, &mut Files).unwrap_or_else(|stop| panic!("{stop}"));

    let store = EpcStore::open(&out, schemas).expect("the package opens");
    let report = energyml_domain::check::of_store(
        &store,
        &energyml_domain::check::Options { schema: true, rules: true, extra_rules: Vec::new() },
    );
    let title = |uuid: &str| -> String {
        let summary = store.find_by_uuid(uuid).unwrap_or_else(|| panic!("{uuid} is in the package")).clone();
        let object = store.get_object(&summary.uri).expect("it reads");
        energyml_core::identity::title_of_reflect(object.as_reflect()).unwrap_or_default().to_string()
    };

    // Nothing else is wrong with the package: the anomalies are all there is —
    // eight weighed as errors, E-04 a warning.
    assert_eq!(report.errors(), 8, "{:#?}", (&report.schema, &report.references, &report.diagnostics, &report.unreadable, &report.rules));
    assert!(report.diagnostics.is_empty() && report.unreadable.is_empty(), "{:#?}", report.diagnostics);
    assert!(report.schema.is_empty(), "every edited object still reads as its type: {:#?}", report.schema);
    assert!(report.relationships.is_empty(), "{:#?}", report.relationships);
    assert!(report.reportable_references().is_empty(), "{:#?}", report.reportable_references());

    let found: BTreeSet<(String, String)> = report
        .rules
        .iter()
        .filter(|one| one.read.is_some())
        .map(|one| (one.rule.clone(), title(&one.uuid)))
        .chain(
            report
                .properties
                .iter()
                .filter(|one| one.is_a_defect)
                .map(|one| (format!("property kind: {}", if one.says.contains("abstract") { "abstract" } else { &one.says }), title(&one.uuid))),
        )
        .collect();
    let expected: BTreeSet<(String, String)> = [
        ("property kind: abstract", "[ERROR] GTH-9 — force per length"), // E-01
        ("property kind: abstract", "[ERROR] GTH-9 — property"),         // E-02
        ("house/discrete-value-in-lookup", "[ERROR] GTH-9 — [ERROR] lithology"), // E-03
        ("house/time-series-asset-resolves", "[ERROR] GTH-99 — wellhead pressure"), // E-04
        ("trg/one-definitive-trajectory-per-wellbore", "[ERROR] GTH-9"), // E-06, on the wellbore
        ("trg/is-listric-iff-normal", "[ERROR] F9 — interpretation"),   // E-07
        ("trg/homogeneous-collection", "[ERROR] wells"),                 // E-08
        ("trg/wellbore-interpretation-set-holds-wellbores", "[ERROR] model"), // E-08
        ("trg/contact-element-is-geological", "[ERROR] structure"),     // E-09
    ]
    .into_iter()
    .map(|(rule, title)| (rule.to_string(), title.to_string()))
    .collect();
    assert_eq!(found, expected);
    // A set would hide a finding given twice: the count says there is none.
    assert_eq!(report.errors() + report.warnings(), expected.len(), "{:#?}", report.rules);

    // Each finding is on an `[ERROR]` object: the clean ones — the doublet, its
    // series, F1 — are silent.
    for (rule, on) in &found {
        assert!(on.starts_with("[ERROR]"), "`{rule}` found on the clean object `{on}`");
    }
    // What E-03 reads: code 0, the rank of `sand`, has no row under keys from 1.
    let e03 = report.rules.iter().find(|one| one.rule == "house/discrete-value-in-lookup").unwrap();
    assert!(e03.read.as_deref().is_some_and(|read| read.starts_with("0 (2 element(s)) not in the key column `code`")), "{e03:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
