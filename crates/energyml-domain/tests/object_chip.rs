//! The chip of an object named by a uri written as a value
//! (`source::object_chip`).
//!
//! The package is a test source holding one `eml23.Activity` built by hand; it
//! counts how often it is asked, so that "not looked up" is something the test
//! can see. The grammar of the uri is ETP v1.2 section 25.3.6 (a dataspace uri
//! names no object), cited, not read.

use std::cell::Cell;

use energyml_core::registry::DynObject;
use energyml_domain::source::{ObjectSource, Presence, UriRefusal, object_chip};
use energyml_eml_2_3 as eml23;

const HELD: &str = "00000000-0000-0000-0000-00000000a001";
const ABSENT: &str = "00000000-0000-0000-0000-00000000a002";

/// Holds one activity, and counts the questions it is asked.
struct Package {
    held: eml23::Activity,
    asked: Cell<usize>,
}

impl ObjectSource for Package {
    fn object(&self, uuid: &str) -> Option<Box<dyn DynObject>> {
        self.asked.set(self.asked.get() + 1);
        uuid.eq_ignore_ascii_case(HELD)
            .then(|| Box::new(self.held.clone()) as Box<dyn DynObject>)
    }
}

fn package() -> Package {
    Package {
        held: eml23::Activity {
            uuid: HELD.to_string(),
            citation: eml23::Citation {
                title: "a held activity".to_string(),
                ..Default::default()
            },
            ..Default::default()
        },
        asked: Cell::new(0),
    }
}

#[test]
fn a_uri_value_absent_from_the_package_is_a_missing_chip() {
    let package = package();
    let chip = object_chip(&format!("eml:///eml23.Activity({ABSENT})"), None, &package).expect("a chip");
    assert_eq!(chip.presence, Presence::NotInPackage);
    assert_eq!(chip.title, None);
    assert_eq!(chip.qualified_type.as_deref(), Some("eml23.Activity"));
}

#[test]
fn a_held_object_gives_its_title_and_type() {
    let package = package();
    let uri = format!("  eml:///eml23.Activity({HELD})  ");
    let chip = object_chip(&uri, None, &package).expect("a chip");
    assert_eq!(chip.presence, Presence::InPackage);
    // The title is the object's own field, not anything the uri says.
    assert_eq!(chip.title.as_deref(), Some(package.held.citation.title.as_str()));
    assert_eq!(chip.qualified_type.as_deref(), Some("eml23.Activity"));
    assert_eq!(chip.uri, uri.trim(), "the uri is kept trimmed, as written");
}

#[test]
fn another_dataspace_is_not_looked_up() {
    let package = package();
    let chip = object_chip(&format!("eml:///dataspace('x')/eml23.Activity({HELD})"), None, &package).expect("a chip");
    assert_eq!(
        chip.presence,
        Presence::OtherDataspace {
            dataspace: "x".to_string()
        }
    );
    assert_eq!(package.asked.get(), 0, "an object of another dataspace is not asked for");
    assert_eq!(chip.title, None);

    // The same uri is looked up when `here` is that dataspace.
    let chip = object_chip(
        &format!("eml:///dataspace('x')/eml23.Activity({HELD})"),
        Some("x"),
        &package,
    )
    .expect("a chip");
    assert_eq!(chip.presence, Presence::InPackage);
    assert_eq!(package.asked.get(), 1);
}

#[test]
fn a_dataspace_uri_is_not_an_object() {
    let package = package();
    assert!(matches!(
        object_chip("eml:///dataspace('x')", Some("x"), &package),
        Err(UriRefusal::NotAnObject { .. })
    ));
    assert!(matches!(
        object_chip("not a uri", None, &package),
        Err(UriRefusal::NotAnEtpUri { .. })
    ));
    assert_eq!(package.asked.get(), 0);
}
