//! `contacts-from-frames` **at a rank** of the BRGM chart — `rank`, `unit` —
//! and the chart's answer underneath it, `energyml_domain::geolts_rank`.
//!
//! Why it exists: gathering the clouds by title
//! would mix internal limits (Lutétien supérieur / Lutétien) into H3. Contacts
//! **at a rank** of the column are needed.
//!
//! **The chart is an extract written here**, installed as the default for this
//! test binary alone: the divisions Lutetia's passes name and the chains above
//! them, plus the Ludien and the Priabonien for a regional synonym — every row
//! copied from the BRGM export of 24/09/2026
//! (`https://data.geoscience.fr/ncl/GeolTs`, CC BY 4.0), in the reduced header
//! of `tests/stratigraphy.rs`. The shape of the assertions comes from the
//! RESQML 2.2 reference guide (§5.34: one unit per interval, so a contact is a
//! node) and from the chart's own statements — `broader`, the ages of the
//! bounds; the values from the extract.
//!
//! **Two sources for a unit's identity.** The frames name their units by the
//! uuids a column of `stratigraphy-from-geolts` writes over the extract — read
//! off its plan, never computed here — and the operation derives the uuid of a
//! division its own way. If the two disagreed, every unit would be foreign to
//! the chart and every test below would fail.
//!
//! Map coordinates (Lambert-93 around the Louvre): a defect of 24/09 only
//! showed far from the origin.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Once;

use serde_json::{Value, json};

use energyml_build::operation::{Choices, Identity, Numbers, Plan, Registry, When};
use energyml_build::pieces::{Known, KnownProperty, Pieces, Ref, Source};
use energyml_domain::geolts::{self, Chart};
use energyml_domain::geolts_rank::{self, Relation};
use energyml_domain::values::Values;
use energyml_geometry::geometry::{Geometry, Patch, Topology};

const CONCEPTS: &str = r#"@id,skos:prefLabel,skos:notation,<https://data.geoscience.fr/def/main#mnemo>,<https://data.geoscience.fr/def/main#typeChronostratAge>,<https://data.geoscience.fr/def/main#nature>,skos:broader,<http://dbpedia.org/ontology/rgbCoordinateRed>,<http://dbpedia.org/ontology/rgbCoordinateGreen>,<http://dbpedia.org/ontology/rgbCoordinateBlue>,time:hasBeginning,time:hasEnd,adms:status,dct:spatial
<https://data.geoscience.fr/ncl/GeolTs/1028>,'Phanerozoic'@en|'Phanérozoïque'@fr,ph,PHAN,<https://data.geoscience.fr/ncl/DivChronoStrat/2>,<https://data.geoscience.fr/ncl/natelem/1>,,'154','217','221',<https://data.geoscience.fr/ncl/GeolTsBound/1028>,<https://data.geoscience.fr/ncl/GeolTsBound/0>,<http://inspire.ec.europa.eu/registry/status/valid>,Monde
<https://data.geoscience.fr/ncl/GeolTs/1241>,'Cenozoic'@en|'Cénozoïque'@fr,cz,CENZ,<https://data.geoscience.fr/ncl/DivChronoStrat/3>,<https://data.geoscience.fr/ncl/natelem/1>,<https://data.geoscience.fr/ncl/GeolTs/1028>,'242','249','29',<https://data.geoscience.fr/ncl/GeolTsBound/1241>,<https://data.geoscience.fr/ncl/GeolTsBound/0>,<http://inspire.ec.europa.eu/registry/status/valid>,Monde
<https://data.geoscience.fr/ncl/GeolTs/1289>,'Quaternary'@en|'Quaternaire'@fr,q|Q|IV,QUAT,<https://data.geoscience.fr/ncl/DivChronoStrat/4>,<https://data.geoscience.fr/ncl/natelem/1>,<https://data.geoscience.fr/ncl/GeolTs/1241>,'249','249','127',<https://data.geoscience.fr/ncl/GeolTsBound/1289>,<https://data.geoscience.fr/ncl/GeolTsBound/0>,<http://inspire.ec.europa.eu/registry/status/valid>,Monde
<https://data.geoscience.fr/ncl/GeolTs/1290>,'Pleistocene'@en|'Pléistocène'@fr,q1-3,PLTN,<https://data.geoscience.fr/ncl/DivChronoStrat/5>,<https://data.geoscience.fr/ncl/natelem/1>,<https://data.geoscience.fr/ncl/GeolTs/1289>,'255','242','174',<https://data.geoscience.fr/ncl/GeolTsBound/1289>,<https://data.geoscience.fr/ncl/GeolTsBound/1326>,<http://inspire.ec.europa.eu/registry/status/valid>,Monde
<https://data.geoscience.fr/ncl/GeolTs/1316>,'Upper/Late Pleistocene'@en|'Pléistocène supérieur'@fr,q3,PLTS,<https://data.geoscience.fr/ncl/DivChronoStrat/6>,<https://data.geoscience.fr/ncl/natelem/1>,<https://data.geoscience.fr/ncl/GeolTs/1290>,'255','242','211',<https://data.geoscience.fr/ncl/GeolTsBound/1316>,<https://data.geoscience.fr/ncl/GeolTsBound/1326>,<http://inspire.ec.europa.eu/registry/status/valid>,Monde
<https://data.geoscience.fr/ncl/GeolTs/1326>,'Holocene'@en|'Holocène'@fr,_z,HOLO,<https://data.geoscience.fr/ncl/DivChronoStrat/5>,<https://data.geoscience.fr/ncl/natelem/1>,<https://data.geoscience.fr/ncl/GeolTs/1289>,'254','242','224',<https://data.geoscience.fr/ncl/GeolTsBound/1326>,<https://data.geoscience.fr/ncl/GeolTsBound/0>,<http://inspire.ec.europa.eu/registry/status/valid>,Monde
<https://data.geoscience.fr/ncl/GeolTs/1242>,'Paleogene'@en|'Paléogène'@fr,e-g|Pg,PLGN,<https://data.geoscience.fr/ncl/DivChronoStrat/4>,<https://data.geoscience.fr/ncl/natelem/1>,<https://data.geoscience.fr/ncl/GeolTs/1241>,'253','154','82',<https://data.geoscience.fr/ncl/GeolTsBound/1241>,<https://data.geoscience.fr/ncl/GeolTsBound/1269>,<http://inspire.ec.europa.eu/registry/status/valid>,Monde
<https://data.geoscience.fr/ncl/GeolTs/1249>,'Eocene'@en|'Eocène'@fr,e4-7,EOCE,<https://data.geoscience.fr/ncl/DivChronoStrat/5>,<https://data.geoscience.fr/ncl/natelem/1>,<https://data.geoscience.fr/ncl/GeolTs/1242>,'253','180','108',<https://data.geoscience.fr/ncl/GeolTsBound/1249>,<https://data.geoscience.fr/ncl/GeolTsBound/1264>,<http://inspire.ec.europa.eu/registry/status/valid>,Monde
<https://data.geoscience.fr/ncl/GeolTs/1250>,'Lower Eocene'@en|'Eocène inférieur'@fr,e4,EOCI,<https://data.geoscience.fr/ncl/DivChronoStrat/6>,<https://data.geoscience.fr/ncl/natelem/3>,<https://data.geoscience.fr/ncl/GeolTs/1249>,'253','180','108',<https://data.geoscience.fr/ncl/GeolTsBound/1249>,<https://data.geoscience.fr/ncl/GeolTsBound/1255>,<http://inspire.ec.europa.eu/registry/status/valid>,Monde
<https://data.geoscience.fr/ncl/GeolTs/1254>,'Middle Eocene'@en|'Eocène moyen'@fr,e5-6,EOCM,<https://data.geoscience.fr/ncl/DivChronoStrat/6>,<https://data.geoscience.fr/ncl/natelem/3>,<https://data.geoscience.fr/ncl/GeolTs/1249>,'253','180','108',<https://data.geoscience.fr/ncl/GeolTsBound/1255>,<https://data.geoscience.fr/ncl/GeolTsBound/1263>,<http://inspire.ec.europa.eu/registry/status/valid>,Monde
<https://data.geoscience.fr/ncl/GeolTs/1251>,'Ypresian'@en|'Yprésien'@fr,e4,YPRE,<https://data.geoscience.fr/ncl/DivChronoStrat/7>,<https://data.geoscience.fr/ncl/natelem/1>,<https://data.geoscience.fr/ncl/GeolTs/1250>|<https://data.geoscience.fr/ncl/GeolTs/1249>,'252','167','115',<https://data.geoscience.fr/ncl/GeolTsBound/1249>,<https://data.geoscience.fr/ncl/GeolTsBound/1255>,<http://inspire.ec.europa.eu/registry/status/valid>,Monde
<https://data.geoscience.fr/ncl/GeolTs/2174>,'Cuisian'@en|'Cuisien'@fr,e4b,CUIS,<https://data.geoscience.fr/ncl/DivChronoStrat/7>,<https://data.geoscience.fr/ncl/natelem/2>,<https://data.geoscience.fr/ncl/GeolTs/1251>,'252','167','115',<https://data.geoscience.fr/ncl/GeolTsBound/2174>,<https://data.geoscience.fr/ncl/GeolTsBound/1255>,<http://inspire.ec.europa.eu/registry/status/valid>,Bassin de Paris
<https://data.geoscience.fr/ncl/GeolTs/2173>,'Sparnacian'@en|'Sparnacien'@fr,e4a,SPAR,<https://data.geoscience.fr/ncl/DivChronoStrat/8>,<https://data.geoscience.fr/ncl/natelem/2>,<https://data.geoscience.fr/ncl/GeolTs/1251>,'252','167','115',<https://data.geoscience.fr/ncl/GeolTsBound/1249>,<https://data.geoscience.fr/ncl/GeolTsBound/2174>,<http://inspire.ec.europa.eu/registry/status/valid>,Bassin de Paris
<https://data.geoscience.fr/ncl/GeolTs/1255>,'Lutetian'@en|'Lutétien'@fr,e5,LUTE,<https://data.geoscience.fr/ncl/DivChronoStrat/7>,<https://data.geoscience.fr/ncl/natelem/1>,<https://data.geoscience.fr/ncl/GeolTs/1249>|<https://data.geoscience.fr/ncl/GeolTs/1254>,'252','180','130',<https://data.geoscience.fr/ncl/GeolTsBound/1255>,<https://data.geoscience.fr/ncl/GeolTsBound/1259>,<http://inspire.ec.europa.eu/registry/status/valid>,Monde
<https://data.geoscience.fr/ncl/GeolTs/1256>,'Lower Lutetian'@en|'Lutétien inférieur'@fr,e5a,LUTI,<https://data.geoscience.fr/ncl/DivChronoStrat/8>,<https://data.geoscience.fr/ncl/natelem/2>,<https://data.geoscience.fr/ncl/GeolTs/1255>,'252','180','130',<https://data.geoscience.fr/ncl/GeolTsBound/1255>,<https://data.geoscience.fr/ncl/GeolTsBound/1257>,<http://inspire.ec.europa.eu/registry/status/valid>,Europe
<https://data.geoscience.fr/ncl/GeolTs/1257>,'Middle Lutetian'@en|'Lutétien moyen'@fr,e5b,LUTM,<https://data.geoscience.fr/ncl/DivChronoStrat/8>,<https://data.geoscience.fr/ncl/natelem/2>,<https://data.geoscience.fr/ncl/GeolTs/1255>,'252','180','130',<https://data.geoscience.fr/ncl/GeolTsBound/1257>,<https://data.geoscience.fr/ncl/GeolTsBound/1258>,<http://inspire.ec.europa.eu/registry/status/valid>,Europe
<https://data.geoscience.fr/ncl/GeolTs/1258>,'Upper Lutetian'@en|'Lutétien supérieur'@fr,e5c,LUTS,<https://data.geoscience.fr/ncl/DivChronoStrat/8>,<https://data.geoscience.fr/ncl/natelem/2>,<https://data.geoscience.fr/ncl/GeolTs/1255>,'252','180','130',<https://data.geoscience.fr/ncl/GeolTsBound/1258>,<https://data.geoscience.fr/ncl/GeolTsBound/e1258>,<http://inspire.ec.europa.eu/registry/status/valid>,Europe
<https://data.geoscience.fr/ncl/GeolTs/1259>,'Bartonian'@en|'Bartonien'@fr,e6,BRTN,<https://data.geoscience.fr/ncl/DivChronoStrat/7>,<https://data.geoscience.fr/ncl/natelem/1>,<https://data.geoscience.fr/ncl/GeolTs/1254>|<https://data.geoscience.fr/ncl/GeolTs/1249>,'253','192','145',<https://data.geoscience.fr/ncl/GeolTsBound/1259>,<https://data.geoscience.fr/ncl/GeolTsBound/1263>,<http://inspire.ec.europa.eu/registry/status/valid>,Monde
<https://data.geoscience.fr/ncl/GeolTs/1262>,'Upper Eocene'@en|'Eocène supérieur'@fr,e7,EOCS,<https://data.geoscience.fr/ncl/DivChronoStrat/6>,<https://data.geoscience.fr/ncl/natelem/3>,<https://data.geoscience.fr/ncl/GeolTs/1249>,'253','180','108',<https://data.geoscience.fr/ncl/GeolTsBound/1263>,<https://data.geoscience.fr/ncl/GeolTsBound/1264>,<http://inspire.ec.europa.eu/registry/status/valid>,Monde
<https://data.geoscience.fr/ncl/GeolTs/1263>,'Priabonian'@en|'Priabonien'@fr,e7,PRIA,<https://data.geoscience.fr/ncl/DivChronoStrat/7>,<https://data.geoscience.fr/ncl/natelem/1>,<https://data.geoscience.fr/ncl/GeolTs/1262>|<https://data.geoscience.fr/ncl/GeolTs/1249>,'253','205','161',<https://data.geoscience.fr/ncl/GeolTsBound/1263>,<https://data.geoscience.fr/ncl/GeolTsBound/1264>,<http://inspire.ec.europa.eu/registry/status/valid>,Monde
<https://data.geoscience.fr/ncl/GeolTs/2183>,'Ludian'@en|'Ludien'@fr,e7,LUDI,<https://data.geoscience.fr/ncl/DivChronoStrat/7>,<https://data.geoscience.fr/ncl/natelem/2>,<https://data.geoscience.fr/ncl/GeolTs/1262>,'253','205','161',<https://data.geoscience.fr/ncl/GeolTsBound/1263>,<https://data.geoscience.fr/ncl/GeolTsBound/1264>,<http://inspire.ec.europa.eu/registry/status/valid>,Bassin de Paris
"#;

/// The bounds those rows name, as the export writes them — a decimal comma,
/// and `e1258`, the Upper Lutetian's young end, which is **no number**: the
/// scanner passes over it, and the snapshot gives that division no young end.
const BOUNDS: &str = r#"@id,time:numericPosition
<https://data.geoscience.fr/ncl/GeolTsBound/0>,'0'
<https://data.geoscience.fr/ncl/GeolTsBound/1028>,'541'
<https://data.geoscience.fr/ncl/GeolTsBound/1241>,'66'
<https://data.geoscience.fr/ncl/GeolTsBound/1249>,'56'
<https://data.geoscience.fr/ncl/GeolTsBound/1255>,"47,8"
<https://data.geoscience.fr/ncl/GeolTsBound/1257>,"46,81"
<https://data.geoscience.fr/ncl/GeolTsBound/1258>,"45,82"
<https://data.geoscience.fr/ncl/GeolTsBound/e1258>,"44,5"
<https://data.geoscience.fr/ncl/GeolTsBound/1259>,"41,2"
<https://data.geoscience.fr/ncl/GeolTsBound/1263>,"37,8"
<https://data.geoscience.fr/ncl/GeolTsBound/1264>,"33,9"
<https://data.geoscience.fr/ncl/GeolTsBound/1269>,"23,03"
<https://data.geoscience.fr/ncl/GeolTsBound/1289>,"2,58"
<https://data.geoscience.fr/ncl/GeolTsBound/1316>,"0,129"
<https://data.geoscience.fr/ncl/GeolTsBound/1326>,"0,0117"
<https://data.geoscience.fr/ncl/GeolTsBound/2174>,"52,72"
"#;

/// The frames' system — a projected one, taken as it stands with a note.
const CRS: &str = "44444444-4444-4444-8444-444444444444";
const PHI_KIND: &str = "11111111-1111-4111-8111-111111111111";
const UNIT: &str = "resqml22.StratigraphicUnitInterpretation";

static CHART: Once = Once::new();

/// The extract, as the chart every operation of this binary reads.
fn extract() {
    CHART.call_once(|| {
        geolts::set_default(Chart::from_csv(CONCEPTS, Some(BOUNDS)).expect("the extract reads"))
    });
}

fn when() -> When {
    When {
        creation: "2026-09-24T00:00:00Z".into(),
        originator: "test".into(),
    }
}

fn plan(operation: &str, pieces: &Pieces, answers: Value) -> Result<Plan, String> {
    extract();
    let mut choices = Choices::new();
    for (name, value) in answers.as_object().expect("an object") {
        choices = choices.set(name, value.clone());
    }
    Registry::new()
        .plan(operation, pieces, &choices, &Identity::derived(), &when())
        .map_err(|why| why.to_string())
}

fn contacts(pieces: &Pieces, answers: Value) -> Result<Plan, String> {
    plan("contacts-from-frames", pieces, answers)
}

/// The units of a column of the extract as the boundary resolves a frame's
/// `IntervalStratigraphicUnits`: the interpretations `stratigraphy-from-geolts`
/// writes, by title — the regional divisions and the substages included, as
/// the passes of Lutetia name them.
fn column() -> BTreeMap<String, Ref> {
    let made = plan(
        "stratigraphy-from-geolts",
        &Pieces::empty(),
        json!({ "units": ["1289", "1242"], "regional": true, "finest": 8 }),
    )
    .expect("the column of the extract");
    made.objects
        .iter()
        .filter(|one| one.qualified_type == UNIT)
        .map(|one| {
            (
                one.title.clone(),
                Ref::new(one.uuid.clone(), one.qualified_type.clone(), one.title.clone()),
            )
        })
        .collect()
}

/// The units of a frame's intervals, top down, by their labels in the chart.
fn units(labels: &[&str]) -> Vec<Option<Ref>> {
    let column = column();
    labels
        .iter()
        .map(|label| {
            Some(
                column
                    .get(*label)
                    .cloned()
                    .unwrap_or_else(|| panic!("`{label}` is not in the column of the extract")),
            )
        })
        .collect()
}

/// Where frame `at` stands: map coordinates, Lambert-93 around the Louvre.
fn head(at: usize) -> [f64; 2] {
    [651_000.0 + 250.0 * at as f64, 6_861_000.0 + 125.0 * at as f64]
}

/// Node `k` of frame `at`: a vertical hole, a node every 5 m from 35 m NGF
/// down — **the construction of [`frame`]**, and nothing the operation kept.
fn node(at: usize, k: usize) -> [f64; 3] {
    let [x, y] = head(at);
    [x, y, 35.0 - 5.0 * k as f64]
}

/// A frame whose intervals' units are these, top down.
fn frame(bag: Pieces, at: usize, title: &str, units: &[Option<Ref>]) -> Pieces {
    let name = if at == 0 {
        "frames".to_string()
    } else {
        format!("frames {}", at + 1)
    };
    let points: Vec<[f64; 3]> = (0..=units.len()).map(|k| node(at, k)).collect();
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
            interval_units: Some(units.to_vec()),
            ..Known::default()
        }),
    )
}

/// A porosity log on the **intervals** of frame `frame`, titled as
/// `properties-from-table` titles them.
fn phi(frame: &str, values: &[f64]) -> Known {
    Known {
        reference: Some(Ref::new(
            format!("uuid-{frame}-PHI"),
            "resqml22.ContinuousProperty",
            format!("{frame} — PHI"),
        )),
        property: Some(KnownProperty {
            supporting: Some(Ref::new(
                format!("uuid-{frame}"),
                "resqml22.WellboreFrameRepresentation",
                frame,
            )),
            indexable: "intervals".to_string(),
            value_count: vec![1],
            timed: false,
            realizations: false,
            uom: Some("m3/m3".to_string()),
            kind: Some(Ref::new(PHI_KIND, "eml23.PropertyKind", "porosity")),
            lookup: None,
            null_value: None,
            facets: Vec::new(),
            values: Ok(Values::Floating(values.to_vec())),
        }),
        ..Known::default()
    }
}

/// The points of a point set, by its title.
fn points_of(made: &Plan, title: &str) -> Option<Vec<[f64; 3]>> {
    let set = made
        .objects
        .iter()
        .find(|one| one.qualified_type == "resqml22.PointSetRepresentation" && one.title == title)?;
    let path = format!("/resqml22/{}/points", set.uuid);
    let dataset = made.datasets.iter().find(|one| one.path == path)?;
    let Numbers::F64(values) = &dataset.values else {
        return None;
    };
    Some(values.chunks(3).map(|p| [p[0], p[1], p[2]]).collect())
}

/// The titles of the point sets written, sorted.
fn sets(made: &Plan) -> Vec<String> {
    let mut out: Vec<String> = made
        .objects
        .iter()
        .filter(|one| one.qualified_type == "resqml22.PointSetRepresentation")
        .map(|one| one.title.clone())
        .collect();
    out.sort();
    out
}

/// Every point of every point set.
fn all_points(made: &Plan) -> Vec<[f64; 3]> {
    sets(made)
        .iter()
        .filter_map(|title| points_of(made, title))
        .flatten()
        .collect()
}

/// A copy's values, by its title.
fn copied(made: &Plan, title: &str) -> Option<Numbers> {
    let object = made
        .objects
        .iter()
        .find(|one| one.title == title && one.qualified_type.ends_with("Property"))?;
    let path = format!("/resqml22/{}/values", object.uuid);
    made.datasets
        .iter()
        .find(|one| one.path == path)
        .map(|one| one.values.clone())
}

/// What the plan says under a key.
fn note(made: &Plan, key: &str) -> String {
    made.says
        .iter()
        .find(|one| one.key == key)
        .map(|one| one.detail.clone())
        .unwrap_or_else(|| {
            panic!(
                "no `{key}` said: {:?}",
                made.says.iter().map(|one| one.key).collect::<Vec<_>>()
            )
        })
}

/// The uuid of an object of a type, by its title.
fn uuid_of(made: &Plan, qualified_type: &str, title: &str) -> Option<String> {
    made.objects
        .iter()
        .find(|one| one.qualified_type == qualified_type && one.title == title)
        .map(|one| one.uuid.clone())
}

// -- the operation, at a rank --------------------------------------------------

#[test]
fn at_its_rank_the_lutetian_has_one_top_and_one_base_never_its_inner_limit() {
    // Bartonien over Lutétien supérieur over Lutétien moyen over Yprésien: the
    // Lutetian is intervals 1 and 2, between nodes 1 and 3; node 2 is the
    // limit between two of its substages.
    let pieces = frame(
        Pieces::empty(),
        0,
        "F1",
        &units(&["Bartonien", "Lutétien supérieur", "Lutétien moyen", "Yprésien"]),
    );
    let made = contacts(&pieces, json!({ "rank": 6, "unit": "Lutétien" })).expect("a plan");

    assert_eq!(points_of(&made, "Lutétien (stage) — top"), Some(vec![node(0, 1)]));
    assert_eq!(
        points_of(&made, "Lutétien (stage) — base"),
        Some(vec![node(0, 3)])
    );
    // The unit asked for and nothing else: what lies outside it is its
    // neighbour, which makes its contacts and has none of its own.
    assert_eq!(
        sets(&made),
        vec!["Lutétien (stage) — base", "Lutétien (stage) — top"]
    );
    // **Never the inner limit**, in any point set.
    let inner = node(0, 2);
    assert!(
        all_points(&made).iter().all(|point| *point != inner),
        "the Upper/Middle Lutetian limit is no contact of the stage"
    );

    // The same frame read unit by unit gives the inner limit — the finding
    // this reading exists to leave out, and what this test would see if the
    // settings were ignored.
    let named = contacts(&pieces, json!({})).expect("a plan");
    assert_eq!(points_of(&named, "Lutétien supérieur — base"), Some(vec![inner]));
}

#[test]
fn at_a_rank_an_interval_property_is_read_on_the_side_of_the_division() {
    let pieces = frame(
        Pieces::empty(),
        0,
        "F1",
        &units(&["Bartonien", "Lutétien supérieur", "Lutétien moyen", "Yprésien"]),
    );
    let pieces = pieces.with("properties", Source::Object(phi("F1", &[0.1, 0.2, 0.3, 0.4])));
    let made = contacts(&pieces, json!({ "unit": "Lutétien" })).expect("a plan");

    // **The division's first interval at its top, its last at its base** —
    // the Upper Lutetian's 0.2 and the Middle Lutetian's 0.3. Read by the
    // node's rank, the base would take the Ypresian's 0.4; read by the
    // substage, the top of the stage and the base of the Upper Lutetian would
    // be two different nodes.
    assert_eq!(
        copied(&made, "Lutétien (stage) — top — PHI"),
        Some(Numbers::F64(vec![0.2]))
    );
    assert_eq!(
        copied(&made, "Lutétien (stage) — base — PHI"),
        Some(Numbers::F64(vec![0.3]))
    );
}

#[test]
fn a_frame_without_the_unit_gives_no_point_and_is_counted() {
    let pieces = frame(
        Pieces::empty(),
        0,
        "F1",
        &units(&["Bartonien", "Lutétien supérieur", "Lutétien moyen", "Yprésien"]),
    );
    // F2 never reaches the Lutetian.
    let pieces = frame(pieces, 1, "F2", &units(&["Holocène", "Cuisien", "Sparnacien"]));
    // F3 reaches it under the Holocene and ends in the Middle Eocene — a
    // coarser division that holds it: part of that pass may be Lutetian, so
    // no base can be read there.
    let pieces = frame(
        pieces,
        2,
        "F3",
        &units(&["Holocène", "Lutétien supérieur", "Eocène moyen"]),
    );
    let made = contacts(&pieces, json!({ "unit": "1255" })).expect("a plan");

    assert_eq!(
        points_of(&made, "Lutétien (stage) — top"),
        Some(vec![node(0, 1), node(2, 1)])
    );
    assert_eq!(
        points_of(&made, "Lutétien (stage) — base"),
        Some(vec![node(0, 3)]),
        "no base where a coarser division holds the unit"
    );
    // F2 gives nothing, anywhere.
    let [x, y] = head(1);
    assert!(
        all_points(&made)
            .iter()
            .all(|point| point[0] != x || point[1] != y),
        "F2 has no Lutetian"
    );

    // Counted, and named.
    let absent = note(&made, "rank-absent");
    assert!(
        absent.starts_with("1 frame(s)") && absent.contains("`F2`"),
        "{absent}"
    );
    let unknown = note(&made, "rank-unknown");
    assert!(
        unknown.contains("Eocène moyen × 1") && unknown.contains("holds it"),
        "{unknown}"
    );
    let reached = note(&made, "rank-units");
    assert!(reached.contains("`Lutétien (stage)` in 2"), "{reached}");
}

#[test]
fn a_division_that_comes_back_in_one_hole_is_refused_by_name() {
    // Named, three different divisions and no refusal; at the stage, the
    // Lutetian twice in one hole — two tops, and which one is *the* top is a
    // geologist's call.
    let pieces = frame(
        Pieces::empty(),
        0,
        "F1",
        &units(&["Lutétien supérieur", "Yprésien", "Lutétien inférieur"]),
    );
    assert!(contacts(&pieces, json!({})).is_ok(), "named, three divisions");
    let why = contacts(&pieces, json!({ "unit": "Lutétien" })).expect_err("the Lutetian twice in one hole");
    assert!(
        why.contains("`F1`") && why.contains("`Lutétien (stage)`") && why.contains("twice"),
        "{why}"
    );
}

#[test]
fn a_rank_or_a_unit_the_chart_does_not_have_is_refused_by_name() {
    let pieces = frame(
        Pieces::empty(),
        0,
        "F1",
        &units(&["Bartonien", "Lutétien", "Yprésien"]),
    );
    // The extract has no supereonothem, so no rank 0 — and the refusal lists
    // the ranks it has.
    let why = contacts(&pieces, json!({ "rank": 0 })).expect_err("no division at rank 0");
    assert!(
        why.contains("`rank`") && why.contains("rank 0") && why.contains("6 stage"),
        "{why}"
    );
    // Past the chart's last level: refused by the slot, before the plan.
    assert!(
        contacts(&pieces, json!({ "rank": 12 })).is_err(),
        "rank 12 is past level 10"
    );
    // A unit of another rank than the one given.
    let why = contacts(&pieces, json!({ "rank": 6, "unit": "Lutétien supérieur" })).expect_err("a substage");
    assert!(
        why.contains("`unit`") && why.contains("substage") && why.contains("rank 7"),
        "{why}"
    );
    // A unit the chart does not know.
    let why = contacts(&pieces, json!({ "unit": "Crétacé" })).expect_err("unknown to the extract");
    assert!(why.contains("Crétacé"), "{why}");
}

#[test]
fn no_contact_of_the_unit_is_refused_by_its_name() {
    // The Lutetian from the top of the log to its bottom: named, two passes
    // and a limit between them; at the stage, no limit at all.
    let pieces = frame(
        Pieces::empty(),
        0,
        "F1",
        &units(&["Lutétien supérieur", "Lutétien"]),
    );
    assert!(
        contacts(&pieces, json!({})).is_ok(),
        "named, the limit between two passes"
    );
    let why = contacts(&pieces, json!({ "unit": "Lutétien" })).expect_err("no limit of the Lutetian");
    assert!(
        why.contains("`unit`")
            && why.contains("`Lutétien (stage)`")
            && why.contains("1 have an interval inside it"),
        "{why}"
    );
}

#[test]
fn uuids_are_stable_and_the_stage_is_not_the_pass_named_after_it() {
    let pieces = frame(
        Pieces::empty(),
        0,
        "F1",
        &units(&["Bartonien", "Lutétien", "Yprésien"]),
    );
    let once = contacts(&pieces, json!({ "unit": "Lutétien" })).expect("a plan");
    let again = contacts(&pieces, json!({ "unit": "Lutétien" })).expect("a plan");
    let all = |made: &Plan| {
        made.objects
            .iter()
            .map(|one| (one.title.clone(), one.uuid.clone()))
            .collect::<BTreeSet<_>>()
    };
    assert_eq!(all(&once), all(&again), "two runs, the same objects");

    // A rank alone reads the Lutetian the same way, and writes the same
    // objects for it.
    let whole = contacts(&pieces, json!({ "rank": 6 })).expect("a plan");
    for kind in [
        "resqml22.BoundaryFeature",
        "resqml22.HorizonInterpretation",
        "resqml22.PointSetRepresentation",
    ] {
        for title in ["Lutétien (stage) — top", "Lutétien (stage) — base"] {
            let one = uuid_of(&once, kind, title);
            assert!(one.is_some(), "{kind} `{title}` is written");
            assert_eq!(one, uuid_of(&whole, kind, title), "{kind} `{title}`");
        }
    }

    // Named, the pass “Lutétien” has its contacts at the same nodes — and
    // they are other objects: the base of a stage and the base of a pass
    // named after it are two findings, even where they coincide.
    let named = contacts(&pieces, json!({})).expect("a plan");
    assert_eq!(
        points_of(&named, "Lutétien — base"),
        points_of(&once, "Lutétien (stage) — base")
    );
    let features = |made: &Plan| {
        made.objects
            .iter()
            .filter(|one| one.qualified_type == "resqml22.BoundaryFeature")
            .map(|one| one.uuid.clone())
            .collect::<BTreeSet<_>>()
    };
    assert!(
        features(&named).is_disjoint(&features(&once)),
        "the stage's features are not the passes'"
    );
}

#[test]
fn the_base_of_the_quaternary_is_found_through_its_series_and_subseries() {
    // H1 of Lutetia: remblais dated Holocene and alluvium dated Upper
    // Pleistocene — a series and a sub-series — on the Lutetian.
    let pieces = frame(
        Pieces::empty(),
        0,
        "F1",
        &units(&[
            "Holocène",
            "Pléistocène supérieur",
            "Lutétien supérieur",
            "Lutétien",
        ]),
    );
    let made = contacts(&pieces, json!({ "unit": "Quaternaire" })).expect("a plan");
    assert_eq!(
        points_of(&made, "Quaternaire (system) — base"),
        Some(vec![node(0, 2)])
    );
    assert_eq!(
        points_of(&made, "Quaternaire (system) — top"),
        None,
        "the top of a log is no contact"
    );

    // At the system rank alone: the Quaternary's base is the Paleogene's
    // top, and nothing between the Holocene and the Upper Pleistocene, nor
    // inside the Lutetian.
    let whole = contacts(&pieces, json!({ "rank": 3 })).expect("a plan");
    assert_eq!(
        sets(&whole),
        vec!["Paléogène (system) — top", "Quaternaire (system) — base"]
    );
    assert_eq!(
        points_of(&whole, "Paléogène (system) — top"),
        Some(vec![node(0, 2)])
    );
}

#[test]
fn a_rank_alone_reads_every_division_of_it_and_a_regional_stage_as_its_reference_one() {
    // The Cuisien, a regional stage the register places under the Ypresian,
    // and the Sparnacien, a regional substage under it, are the Ypresian at
    // the stage level.
    let pieces = frame(
        Pieces::empty(),
        0,
        "F1",
        &units(&[
            "Bartonien",
            "Lutétien supérieur",
            "Lutétien moyen",
            "Cuisien",
            "Sparnacien",
        ]),
    );
    let made = contacts(&pieces, json!({ "rank": 6 })).expect("a plan");
    assert_eq!(
        sets(&made),
        vec![
            "Bartonien (stage) — base",
            "Lutétien (stage) — base",
            "Lutétien (stage) — top",
            "Yprésien (stage) — top"
        ]
    );
    assert_eq!(
        points_of(&made, "Lutétien (stage) — base"),
        Some(vec![node(0, 3)])
    );
    assert_eq!(points_of(&made, "Yprésien (stage) — top"), Some(vec![node(0, 3)]));
    // Neither the Lutetian's inner limit (node 2) nor the Cuisien's base on
    // the Sparnacien (node 4): both are inside one stage.
    for inner in [node(0, 2), node(0, 4)] {
        assert!(
            all_points(&made).iter().all(|point| *point != inner),
            "{inner:?} is inside a stage"
        );
    }
}

#[test]
fn a_unit_the_chart_does_not_place_is_read_as_none_and_named_even_under_a_chart_title() {
    // A local unit of the frame's own column, **titled like a division of the
    // chart** but without its uuid: a title is not an identity, so it is no
    // Bartonian, and the Lutetian has no top under it.
    let mut named = units(&["Bartonien", "Lutétien", "Yprésien"]);
    named[0] = Some(Ref::new(
        "0f0f0f0f-0000-4000-8000-000000000001",
        UNIT,
        "Bartonien",
    ));
    let pieces = frame(Pieces::empty(), 0, "F1", &named);
    let made = contacts(&pieces, json!({ "unit": "Lutétien" })).expect("a plan");
    assert_eq!(
        points_of(&made, "Lutétien (stage) — top"),
        None,
        "no contact with what the chart cannot place"
    );
    assert_eq!(
        points_of(&made, "Lutétien (stage) — base"),
        Some(vec![node(0, 2)])
    );
    let foreign = note(&made, "rank-foreign");
    assert!(
        foreign.starts_with("1 interval(s)") && foreign.contains("Bartonien × 1"),
        "{foreign}"
    );
}

// -- the chart's answer, `energyml_domain::geolts_rank` ------------------------

#[test]
fn the_chart_says_inside_by_its_hierarchy_and_outside_by_its_ages() {
    extract();
    geolts::with_default(|chart| {
        use geolts_rank::{at_level, relation, span};

        assert_eq!(
            relation(chart, 1258, 1255),
            Relation::Inside,
            "the Upper Lutetian is in the Lutetian"
        );
        assert_eq!(
            relation(chart, 1326, 1289),
            Relation::Inside,
            "the Holocene is in the Quaternary"
        );
        assert_eq!(
            relation(chart, 1316, 1289),
            Relation::Inside,
            "the Upper Pleistocene, two levels down"
        );
        // Touching spans are outside each other: the Cuisien ends at 47.8 Ma,
        // where the Lutetian begins; the Bartonian begins where it ends.
        assert_eq!(relation(chart, 2174, 1255), Relation::Outside);
        assert_eq!(relation(chart, 1259, 1255), Relation::Outside);
        // The Upper Lutetian's young end is `e1258`, no number: its span is
        // the Lutetian's, and it is still outside the Quaternary.
        assert_eq!(span(chart, 1258), Some((47.8, 41.2)));
        assert_eq!(relation(chart, 1258, 1289), Relation::Outside);
        assert_eq!(
            relation(chart, 1254, 1255),
            Relation::Holds,
            "the Middle Eocene holds the Lutetian"
        );
        // **Outside is the ages' answer, never the tree's**: the Ludien sits
        // beside the Priabonien under the Upper Eocene, and spans the same
        // 37.8–33.9 Ma. A tree-only reading would say outside, and draw a
        // contact between two names for one time.
        assert_eq!(relation(chart, 2183, 1263), Relation::Overlaps);

        assert_eq!(
            at_level(chart, 2174, 7),
            vec![1251],
            "the Cuisien is the Ypresian at the stage"
        );
        assert_eq!(at_level(chart, 2173, 7), vec![1251], "and so is the Sparnacien");
        assert_eq!(
            at_level(chart, 1254, 7),
            Vec::<u32>::new(),
            "the Middle Eocene has no stage"
        );
        assert_eq!(
            at_level(chart, 1326, 4),
            vec![1289],
            "the Holocene is the Quaternary at the system"
        );
    });
}
