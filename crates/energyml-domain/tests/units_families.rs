//! The unit families added for resistivity, conductivity and dogleg severity,
//! and the reciprocal between the first two.
//!
//! The strings come from the generated `*Uom` enumerations (identical in eml
//! 2.0 to 2.3); the factors come from the SI prefixes and the exact
//! international foot. Every expectation below is a definition, not a value
//! read from a file.

use energyml_core::reflect::EnergymlType;
use energyml_domain::Error;
use energyml_domain::property::PropertyKindRef;
use energyml_domain::property_kind::{self, Dictionary, KindIssue, Resolver};
use energyml_domain::source::NoObjects;
use energyml_domain::units::{self, Dimension, convert, resistivity_reciprocal};

fn close(a: f64, b: f64, tolerance: f64) {
    assert!((a - b).abs() <= tolerance, "{a} is not within {tolerance} of {b}");
}

fn resolution_of(class: &str) -> (Dictionary, String) {
    (
        Dictionary::from_compact(&format!("aaaa\ta kind\t\t{class}\t0\t\n")),
        "aaaa".to_string(),
    )
}

fn issues(class: &str, uom: &str) -> Vec<KindIssue> {
    let (dictionary, uuid) = resolution_of(class);
    let resolver = Resolver::with_dictionary(&NoObjects, &dictionary);
    let resolution = resolver
        .resolve(&PropertyKindRef::Object { uuid, title: None })
        .expect("the kind resolves");
    property_kind::check(&resolution, Some(uom))
}

#[test]
fn every_resistivity_unit_of_the_schema_is_known() {
    // Triangulation: the eml 2.3 schema against the table written here.
    let resistivity = <energyml_eml_2_3::ElectricalResistivityUom as EnergymlType>::type_info();
    assert!(!resistivity.variants.is_empty());
    for variant in resistivity.variants {
        assert_eq!(
            Dimension::of(variant.value),
            Some(Dimension::ElectricalResistivity),
            "`{}` of ElectricalResistivityUom",
            variant.value
        );
    }
    let conductivity = <energyml_eml_2_3::ElectricConductivityUom as EnergymlType>::type_info();
    assert!(!conductivity.variants.is_empty());
    for variant in conductivity.variants {
        assert_eq!(
            Dimension::of(variant.value),
            Some(Dimension::ElectricConductivity),
            "`{}` of ElectricConductivityUom",
            variant.value
        );
    }
    let angle = <energyml_eml_2_3::AnglePerLengthUom as EnergymlType>::type_info();
    assert!(!angle.variants.is_empty());
    for variant in angle.variants {
        assert_eq!(
            Dimension::of(variant.value),
            Some(Dimension::AnglePerLength),
            "`{}` of AnglePerLengthUom",
            variant.value
        );
    }
}

#[test]
fn ohm_cm_converts_to_ohm_m() {
    // SI definition: 1 cm = 1e-2 m.
    close(convert(100.0, "ohm.cm", "ohm.m").unwrap(), 1.0, 1e-15);
}

#[test]
fn a_degree_per_thirty_metres_is_a_degree_over_thirty_metres() {
    // The schema string `1/30 dega/m` and the international foot, exact by the
    // 1959 agreement.
    close(convert(3.0, "1/30 dega/m", "dega/m").unwrap(), 0.1, 1e-12);
    // 0.01 dega/ft is a degree per 100 ft: 100 ft = 30.48 m, so a degree per
    // 30 m is 30.48 / 30 times as steep as a degree per 100 ft.
    close(
        convert(1.0, "0.01 dega/ft", "1/30 dega/m").unwrap(),
        30.0 / 30.48,
        1e-12,
    );
}

#[test]
fn a_resistivity_is_not_an_angle_per_length() {
    let refused = convert(1.0, "ohm.m", "rad/m");
    assert!(
        matches!(refused, Err(Error::MismatchedUnits { .. })),
        "{refused:?}"
    );
}

#[test]
fn the_reciprocal_of_ten_ohm_metres_is_a_hundred_millisiemens_per_metre() {
    // Energistics dictionary, kind `conductivity`: "In isotropic material, it
    // is the reciprocal of Resistivity". 1 / 10 ohm.m = 0.1 S/m = 100 mS/m.
    close(
        resistivity_reciprocal(10.0, "ohm.m", "mS/m").unwrap().unwrap(),
        100.0,
        1e-12,
    );
    close(
        resistivity_reciprocal(100.0, "mS/m", "ohm.m").unwrap().unwrap(),
        10.0,
        1e-12,
    );
}

#[test]
fn a_zero_resistivity_has_no_reciprocal() {
    // An absence, never an infinity.
    assert_eq!(resistivity_reciprocal(0.0, "ohm.m", "S/m").unwrap(), None);
    assert_eq!(resistivity_reciprocal(f64::NAN, "ohm.m", "S/m").unwrap(), None);
}

#[test]
fn the_reciprocal_refuses_two_resistivities() {
    // Not a conversion of unit (fiche 05 Q11): two resistivities are converted,
    // not inverted.
    let refused = resistivity_reciprocal(1.0, "ohm.m", "ohm.cm");
    assert!(
        matches!(refused, Err(Error::MismatchedUnits { .. })),
        "{refused:?}"
    );
}

#[test]
fn the_reciprocal_names_an_unknown_unit() {
    let refused = resistivity_reciprocal(1.0, "exoticOhm", "S/m");
    assert!(matches!(refused, Err(Error::UnknownUnit { .. })), "{refused:?}");
}

#[test]
fn ohm_m_is_checked_against_electrical_resistivity() {
    // RESQML TRG 2.2 section 7.6: the unit must be convertible into the kind's.
    assert!(issues("electrical resistivity", "ohm.m").is_empty());
    let found = issues("electrical resistivity", "S/m");
    assert!(
        matches!(
            found.as_slice(),
            [KindIssue::UomOutsideQuantityClass { found: Some(found), .. }] if found == "electric conductivity"
        ),
        "{found:?}"
    );
}

#[test]
fn every_class_name_is_a_class_of_the_dictionary() {
    // Triangulation: the Energistics dictionary shipped in the crate against
    // the spelling written in `class_name`.
    for dimension in Dimension::ALL {
        let present = property_kind::with_default(|dictionary| {
            dictionary
                .kinds()
                .iter()
                .any(|kind| kind.quantity_class.as_deref() == Some(dimension.class_name()))
        });
        assert!(
            present,
            "`{}` is no quantity class of the dictionary",
            dimension.class_name()
        );
    }
}

#[test]
fn the_dls_unit_of_a_dogleg_severity_is_checked() {
    // Dictionary kind `dogleg severity`, class `angle per length`.
    assert!(issues("angle per length", "1/30 dega/m").is_empty());
    assert!(issues("angle per length", "dega/ft").is_empty());
    assert_eq!(issues("angle per length", "m").len(), 1);
}

#[test]
fn the_base_of_every_family_is_in_that_family() {
    for dimension in Dimension::ALL {
        assert_eq!(Dimension::of(dimension.base()), Some(dimension));
        assert_eq!(units::factor(dimension.base()), Some(1.0));
    }
}
