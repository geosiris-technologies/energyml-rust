//! The north of a survey: where it comes from, the corrections the file states, and how it lies in the
//! frame of the datum (WITSML TRG 2.1 §16.7 p. 352-353, §16.2 p. 345).

use energyml_core::measure;
use energyml_core::value::Reflect;
use energyml_domain::crs::AxisOrder;
use energyml_domain::units;

use super::checks::{CheckKind, Fault, Finding, MissingCorrection};
use super::{DeclaredNorth, Facts, Kept, NorthReference, SurveyKind};
use crate::error::Error;
use crate::geometry::field_object;
use crate::min_curvature::{self, Measured};
use crate::source::ValueSource;
use crate::well::{PlacedDatum, reference_of};

/// Where the north a survey was placed with comes from - carried by every result, never lost.
#[derive(Debug, Clone, PartialEq)]
pub enum NorthSource {
    /// Grid or plant north, as the file states it (a deviation survey: grid, by its schema).
    Stated(NorthReference),
    /// True or magnetic north, turned to grid north by the corrections the file states:
    /// `turn[k]` is the angle in degrees added to the azimuth of kept station k (declination minus
    /// convergence), `None` where the station has no azimuth.
    Corrected {
        /// The north the file states.
        stated: NorthReference,
        /// The turn applied to each kept station, degrees.
        turn: Vec<Option<f64>>,
    },
    /// Declared by the caller over a survey that states none.
    Declared(DeclaredNorth),
    /// No kept station leaves the vertical: no azimuth moves anything.
    NotNeeded,
}

/// The frame a north is honoured in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Frame {
    Grid,
    Plant,
}

/// The north check: from the kept stations, the stated north, its corrections and the declaration,
/// the source, the frame and the azimuths to place with.
pub(super) fn north_of(
    facts: &Facts,
    kept: &Kept,
    declared: Option<DeclaredNorth>,
) -> Result<(NorthSource, Option<Frame>, Vec<Measured>), Fault> {
    let survey = facts.kind;
    // A declaration never replaces what the file says (true even of a vertical survey, so that the
    // rule is simple to state).
    if declared.is_some()
        && let Some(stated) = &facts.north
    {
        return Err(Fault::new(
            CheckKind::North,
            Finding::NorthAlreadyStated {
                stated: stated.clone(),
            },
            survey,
        ));
    }
    let vertical = |station: &Measured| {
        matches!(station.inclination, Some(i) if i.to_radians().sin().abs() <= min_curvature::VERTICAL)
    };
    if kept.measured.iter().all(vertical) {
        return Ok((NorthSource::NotNeeded, None, kept.measured.clone()));
    }
    match (&facts.north, declared) {
        (None, None) => {
            let at = kept
                .measured
                .iter()
                .position(|station| !vertical(station))
                .expect("a station leaves the vertical");
            let station = &kept.measured[at];
            Err(Fault::new(
                CheckKind::North,
                Finding::NorthNotStated {
                    station: kept.stations[at],
                    md: station.md,
                    inclination: station.inclination.unwrap_or(f64::NAN),
                },
                survey,
            ))
        }
        (None, Some(declared)) => {
            let frame = match declared {
                DeclaredNorth::Grid => Frame::Grid,
                DeclaredNorth::Plant => Frame::Plant,
            };
            Ok((NorthSource::Declared(declared), Some(frame), kept.measured.clone()))
        }
        (Some(NorthReference::Grid), _) => Ok((
            NorthSource::Stated(NorthReference::Grid),
            Some(Frame::Grid),
            kept.measured.clone(),
        )),
        (Some(NorthReference::Plant), _) => Ok((
            NorthSource::Stated(NorthReference::Plant),
            Some(Frame::Plant),
            kept.measured.clone(),
        )),
        (Some(north @ (NorthReference::True | NorthReference::Magnetic)), _) => turned(facts, kept, north),
        (Some(other), _) => Err(Fault::new(
            CheckKind::North,
            Finding::NorthNotApplied {
                north: other.clone(),
                missing: MissingCorrection::NoRule,
            },
            survey,
        )),
    }
}

/// Which correction a station's azimuth needs.
#[derive(Clone, Copy)]
enum Correction {
    Convergence,
    Declination,
}

/// A correction the file states for station `number` (from 1): the station's own first, then the
/// trajectory's. A correction that is not stated, or whose unit cannot be read, is a refusal by name.
fn correction(facts: &Facts, number: usize, which: Correction, north: &NorthReference) -> Result<f64, Fault> {
    let own = facts.corrections.per_station.get(number - 1);
    let (station, trajectory) = match which {
        Correction::Convergence => (
            own.and_then(|own| own.convergence.clone()),
            facts.corrections.convergence.clone(),
        ),
        Correction::Declination => (
            own.and_then(|own| own.declination.clone()),
            facts.corrections.declination.clone(),
        ),
    };
    let refuse = |missing: MissingCorrection| {
        Fault::new(
            CheckKind::North,
            Finding::NorthNotApplied {
                north: north.clone(),
                missing,
            },
            facts.kind,
        )
    };
    match station.or(trajectory) {
        Some(Ok(value)) => Ok(value),
        Some(Err(fault)) => Err(refuse(MissingCorrection::Unreadable { fault })),
        None => Err(refuse(match which {
            Correction::Convergence => MissingCorrection::Convergence { station: number },
            Correction::Declination => MissingCorrection::Declination { station: number },
        })),
    }
}

/// True or magnetic north turned to grid north by the corrections the file states: the convergence is
/// subtracted (`GridConUsed`, WITSML TRG 2.1 §16.7 p. 352), and from magnetic north the declination is
/// added first (`MagDeclUsed`, p. 353). The declination of a station is looked for before its convergence.
fn turned(
    facts: &Facts,
    kept: &Kept,
    north: &NorthReference,
) -> Result<(NorthSource, Option<Frame>, Vec<Measured>), Fault> {
    let magnetic = *north == NorthReference::Magnetic;
    let mut corrected = Vec::with_capacity(kept.measured.len());
    let mut turn = Vec::with_capacity(kept.measured.len());
    for (at, station) in kept.measured.iter().enumerate() {
        let number = kept.stations[at];
        let Some(azimuth) = station.azimuth else {
            corrected.push(*station);
            turn.push(None);
            continue;
        };
        let declination = if magnetic {
            correction(facts, number, Correction::Declination, north)?
        } else {
            0.0
        };
        let convergence = correction(facts, number, Correction::Convergence, north)?;
        let delta = declination - convergence;
        let mut grid = (azimuth + delta).rem_euclid(360.0);
        // An arithmetic that rounds up to the turn is the same direction as 0.
        if grid >= 360.0 {
            grid = 0.0;
        }
        corrected.push(Measured {
            azimuth: Some(grid),
            ..*station
        });
        turn.push(Some(delta));
    }
    Ok((
        NorthSource::Corrected {
            stated: north.clone(),
            turn,
        },
        Some(Frame::Grid),
        corrected,
    ))
}

/// How a displacement north and east of the datum lands on the projected axes.
pub(crate) enum Horizontal {
    /// Along the projected axes, in their unit.
    Grid { metres_per_unit: f64 },
    /// Along the axes of the compound, turned by its azimuth.
    Plant {
        compound: Box<energyml_domain::Crs>,
        metres_per_unit: f64,
    },
}

impl Horizontal {
    /// Metres per unit of the projected axes.
    pub(super) fn metres_per_unit(&self) -> f64 {
        match self {
            Horizontal::Grid { metres_per_unit } | Horizontal::Plant { metres_per_unit, .. } => *metres_per_unit,
        }
    }

    /// Where `(east, north)` metres from the datum is, from the placed datum, in the unit of the axes.
    pub(super) fn offset(&self, east: f64, north: f64) -> (f64, f64) {
        match self {
            Horizontal::Grid { metres_per_unit } => (east / metres_per_unit, north / metres_per_unit),
            Horizontal::Plant {
                compound,
                metres_per_unit,
            } => {
                // The compound's own turn, not a second copy of it: the
                // placement of the displacement less that of its origin.
                let there = compound.to_projected([east / metres_per_unit, north / metres_per_unit, 0.0]);
                let origin = compound.to_projected([0.0, 0.0, 0.0]);
                (there[0] - origin[0], there[1] - origin[1])
            }
        }
    }

    /// The axes of the frame, as the check `NorthInFrame`: `at` is where the north is read from, in words.
    pub(super) fn of(
        frame: Frame,
        datum_uri: &str,
        placed: &PlacedDatum,
        source: &dyn ValueSource,
        at: &str,
        survey: SurveyKind,
    ) -> Result<Horizontal, Fault> {
        let fail = |finding: Finding| Fault::at(CheckKind::NorthInFrame, finding, survey, at);
        let datum = source.object_at(datum_uri).ok_or_else(|| {
            Fault::with_error(
                CheckKind::NorthInFrame,
                Finding::DatumNotPlaced {
                    datum: datum_uri.to_string(),
                    reason: format!("the datum {datum_uri} is not in this store"),
                },
                Error::Missing(format!("the datum {datum_uri}")),
            )
        })?;
        let datum = datum.as_reflect();
        match (frame, &placed.compound) {
            (Frame::Grid, Some(compound)) => {
                if let Some(order) = compound_plane(datum, source).and_then(|plane| {
                    field_object(plane.as_reflect(), "OriginProjectedCrs")
                        .and_then(|projected| measure::enum_value(projected, "AxisOrder"))
                }) {
                    east_then_north(&order).map_err(fail)?;
                }
                Ok(Horizontal::Grid {
                    metres_per_unit: horizontal_unit(compound.horizontal_uom.as_deref(), at, survey)?,
                })
            }
            (Frame::Grid, None) => {
                // A point in a CRS: its projected system states its axes and
                // its unit, and both are asked.
                let projected = field_object(datum, "HorizontalCoordinates")
                    .and_then(|holder| reference_of(holder, "Crs"))
                    .and_then(|uri| source.object_at(&uri))
                    .filter(|crs| crs.as_reflect().type_info().type_key == "ProjectedCrs")
                    .ok_or_else(|| fail(Finding::GridWithoutProjected))?;
                let projected = projected.as_reflect();
                match measure::enum_value(projected, "AxisOrder") {
                    Some(order) => east_then_north(&order).map_err(&fail)?,
                    None => return Err(fail(Finding::AxisOrderUnstated)),
                }
                let unit = energyml_domain::crs::stated_uom(projected);
                Ok(Horizontal::Grid {
                    metres_per_unit: horizontal_unit(unit.as_deref(), at, survey)?,
                })
            }
            (Frame::Plant, Some(compound)) => {
                let axes = compound_plane(datum, source).and_then(|plane| {
                    let axes = field_object(plane.as_reflect(), "HorizontalAxes")?;
                    Some((
                        measure::enum_value(axes, "Direction1"),
                        measure::enum_value(axes, "Direction2"),
                    ))
                });
                match axes {
                    Some((Some(first), Some(second)))
                        if AxisOrder::from_directions(&first, &second).is_some_and(is_east_then_north) => {}
                    Some((first, second)) => {
                        return Err(fail(Finding::PlantAxes {
                            first: first.unwrap_or_default(),
                            second: second.unwrap_or_default(),
                        }));
                    }
                    None => return Err(fail(Finding::PlantAxesUnreadable)),
                }
                Ok(Horizontal::Plant {
                    compound: Box::new(compound.clone()),
                    metres_per_unit: horizontal_unit(compound.horizontal_uom.as_deref(), at, survey)?,
                })
            }
            (Frame::Plant, None) => Err(fail(Finding::PlantWithoutCompound)),
        }
    }
}

/// Whether the axes run east then north: the order a survey's azimuths are laid in.
fn is_east_then_north(order: AxisOrder) -> bool {
    order.first_degrees == 90.0 && order.second_degrees == 0.0
}

/// The 2D half of the compound a datum is written in.
fn compound_plane(
    datum: &dyn Reflect,
    source: &dyn ValueSource,
) -> Option<Box<dyn energyml_core::registry::DynObject>> {
    let compound = source.object_at(&reference_of(datum, "Crs")?)?;
    source.object_at(&reference_of(compound.as_reflect(), "LocalEngineering2dCrs")?)
}

/// `Ok` when the order is easting then northing, through the one reading of an axis order.
fn east_then_north(order: &str) -> Result<(), Finding> {
    if AxisOrder::from_order(order).is_some_and(is_east_then_north) {
        return Ok(());
    }
    Err(Finding::AxisOrder {
        order: order.to_string(),
    })
}

/// Metres per unit of the projected axes - stated, or refused.
fn horizontal_unit(unit: Option<&str>, at: &str, survey: SurveyKind) -> Result<f64, Fault> {
    let unit = unit.ok_or_else(|| {
        Fault::at(
            CheckKind::NorthInFrame,
            Finding::HorizontalUnitUnstated,
            survey,
            at,
        )
    })?;
    units::metres(1.0, unit).map_err(|why| {
        Fault::with_error(
            CheckKind::NorthInFrame,
            Finding::HorizontalUnitRefused {
                unit: unit.to_string(),
                reason: why.to_string(),
            },
            Error::Domain(why),
        )
    })
}
