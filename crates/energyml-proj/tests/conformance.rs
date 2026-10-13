//! Does this backend agree with PROJ?
//!
//! The reference values below were produced by **pyproj 3.7.2 / PROJ 9.5.1**,
//! handed the *same proj4 string* this crate uses, so what is compared is the
//! arithmetic and not the choice of transformation. The generating script is in
//! the reference-CRS script (pyproj); rerunning it is how you refresh the table.
//!
//! Why bother, when there is a perfectly good test suite inside proj4rs? Because
//! the thing that went wrong here was never the arithmetic. It was a table of
//! constants, silently one version behind, and no unit test inside the geodesy
//! crate could have caught it. This file is the only thing in the workspace that
//! would notice if the datum shifts went missing again: it fails by **151 m**,
//! not by a compile error.
//!
//! `pyproj` is deliberately *not* a dependency. It is an oracle, consulted once,
//! and its answers are frozen here as literals.

use energyml_domain::reproject::Reprojector;
use energyml_proj::{DatumState, Proj4, datum_state};

/// EPSG code, easting, northing, and PROJ 9.5.1's longitude and latitude.
const REFERENCE: &[(u64, f64, f64, f64, f64)] = &[
    (23031, 500000.0, 4650000.0, 2.998873735512756, 42.00020043060782),
    (23031, 300000.0, 4400000.0, 0.6653762599384297, 39.72463201546493),
    (23031, 700000.0, 4900000.0, 5.5027972939268315, 44.22397656502577),
    (32631, 500000.0, 4650000.0, 2.9999999999999916, 42.002015468347004),
    (2154, 650000.0, 6850000.0, 2.3199639026004952, 48.7481763008535),
    (2154, 300000.0, 6300000.0, -1.9630755947680587, 43.6858609528962),
    (27700, 400000.0, 300000.0, -2.0014331155108924, 52.59779243083379),
    (31370, 150000.0, 170000.0, 4.36875214090419, 50.840411283117874),
    (28992, 155000.0, 463000.0, 5.38720358055895, 52.15517230263769),
    (21781, 600000.0, 200000.0, 7.43863242087181, 46.9510827718711),
    (25831, 500000.0, 4650000.0, 2.9999999999999916, 42.00201546939624),
    (26910, 500000.0, 5000000.0, -123.00000000000001, 45.15347718443478),
    (3857, 333958.47, 5160979.44, 2.999999978621706, 41.999999972964545),
];

/// Degrees apart, converted to metres on the ground.
fn metres_apart(a: (f64, f64), b: (f64, f64)) -> f64 {
    let metres_per_degree = 111_320.0;
    let east = (a.0 - b.0) * metres_per_degree * a.1.to_radians().cos();
    let north = (a.1 - b.1) * metres_per_degree;
    east.hypot(north)
}

#[test]
fn every_reference_point_matches_proj_to_the_millimetre() {
    let backend = Proj4::new();
    let mut worst = (0.0f64, 0u64);
    for &(code, east, north, longitude, latitude) in REFERENCE {
        let out = backend
            .reproject([east, north, 0.0], code, 4326)
            .unwrap_or_else(|e| panic!("EPSG:{code}: {e}"));
        let offset = metres_apart((out[0], out[1]), (longitude, latitude));
        assert!(
            offset < 0.005,
            "EPSG:{code} at ({east}, {north}) is {:.4} mm from PROJ 9.5.1: \
             got ({}, {}), expected ({longitude}, {latitude})",
            offset * 1000.0,
            out[0],
            out[1],
        );
        if offset > worst.0 {
            worst = (offset, code);
        }
    }
    // Printed rather than asserted tightly: the useful thing on a future
    // failure is knowing whether the whole table drifted or one system did.
    println!(
        "worst agreement with PROJ 9.5.1: {:.4} mm on EPSG:{}",
        worst.0 * 1000.0,
        worst.1
    );
}

#[test]
fn the_corpus_crs_lands_where_the_datum_shift_puts_it() {
    // The single most valuable assertion in the crate. EPSG:23031 is the CRS of
    // every fesapi package in `testdata/`. Without `+towgs84` this point comes
    // out at exactly 3.0° — the central meridian, suspiciously round — and 151 m
    // from the truth. The roundness is the tell.
    let out = Proj4::new()
        .reproject([500000.0, 4650000.0, 0.0], 23031, 4326)
        .unwrap();
    let with_datum = metres_apart((out[0], out[1]), (2.998873735512756, 42.00020043060782));
    let without_datum = metres_apart((out[0], out[1]), (3.0, 42.00127347));

    assert!(with_datum < 0.005, "{with_datum} m from the reference");
    assert!(
        without_datum > 100.0,
        "only {without_datum:.1} m from the no-datum answer — the shift is not being applied"
    );
}

#[test]
fn every_reference_system_states_its_datum() {
    // If the built-in table ever regresses to the 0.4.0 content, this fails
    // first and says which system lost it.
    let backend = Proj4::new();
    for &(code, ..) in REFERENCE {
        let definition = backend.definition(code).unwrap();
        assert_eq!(
            datum_state(definition),
            DatumState::Stated,
            "EPSG:{code} lost its datum: `{definition}`",
        );
    }
}

#[test]
fn a_whole_array_converts_the_same_as_the_points_one_by_one() {
    // `reproject_all` builds the projections once and loops; `reproject` goes
    // through it with a single point. A divergence between the two would show
    // up as a surface that is right at one node and wrong at the next.
    let backend = Proj4::new();
    let points: Vec<[f64; 3]> = REFERENCE
        .iter()
        .filter(|(code, ..)| *code == 23031)
        .map(|&(_, east, north, ..)| [east, north, -500.0])
        .collect();

    let together = backend.reproject_all(&points, 23031, 4326).unwrap();
    for (point, bulk) in points.iter().zip(&together) {
        let single = backend.reproject(*point, 23031, 4326).unwrap();
        assert_eq!(&single, bulk);
    }
}

#[test]
fn web_mercator_is_reachable_and_round_trips() {
    // The other target every map wants, and the one whose definition uses
    // `+nadgrids=@null` — the spelling the datum check has to read as a
    // statement rather than as silence.
    let backend = Proj4::new();
    let mercator = backend
        .reproject([500000.0, 4650000.0, 0.0], 23031, 3857)
        .unwrap();
    let back = backend.reproject(mercator, 3857, 23031).unwrap();
    assert!((back[0] - 500000.0).abs() < 0.001, "{back:?}");
    assert!((back[1] - 4650000.0).abs() < 0.001, "{back:?}");
}
