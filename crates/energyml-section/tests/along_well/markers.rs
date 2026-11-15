//! `along_well::markers`: the ticks of markers and the dip they show in the plane of the section.
//!
//! WITSML TRG 2.1: a `WellboreMarker` states `DipAngle`, `DipDirection` and `DipDirectionRef`; the apparent
//! dip in a vertical plane is `tan(apparent) = tan(dip) cos(direction - azimuth)`.

use std::f64::consts::FRAC_PI_2;

use energyml_geometry::around_well::projection::{Developed, MdTvd};
use energyml_section::along_well::{Dip, DipReference, MarkerNote, WellMarker, marker_ticks};

use super::{axis_of, drain_east, drain_north};

fn marker(reference: DipReference) -> WellMarker {
    WellMarker {
        uri: "eml:///witsml21.WellboreMarker(c0000000-0000-4000-8000-0000000000aa)".to_string(),
        md: 1500.0,
        dip: Some(Dip {
            angle: 30.0_f64.to_radians(),
            direction: 0.0,
            reference,
        }),
    }
}

/// A dip of 30 degrees towards the north: a drain heading east sees it edge on (apparent dip 0), a drain
/// heading north sees all of it (30 degrees).
///
/// To break it: use `sin` where the rule has `cos` (the east drain sees 30 degrees, the north one 0).
#[test]
fn the_apparent_dip_follows_the_cosine_of_the_angle_to_the_hole() {
    let east = drain_east();
    let ticks = marker_ticks(
        &east,
        &axis_of(&east),
        &[marker(DipReference::GridNorth)],
        &Developed,
        None,
    );
    assert!(ticks[0].apparent_dip.unwrap().abs() < 1e-12);
    assert_eq!(ticks[0].note, None);

    let north = drain_north();
    let ticks = marker_ticks(
        &north,
        &axis_of(&north),
        &[marker(DipReference::GridNorth)],
        &Developed,
        None,
    );
    assert!((ticks[0].apparent_dip.unwrap() - 30.0_f64.to_radians()).abs() < 1e-12);
}

/// A dip against the true north, with no declared angle to grid north, is drawn level and said: the section
/// does not guess the convergence. Once the angle is declared the dip is read (here 90 degrees: the true
/// north is the grid's east, so the dip direction is east and the east drain sees all of it).
///
/// To break it: take the direction as it is (the tick has a dip, and the note is missing).
#[test]
fn a_true_north_dip_without_a_declared_angle_is_drawn_level_and_said() {
    let east = drain_east();
    let axis = axis_of(&east);
    let true_north = marker(DipReference::TrueNorth);
    let ticks = marker_ticks(&east, &axis, std::slice::from_ref(&true_north), &Developed, None);
    assert_eq!(ticks[0].apparent_dip, None);
    assert_eq!(ticks[0].note, Some(MarkerNote::NorthNotDeclared));

    let ticks = marker_ticks(&east, &axis, &[true_north], &Developed, Some(FRAC_PI_2));
    assert!((ticks[0].apparent_dip.unwrap() - 30.0_f64.to_radians()).abs() < 1e-12);
}

/// A marker with no dip is a tick drawn level, with the note; a depth the projection does not place is
/// never extrapolated: no point, the note says so.
///
/// To break it: place a marker beyond the end of the hole (it lands on the last point).
#[test]
fn a_marker_without_dip_or_off_the_axis_is_said() {
    let east = drain_east();
    let axis = axis_of(&east);
    let mut flat = marker(DipReference::GridNorth);
    flat.dip = None;
    let mut beyond = marker(DipReference::GridNorth);
    beyond.md = 5000.0;
    let ticks = marker_ticks(&east, &axis, &[flat, beyond], &Developed, None);
    assert_eq!(ticks[0].note, Some(MarkerNote::NoDip));
    assert_eq!(ticks[0].apparent_dip, None);
    assert_eq!(ticks[1].note, Some(MarkerNote::NotPlaced));
    assert!(ticks[1].at[0].is_nan());
}

/// Against the MD the slope of a dip is multiplied by `sin I` (the abscissa is a depth, not a length): the
/// note says so. On a horizontal drain `sin I = 1`, so the value is unchanged.
///
/// To break it: leave out the note.
#[test]
fn against_md_the_slope_is_said_to_be_bent() {
    let north = drain_north();
    let ticks = marker_ticks(
        &north,
        &axis_of(&north),
        &[marker(DipReference::GridNorth)],
        &MdTvd,
        None,
    );
    assert_eq!(ticks[0].note, Some(MarkerNote::SlopeAgainstMd));
    assert!((ticks[0].apparent_dip.unwrap() - 30.0_f64.to_radians()).abs() < 1e-12);
}
