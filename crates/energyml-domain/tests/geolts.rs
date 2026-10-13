//! The BRGM chronostratigraphic chart (`energyml_domain::geolts`).
//!
//! **An extract of the register, written here** — five concepts and their
//! bounds, copied from the export `https://data.geoscience.fr/ncl/GeolTs?_format=csv`
//! and `…/GeolTsBound?_format=csv` on 24/09/2026 (BRGM, CC BY 4.0), reduced to
//! the columns the reader uses. The house rule: a reference document is cited,
//! never read by a test — the snapshot under `data/geolts/` is what the build
//! embeds, not what these assertions rest on.

use energyml_domain::geolts::Chart;

/// Five concepts: the Eocene, its upper sub-series, two of its stages and a
/// regional synonym. Verbatim rows, the columns the reader uses.
const CONCEPTS: &str = r#"@id,skos:prefLabel,skos:notation,<https://data.geoscience.fr/def/main#mnemo>,<https://data.geoscience.fr/def/main#typeChronostratAge>,<https://data.geoscience.fr/def/main#nature>,skos:broader,<http://dbpedia.org/ontology/rgbCoordinateRed>,<http://dbpedia.org/ontology/rgbCoordinateGreen>,<http://dbpedia.org/ontology/rgbCoordinateBlue>,time:hasBeginning,time:hasEnd,adms:status,dct:spatial
<https://data.geoscience.fr/ncl/GeolTs/1249>,'Eocene'@en|'Eocène'@fr,e4-7,EOCE,<https://data.geoscience.fr/ncl/DivChronoStrat/5>,<https://data.geoscience.fr/ncl/natelem/1>,<https://data.geoscience.fr/ncl/GeolTs/1242>,'253','180','108',<https://data.geoscience.fr/ncl/GeolTsBound/1249>,<https://data.geoscience.fr/ncl/GeolTsBound/1264>,<http://inspire.ec.europa.eu/registry/status/valid>,Monde
<https://data.geoscience.fr/ncl/GeolTs/1259>,'Bartonian'@en|'Bartonien'@fr,e6,BRTN,<https://data.geoscience.fr/ncl/DivChronoStrat/7>,<https://data.geoscience.fr/ncl/natelem/1>,<https://data.geoscience.fr/ncl/GeolTs/1254>|<https://data.geoscience.fr/ncl/GeolTs/1249>,'253','192','145',<https://data.geoscience.fr/ncl/GeolTsBound/1259>,<https://data.geoscience.fr/ncl/GeolTsBound/1263>,<http://inspire.ec.europa.eu/registry/status/valid>,Monde
<https://data.geoscience.fr/ncl/GeolTs/1262>,'Upper Eocene'@en|'Eocène supérieur'@fr,e7,EOCS,<https://data.geoscience.fr/ncl/DivChronoStrat/6>,<https://data.geoscience.fr/ncl/natelem/3>,<https://data.geoscience.fr/ncl/GeolTs/1249>,'253','180','108',<https://data.geoscience.fr/ncl/GeolTsBound/1263>,<https://data.geoscience.fr/ncl/GeolTsBound/1264>,<http://inspire.ec.europa.eu/registry/status/valid>,Monde
<https://data.geoscience.fr/ncl/GeolTs/1263>,'Priabonian'@en|'Priabonien'@fr,e7,PRIA,<https://data.geoscience.fr/ncl/DivChronoStrat/7>,<https://data.geoscience.fr/ncl/natelem/1>,<https://data.geoscience.fr/ncl/GeolTs/1262>|<https://data.geoscience.fr/ncl/GeolTs/1249>,'253','205','161',<https://data.geoscience.fr/ncl/GeolTsBound/1263>,<https://data.geoscience.fr/ncl/GeolTsBound/1264>,<http://inspire.ec.europa.eu/registry/status/valid>,Monde
<https://data.geoscience.fr/ncl/GeolTs/2183>,'Ludian'@en|'Ludien'@fr,e7,LUDI,<https://data.geoscience.fr/ncl/DivChronoStrat/7>,<https://data.geoscience.fr/ncl/natelem/2>,<https://data.geoscience.fr/ncl/GeolTs/1262>,'253','205','161',<https://data.geoscience.fr/ncl/GeolTsBound/1263>,<https://data.geoscience.fr/ncl/GeolTsBound/1264>,<http://inspire.ec.europa.eu/registry/status/valid>,Bassin de Paris
"#;

/// Their bounds, verbatim — note the **decimal comma** the export writes.
const BOUNDS: &str = r#"@id,time:numericPosition
<https://data.geoscience.fr/ncl/GeolTsBound/1249>,'56'
<https://data.geoscience.fr/ncl/GeolTsBound/1259>,"41,2"
<https://data.geoscience.fr/ncl/GeolTsBound/1263>,"37,8"
<https://data.geoscience.fr/ncl/GeolTsBound/1264>,"33,9"
"#;

fn chart() -> Chart {
    Chart::from_csv(CONCEPTS, Some(BOUNDS)).expect("the extract reads")
}

#[test]
fn a_concept_reads_with_its_labels_codes_colour_and_ages() {
    let chart = chart();
    assert_eq!(chart.len(), 5);
    let priabonian = chart.get(1263).expect("the Priabonien");
    assert_eq!(priabonian.label, "Priabonien");
    assert_eq!(priabonian.label_en, "Priabonian");
    assert_eq!(priabonian.notations, vec!["e7".to_string()]);
    assert_eq!(priabonian.mnemonic, "PRIA");
    assert_eq!(priabonian.level, 7, "a stage");
    assert_eq!(priabonian.colour, Some([253, 205, 161]));
    assert_eq!(priabonian.uri(), "https://data.geoscience.fr/ncl/GeolTs/1263");
    // **The decimal comma**: `37,8` is 37.8 Ma. Read as a point number only,
    // the age would be dropped and the stage would lose its place.
    assert_eq!(priabonian.begins_ma, Some(37.8));
    assert_eq!(priabonian.ends_ma, Some(33.9));
}

#[test]
fn the_parent_is_the_deepest_broader_above() {
    // The Priabonien names the Upper Eocene (sub-series, 6) **and** the
    // Eocene (series, 5). A tree has one parent: the nearer one.
    let chart = chart();
    assert_eq!(chart.parent(1263).map(|one| one.id), Some(1262));
    assert_eq!(chart.parent(1262).map(|one| one.id), Some(1249));
    assert_eq!(chart.ancestors(1263), vec![1262, 1249]);
    // The Bartonien's nearer parent (the Middle Eocene, 1254) is not in the
    // extract, so the Eocene is what remains — never nothing.
    assert_eq!(chart.parent(1259).map(|one| one.id), Some(1249));
}

#[test]
fn a_branch_is_the_concept_and_everything_below_it() {
    let chart = chart();
    let branch = chart.descendants(1249);
    assert_eq!(branch[0], 1249, "the concept first");
    for id in [1259, 1262, 1263, 2183] {
        assert!(branch.contains(&id), "{id} is under the Eocene: {branch:?}");
    }
    assert_eq!(chart.descendants(1262).len(), 3, "the Upper Eocene, the Priabonien and the Ludien");
}

#[test]
fn a_column_lists_youngest_first() {
    // RESQML's `age` criterion: *"from youngest to oldest period"* (TRG 2.2
    // §6.34). The Priabonien (37.8–33.9) sits above the Bartonien (41.2–37.8).
    let chart = chart();
    let order: Vec<u32> = chart.by_age(&[1259, 1263]).expect("both dated").iter().map(|one| one.id).collect();
    assert_eq!(order, vec![1263, 1259]);
}

#[test]
fn a_concept_without_ages_is_not_placed_by_a_guess() {
    let chart = Chart::from_csv(CONCEPTS, None).expect("reads without bounds");
    let why = chart.by_age(&[1259, 1263]).expect_err("no age, no order");
    assert!(why.contains("Bartonien") && why.contains("GeolTs/1259"), "{why}");
}

#[test]
fn a_code_needs_its_label_to_name_one_concept() {
    // Measured on a partner borehole corpus: `STRA` = `e7`, `Nom_STRA` = `Priabonien`.
    // The code alone names three concepts here (four in the whole chart).
    let chart = chart();
    assert_eq!(chart.find("e7", "Priabonien").expect("one").map(|one| one.id), Some(1263));
    assert_eq!(chart.find("E7", "priabonian").expect("one").map(|one| one.id), Some(1263), "case and English");
    // A pair the chart does not know is a local unit, not an error.
    assert_eq!(chart.find("n.d.", "No data").expect("no match is an answer"), None);
    assert!(chart.resolve("e7").expect_err("ambiguous").contains("give the id"));
    assert_eq!(chart.resolve("Ludien").expect("unique").id, 2183);
    assert_eq!(chart.resolve("GeolTs/1259").expect("by id").id, 1259);
}

#[test]
fn a_reference_division_is_told_from_a_regional_one() {
    let chart = chart();
    assert!(chart.get(1263).expect("Priabonien").is_reference());
    assert!(!chart.get(2183).expect("Ludien").is_reference(), "regional: Bassin de Paris");
}
