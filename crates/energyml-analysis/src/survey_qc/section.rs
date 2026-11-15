//! The azimuth and the origin of the vertical section of a survey. The projection itself is
//! `energyml_geometry::around_well`'s; this chooses what to project on.

use std::collections::HashSet;

use energyml_geometry::min_curvature::{Located, Measured};
use energyml_geometry::stations::SurveyStations;

use crate::survey::{Point, offsets};

/// What a source sees: the stations as read, and their positions (survey frame, `positions` on the kept stations,
/// azimuths in the file's own north).
pub struct SectionInput<'a> {
    /// The stations as read, rejected ones included.
    pub stations: &'a SurveyStations,
    /// The kept stations placed from the first one.
    pub located: &'a [Located],
}

impl SectionInput<'_> {
    /// The stations that were placed: those of `stations.measured` whose measured depth is one of `located`
    /// (the stations left out are not placed, and a source must not look at them).
    fn kept(&self) -> Vec<Measured> {
        let placed: HashSet<u64> = self.located.iter().map(|l| l.md.to_bits()).collect();
        self.stations
            .measured
            .iter()
            .filter(|m| placed.contains(&m.md.to_bits()))
            .copied()
            .collect()
    }
}

/// A source of the vertical section's azimuth (degrees, clockwise from the survey's north). A registry.
pub trait SectionAzimuth: Send + Sync {
    /// A stable identifier, for a recipe to name its choice.
    fn id(&self) -> &'static str;
    /// The azimuth this source gives, `None` when it has none.
    fn azimuth(&self, input: &SectionInput<'_>) -> Option<f64>;
}

/// The bearing of a horizontal vector `(north, east)`, through the one function of the crate that computes one.
fn bearing_of(north: f64, east: f64) -> Option<f64> {
    if !north.is_finite() || !east.is_finite() {
        return None;
    }
    let point = Point {
        md: 0.0,
        at: [north, east, 0.0],
    };
    offsets(&[point], [0.0; 3])[0].closure_direction
}

/// `AziVertSect`, as the file states it. Id `stated`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StatedAzimuth;

impl SectionAzimuth for StatedAzimuth {
    fn id(&self) -> &'static str {
        "stated"
    }

    fn azimuth(&self, input: &SectionInput<'_>) -> Option<f64> {
        match input.stations.section.azimuth {
            Some(Ok(a)) if a.is_finite() => Some(a.rem_euclid(360.0)),
            _ => None,
        }
    }
}

/// A value the caller gives. Id `user`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UserAzimuth(pub f64);

impl SectionAzimuth for UserAzimuth {
    fn id(&self) -> &'static str {
        "user"
    }

    fn azimuth(&self, _input: &SectionInput<'_>) -> Option<f64> {
        self.0.is_finite().then(|| self.0.rem_euclid(360.0))
    }
}

/// The closure direction at the last station (`survey::offsets`). Id `closure`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ClosureAzimuth;

impl SectionAzimuth for ClosureAzimuth {
    fn id(&self) -> &'static str {
        "closure"
    }

    fn azimuth(&self, input: &SectionInput<'_>) -> Option<f64> {
        let last = input.located.last()?;
        let point = Point {
            md: last.md,
            at: [last.north, last.east, last.tvd],
        };
        offsets(&[point], [0.0; 3])[0].closure_direction
    }
}

/// The vector mean of the azimuths of the stations inclined above `above` degrees (the drain). Id `drain-mean`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrainMeanAzimuth {
    /// The inclination, in degrees, above which a station is part of the drain (the threshold excluded).
    pub above: f64,
}

impl SectionAzimuth for DrainMeanAzimuth {
    fn id(&self) -> &'static str {
        "drain-mean"
    }

    fn azimuth(&self, input: &SectionInput<'_>) -> Option<f64> {
        let (mut cos, mut sin) = (0.0_f64, 0.0_f64);
        let mut any = false;
        for station in input.kept() {
            let (Some(inclination), Some(azimuth)) = (station.inclination, station.azimuth) else {
                continue;
            };
            if !inclination.is_finite() || inclination <= self.above || !azimuth.is_finite() {
                continue;
            }
            let a = azimuth.to_radians();
            cos += a.cos();
            sin += a.sin();
            any = true;
        }
        if !any || cos.hypot(sin) < 1e-12 {
            return None;
        }
        bearing_of(cos, sin)
    }
}

/// The sources of the section's azimuth, the caller's first.
pub struct SectionAzimuths {
    items: Vec<Box<dyn SectionAzimuth>>,
}

impl SectionAzimuths {
    /// `StatedAzimuth`, `ClosureAzimuth`, `DrainMeanAzimuth { above: 80.0 }`, in that order.
    pub fn builtin() -> SectionAzimuths {
        SectionAzimuths {
            items: vec![
                Box::new(StatedAzimuth),
                Box::new(ClosureAzimuth),
                Box::new(DrainMeanAzimuth { above: 80.0 }),
            ],
        }
    }

    /// Tried before every source already there (a `UserAzimuth` goes here).
    pub fn register(&mut self, source: Box<dyn SectionAzimuth>) {
        self.items.insert(0, source);
    }

    /// The first source that carries this id.
    pub fn get(&self, id: &str) -> Option<&dyn SectionAzimuth> {
        self.items.iter().find(|s| s.id() == id).map(|s| s.as_ref())
    }
}

/// The section chosen: its azimuth (degrees), its origin (`[east, north]` in metres from the first station, the
/// order of `VerticalSection.origin`) and who answered.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChosenSection {
    /// Degrees, clockwise from the survey's north, in `[0, 360)`.
    pub azimuth: f64,
    /// `[east, north]`, metres from the first station.
    pub origin: [f64; 2],
    /// Whether the file states the origin (both coordinates); otherwise it is the first station.
    pub origin_stated: bool,
    /// The id of the source that gave the azimuth.
    pub by: &'static str,
}

/// The first source that answers, or `None` when none does (a vertical hole with nothing stated).
///
/// Everything is in the survey's own north (`AziVertSect` follows `AziRef`): no north is asked.
pub fn section_azimuth(sources: &SectionAzimuths, input: &SectionInput<'_>) -> Option<ChosenSection> {
    let (azimuth, by) = sources
        .items
        .iter()
        .find_map(|source| source.azimuth(input).map(|a| (a, source.id())))?;
    let section = &input.stations.section;
    // Both coordinates, or neither: never a mixture of a stated and a defaulted one.
    let (origin, origin_stated) = match (&section.origin_east, &section.origin_north) {
        (Some(Ok(east)), Some(Ok(north))) if east.is_finite() && north.is_finite() => ([*east, *north], true),
        _ => ([0.0, 0.0], false),
    };
    Some(ChosenSection {
        azimuth,
        origin,
        origin_stated,
        by,
    })
}
