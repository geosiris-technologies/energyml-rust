//! A WITSML `Trajectory`, drawn: its stations placed by the minimum curvature
//! method from the datum its measured depths are counted from.
//!
//! A RESQML trajectory states a **line** — control points, a parametric
//! spline — and [`crate::parametric`] evaluates it. A WITSML 2.1 trajectory
//! states **measurements**: at each `TrajectoryStation` a measured depth, an
//! inclination *« measured from vertical »* and an azimuth *« Corrected to
//! wells azimuth reference »* (WITSML TRG 2.1 §16.10 p. 356–358). Where the
//! hole is follows from them by a method, and the method is
//! [`crate::min_curvature`] — the minimum curvature method run forward
//! (WITSML/PRODML business views). This module is
//! what it takes to hand that function numbers it can trust, and to put its
//! answer in the world.
//!
//! # Placed by the trajectory's datum, not the well's
//!
//! The WITSML 2.1 `Well` carries no datum. The trajectory names
//! the one its depths count from — `DefaultMdDatum`, an `AbstractReferencePoint`
//! (§16.6 p. 350) — and a station may name its own in `Md.Datum`. The two must
//! agree, and so must `MdInterval.Datum` when it is stated, because that is the
//! one `energyml_domain::crs::of` reads to give the trajectory its CRS: a
//! trajectory drawn from one datum and labelled with another is placed twice
//! or not at all. The datum is placed by [`crate::well::place_datum`], the
//! reading a RESQML well's datum goes through — never a second one.
//!
//! **MD 0 is the datum**, by definition of a measured depth, so the first
//! station must be there. One that starts below it — a survey tied on to
//! another (`MdTieOn`, `ParentTrajectory`) — does not say where the hole runs
//! above it, and is refused rather than hung from the datum.
//!
//! # Which north
//!
//! *« Specifies the definition of north. While this is optional because of
//! legacy data, it is strongly recommended that this always be specified »*
//! (`AziRef`, §16.7 p. 352). Honoured, or refused:
//!
//! - **grid north** — the north of the projected system the datum is placed in:
//!   the displacement is added to the placed datum as it stands, east along the
//!   first projected axis and north along the second;
//! - **plant north** — *« a direction in a local engineering coordinate
//!   reference system »* (eml 2.3 `NorthReferenceKind`): the second axis of the
//!   compound the datum is written in, so the displacement is turned by that
//!   compound's azimuth — through `Crs::to_projected`, the placement every
//!   geometry of the compound goes through. A datum written in no compound
//!   names no plant, and is refused;
//! - **true, magnetic, astronomic, compass north** — need a convergence or a
//!   declination this build does not apply: refused by name;
//! - **none** — refused, unless no station leaves the vertical below the
//!   datum, in which case no azimuth moves anything and none is asked.
//!
//! An axis order the file states other than east then north is refused where
//! it would turn the hole: the compound's `HorizontalAxes` for plant north,
//! the projected system's `AxisOrder` for grid north.
//!
//! # One decision, stated
//!
//! **No algorithm stated is read as minimum curvature.** A station's
//! `CalcAlgorithm` and the trajectory's `NominalCalcAlgorithm` are optional,
//! and absent on most surveys; the angles are measurements and some method must
//! join them. Minimum curvature is the one API D20 and the industry settle on,
//! and the one the design names. A method the file **states** otherwise —
//! tangential, radius of curvature, … — is refused: drawing it by another would
//! move the hole away from where its producer computed it.
//!
//! # What comes out
//!
//! One polyline, **one node per station** — the stations are what the object
//! states, and what a log indexed on the trajectory hangs on — in the convention
//! of `crate::well`: projected x and y, an **elevation** in metres, z-up. The
//! CRS `energyml_domain::crs::of` gives the trajectory through its
//! `MdInterval.Datum` carries the identity of the system and none of its
//! placement, so the two read together place the hole once.

use crate::min_curvature::{self as stations, Measured};
use energyml_core::measure;
use energyml_core::value::Reflect;
use energyml_domain::units;

use crate::error::{Error, Result};
use crate::geometry::{Patch, Topology, field_list, field_object};
use crate::source::ValueSource;
use crate::well::{PlacedDatum, place_datum, reference_of};

/// Where a refusal is, in words.
const AT: &str = "Trajectory";

/// The one calculation method this reader applies, as WITSML spells it
/// (`TrajStnCalcAlgorithm`, WITSML TRG 2.1 §16.15).
const MINIMUM_CURVATURE: &str = "minimum curvature";

/// The stations, as the document states them: what a listing may show
/// without a read.
pub(crate) fn declared(trajectory: &dyn Reflect) -> Option<u64> {
    Some(field_list(trajectory, "TrajectoryStation").len() as u64)
}

/// The trajectory, drawn — see the module.
pub(crate) fn read(trajectory: &dyn Reflect, source: &dyn ValueSource) -> Result<Vec<Patch>> {
    let listed = field_list(trajectory, "TrajectoryStation");
    if listed.is_empty() {
        return Err(Error::malformed(AT, "a trajectory with no `TrajectoryStation`: nothing states where the hole is"));
    }
    stated_method(trajectory, "NominalCalcAlgorithm", AT)?;

    // -- the stations, in the file's order and units --------------------------
    let default_datum = reference_of(trajectory, "DefaultMdDatum");
    let mut datum: Option<String> = None;
    let mut measured = Vec::with_capacity(listed.len());
    for (at, station) in listed.iter().enumerate() {
        let here = format!("{AT}.TrajectoryStation[{}]", at + 1);
        if measure::enum_value(*station, "StatusTrajStation").as_deref() == Some("rejected") {
            return Err(Error::unsupported(
                "a trajectory holding a rejected station — whether it is left out is not decided here",
                here,
            ));
        }
        stated_method(*station, "CalcAlgorithm", &here)?;
        let holder = field_object(*station, "Md").ok_or_else(|| Error::malformed(&here, "a station with no `Md`"))?;
        let md = measured_depth(holder, &here)?;
        let counted_from = reference_of(holder, "Datum").or_else(|| default_datum.clone()).ok_or_else(|| {
            Error::unsupported(
                "a WITSML trajectory whose measured depths name no datum — neither `DefaultMdDatum` nor the station's `Md.Datum`",
                &here,
            )
        })?;
        match &datum {
            None => datum = Some(counted_from),
            Some(first) if crate::well::same_uri(first, &counted_from) => {}
            Some(first) => {
                return Err(Error::unsupported(
                    "a WITSML trajectory whose stations count their depths from two datums",
                    format!("{here}: {counted_from}, where the stations before count from {first}"),
                ));
            }
        }
        measured.push(Measured {
            md,
            inclination: angle(*station, "Incl", &here)?,
            azimuth: angle(*station, "Azi", &here)?,
        });
    }
    let datum = datum.expect("one station at least, each with a datum");
    if let Some(interval) = field_object(trajectory, "MdInterval")
        && let Some(stated) = reference_of(interval, "Datum")
        && !crate::well::same_uri(&stated, &datum)
    {
        return Err(Error::malformed(
            AT,
            format!(
                "`MdInterval.Datum` names {stated} and the stations count from {datum}: the CRS the trajectory is given is read from the first, the hole would be drawn from the second"
            ),
        ));
    }
    if measured[0].md != 0.0 {
        return Err(Error::unsupported(
            "a WITSML trajectory whose first station is not at its datum — where the hole runs above it is not stated",
            format!("{AT}.TrajectoryStation[1]: MD {} m", measured[0].md),
        ));
    }

    // -- the survey, then the world -------------------------------------------
    let located = stations::positions(&measured).map_err(|why| Error::malformed(AT, why))?;
    let placed = place_datum(&datum, source, AT)?;
    let moves = located.iter().any(|one| one.north != 0.0 || one.east != 0.0);
    let horizontal = if moves { Some(Horizontal::of(trajectory, &datum, &placed, source)?) } else { None };
    let points = located
        .iter()
        .map(|one| {
            let (dx, dy) = horizontal.as_ref().map_or((0.0, 0.0), |frame| frame.offset(one.east, one.north));
            [placed.x + dx, placed.y + dy, placed.elevation - one.tvd]
        })
        .collect::<Vec<_>>();
    let count = points.len() as u32;
    Ok(vec![Patch { index: 0, node_offset: 0, points, topology: Topology::Polylines(vec![count]), closed: Vec::new() }])
}

/// A method the file states, which must be the one applied.
fn stated_method(object: &dyn Reflect, field: &str, at: &str) -> Result<()> {
    match measure::enum_value(object, field) {
        Some(method) if !method.eq_ignore_ascii_case(MINIMUM_CURVATURE) => Err(Error::unsupported(
            "a survey computed by a method other than minimum curvature — drawing it by minimum curvature would move the hole from where its producer put it",
            format!("{at}.{field} = `{method}`"),
        )),
        _ => Ok(()),
    }
}

/// A measured depth, in metres: `Md.MeasuredDepth` (eml 2.3), or `Md` itself
/// where it is the measure (WITSML 2.0) — the field present, never the release.
fn measured_depth(holder: &dyn Reflect, at: &str) -> Result<f64> {
    let found = measure::read(holder)
        .or_else(|| field_object(holder, "MeasuredDepth").and_then(measure::read))
        .ok_or_else(|| Error::malformed(at, "an `Md` holding no number"))?;
    let unit = found.uom.ok_or_else(|| Error::malformed(at, "an `Md` with no unit: a length in nothing is not a depth"))?;
    Ok(units::metres(found.value, &unit)?)
}

/// An angle, in degrees — `None` when the station states none. A unit that is
/// not an angle is refused by `energyml_domain::units`, by name.
fn angle(station: &dyn Reflect, field: &str, at: &str) -> Result<Option<f64>> {
    let Some(holder) = field_object(station, field) else {
        return Ok(None);
    };
    let found = measure::read(holder).ok_or_else(|| Error::malformed(format!("{at}.{field}"), "no number"))?;
    let unit = found
        .uom
        .ok_or_else(|| Error::malformed(format!("{at}.{field}"), "an angle with no unit — degrees or radians is not a guess to make"))?;
    // Into degrees directly: `convert` leaves a value in `dega` untouched, so
    // 360 stays 360 and is not refused as 360.00000000000006.
    Ok(Some(units::convert(found.value, &unit, "dega")?))
}

/// How a displacement north and east of the datum lands on the projected axes.
enum Horizontal {
    /// Along the projected axes, in their unit.
    Grid { metres_per_unit: f64 },
    /// Along the axes of the compound, turned by its azimuth.
    Plant { compound: Box<energyml_domain::Crs>, metres_per_unit: f64 },
}

impl Horizontal {
    /// Where `(east, north)` metres from the datum is, from the placed datum.
    fn offset(&self, east: f64, north: f64) -> (f64, f64) {
        match self {
            Horizontal::Grid { metres_per_unit } => (east / metres_per_unit, north / metres_per_unit),
            Horizontal::Plant { compound, metres_per_unit } => {
                // The compound's own turn, not a second copy of it: the
                // placement of the displacement less that of its origin.
                let there = compound.to_projected([east / metres_per_unit, north / metres_per_unit, 0.0]);
                let origin = compound.to_projected([0.0, 0.0, 0.0]);
                (there[0] - origin[0], there[1] - origin[1])
            }
        }
    }

    fn of(trajectory: &dyn Reflect, datum_uri: &str, placed: &PlacedDatum, source: &dyn ValueSource) -> Result<Self> {
        let at = format!("{AT}.AziRef");
        let Some(north) = measure::enum_value(trajectory, "AziRef") else {
            return Err(Error::unsupported(
                "a WITSML trajectory that leaves the vertical and states no `AziRef` — which north its azimuths turn from is not said (WITSML TRG 2.1 §16.7 p. 352)",
                at,
            ));
        };
        let datum = source.object_at(datum_uri).ok_or_else(|| Error::Missing(format!("the datum {datum_uri}")))?;
        let datum = datum.as_reflect();
        match (north.as_str(), &placed.compound) {
            ("grid north", Some(compound)) => {
                if let Some(order) = compound_plane(datum, source)
                    .and_then(|plane| field_object(plane.as_reflect(), "OriginProjectedCrs").and_then(|p| measure::enum_value(p, "AxisOrder")))
                {
                    east_then_north(&order, &at)?;
                }
                Ok(Horizontal::Grid { metres_per_unit: horizontal_unit(compound.horizontal_uom.as_deref(), &at)? })
            }
            ("grid north", None) => {
                // A point in a CRS: its projected system states its axes and
                // its unit, and both are asked.
                let projected = field_object(datum, "HorizontalCoordinates")
                    .and_then(|holder| reference_of(holder, "Crs"))
                    .and_then(|uri| source.object_at(&uri))
                    .filter(|crs| crs.as_reflect().type_info().type_key == "ProjectedCrs")
                    .ok_or_else(|| {
                        Error::unsupported(
                            "a grid-north survey whose datum is not placed in a projected CRS — the grid it turns from is not named",
                            &at,
                        )
                    })?;
                let projected = projected.as_reflect();
                match measure::enum_value(projected, "AxisOrder") {
                    Some(order) => east_then_north(&order, &at)?,
                    None => {
                        return Err(Error::unsupported("a projected CRS that states no axis order", &at));
                    }
                }
                Ok(Horizontal::Grid { metres_per_unit: horizontal_unit(measure::enum_value(projected, "uom").as_deref(), &at)? })
            }
            ("plant north", Some(compound)) => {
                let axes = compound_plane(datum, source).and_then(|plane| {
                    let axes = field_object(plane.as_reflect(), "HorizontalAxes")?;
                    Some((measure::enum_value(axes, "Direction1"), measure::enum_value(axes, "Direction2")))
                });
                match axes {
                    Some((Some(first), Some(second))) if first == "east" && second == "north" => {}
                    Some((first, second)) => {
                        return Err(Error::unsupported(
                            "a plant-north survey in a compound whose axes are not east then north",
                            format!("{at}: `{}`, `{}`", first.unwrap_or_default(), second.unwrap_or_default()),
                        ));
                    }
                    None => {
                        return Err(Error::unsupported(
                            "a plant-north survey whose compound's horizontal axes cannot be read — which way its north points is not known",
                            at,
                        ));
                    }
                }
                Ok(Horizontal::Plant {
                    compound: Box::new(compound.clone()),
                    metres_per_unit: horizontal_unit(compound.horizontal_uom.as_deref(), &at)?,
                })
            }
            ("plant north", None) => Err(Error::unsupported(
                "a plant-north survey whose datum is written in no local engineering compound — no plant is named to turn from",
                at,
            )),
            (_, _) => Err(Error::unsupported(
                "an azimuth reference other than grid or plant north — it needs a convergence or a declination this build does not apply",
                format!("{at} = `{north}`"),
            )),
        }
    }
}

/// The 2D half of the compound a datum is written in.
fn compound_plane(datum: &dyn Reflect, source: &dyn ValueSource) -> Option<Box<dyn energyml_core::registry::DynObject>> {
    let compound = source.object_at(&reference_of(datum, "Crs")?)?;
    source.object_at(&reference_of(compound.as_reflect(), "LocalEngineering2dCrs")?)
}

fn east_then_north(order: &str, at: &str) -> Result<()> {
    if order == "easting northing" {
        return Ok(());
    }
    Err(Error::unsupported(
        "a grid-north survey in a projected system whose axes are not easting then northing",
        format!("{at}: AxisOrder `{order}`"),
    ))
}

/// Metres per unit of the projected axes — stated, or refused.
fn horizontal_unit(unit: Option<&str>, at: &str) -> Result<f64> {
    let unit = unit.ok_or_else(|| {
        Error::unsupported("a projected system that states no unit — a displacement in metres cannot be added to it", at)
    })?;
    Ok(units::metres(1.0, unit)?)
}
