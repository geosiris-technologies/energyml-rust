//! The extract of the BRGM chart that the stratigraphy tests install — shared
//! by `tests/stratigraphy.rs` and `tests/earth_model_from_stratigraphy.rs`.
//!
//! Every row copied from the BRGM export of 24/09/2026
//! (`https://data.geoscience.fr/ncl/GeolTs`, CC BY 4.0), the colour columns put
//! in red-green-blue order: the five divisions of
//! `energyml-domain/tests/geolts.rs`, and the chain above the Eocene with its
//! two sister series and the Precambrian, the chart's only supereonothem.

use std::sync::Once;

use energyml_domain::geolts::{self, Chart};

pub const CONCEPTS: &str = r#"@id,skos:prefLabel,skos:notation,<https://data.geoscience.fr/def/main#mnemo>,<https://data.geoscience.fr/def/main#typeChronostratAge>,<https://data.geoscience.fr/def/main#nature>,skos:broader,<http://dbpedia.org/ontology/rgbCoordinateRed>,<http://dbpedia.org/ontology/rgbCoordinateGreen>,<http://dbpedia.org/ontology/rgbCoordinateBlue>,time:hasBeginning,time:hasEnd,adms:status,dct:spatial
<https://data.geoscience.fr/ncl/GeolTs/1249>,'Eocene'@en|'Eocène'@fr,e4-7,EOCE,<https://data.geoscience.fr/ncl/DivChronoStrat/5>,<https://data.geoscience.fr/ncl/natelem/1>,<https://data.geoscience.fr/ncl/GeolTs/1242>,'253','180','108',<https://data.geoscience.fr/ncl/GeolTsBound/1249>,<https://data.geoscience.fr/ncl/GeolTsBound/1264>,<http://inspire.ec.europa.eu/registry/status/valid>,Monde
<https://data.geoscience.fr/ncl/GeolTs/1259>,'Bartonian'@en|'Bartonien'@fr,e6,BRTN,<https://data.geoscience.fr/ncl/DivChronoStrat/7>,<https://data.geoscience.fr/ncl/natelem/1>,<https://data.geoscience.fr/ncl/GeolTs/1254>|<https://data.geoscience.fr/ncl/GeolTs/1249>,'253','192','145',<https://data.geoscience.fr/ncl/GeolTsBound/1259>,<https://data.geoscience.fr/ncl/GeolTsBound/1263>,<http://inspire.ec.europa.eu/registry/status/valid>,Monde
<https://data.geoscience.fr/ncl/GeolTs/1262>,'Upper Eocene'@en|'Eocène supérieur'@fr,e7,EOCS,<https://data.geoscience.fr/ncl/DivChronoStrat/6>,<https://data.geoscience.fr/ncl/natelem/3>,<https://data.geoscience.fr/ncl/GeolTs/1249>,'253','180','108',<https://data.geoscience.fr/ncl/GeolTsBound/1263>,<https://data.geoscience.fr/ncl/GeolTsBound/1264>,<http://inspire.ec.europa.eu/registry/status/valid>,Monde
<https://data.geoscience.fr/ncl/GeolTs/1263>,'Priabonian'@en|'Priabonien'@fr,e7,PRIA,<https://data.geoscience.fr/ncl/DivChronoStrat/7>,<https://data.geoscience.fr/ncl/natelem/1>,<https://data.geoscience.fr/ncl/GeolTs/1262>|<https://data.geoscience.fr/ncl/GeolTs/1249>,'253','205','161',<https://data.geoscience.fr/ncl/GeolTsBound/1263>,<https://data.geoscience.fr/ncl/GeolTsBound/1264>,<http://inspire.ec.europa.eu/registry/status/valid>,Monde
<https://data.geoscience.fr/ncl/GeolTs/2183>,'Ludian'@en|'Ludien'@fr,e7,LUDI,<https://data.geoscience.fr/ncl/DivChronoStrat/7>,<https://data.geoscience.fr/ncl/natelem/2>,<https://data.geoscience.fr/ncl/GeolTs/1262>,'253','205','161',<https://data.geoscience.fr/ncl/GeolTsBound/1263>,<https://data.geoscience.fr/ncl/GeolTsBound/1264>,<http://inspire.ec.europa.eu/registry/status/valid>,Bassin de Paris
<https://data.geoscience.fr/ncl/GeolTs/1001>,'Precambrian'@en|'Précambrien'@fr,Pk|PꞒ,PCMB,<https://data.geoscience.fr/ncl/DivChronoStrat/1>,<https://data.geoscience.fr/ncl/natelem/5>,,'247','67','112',<https://data.geoscience.fr/ncl/GeolTsBound/1001>,<https://data.geoscience.fr/ncl/GeolTsBound/1028>,<http://inspire.ec.europa.eu/registry/status/valid>,Monde
<https://data.geoscience.fr/ncl/GeolTs/1028>,'Phanerozoic'@en|'Phanérozoïque'@fr,ph,PHAN,<https://data.geoscience.fr/ncl/DivChronoStrat/2>,<https://data.geoscience.fr/ncl/natelem/1>,,'154','217','221',<https://data.geoscience.fr/ncl/GeolTsBound/1028>,<https://data.geoscience.fr/ncl/GeolTsBound/0>,<http://inspire.ec.europa.eu/registry/status/valid>,Monde
<https://data.geoscience.fr/ncl/GeolTs/1241>,'Cenozoic'@en|'Cénozoïque'@fr,cz,CENZ,<https://data.geoscience.fr/ncl/DivChronoStrat/3>,<https://data.geoscience.fr/ncl/natelem/1>,<https://data.geoscience.fr/ncl/GeolTs/1028>,'242','249','29',<https://data.geoscience.fr/ncl/GeolTsBound/1241>,<https://data.geoscience.fr/ncl/GeolTsBound/0>,<http://inspire.ec.europa.eu/registry/status/valid>,Monde
<https://data.geoscience.fr/ncl/GeolTs/1242>,'Paleogene'@en|'Paléogène'@fr,e-g|Pg,PLGN,<https://data.geoscience.fr/ncl/DivChronoStrat/4>,<https://data.geoscience.fr/ncl/natelem/1>,<https://data.geoscience.fr/ncl/GeolTs/1241>,'253','154','82',<https://data.geoscience.fr/ncl/GeolTsBound/1241>,<https://data.geoscience.fr/ncl/GeolTsBound/1269>,<http://inspire.ec.europa.eu/registry/status/valid>,Monde
<https://data.geoscience.fr/ncl/GeolTs/1243>,'Paleocene'@en|'Paléocène'@fr,e1-3,PCNE,<https://data.geoscience.fr/ncl/DivChronoStrat/5>,<https://data.geoscience.fr/ncl/natelem/1>,<https://data.geoscience.fr/ncl/GeolTs/1242>,'253','167','95',<https://data.geoscience.fr/ncl/GeolTsBound/1241>,<https://data.geoscience.fr/ncl/GeolTsBound/1249>,<http://inspire.ec.europa.eu/registry/status/valid>,Monde
<https://data.geoscience.fr/ncl/GeolTs/1264>,'Oligocene'@en|'Oligocène'@fr,g,OLIG,<https://data.geoscience.fr/ncl/DivChronoStrat/5>,<https://data.geoscience.fr/ncl/natelem/1>,<https://data.geoscience.fr/ncl/GeolTs/1242>,'253','192','122',<https://data.geoscience.fr/ncl/GeolTsBound/1264>,<https://data.geoscience.fr/ncl/GeolTsBound/1269>,<http://inspire.ec.europa.eu/registry/status/valid>,Monde
"#;

pub const BOUNDS: &str = r#"@id,time:numericPosition
<https://data.geoscience.fr/ncl/GeolTsBound/0>,'0'
<https://data.geoscience.fr/ncl/GeolTsBound/1001>,'4600'
<https://data.geoscience.fr/ncl/GeolTsBound/1028>,'541'
<https://data.geoscience.fr/ncl/GeolTsBound/1241>,'66'
<https://data.geoscience.fr/ncl/GeolTsBound/1249>,'56'
<https://data.geoscience.fr/ncl/GeolTsBound/1259>,"41,2"
<https://data.geoscience.fr/ncl/GeolTsBound/1263>,"37,8"
<https://data.geoscience.fr/ncl/GeolTsBound/1264>,"33,9"
<https://data.geoscience.fr/ncl/GeolTsBound/1269>,"23,03"
"#;

static CHART: Once = Once::new();

/// The extract, as the chart every operation of this binary reads.
pub fn extract() {
    CHART.call_once(|| geolts::set_default(Chart::from_csv(CONCEPTS, Some(BOUNDS)).expect("the extract reads")));
}

