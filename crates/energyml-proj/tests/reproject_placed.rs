//! `energyml_domain::reproject::reproject_placed`, exercised over two real
//! French systems - the pair from a bug report: a SIGEOL
//! package in EPSG:2154 (RGF93 / Lambert-93) and another producer's package in
//! EPSG:3949 (RGF93 / CC49), both meant to sit in the same corner of the
//! Paris basin.
//!
//! Two things worth knowing about this pair before reading the numbers:
//! neither system needs a datum shift to reach the other (both are RGF93 on
//! GRS80, `+towgs84=0,0,0,0,0,0,0` - see `crs-definitions`), so nothing here
//! exercises the datum-check this crate is otherwise built around; and both
//! are already in metres, so the unit-conversion half of `reproject_placed`
//! is the identity on every leg below. What this file pins is the other
//! half: that going through the backend and back returns to the start, and
//! that a direct hop agrees with going by way of WGS 84 - two roads to the
//! same place, which is the property a scene with a "keep the system while
//! the scene is non-empty" rule depends on.

use energyml_domain::reproject::{System, Wgs84, reproject_placed};
use energyml_proj::Proj4;

/// A point in the area both packages describe (from the bug report:
/// SIGEOL lon 2.12-2.73 / lat 48.73-49.01), in Lambert-93 metres, with a
/// depth that must survive every leg untouched.
const LAMBERT_93: [f64; 3] = [650_000.0, 6_860_000.0, -123.4];

fn metres() -> System {
    System {
        epsg: 2154,
        unit: "m".to_string(),
    }
}

fn cc49() -> System {
    System {
        epsg: 3949,
        unit: "m".to_string(),
    }
}

#[test]
fn round_trip_2154_to_3949_and_back_is_within_a_millimetre() {
    let backend = Proj4::new();
    let there = reproject_placed(&[LAMBERT_93], &metres(), &cc49(), &backend).unwrap();
    let back = reproject_placed(&there, &cc49(), &metres(), &backend).unwrap();

    let dx = (back[0][0] - LAMBERT_93[0]).abs();
    let dy = (back[0][1] - LAMBERT_93[1]).abs();
    assert!(dx < 0.001, "{dx} m of easting drift");
    assert!(dy < 0.001, "{dy} m of northing drift");
    // The vertical is not merely close: nothing horizontal touches it.
    assert_eq!(back[0][2], LAMBERT_93[2]);
}

#[test]
fn the_direct_hop_agrees_with_going_by_way_of_wgs_84() {
    let backend = Proj4::new();

    let direct = reproject_placed(&[LAMBERT_93], &metres(), &cc49(), &backend).unwrap();

    // WGS 84 is geographic, not projected - its horizontal coordinates are
    // degrees, not a length `units::metres` can convert. Labelling it `"m"`
    // here keeps `reproject_placed`'s own unit step an identity on both legs
    // that touch it, so what is actually being compared is the backend's
    // reprojection chain, exactly as a WebAssembly front end's `wgs84_of` /
    // `from_wgs84` already trust `energyml_proj::Proj4` to do.
    let wgs84 = System {
        epsg: Wgs84::EPSG,
        unit: "m".to_string(),
    };
    let via_wgs84_step1 = reproject_placed(&[LAMBERT_93], &metres(), &wgs84, &backend).unwrap();
    let via_wgs84 = reproject_placed(&via_wgs84_step1, &wgs84, &cc49(), &backend).unwrap();

    let dx = (direct[0][0] - via_wgs84[0][0]).abs();
    let dy = (direct[0][1] - via_wgs84[0][1]).abs();
    assert!(dx < 0.001, "the two roads disagree by {dx} m of easting");
    assert!(dy < 0.001, "the two roads disagree by {dy} m of northing");
}
