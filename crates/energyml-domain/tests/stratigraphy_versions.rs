//! A stratigraphic unit **at a version** - the column names its units by uri.
//!
//! Internally, full uris only (ADR-0003). A rank may list one unit twice, in two `objectVersion`s (the
//! identity of a held entry is (uuid, type, version)); `Named::uri` is the
//! identity of the object a reference names, the version included, so a view
//! opens the right one and an `unconformable` side names the version it is
//! about - not the uuid, which both versions share.
//!
//! Built in memory from the typed model: the rule tested is the project's, not
//! a corpus's. What would have to break: `Named` resolving a reference by uuid
//! (both entries would carry the last version's title and uri), or an
//! `Unconformable` side spelt with the uuid.

use energyml_core::value::Reflect;
use energyml_domain::stratigraphy::{self, Named};
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_2 as resqml22;

use crate::inherit::{citation, u};

const UNIT: u32 = 1;
const OTHER: u32 = 2;
const RANK: u32 = 3;
const HORIZON: u32 = 4;
const TYPE: &str = "resqml22.StratigraphicUnitInterpretation";

fn unit(uuid: u32, version: &str, title: &str) -> resqml22::StratigraphicUnitInterpretation {
    resqml22::StratigraphicUnitInterpretation {
        uuid: u(uuid),
        schema_version: "2.2".into(),
        object_version: Some(version.to_string()),
        citation: citation(title),
        ..Default::default()
    }
}

fn reference(uuid: u32, version: &str) -> eml23::DataObjectReference {
    eml23::DataObjectReference {
        uuid: u(uuid),
        object_version: Some(version.to_string()),
        qualified_type: TYPE.to_string(),
        title: "as the reference spells it".to_string(),
        ..Default::default()
    }
}

fn element(uuid: u32, version: &str, mode: Option<resqml22::ContactMode>) -> resqml22::ContactElement {
    resqml22::ContactElement {
        uuid: u(uuid),
        object_version: Some(version.to_string()),
        qualified_type: TYPE.to_string(),
        title: "as the reference spells it".to_string(),
        secondary_qualifier: mode,
        ..Default::default()
    }
}

/// The rank lists the unit at version 1, at version 2 and another unit; a contact
/// says the version 1 side is unconformable.
fn rank() -> resqml22::StratigraphicColumnRankInterpretation {
    resqml22::StratigraphicColumnRankInterpretation {
        uuid: u(RANK),
        schema_version: "2.2".into(),
        citation: citation("the rank"),
        stratigraphic_units: vec![reference(UNIT, "1"), reference(UNIT, "2"), reference(OTHER, "1")],
        contact_interpretation: vec![
            resqml22::AbstractContactInterpretationPart::BinaryContactInterpretationPart(
                resqml22::BinaryContactInterpretationPart {
                    part_of: Some(eml23::DataObjectReference {
                        uuid: u(HORIZON),
                        qualified_type: "resqml22.HorizonInterpretation".to_string(),
                        title: "a horizon".to_string(),
                        ..Default::default()
                    }),
                    subject: element(UNIT, "1", Some(resqml22::ContactMode::Unconformable)),
                    direct_object: element(OTHER, "1", None),
                    verb: Default::default(),
                },
            ),
        ],
        ..Default::default()
    }
}

fn columns() -> stratigraphy::Columns {
    let (v1, v2, other) = (
        unit(UNIT, "1", "unit, first version"),
        unit(UNIT, "2", "unit, second version"),
        unit(OTHER, "1", "other"),
    );
    let rank = rank();
    let objects: Vec<&dyn Reflect> = vec![&v1, &v2, &other, &rank];
    stratigraphy::columns(objects.into_iter())
}

fn uri(version: &str, uuid: u32) -> String {
    format!("eml:///{TYPE}(uuid={},version='{version}')", u(uuid))
}

/// Each entry of the rank is the object **its reference names**: the title is
/// the target's, and the uri carries the version the target declares.
#[test]
fn a_reference_is_the_object_at_the_version_it_names() {
    let all = columns();
    let rank = all
        .loose_ranks
        .first()
        .expect("the rank is listed by no column, and returned apart");
    let seen: Vec<(String, String)> = rank
        .units
        .iter()
        .map(|one| (one.unit.title.clone(), one.unit.uri.clone()))
        .collect();
    assert_eq!(
        seen,
        vec![
            ("unit, first version".to_string(), uri("1", UNIT)),
            ("unit, second version".to_string(), uri("2", UNIT)),
            ("other".to_string(), uri("1", OTHER)),
        ],
        "two versions of one uuid are two units, with two titles and two uris"
    );
    assert!(rank.units.iter().all(|one| one.resolved), "the three are held");
    assert_eq!(
        rank.units[0].unit.uuid, rank.units[1].unit.uuid,
        "one uuid, two uris"
    );
}

/// The side the file says is not conformable is **the version it names**.
#[test]
fn an_unconformable_side_is_the_uri_of_its_unit() {
    let all = columns();
    let rank = &all.loose_ranks[0];
    let contact = rank.contacts.first().expect("the contact is read");
    assert_eq!(contact.subject.uri, uri("1", UNIT));
    let sides: Vec<&str> = contact
        .unconformable
        .iter()
        .map(|side| side.unit.as_str())
        .collect();
    assert_eq!(
        sides,
        vec![uri("1", UNIT)],
        "version 1 is the unconformable one (the one NOT held last), version 2 is not named"
    );
}

/// A uuid held **once** answers whatever version a reference spells, as the
/// store's lookup does - the checker reports the mismatch, a name does not refuse.
#[test]
fn a_wrong_version_of_a_uuid_held_once_still_names_the_held_object() {
    let (v1, other) = (unit(UNIT, "1", "unit, first version"), unit(OTHER, "1", "other"));
    let mut rank = rank();
    rank.stratigraphic_units = vec![reference(UNIT, "1"), reference(OTHER, "7")];
    rank.contact_interpretation.clear();
    let objects: Vec<&dyn Reflect> = vec![&v1, &other, &rank];
    let all = stratigraphy::columns(objects.into_iter());
    let units: Vec<&Named> = all.loose_ranks[0].units.iter().map(|one| &one.unit).collect();
    // Version 7 of `other` is not held, but the uuid is held once: the store's
    // lookup does the same ("a wrong version is not a wrong type"), and the
    // checker reports the mismatch.
    assert_eq!(units[0].uri, uri("1", UNIT));
    assert_eq!(
        units[1].uri,
        uri("1", OTHER),
        "the held object's own identity, not what the reference spells"
    );
}
