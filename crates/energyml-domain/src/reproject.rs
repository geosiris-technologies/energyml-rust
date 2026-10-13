//! Going from one projected system to another.
//!
//! # Why this is a seam and not an implementation
//!
//! Reprojecting between EPSG systems needs a geodesy library: the projection
//! formulae, the ellipsoid parameters, and - for anything crossing datums -
//! the transformation grids. That is a large, versioned, licensed body of data.
//! Two facts decide the shape:
//!
//! - **wasm is a hard requirement here.** CI checks that `energyml-core`, the
//!   generated crates, the storage layer, the domain layer and the geometry
//!   layer build for `wasm32-unknown-unknown`. That rules out bindings to the
//!   PROJ C library outright, however convenient.
//! - **Approximating is worse than refusing.** A model placed with the wrong
//!   datum lands hundreds of metres from where it belongs, and every coordinate
//!   still looks entirely reasonable.
//!
//! So this crate defines the trait and implements nothing. The layers above go
//! through [`Reprojector`] without caring what is behind it - the same
//! arrangement as `ArrayBackend` for HDF5, where the capability is optional and
//! its absence is *stated* rather than silently producing unmoved coordinates.
//!
//! **`energyml-proj` is the implementation this workspace ships**, over
//! `proj4rs`; a caller that wants PROJ, a service, or its own tables writes
//! another. Nothing here depends on it, on purpose.
//!
//! # If you write your own backend, read this first
//!
//! **Do not trust the bundled EPSG table for anything that crosses a datum.**
//! proj4rs 0.1.10 pins `crs-definitions` **0.4.0**, and that release carries no
//! Helmert parameters. Of nine systems checked, 0.4.0 documents `+towgs84` for
//! exactly one - EPSG:32631, which is already on WGS 84 and needs no shift.
//! 0.5.0 carries them for all nine.
//!
//! Measured on EPSG:23031 (ED50 / UTM 31N - the system every package in the
//! reference corpus uses), at `500000 E, 4650000 N`:
//!
//! | source definition | longitude | latitude |
//! |---|---|---|
//! | the 0.4.0 string (so also `from_epsg_code`) | 3.00000000 | 42.00127347 |
//! | with `+towgs84=-87,-98,-121,0,0,0,0` | 2.99887374 | 42.00020043 |
//!
//! **151 m apart** - 93 m east, 119 m north.
//!
//! `Proj::from_epsg_code` itself is fine: it calls `crs_definitions::from_code`
//! and hands the string straight to `from_proj_string`, losing nothing. It is
//! only as good as the table behind it. So supply the proj4 definition
//! yourself - from `crs-definitions` 0.5+, from a current EPSG database, or
//! hard-coded for the systems of your site - and **check for `+towgs84` or
//! `+datum=` before converting**.
//!
//! One more instance of the failure this whole workspace is arranged against:
//! nothing errors, every coordinate looks reasonable, and the model sits a
//! block and a half from where it belongs. `energyml-proj` handles it by
//! refusing a definition that states no datum; a backend of your own should do
//! something equivalent.
//!
//! ```no_run
//! use energyml_domain::crs::Crs;
//! use energyml_domain::reproject::{self, Reprojector, Wgs84};
//! # struct Proj4;
//! # impl Reprojector for Proj4 {
//! #     fn reproject(&self, p: [f64; 3], _f: u64, _t: u64) -> energyml_domain::Result<[f64; 3]> { Ok(p) }
//! # }
//! # fn run(crs: &Crs, point: [f64; 3]) -> energyml_domain::Result<()> {
//! let backend = Proj4;
//! // Local placement, unit normalisation and the datum work, in one call.
//! let lonlat = reproject::to_epsg(crs, point, Wgs84::EPSG, &backend)?;
//! # Ok(())
//! # }
//! ```

use crate::crs::{Crs, Point};
use crate::error::{Error, Result};
use crate::units;

/// WGS 84, the system almost every web map wants.
pub struct Wgs84;

impl Wgs84 {
    /// Geographic WGS 84 - longitude and latitude in degrees.
    pub const EPSG: u64 = 4326;
    /// Web Mercator, what tile servers actually draw in.
    pub const WEB_MERCATOR: u64 = 3857;
}

/// Converting coordinates between EPSG systems.
///
/// Implement this over a geodesy library to give the layers above real
/// reprojection. Nothing in this workspace implements it, on purpose.
pub trait Reprojector {
    /// Convert a point from one EPSG system to another.
    ///
    /// Input and output are in each system's own axis order and units - the
    /// caller is expected to have normalised the local placement first, with
    /// [`Crs::to_metres`].
    fn reproject(&self, point: Point, from: u64, to: u64) -> Result<Point>;

    /// Convert many points at once.
    ///
    /// Worth overriding: a geodesy library usually amortises the setup of a
    /// transformation across a whole array, and a surface is thousands of
    /// points.
    fn reproject_all(&self, points: &[Point], from: u64, to: u64) -> Result<Vec<Point>> {
        points
            .iter()
            .map(|point| self.reproject(*point, from, to))
            .collect()
    }
}

/// Place a point in a target EPSG system, all the way from local coordinates.
///
/// Local placement and unit normalisation happen here; the datum work is the
/// backend's. Errors - rather than returning the point unmoved - when the CRS
/// names no EPSG code, because "I could not convert this" and "this needed no
/// conversion" must not look the same to the caller.
pub fn to_epsg(crs: &Crs, point: Point, target: u64, backend: &dyn Reprojector) -> Result<Point> {
    let placed = crs.to_metres(point)?;
    let Some(source) = crs.projected.epsg() else {
        return Err(Error::unsupported(
            "reprojecting from a system with no EPSG code",
            format!("the CRS {} says `{:?}`", crs.uuid, crs.projected),
        ));
    };
    if source == target {
        return Ok(placed);
    }
    backend.reproject(placed, source, target)
}

/// A horizontal system: an EPSG code, and the unit its coordinates are
/// written in.
///
/// The pivot [`reproject_placed`] moves points between: not a [`Crs`], which
/// also carries a local placement (offset, rotation, vertical direction) that
/// a scene has already applied by the time it draws - see the module doc on
/// [`reproject_placed`] for why applying that placement a second time is
/// exactly the bug this type exists to make impossible to write by accident.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct System {
    /// The EPSG code of the horizontal system.
    pub epsg: u64,
    /// The unit horizontal coordinates in this system are written in, as
    /// [`units`] spells it - `"m"` when nothing says otherwise, the schema's
    /// own default and not a guess.
    pub unit: String,
}

impl System {
    /// The horizontal system a CRS places its coordinates in.
    ///
    /// Refuses by name - the same refusal [`to_epsg`] makes - when the CRS
    /// states no EPSG code: a system with no code cannot be a *target* either,
    /// and guessing one would misplace whatever gets reprojected into it.
    pub fn of(crs: &Crs) -> Result<Self> {
        let Some(epsg) = crs.projected.epsg() else {
            return Err(Error::unsupported(
                "naming a horizontal system with no EPSG code",
                format!("the CRS {} says `{:?}`", crs.uuid, crs.projected),
            ));
        };
        Ok(System {
            epsg,
            unit: crs.horizontal_uom.clone().unwrap_or_else(|| "m".to_string()),
        })
    }
}

/// Move already-**placed** points from one horizontal system to another.
///
/// # Placed, not local - and why that is the whole point of a separate function
///
/// The input is a point as a scene already draws it: a representation's local
/// coordinates, run through [`Crs::to_projected`] (offset, rotation, the
/// vertical flip) and normalised to metres - exactly what [`to_epsg`] returns.
/// Calling this on a *local* point instead would skip that placement, which is
/// invisible on a CRS whose origin is (0, 0) and axis is up - every corpus
/// package - and wrong on a real one, with every coordinate still looking
/// plausible. [`to_epsg`] is the function that starts from a local point;
/// this one starts from where that one, or a file's own written coordinates,
/// already leaves off.
///
/// # The three steps, always in this order
///
/// `from`'s unit to metres, [`Reprojector::reproject_all`] between the two
/// EPSG codes (skipped entirely when they agree - a scene with one CRS never
/// pays for a backend it does not need), metres to `to`'s unit. Getting the
/// order backwards - reprojecting feet as if they were metres - succeeds and
/// is wrong by the unit ratio, the same trap a WebAssembly front end's `from_wgs84`
/// documents for the mirror direction.
///
/// The vertical coordinate is carried through **untouched**: a horizontal
/// reprojection has nothing to say about a depth or an elevation, and
/// `energyml-proj`'s own module doc explains why a Helmert shift must not be
/// allowed to move it.
///
/// # What is refused, and how
///
/// Same EPSG code *and* same unit is the identity - returned without calling
/// `backend` at all, so a scene whose objects share one system never depends
/// on a geodesy backend being configured. Otherwise: an unknown unit on either
/// side is `Error::UnknownUnit`, named; anything `backend` refuses - no datum
/// stated, a correction grid it does not carry - comes back exactly as that
/// backend names it, not wrapped into something vaguer.
pub fn reproject_placed(
    points: &[Point],
    from: &System,
    to: &System,
    backend: &dyn Reprojector,
) -> Result<Vec<Point>> {
    if from.epsg == to.epsg && from.unit == to.unit {
        return Ok(points.to_vec());
    }
    let metres: Vec<Point> = points
        .iter()
        .map(|&[x, y, z]| -> Result<Point> {
            Ok([units::metres(x, &from.unit)?, units::metres(y, &from.unit)?, z])
        })
        .collect::<Result<_>>()?;
    let reprojected = if from.epsg == to.epsg {
        metres
    } else {
        backend.reproject_all(&metres, from.epsg, to.epsg)?
    };
    reprojected
        .iter()
        .map(|&[x, y, z]| -> Result<Point> {
            Ok([
                units::convert(x, "m", &to.unit)?,
                units::convert(y, "m", &to.unit)?,
                z,
            ])
        })
        .collect()
}

/// A backend that refuses every conversion, naming what is missing.
///
/// The default when an application has plugged nothing in. It exists so that
/// the failure is a message about a missing capability rather than a plausible
/// set of coordinates in the wrong place.
pub struct NoReprojection;

impl Reprojector for NoReprojection {
    fn reproject(&self, _point: Point, from: u64, to: u64) -> Result<Point> {
        Err(Error::unsupported(
            "reprojecting between EPSG systems",
            format!(
                "EPSG:{from} to EPSG:{to} needs a geodesy backend; \
                 implement `Reprojector` over one, or use `energyml_proj::Proj4`"
            ),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crs::{CrsKind, VerticalDirection, VerticalQuantity};

    fn crs(projected: CrsKind) -> Crs {
        Crs {
            uuid: "test".into(),
            kind: "LocalDepth3dCrs",
            x_offset: 0.0,
            y_offset: 0.0,
            z_offset: 0.0,
            areal_rotation: 0.0,
            vertical_direction: VerticalDirection::Up,
            vertical_quantity: VerticalQuantity::Length,
            horizontal_uom: None,
            vertical_uom: None,
            time_uom: None,
            projected,
            vertical: CrsKind::Absent,
        }
    }

    #[test]
    fn the_same_system_needs_no_backend() {
        let placed = to_epsg(
            &crs(CrsKind::Epsg(23031)),
            [1.0, 2.0, 3.0],
            23031,
            &NoReprojection,
        )
        .unwrap();
        assert_eq!(placed, [1.0, 2.0, 3.0]);
    }

    #[test]
    fn a_missing_backend_says_so_rather_than_returning_the_point() {
        let error = to_epsg(&crs(CrsKind::Epsg(23031)), [1.0, 2.0, 3.0], 4326, &NoReprojection).unwrap_err();
        let message = format!("{error}");
        assert!(message.contains("23031") && message.contains("4326"), "{message}");
    }

    #[test]
    fn a_crs_with_no_epsg_code_is_refused() {
        let error = to_epsg(&crs(CrsKind::Unknown), [1.0, 2.0, 3.0], 4326, &NoReprojection).unwrap_err();
        assert!(format!("{error}").contains("no EPSG code"));
    }

    /// Doubles the horizontal coordinates and refuses to be called on the
    /// same system - the identity short-circuit must never reach it.
    struct Doubling;
    impl Reprojector for Doubling {
        fn reproject(&self, point: Point, from: u64, to: u64) -> Result<Point> {
            assert_ne!(
                from, to,
                "reproject_placed must skip the backend when the systems agree"
            );
            Ok([point[0] * 2.0, point[1] * 2.0, point[2]])
        }
    }

    #[test]
    fn the_unit_conversion_runs_before_and_after_the_backend() {
        // 1 ft on both sides of a backend that doubles metres: 1 ft = 0.3048 m,
        // doubled to 0.6096 m, back to 2 ft. Getting the order wrong - handing
        // the backend a number of feet labelled as metres, or converting the
        // backend's metres with the *source* unit - would give a different,
        // still plausible-looking, wrong number.
        let from = System {
            epsg: 1,
            unit: "ft".to_string(),
        };
        let to = System {
            epsg: 2,
            unit: "ft".to_string(),
        };
        let out = reproject_placed(&[[1.0, 1.0, 5.0]], &from, &to, &Doubling).unwrap();
        assert!((out[0][0] - 2.0).abs() < 1e-9, "{:?}", out[0]);
        assert!((out[0][1] - 2.0).abs() < 1e-9, "{:?}", out[0]);
        // The vertical is carried through untouched, not merely unscaled.
        assert_eq!(out[0][2], 5.0);
    }

    #[test]
    fn the_same_system_and_unit_never_call_the_backend() {
        let system = System {
            epsg: 2154,
            unit: "m".to_string(),
        };
        let out = reproject_placed(&[[1.0, 2.0, 3.0]], &system, &system, &Doubling).unwrap();
        // Had the backend been called, this would be [2.0, 4.0, 3.0].
        assert_eq!(out, vec![[1.0, 2.0, 3.0]]);
    }

    #[test]
    fn an_unknown_unit_is_a_named_refusal_not_a_guess() {
        let from = System {
            epsg: 1,
            unit: "bogus".to_string(),
        };
        let to = System {
            epsg: 1,
            unit: "m".to_string(),
        };
        let error = reproject_placed(&[[1.0, 1.0, 1.0]], &from, &to, &NoReprojection).unwrap_err();
        assert!(format!("{error}").contains("bogus"));
    }

    #[test]
    fn a_backend_refusal_travels_through_by_name() {
        let from = System {
            epsg: 1,
            unit: "m".to_string(),
        };
        let to = System {
            epsg: 2,
            unit: "m".to_string(),
        };
        let error = reproject_placed(&[[1.0, 1.0, 1.0]], &from, &to, &NoReprojection).unwrap_err();
        let message = format!("{error}");
        assert!(message.contains('1') && message.contains('2'), "{message}");
    }

    #[test]
    fn a_system_with_no_epsg_code_is_refused_by_name() {
        let error = System::of(&crs(CrsKind::Unknown)).unwrap_err();
        assert!(format!("{error}").contains("no EPSG code"));
    }

    #[test]
    fn a_system_with_no_stated_unit_defaults_to_metres() {
        let system = System::of(&crs(CrsKind::Epsg(2154))).unwrap();
        assert_eq!(system.epsg, 2154);
        assert_eq!(system.unit, "m");
    }
}
